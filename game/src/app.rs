//! Вікно гри (ROADMAP J1).
//!
//! Цикл подій тут власний, а не позичений у `engine::app`, і це не
//! дублювання, а межа: гра володіє світом і часом (PROJECT.md §6), рушій —
//! ні. Спільним лишається все, де можна помилитися мовчки: поверхня та її
//! стани живуть в `engine::window::Target`, кадр — в `engine::frame::Frame`,
//! камера — в `engine::orbit`.
//!
//! Порядок кадру, з J4 остаточний:
//!
//!   1. взяти незмінний зріз — `Sim::snapshot`, рівно один раз;
//!   2. забрати події, що накопичилися в каналі;
//!   3. перекласти зріз у сцену;
//!   4. намалювати.
//!
//! Світ головна нитка не рахує й не чіпає — вона його **читає**. Усе, що
//! гравець робить із часом і планом, іде командою в нитку симуляції
//! (`crate::sim`), а відповідь приходить наступним снапшотом.

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowId;

use engine::frame::Frame;
use engine::gpu::Gpu;
use engine::orbit::Orbit;
use engine::ui::{Ui, Viewport, WindowInput};
use engine::window::{self, Target};

use crate::clock::Stall;
use crate::hud;
use crate::leg::restart_at;
use crate::mission;
use crate::node;
use crate::plan::Manoeuvre;
use crate::planner::{Planner, Preview, PreviewRequest, Request};
use crate::porkchop::{Grid, GridRequest};
use crate::save::{self, Save};
use crate::schedule;
use crate::sim::{Command, Event, Sim};
use crate::text::{tr, Key as TextKey, Language};
use crate::view;
use crate::world::{PlanRejected, World, EARTH, MOON};

pub struct Options {
    pub width: u32,
    pub height: u32,
    /// Скільки кадрів намалювати й вийти. `None` — доки не закриють.
    pub frames: Option<u32>,
    pub vsync: bool,
    pub asset: std::path::PathBuf,
    /// Додати демонстраційний маневр (`mission::demo_plan`).
    pub demo_plan: bool,
    /// Підняти гру з сейву замість нової місії.
    pub load: Option<std::path::PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            width: 1280,
            height: 720,
            frames: None,
            vsync: true,
            asset: mission::default_asset(),
            demo_plan: false,
            load: None,
        }
    }
}

struct App {
    options: Options,
    drawn: u32,
    state: Option<State>,
    error: Option<String>,
}

struct State {
    target: Target,
    gpu: Gpu,
    frame: Frame,
    orbit: Orbit,

    /// Інтерфейс: провід у рушії, віджети тут (ROADMAP-UI.md, U1b).
    ui: Ui,
    input: WindowInput,
    language: Language,
    /// Радіус Землі з ассета, прочитаний **один раз** (`eph_body_radius`,
    /// U2a). Панель висоти його не переобчислює щокадру: розмір тіла не
    /// змінюється, а правило 5 забороняє кликати ефемериду з кадру.
    earth_radius_m: f64,

    /// Світ живе у власній нитці; тут лише ручка до неї (`crate::sim`).
    /// Головна нитка світ не рахує й не чіпає — вона його читає.
    sim: Sim,

    /// Спекулятивні прогони (`crate::planner`). У світ не пишуть нічого,
    /// доки гравець не скаже.
    planner: Planner,
    preview: Option<Preview>,
    next_request: u64,

    dragging: bool,
    cursor: Option<(f64, f64)>,

    /// План, який гравець редагує (ROADMAP-UI.md, U4a). Власний стан UI:
    /// поза екраном його не існує, доки не піде запитом або комітом.
    draft: hud::PlanDraft,
    /// Остання відповідь світу на план — те, що панель показує замість
    /// власного припущення про успіх (правило 8).
    notice: Option<String>,

    /// Вузли чернетки на екрані, пораховані минулим кадром (U4b).
    ///
    /// Саме минулим: подія миші приходить між кадрами, а вузли залежать від
    /// камери й від того, що вже пораховано. Кадр — це і є та мить, коли
    /// гравець їх бачив.
    nodes: Vec<node::NodeOnScreen>,
    /// Схоплена ручка, доки кнопку тримають.
    grab: Option<node::Grab>,
    /// Чернетку змінили тягненням — треба попросити прев'ю.
    draft_changed: bool,

