//! Вікно й поверхня: усе, чого знімок не має (ROADMAP J1).
//!
//! Виділено з `app` тому, що з J1 циклів подій стало два — зонди рушія
//! лишаються в [`crate::app`], а гра має свій, бо володіє світом і часом
//! (PROJECT.md §6). Дублювати сюди-туди вміст `App` не страшно: це
//! перекладач подій `winit` у виклики, і в кожного з двох він свій.
//!
//! А от **поверхню дублювати не можна**, і це не смак. Тут живуть три
//! випадки, кожен з яких уже одного разу зависав намертво й жодного разу не
//! падав з помилкою:
//!
//!   1. переконфігурація не тим розміром ([`Target::resync`]);
//!   2. `Outdated` / `Lost` як звичайні стани, а не помилки ([`Target::acquire`]);
//!   3. `request_inner_size`, що змінює розмір ЗРАЗУ й не шле `Resized`
//!      ([`Target::request_size`]).
//!
//! Два місця з такою логікою розходяться, і розходження виглядає як «в одному
//! режимі чомусь не малює».

use std::sync::Arc;

use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use crate::gpu::Gpu;

pub struct Options {
    pub title: String,
    pub width: u32,
    pub height: u32,

    /// Чекати на вертикальну синхронізацію.
    ///
    /// Для гри — так, звісно. Для обмеженого прогону — ні, і це виміряно, а
    /// не вгадано: під X11 вікно процесу, який не має фокуса, може взагалі не
    /// показуватися, і тоді черга Fifo ніколи не звільняє кадр —
    /// `get_current_texture` блокується назавжди. Прогін зупинявся рівно на
    /// двадцятому кадрі й висів без жодної помилки.
    pub vsync: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            title: "space_sim".to_string(),
            width: 1280,
            height: 720,
            vsync: true,
        }
    }
}

/// Вікно, поверхня й конфігурація, яку та поверхня зараз має.
pub struct Target {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

impl Target {
    /// Відкриває вікно й створює під нього пристрій.
    ///
    /// Пристрій повертається поруч, а не приймається аргументом: адаптер має
    /// вміти малювати саме в цю поверхню (`Gpu::new(.., Some(&surface))`), а
    /// поверхня буває лише після вікна. Порядок тут не наш, а wgpu.
    pub fn open(event_loop: &ActiveEventLoop, options: &Options) -> Result<(Target, Gpu), String> {
        let attributes = Window::default_attributes()
            .with_title(options.title.clone())
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
            // Простір кольору вибирає бекенд: нам байдуже, а прив'язатися до
            // конкретного означало б відсікти поверхні, які його не
            // підтримують.
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

        Ok((
            Target {
                window,
                surface,
                config,
            },
            gpu,
        ))
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    pub fn width(&self) -> u32 {
        self.config.width
    }

    pub fn height(&self) -> u32 {
        self.config.height
    }

    /// Формат поверхні. Пайплайн прив'язаний до нього, тож `Frame` будується
    /// після того, як формат обрано, і переживає зміни розміру.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    pub fn describe(&self) -> String {
        format!(
            "{}×{}, {:?}, {:?}",
            self.config.width, self.config.height, self.config.format, self.config.present_mode
        )
    }

    pub fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) {
        // Згорнуте вікно дає нуль, а поверхня нульового розміру — помилка
        // валідації. Пропускаємо, а не затискаємо в одиницю: кадру все одно
        // нікуди йти.
        if width == 0 || height == 0 {
            return;
        }

        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&gpu.device, &self.config);
    }

    /// Переконфігурувати під **фактичний** розмір вікна.
    ///
    /// Саме фактичний, а не збережений. Перша версія переконфігуровувала
    /// поверхню тим самим `config`, і це зависало намертво: якщо вікно вже
    /// іншого розміру, поверхня лишається `Outdated`, кадр не малюється,
    /// лічильник не росте — і так вічно. Помилки при цьому немає жодної,
    /// програма просто перестає малювати.
    pub fn resync(&mut self, gpu: &Gpu) {
        let size = self.window.inner_size();
        self.resize(gpu, size.width, size.height);
    }

    /// Попросити інший розмір вікна.
    ///
    /// `request_inner_size` може змінити розмір ЗРАЗУ й повернути новий —
    /// тоді події `Resized` не буде взагалі. Пропустити цей випадок означає
    /// лишити поверхню старого розміру, а далі все зависає (див.
    /// [`Target::resync`]).
    pub fn request_size(&mut self, gpu: &Gpu, width: u32, height: u32) {
        let asked = winit::dpi::PhysicalSize::new(width, height);
        if let Some(now) = self.window.request_inner_size(asked) {
            self.resize(gpu, now.width, now.height);
        }
    }

    /// Наступна текстура поверхні, або `None`, якщо цього кадру не буде.
    ///
    /// wgpu 30 віддає не `Result`, а перелік станів, і більшість із них — не
    /// помилки, а звичайні події: зміна розміру, перекритий монітор,
    /// втрачена поверхня. Малювати в них нікуди, але й падати нема через що —
    /// переконфігуруємо й чекаємо наступного кадру.
    pub fn acquire(&mut self, gpu: &Gpu) -> Result<Option<wgpu::SurfaceTexture>, String> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(target) => Ok(Some(target)),
            wgpu::CurrentSurfaceTexture::Suboptimal(target) => Ok(Some(target)),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.resync(gpu);
                Ok(None)
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                Ok(None)
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                Err("поверхня відхилена валідацією".to_string())
            }
        }
    }
}
