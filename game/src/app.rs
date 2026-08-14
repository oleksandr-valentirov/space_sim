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
use crate::plan::Manoeuvre;
use crate::planner::{Planner, Preview, Request};
use crate::save::{self, Save};
use crate::sim::{Command, Event, Sim};
use crate::text::Language;
use crate::view;
use crate::world::{World, EARTH};

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
                state.dragging = button_state == ElementState::Pressed && !consumed;
                if !state.dragging {
                    state.cursor = None;
                }
            }

            WindowEvent::CursorLeft { .. } => state.cursor = None,

            WindowEvent::CursorMoved { position, .. } => {
                let now = (position.x, position.y);
                if state.dragging {
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
        self.planner.request(Request {
            id: self.next_request,
            vessel: vessel.id,
            from: restart.state,
            step: restart.step,
            plan,
            horizon_end: vessel.horizon_end,
            params: vessel.params,
        });
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

        // Дискретне приходить каналом, а не снапшотом (`crate::sim`).
        for event in self.sim.events() {
            match event {
                Event::VesselFailed { vessel, error } => {
                    println!("апарат {vessel:?} зупинився: {error}");
                }
                Event::PlanRejected { vessel, why } => {
                    println!("план для {vessel:?} відхилено: {why:?}");
                }
                Event::Saved { error } => match error {
                    Some(e) => println!("сейв не записався: {e}"),
                    None => println!("сейв: {}", save::default_path().display()),
                },
                Event::PlanCommitted { vessel, from } => {
                    println!("план для {vessel:?} прийнято, перерахунок з {from:?}");
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
        let language = self.language;
        let radius = self.earth_radius_m;
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
                        }
                    });
            });
        self.input.apply(self.target.window(), platform);

        for command in commands {
            self.sim.send(command);
        }

        self.gpu.queue.submit([encoder.finish()]);
        self.gpu.queue.present(surface);
        Ok(())
    }
}