    /// Плот вікон перельоту: текстура й обране вікно (ROADMAP-UI.md, U5c).
    plot: hud::PlotState,
    /// Остання сітка з планувальника. Не стан світу — відповідь на запит.
    grid: Option<Grid>,
    /// `mu` Землі з ассета, прочитана **один раз**, як і радіус вище.
    earth_mu: f64,
}

/// Світ за опціями — спільне для вікна й для знімка.
pub fn build_world(options: &Options) -> Result<World, String> {
    if let Some(path) = &options.load {
        // Ефемерида потрібна сейву готовою: він несе стан і план, але не
        // ассет (`crate::save`).
        let eph = core_rs::Ephemeris::load(&options.asset)
            .map(std::sync::Arc::new)
            .map_err(|e| format!("ассет не читається ({}): {e}", options.asset.display()))?;

        return Save::read(path)?
            .into_world(eph, mission::config())
            .map_err(|e| format!("сейв не піднімається ({}): {e}", path.display()));
    }

    let build = if options.demo_plan {
        mission::world_with_demo_plan
    } else {
        mission::world
    };
    build(&options.asset)
        .map_err(|e| format!("світ не будується ({}): {e}", options.asset.display()))
}

pub fn run(options: Options) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| format!("немає циклу подій: {e}"))?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App {
        options,
        drawn: 0,
        state: None,
        error: None,
    };

    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("цикл подій зупинився помилкою: {e}"))?;

    match app.error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        match State::new(event_loop, &self.options) {
            Ok(state) => {
                println!("адаптер: {}", state.gpu.describe());
                println!("поверхня: {}", state.target.describe());
                println!("ассет: {}", self.options.asset.display());
                println!(
                    "камера: тягніть лівою кнопкою — обертання, колесо — висота\n\
                     час: пробіл — пауза, «.» і «,» — warp удвічі\n\
                     план: «p» — показати гальмування через 5 діб, Enter — летіти ним\n\
                     F5 — зберегти, Esc — вихід"
                );
                state.report_time(&state.sim.snapshot());
                self.state = Some(state);
            }
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else {
            return;
        };

        // Ввід має власника, і питається це **рівно тут**, до всього іншого
        // (ROADMAP-UI.md, правило 4 і U1c). `consumed` означає «гра цієї
        // події не бачить»: клік по кнопці не має заодно крутити камеру.
        let consumed = state.input.on_window_event(state.target.window(), &event);

        match event {
            // Вихід, зміна розміру й перемальовка не належать нікому: egui їх
            // теж бачить, але вікно однаково мусить на них відповісти.
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::KeyboardInput { event, .. } => {
                if !event.state.is_pressed() || consumed {
                    return;
                }
                match event.logical_key.as_ref() {
                    Key::Named(NamedKey::Escape) => event_loop.exit(),
                    // Керування часом — команди в нитку, а не виклики.
                    // Відповідь прийде наступним снапшотом.
                    Key::Named(NamedKey::Space) => state.sim.send(Command::TogglePause),
                    // Warp множиться, а не додається: від 1 до 10⁷ сім
                    // порядків (`crate::clock`).
                    Key::Character(".") => state.sim.send(Command::ScaleWarp(2.0)),
                    Key::Character(",") => state.sim.send(Command::ScaleWarp(0.5)),
                    // Показати, що буде, якщо загальмувати через п'ять діб.
                    Key::Character("p") => state.ask_for_preview(),
                    // І полетіти цим планом.
                    Key::Named(NamedKey::Enter) => state.commit_preview(),
                    Key::Named(NamedKey::F5) => {
                        state.sim.send(Command::Save(save::default_path()));
                    }
                    _ => {}
                }
            }

            WindowEvent::Resized(size) => state.target.resize(&state.gpu, size.width, size.height),

            WindowEvent::MouseInput {
                state: button_state,
                button: MouseButton::Left,
                ..
            } => {
                // Натискання, що почалося в панелі, світові не належить —
                // інакше кнопка «пауза» заодно почала б обертати камеру.
                let pressed = button_state == ElementState::Pressed && !consumed;

                // Спершу ручка вузла, і лише потім камера: тягнення за
                // ручку — це правка плану, а не погляд на нього (U4b).
                state.grab = None;
                if pressed {
                    if let Some(cursor) = state.cursor {
                        let at = [cursor.0 as f32, cursor.1 as f32];
                        state.grab = node::pick_handle(&state.nodes, at);
                    }
                }

                state.dragging = pressed && state.grab.is_none();
                if button_state != ElementState::Pressed {
                    state.cursor = None;
                }
            }

            WindowEvent::CursorLeft { .. } => state.cursor = None,

            WindowEvent::CursorMoved { position, .. } => {
                let now = (position.x, position.y);
                if let (Some(grab), Some(was)) = (state.grab, state.cursor) {
                    // Тягнення за ручку править рівно одну компоненту Δv
                    // схопленої осі — решта не рухається за побудовою (U4b).
                    let drag = [(now.0 - was.0) as f32, (now.1 - was.1) as f32];
                    if let Some(node) = state.nodes.iter().find(|n| n.index == grab.node) {
                        let delta = node::drag_to_delta(node, grab.axis, drag);
                        if let Some(manoeuvre) = state.draft.manoeuvres.get_mut(grab.node) {
                            manoeuvre.dv[grab.axis] += delta;
                            state.draft_changed = true;
                        }
                    }
                } else if state.dragging {
                    if let Some(was) = state.cursor {
                        state.orbit.drag(now.0 - was.0, now.1 - was.1);
                    }
                }
                state.cursor = Some(now);
            }

            WindowEvent::MouseWheel { delta, .. } => {
                if consumed {
                    return;
                }
                let notches = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                    MouseScrollDelta::PixelDelta(p) => p.y / 50.0,
                };
                state.orbit.zoom(notches);
            }

            WindowEvent::RedrawRequested => {
                if let Err(e) = state.draw() {
                    self.error = Some(e);
                    event_loop.exit();
                    return;
                }

                self.drawn += 1;

                if let Some(limit) = self.options.frames {
                    if self.drawn >= limit {
                        let snapshot = state.sim.snapshot();
                        println!(
                            "намальовано кадрів: {}, семплів прогнозу: {}",
                            self.drawn,
                            snapshot
                                .vessels
                                .iter()
                                .map(|v| v.sample_count())
                                .sum::<usize>()
                        );
                        state.report_time(&snapshot);
                        event_loop.exit();
                        return;
                    }
                }

                state.target.window().request_redraw();
            }

            _ => {}
        }
    }
}

