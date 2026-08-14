//! Вікно гри (ROADMAP J1).
//!
//! Цикл подій тут власний, а не позичений у `engine::app`, і це не
//! дублювання, а межа: гра володіє світом і часом (PROJECT.md §6), рушій —
//! ні. Спільним лишається все, де можна помилитися мовчки: поверхня та її
//! стани живуть в `engine::window::Target`, кадр — в `engine::frame::Frame`,
//! камера — в `engine::orbit`.
//!
//! Порядок кадру такий, яким він лишиться й після J4, коли світ поїде у свою
//! нитку:
//!
//!   1. посунути світ уперед — тік із бюджетом у ланках;
//!   2. взяти незмінний зріз — снапшот;
//!   3. перекласти зріз у сцену;
//!   4. намалювати.
//!
//! Різниця буде лише в тому, що крок 1 робитиме інша нитка, а крок 2 стане
//! `load_full()`. Саме тому вони вже розділені: межа, проведена після появи
//! потоку, проходить там, де зручно потоку.

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowId;

use engine::frame::Frame;
use engine::gpu::Gpu;
use engine::orbit::Orbit;
use engine::window::{self, Target};

use crate::mission;
use crate::view;
use crate::world::World;

/// Скільки ланок дозволено порахувати за один кадр.
///
/// Не оптимізація, а стеля затримки: тік не має права тримати кадр довше, ніж
/// на цю роботу. Число мале навмисно — прогноз росте на очах, і видно, що
/// траєкторія рахується ланками, а не з'являється цілою.
///
/// Скільки ланок устигнеться, залежить від машини; **де** вони закінчаться —
/// ні (CLAUDE.md, інваріант 9). Тому міняти це число безпечно: воно не
/// впливає на числа.
const LEGS_PER_FRAME: usize = 2;

pub struct Options {
    pub width: u32,
    pub height: u32,
    /// Скільки кадрів намалювати й вийти. `None` — доки не закриють.
    pub frames: Option<u32>,
    pub vsync: bool,
    pub asset: std::path::PathBuf,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            width: 1280,
            height: 720,
            frames: None,
            vsync: true,
            asset: mission::default_asset(),
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
    world: World,

    dragging: bool,
    cursor: Option<(f64, f64)>,
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
                    "камера: тягніть лівою кнопкою — обертання, колесо — висота, \
                     Esc — вихід"
                );
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
                if event.state.is_pressed() && event.logical_key == Key::Named(NamedKey::Escape) {
                    event_loop.exit();
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
                        println!(
                            "намальовано кадрів: {}, семплів прогнозу: {}",
                            self.drawn,
                            state
                                .world
                                .vessels()
                                .iter()
                                .map(|v| v.trajectory.sample_count())
                                .sum::<usize>()
                        );
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
        let world = mission::world(&options.asset)
            .map_err(|e| format!("світ не будується ({}): {e}", options.asset.display()))?;

        target.window().request_redraw();

        Ok(State {
            target,
            gpu,
            frame,
            orbit: Orbit::at_altitude(mission::CAMERA_ALTITUDE_M),
            world,
            dragging: false,
            cursor: None,
        })
    }

    fn draw(&mut self) -> Result<(), String> {
        self.world.tick(LEGS_PER_FRAME);
        let snapshot = self.world.snapshot();

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
