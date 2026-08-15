//! Вікно рушія: цикл подій для зондів (ROADMAP F1, I2).
//!
//! Кадр береться з [`crate::frame`], той самий, що йде у знімок. Поверхня й
//! усе, що з нею буває, живуть у [`crate::window`] — з J1 циклів подій два,
//! і саме поверхню дублювати не можна.
//!
//! Тут малюється сцена з самою планетою: гра має власний цикл, бо володіє
//! світом і часом (PROJECT.md §6), а цей лишається тим, чим був, — способом
//! подивитися на рендер без гри.

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowId;

use crate::frame::{self, Frame};
use crate::gpu::Gpu;
use crate::orbit::Orbit;
use crate::window::{self, Target};

pub struct Options {
    pub width: u32,
    pub height: u32,

    /// Скільки кадрів намалювати й вийти. `None` — доки не закриють.
    ///
    /// Існує заради перевірки: запуск, який сам завершується, можна ганяти
    /// в скрипті й у CI, а «відкрилось вікно, подивіться» — не можна.
    pub frames: Option<u32>,

    /// Чекати на вертикальну синхронізацію. Чому це прапорець і чим воно
    /// колись зависало — [`crate::window::Options::vsync`].
    pub vsync: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            width: 1280,
            height: 720,
            frames: None,
            vsync: true,
        }
    }
}

struct App {
    options: Options,
    drawn: u32,
    resized_once: bool,
    state: Option<State>,
    error: Option<String>,
}

struct State {
    target: Target,
    gpu: Gpu,
    frame: Frame,

    /// Камера й те, чим гравець її рухає. Позиція виводиться з кутів і
    /// висоти щокадру, тож тут не накопичується нічого, що могло б сповзти
    /// (`crate::orbit`).
    orbit: Orbit,

    /// Ліва кнопка тримається — тягнемо камеру.
    dragging: bool,
    /// Де курсор був минулого разу; різниця й є зсув. `None` — курсор ще не
    /// з'являвся у вікні або щойно повернувся, і зсув порахувати нема від
    /// чого.
    cursor: Option<(f64, f64)>,
}

pub fn run(options: Options) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| format!("немає циклу подій: {e}"))?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App {
        options,
        drawn: 0,
        resized_once: false,
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
                println!(
                    "камера: тягніть лівою кнопкою — обертання, колесо — висота \
                     (від {:.0} м до {:.0e} м), Esc — вихід",
                    crate::orbit::MIN_ALTITUDE_M,
                    crate::orbit::MAX_ALTITUDE_M
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

            // Керування камерою. Вікно лише перекладає події в числа —
            // що з них виходить, вирішує `orbit`, і саме тому воно
            // перевіряється без вікна й без GPU.
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
                    // Перший рух після натискання не має від чого рахувати
                    // зсув: без цього камера смикалася б на всю відстань від
                    // попереднього положення курсора.
                    if let Some(was) = state.cursor {
                        state.orbit.drag(now.0 - was.0, now.1 - was.1);
                    }
                }
                state.cursor = Some(now);
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let notches = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                    // Тачпад дає пікселі. Півсотні на клац — щоб один рух
                    // пальцем не пролітав три порядки висоти.
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

                // Зміна розміру посеред обмеженого прогону — щоб той шлях
                // теж був пройдений, а не лише відкриття вікна. Саме на ньому
                // ламається surface, і ламається мовчки.
                if let Some(limit) = self.options.frames {
                    if !self.resized_once && self.drawn == limit / 2 {
                        self.resized_once = true;

                        let (width, height) = (self.options.width / 2, self.options.height / 2);
                        println!("зміна розміру: {width}×{height}");
                        state.target.request_size(&state.gpu, width, height);
                    }

                    if self.drawn >= limit {
                        println!("намальовано кадрів: {}", self.drawn);
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

        // Пайплайн прив'язаний до формату цілі, тож будується після того, як
        // формат обрано, і переживає зміни розміру — вони формату не чіпають.
        let frame = Frame::new(&gpu, target.format());

        target.window().request_redraw();

        Ok(State {
            target,
            gpu,
            frame,
            orbit: Orbit::default(),
            dragging: false,
            cursor: None,
        })
    }

    fn draw(&mut self) -> Result<(), String> {
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
                label: Some("frame"),
            });

        self.frame.draw(
            &self.gpu,
            &mut encoder,
            &view,
            self.target.width(),
            self.target.height(),
            // Зонд рушія дивиться на одне тіло радіуса Землі: гра свою сцену
            // збирає сама, а тут її немає (R1e).
            &frame::default_scene(self.orbit.camera()),
        );

        self.gpu.queue.submit([encoder.finish()]);
        // У wgpu 30 показ кадру перейшов на чергу: раніше це був метод самої
        // текстури.
        self.gpu.queue.present(surface);
        Ok(())
    }
}