impl State {
    fn new(event_loop: &ActiveEventLoop, options: &Options) -> Result<State, String> {
        let (target, gpu) = Target::open(
            event_loop,
            &window::Options {
                title: "space_sim".to_string(),
                width: options.width,
                height: options.height,
                vsync: options.vsync,
            },
        )?;

        let frame = Frame::new(&gpu, target.format());
        let ui = Ui::new(&gpu, target.format());
        let input = WindowInput::new(&ui, target.window());
        let sim = Sim::spawn(build_world(options)?)?;
        let earth_radius_m = sim.ephemeris().body_radius(EARTH);
        let earth_mu = sim.ephemeris().body_mu(EARTH);
        // Планувальник ділить із симуляцією ассет, але не пропагатор:
        // `Ephemeris` — `Sync`, `Propagator` — ні (D3, H4).
        let planner = Planner::spawn(sim.ephemeris(), mission::config())?;

        target.window().request_redraw();

        Ok(State {
            target,
            gpu,
            frame,
            ui,
            input,
            // Англійська як основна (ROADMAP-UI.md, правило 7). Перемикача
            // ще немає — він з'явиться разом із рештою налаштувань, і саме
            // тому мова вже поле, а не константа в кожному виклику.
            language: Language::default(),
            earth_radius_m,
            orbit: Orbit::at_altitude(mission::CAMERA_ALTITUDE_M),
            sim,
            planner,
            preview: None,
            next_request: 0,
            dragging: false,
            cursor: None,
            draft: hud::PlanDraft::default(),
            notice: None,
            nodes: Vec::new(),
            grab: None,
            draft_changed: false,
            plot: hud::PlotState::default(),
            grid: None,
            earth_mu,
        })
    }

