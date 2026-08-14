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
use engine::window::{self, Target};

use crate::clock::Stall;
use crate::mission;
use crate::sim::{Command, Event, Sim};
use crate::view;
use crate::world::World;

pub struct Options {
    pub width: u32,
    pub height: u32,
    /// Скільки кадрів намалювати й вийти. `None` — доки не закриють.
    pub frames: Option<u32>,
    pub vsync: bool,
    pub asset: std::path::PathBuf,
    /// Додати демонстраційний маневр (`mission::demo_plan`).
    pub demo_plan: bool,
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

    /// Світ живе у власній нитці; тут лише ручка до неї (`crate::sim`).
    /// Головна нитка світ не рахує й не чіпає — вона його читає.
    sim: Sim,

    dragging: bool,
    cursor: Option<(f64, f64)>,
}

/// Світ за опціями — спільне для вікна й для знімка.
pub fn build_world(options: &Options) -> Result<World, String> {
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
                     час: пробіл — пауза, «.» і «,» — warp удвічі, Esc — вихід"
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

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::KeyboardInput { event, .. } => {
                if !event.state.is_pressed() {
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
                    _ => {}
                }
            }

            WindowEvent::Resized(size) => state.target.resize(&state.gpu, size.width, size.height),

            WindowEvent::MouseInput {
                state: button_state,
                button: MouseButton::Left,
                ..
            } => {
                state.dragging = button_state == ElementState::Pressed;
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
        let sim = Sim::spawn(options.asset.clone(), options.demo_plan)?;

        target.window().request_redraw();

        Ok(State {
            target,
            gpu,
            frame,
            orbit: Orbit::at_altitude(mission::CAMERA_ALTITUDE_M),
            sim,
            dragging: false,
            cursor: None,
        })
    }

    /// Друкує стан годинника зі снапшоту. UI тут поки що — це stdout: `egui`
    /// приходить разом із флайт-планером (M3), і заводити його заради двох
    /// чисел означало б вирішувати наперед, як виглядатиме те, чого ще немає.
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

    fn draw(&mut self) -> Result<(), String> {
        // Рівно один раз за кадр, і тримається весь кадр: два завантаження
        // дали б дві різні миті в одній картинці.
        let snapshot = self.sim.snapshot();

        // Дискретне приходить каналом, а не снапшотом (`crate::sim`).
        for event in self.sim.events() {
            match event {
                Event::VesselFailed { vessel, error } => {
                    println!("апарат {vessel:?} зупинився: {error}");
                }
                Event::PlanRejected { vessel, why } => {
                    println!("план для {vessel:?} відхилено: {why:?}");
                }
                Event::PlanCommitted { vessel, from } => {
                    println!("план для {vessel:?} прийнято, перерахунок з {from:?}");
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
            &view::build(&snapshot, self.orbit.camera()),
        );

        self.gpu.queue.submit([encoder.finish()]);
        self.gpu.queue.present(surface);
        Ok(())
    }
}
