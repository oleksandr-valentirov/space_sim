//! Вікно. `winit` створює його, `wgpu` малює в його поверхню.
//!
//! Кадр береться з [`crate::frame`], той самий, що йде у знімок. Тут лише
//! те, чого знімок не має: поверхня, її переконфігурація при зміні розміру
//! і цикл подій.

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::frame::Frame;
use crate::gpu::Gpu;

pub struct Options {
    pub width: u32,
    pub height: u32,

    /// Скільки кадрів намалювати й вийти. `None` — доки не закриють.
    ///
    /// Існує заради перевірки: запуск, який сам завершується, можна ганяти
    /// в скрипті й у CI, а «відкрилось вікно, подивіться» — не можна.
    pub frames: Option<u32>,

    /// Чекати на вертикальну синхронізацію.
    ///
    /// Для гри — так, звісно. Для обмеженого прогону — ні, і це виміряно, а
    /// не вгадано: під X11 вікно процесу, який не має фокуса, може взагалі не
    /// показуватися, і тоді черга Fifo ніколи не звільняє кадр —
    /// `get_current_texture` блокується назавжди. Прогін зупинявся рівно на
    /// двадцятому кадрі й висів без жодної помилки.
    ///
    /// Тобто перевірка, яка залежить від того, чи компонувальник справді
    /// показує вікно, — не перевірка рендера. Тому `--frames` вимикає vsync,
    /// а `--vsync` вмикає назад, якщо треба подивитися саме на нього.
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
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    gpu: Gpu,
    frame: Frame,
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
                println!(
                    "поверхня: {}×{}, {:?}, {:?}",
                    state.config.width,
                    state.config.height,
                    state.config.format,
                    state.config.present_mode
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

            WindowEvent::Resized(size) => state.resize(size.width, size.height),

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

                        // request_inner_size може змінити розмір ЗРАЗУ й
                        // повернути новий — тоді події Resized не буде.
                        // Пропустити цей випадок означає лишити поверхню
                        // старого розміру, і далі все зависає (див. resync).
                        let asked = winit::dpi::PhysicalSize::new(
                            self.options.width / 2,
                            self.options.height / 2,
                        );
                        println!("зміна розміру: {}×{}", asked.width, asked.height);
                        if let Some(now) = state.window.request_inner_size(asked) {
                            state.resize(now.width, now.height);
                        }
                    }

                    if self.drawn >= limit {
                        println!("намальовано кадрів: {}", self.drawn);
                        event_loop.exit();
                        return;
                    }
                }

                state.window.request_redraw();
            }

            _ => {}
        }
    }
}

impl State {
    fn new(event_loop: &ActiveEventLoop, options: &Options) -> Result<State, String> {
        let attributes = Window::default_attributes()
            .with_title("space_sim")
            .with_inner_size(winit::dpi::PhysicalSize::new(options.width, options.height));

        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| format!("вікно не створюється: {e}"))?,
        );

        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| format!("поверхня не створюється: {e}"))?;

        let gpu = Gpu::new(instance, Some(&surface))?;

        let size = window.inner_size();
        let capabilities = surface.get_capabilities(&gpu.adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(capabilities.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            // Простір кольору вибирає бекенд: на F1 нам байдуже, а
            // прив'язатися до конкретного означало б відсікти поверхні,
            // які його не підтримують.
            color_space: wgpu::SurfaceColorSpace::default(),
            present_mode: if options.vsync {
                wgpu::PresentMode::AutoVsync
            } else {
                wgpu::PresentMode::AutoNoVsync
            },
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&gpu.device, &config);

        // Пайплайн прив'язаний до формату цілі, тож будується після того, як
        // формат обрано, і переживає зміни розміру — вони формату не чіпають.
        let frame = Frame::new(&gpu.device, config.format);

        window.request_redraw();

        Ok(State {
            window,
            surface,
            config,
            gpu,
            frame,
        })
    }

    /// Переконфігурувати під фактичний розмір вікна.
    ///
    /// Саме фактичний, а не збережений. Перша версія переконфігуровувала
    /// поверхню тим самим `self.config`, і це зависало намертво: якщо вікно
    /// уже іншого розміру, поверхня лишається Outdated, кадр не малюється,
    /// лічильник не росте — і так вічно. Помилки при цьому немає жодної,
    /// програма просто перестає малювати.
    fn resync(&mut self) {
        let size = self.window.inner_size();
        self.resize(size.width, size.height);
    }

    fn resize(&mut self, width: u32, height: u32) {
        // Згорнуте вікно дає нуль, а поверхня нульового розміру — помилка
        // валідації. Пропускаємо, а не затискаємо в одиницю: кадру все одно
        // нікуди йти.
        if width == 0 || height == 0 {
            return;
        }

        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.gpu.device, &self.config);
    }

    fn draw(&mut self) -> Result<(), String> {
        // wgpu 30 віддає не Result, а перелік станів, і більшість із них —
        // не помилки, а звичайні події: зміна розміру, перекритий монітор,
        // втрачена поверхня. Малювати в них нікуди, але й падати нема через
        // що — переконфігуруємо й чекаємо наступного кадру.
        let target = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(target) => target,
            wgpu::CurrentSurfaceTexture::Suboptimal(target) => target,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.resync();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("поверхня відхилена валідацією".to_string());
            }
        };

        let view = target
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });

        self.frame.draw(&mut encoder, &view);

        self.gpu.queue.submit([encoder.finish()]);
        // У wgpu 30 показ кадру перейшов на чергу: раніше це був метод самої
        // текстури.
        self.gpu.queue.present(target);
        Ok(())
    }
}