    /// Друкує стан годинника зі снапшоту — **для прогонів без очей**.
    ///
    /// З U2b те саме показує панель часу (`crate::hud`), і вона тепер
    /// головна. Цей друк лишається для `--frames N`, який виходить сам і
    /// нікому нічого не показує: рядок у stdout — єдине, що з такого прогону
    /// можна прочитати в CI.
    fn report_time(&self, snapshot: &crate::snapshot::WorldSnapshot) {
        let day = (snapshot.t - mission::start().t) / 86400.0;
        println!(
            "  доба {day:.2} з {:.2}, warp ×{:.0}{}",
            mission::DAYS,
            snapshot.warp,
            match snapshot.stall {
                Some(Stall::Paused) => " (пауза)",
                Some(Stall::Horizon) => " (упирається в горизонт)",
                Some(Stall::MissionEnd) => " (місія скінчилася)",
                None => "",
            }
        );
    }

    /// Просить прогноз для гіпотетичного гальмування через п'ять діб.
    ///
    /// Одна кнопка замість тягнення вузла: UI флайт-планера — це M3, а тут
    /// перевіряється шлях, а не інтерфейс.
    fn ask_for_preview(&mut self) {
        let snapshot = self.sim.snapshot();
        let Some(vessel) = snapshot.vessels.first() else {
            return;
        };

        let burn_t = snapshot.t + 5.0 * 86400.0;
        if burn_t >= vessel.horizon_end || vessel.computed_to < burn_t {
            println!("прев'ю: прогноз ще не дійшов до тієї доби");
            return;
        }

        let mut plan = vessel.plan.clone();
        plan.insert(Manoeuvre {
            t: burn_t,
            dv: [-8.0, 0.0, 0.0],
            frame: crate::plan::Frame::Vnb { body: EARTH },
        });

        // Точка перезапуску — та сама функція, якою скористається симуляція,
        // коли прийме план. Одна функція, а не два однакові правила.
        let restart = restart_at(&vessel.legs, vessel.start, burn_t);

        self.next_request += 1;
        self.planner.request(Request::Preview(PreviewRequest {
            id: self.next_request,
            vessel: vessel.id,
            from: restart.state,
            step: restart.step,
            plan,
            horizon_end: vessel.horizon_end,
            params: vessel.params,
        }));
    }

    /// Що зробити з тим, що попросила панель плану (ROADMAP-UI.md, U4a).
    ///
    /// Прев'ю йде в планувальник, коміт — у нитку симуляції. Розділення не
    /// косметичне: планувальник не пише у світ нічого (J5), і саме тому
    /// правку можна показувати на кожен рух повзунка.
    fn apply_plan_action(
        &mut self,
        action: hud::PlanAction,
        snapshot: &crate::snapshot::WorldSnapshot,
    ) {
        let Some(vessel) = snapshot.vessels.first() else {
            return;
        };

        match action {
            hud::PlanAction::Preview(plan) => {
                // Точка перезапуску — та сама функція, якою скористається
                // симуляція, коли прийме план. Одна функція, а не два
                // однакові правила.
                let Some(first) = plan.manoeuvres().first() else {
                    // Порожній план прев'ю не потребує: летіти ним — це те
                    // саме, що вже намальовано.
                    self.preview = None;
                    return;
                };
                let restart = restart_at(&vessel.legs, vessel.start, first.t);

                self.next_request += 1;
                self.planner.request(Request::Preview(PreviewRequest {
                    id: self.next_request,
                    vessel: vessel.id,
                    from: restart.state,
                    step: restart.step,
                    plan,
                    horizon_end: vessel.horizon_end,
                    params: vessel.params,
                }));
            }
            hud::PlanAction::Commit(plan) => {
                self.notice = None;
                self.sim.send(Command::CommitPlan {
                    vessel: vessel.id,
                    plan,
                });
            }
        }
    }

    /// Що зробити з тим, що попросив плот вікон (ROADMAP-UI.md, U5c).
    fn apply_porkchop_action(
        &mut self,
        action: hud::PorkchopAction,
        snapshot: &crate::snapshot::WorldSnapshot,
    ) {
        match action {
            hud::PorkchopAction::Compute => {
                let Some(vessel) = snapshot.vessels.first() else {
                    return;
                };
                self.next_request += 1;
                if let Some(request) = self.grid_request(snapshot.t, vessel) {
                    self.planner.request(Request::Grid(request));
                } else {
                    self.notice = Some(tr(self.language, TextKey::NoGridYet).to_string());
                }
            }
            // Вибір вікна поки що лишається на екрані: перетворити його на
            // маневр — окремий крок із власним оракулом (U5d).
            hud::PorkchopAction::Choose(..) => {}
        }
    }

    /// Осі сітки: моменти відходу з траєкторії, перельоти — від пів доби.
    ///
    /// Відхід іде **не далі за пораховане**, і це не обережність, а умова
    /// правильності: `leg::state_at` за горизонтом віддає крайню точку, тож
    /// сітка, побудована на ній, показувала б переліт із місця, де апарата не
    /// буде. `None` означає «прогноз ще не відійшов від курсора» — тоді
    /// рахувати нема з чого, і сказати про це чесніше, ніж намалювати рядок
    /// із самих дірок.
    ///
    /// Решта чисел — властивість цієї місії, а не плоту: переліт Земля—Місяць
    /// триває від трьох до семи діб, тож вісь від пів доби до чотирнадцяти
    /// лишає видимими обидва краї заборонених зон.
    fn grid_request(
        &self,
        now: f64,
        vessel: &crate::snapshot::VesselSnapshot,
    ) -> Option<GridRequest> {
        const DAY: f64 = 86400.0;
        const COLUMNS: usize = 40;

        // Крок осі відходу: рівно стільки, щоб сорок стовпців улягалися в
        // пораховане. Порожній горизонт — це `None` вище.
        let span = vessel.computed_to - now;
        if span < DAY {
            return None;
        }
        let step = (span / COLUMNS as f64).min(DAY);

        let depart = (0..COLUMNS)
            .map(|i| crate::leg::state_at(&vessel.legs, vessel.start, now + i as f64 * step))
            .collect();

        Some(GridRequest {
            id: self.next_request,
            depart,
            arrive_body: MOON,
            centre_body: EARTH,
            mu: self.earth_mu,
            prograde: true,
            tof: (1..=28).map(|j| f64::from(j) * 0.5 * DAY).collect(),
        })
    }

    /// Летіти показаним планом.
    fn commit_preview(&mut self) {
        let Some(preview) = self.preview.take() else {
            println!("нема чого комітити — спершу «p»");
            return;
        };
        self.sim.send(Command::CommitPlan {
            vessel: preview.vessel,
            plan: preview.plan,
        });
    }

    fn draw(&mut self) -> Result<(), String> {
        // Рівно один раз за кадр, і тримається весь кадр: два завантаження
        // дали б дві різні миті в одній картинці.
        let snapshot = self.sim.snapshot();

        // Прев'ю з планувальника: беремо найсвіжіше, старіші відкидаються.
        if let Some(preview) = self.planner.latest() {
            println!("прев'ю {}: {} ланок", preview.id, preview.legs.len());
            self.preview = Some(preview);
        }

        // Так само й сітка вікон — інший канал, те саме правило (U5b).
        if let Some(grid) = self.planner.latest_grid() {
            println!(
                "сітка {}: {} клітинок, {} без розв'язку",
                grid.id,
                grid.cells.len(),
                grid.cells.iter().filter(|c| c.is_none()).count()
            );
            // Вибір із попередньої сітки на нову не переноситься: індекси ті
            // самі, а осі інші, тож «те саме вікно» вказувало б на інший час.
            self.plot.chosen = None;
            self.grid = Some(grid);
        }

        // Дискретне приходить каналом, а не снапшотом (`crate::sim`).
        for event in self.sim.events() {
            match event {
                Event::VesselFailed { vessel, error } => {
                    println!("апарат {vessel:?} зупинився: {error}");
                }
                // Правило 8 етапу U: відповідь показує панель, а не власне
                // припущення про успіх. Поки панелі повідомлень немає —
                // stdout, як і решта подій тут.
                Event::SeekRejected { t, why } => {
                    println!("перемотування на {t:.1} відхилено: {why:?}");
                }
                Event::PlanRejected { vessel, why } => {
                    println!("план для {vessel:?} відхилено: {why:?}");
                    self.notice = Some(
                        match why {
                            PlanRejected::InThePast => {
                                tr(self.language, TextKey::RejectedInThePast)
                            }
                            PlanRejected::NoSuchVessel => tr(self.language, TextKey::Failed),
                        }
                        .to_string(),
                    );
                }
                Event::Saved { error } => match error {
                    Some(e) => println!("сейв не записався: {e}"),
                    None => println!("сейв: {}", save::default_path().display()),
                },
                Event::PlanCommitted { vessel, from } => {
                    println!("план для {vessel:?} прийнято, перерахунок з {from:?}");
                    self.notice = Some(tr(self.language, TextKey::PlanAccepted).to_string());
                    // Прев'ю стало реальністю — стирати його з кадру.
                    self.preview = None;
                }
            }
        }

        let Some(surface) = self.target.acquire(&self.gpu)? else {
            return Ok(());
        };

        let view = surface
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("game frame"),
            });

        self.frame.draw(
            &self.gpu,
            &mut encoder,
            &view,
            self.target.width(),
            self.target.height(),
            &view::build_with_preview(
                &snapshot,
                self.orbit.camera(),
                self.preview.as_ref().map_or(&[], |p| p.legs.as_slice()),
            ),
        );

        // Інтерфейс — останнім проходом, у ту саму текстуру (U1b). Панель
        // повертає команди, а не надсилає їх: хто надсилає, той і знає про
        // канал, а панель знає лише про те, що намальовано.
        let mut commands = Vec::new();
        let mut plan_actions = Vec::new();
        let mut plot_actions = Vec::new();
        let language = self.language;
        let radius = self.earth_radius_m;
        let notice = self.notice.clone();
        let draft = &mut self.draft;
        let plot = &mut self.plot;
        let grid = self.grid.as_ref();
        let viewport = Viewport::new(
            self.target.width(),
            self.target.height(),
            self.target.window().scale_factor() as f32,
        );
        let input = self.input.take(self.target.window());
        let platform = self
            .ui
            .draw(&self.gpu, &mut encoder, &view, viewport, input, |ui| {
                engine::egui::Panel::left("time")
                    .exact_size(220.0)
                    .resizable(false)
                    .show(ui, |ui| {
                        commands.extend(hud::time_panel(ui, language, &snapshot));
                        ui.separator();
                        if let Some(vessel) = snapshot.vessels.first() {
                            let readout = hud::read_vessel(&snapshot, vessel, radius);
                            hud::vessel_panel(ui, language, &vessel.name, &readout);

                            ui.separator();
                            // Скан по вже порахованих семплах — нічого не
                            // озброюється й не інтегрується (U3a).
                            let markers = schedule::scan(&vessel.legs);
                            commands
                                .extend(hud::schedule_panel(ui, language, snapshot.t, &markers));

                            ui.separator();
                            plan_actions.extend(hud::plan_panel(
                                ui,
                                language,
                                snapshot.t,
                                EARTH,
                                draft,
                                notice.as_deref(),
                            ));
                        }
                    });

                // Плот — праворуч, окремою панеллю: він квадратний і живе
                // своїм життям, а ліва колонка вже про апарат і його план.
                engine::egui::Panel::right("windows")
                    .exact_size(230.0)
                    .resizable(false)
                    .show(ui, |ui| {
                        plot_actions.extend(hud::porkchop_panel(ui, language, grid, plot));
                    });
            });
        self.input.apply(self.target.window(), platform);

        for command in commands {
            self.sim.send(command);
        }
        for action in plan_actions {
            self.apply_plan_action(action, &snapshot);
        }
        for action in plot_actions {
            self.apply_porkchop_action(action, &snapshot);
        }

        // Вузли для наступного кадру: подія миші прийде між кадрами, а
        // порівнювати їй треба з тим, що гравець бачив (U4b).
        self.nodes = match snapshot.vessels.first() {
            Some(vessel) => node::nodes_on_screen(
                &self.orbit.camera(),
                engine::frame::FOV_Y,
                self.target.width(),
                self.target.height(),
                vessel,
                &self.draft.manoeuvres,
            ),
            None => Vec::new(),
        };

        // Тягнення за ручку — така сама правка, як поле в панелі, тож і
        // відповідь та сама: запит прев'ю (U4a). Один на кадр, а не на
        // кожну подію миші: скасування між ланками вже задумано під це (J5).
        if self.draft_changed {
            self.draft_changed = false;
            self.apply_plan_action(hud::PlanAction::Preview(self.draft.plan()), &snapshot);
        }

        self.gpu.queue.submit([encoder.finish()]);
        self.gpu.queue.present(surface);
        Ok(())
    }
}
