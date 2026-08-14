//! Провід інтерфейсу (ROADMAP-UI.md, U1b). Віджети пише гра, не рушій.
//!
//! ## Чому це не порушує «рушій не знає про гру»
//!
//! `egui::Context` — тип бібліотеки, такий самий сторонній, як
//! `wgpu::Device`. Тут немає ні апарата, ні плану, ні часу: [`Ui::draw`]
//! бере замикання, яке малює **щось**, і не питає, що саме. Напрямок
//! лишається `game → engine`.
//!
//! ## Чому вікна тут теж немає
//!
//! Ввід приходить [`egui::RawInput`]ом **ззовні**, а не збирається всередині:
//! у вікні його дає `egui-winit` (U1c), у тесті — синтетична структура.
//! Це те саме рішення, що вже діє для кадру: `engine::frame` пише в текстуру
//! й нічого не знає про поверхню. Панель із правильними числами й панель із
//! NaN виглядають однаково, поки на них не подивитись, — тож дивитись треба
//! знімком, а знімок вікна не має.
//!
//! ## Порядок проходу
//!
//! Прохід egui — **останній**, у ту саму текстуру, `load` замість `clear`,
//! без глибини: кадр малює сцену, потім поверх неї — інтерфейс. Глибина тут
//! не потрібна взагалі, бо порядок віджетів задає egui своєю тесселяцією, а
//! не z-буфер.

use crate::gpu::Gpu;

/// Куди малюємо: розмір цілі в пікселях і масштаб інтерфейсу.
///
/// Трійка разом, а не трьома аргументами поспіль, і причина проста: два `u32`
/// підряд переставляються місцями мовчки, а кадр 720×1280 виглядає як помилка
/// лише на широкому екрані. Текстура сюди не входить — структура з
/// посиланням вимагала б лайфтайма (CLAUDE.md, стиль Rust).
#[derive(Clone, Copy)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    /// Пікселів на точку. Вікно бере його з `scale_factor`, знімок — 1.0.
    pub scale: f32,
}

impl Viewport {
    pub fn new(width: u32, height: u32, scale: f32) -> Viewport {
        Viewport {
            width,
            height,
            scale,
        }
    }

    /// Ввід, у якому нічого не відбувається.
    ///
    /// Потрібен усім, хто малює інтерфейс без вікна: знімкам, тестам, зондам.
    /// Живе тут, а не в тесті, бо `screen_rect` задається **в точках**, не в
    /// пікселях, — саме той перерахунок, який кожен викликач зробив би
    /// по-своєму.
    pub fn quiet_input(&self) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(
                    self.width as f32 / self.scale,
                    self.height as f32 / self.scale,
                ),
            )),
            ..Default::default()
        }
    }
}

/// Контекст egui й рендерер до нього. Один на ціль: пайплайн прив'язаний до
/// формату, рівно як у [`crate::frame::Frame`] і з тієї ж причини.
pub struct Ui {
    context: egui::Context,
    renderer: egui_wgpu::Renderer,
}

impl Ui {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Ui {
        Ui {
            context: egui::Context::default(),
            renderer: egui_wgpu::Renderer::new(
                &gpu.device,
                format,
                egui_wgpu::RendererOptions {
                    msaa_samples: 1,
                    // Глибини в інтерфейсі немає — порядок задає тесселяція.
                    depth_stencil_format: None,
                    // Дизеринг додає шум у пікселі, тобто робить «бітово те
                    // саме» недосяжним там, де нічого не намальовано (U1a).
                    dithering: false,
                    predictable_texture_filtering: true,
                },
            ),
        }
    }

    /// Контекст — для того, хто збирає ввід (`egui-winit` у U1c). Більше він
    /// нікому не потрібен: усе, що малюється, малюється в [`Ui::draw`].
    pub fn context(&self) -> &egui::Context {
        &self.context
    }

    /// Чи забрав інтерфейс мишу цього кадру (ROADMAP-UI.md, U1c).
    ///
    /// Питається **після** [`Ui::draw`]: відповідь залежить від того, що
    /// намальовано й де курсор, а обидва відомі лише тоді. Одне місце, один
    /// порядок — правило 4 етапу забороняє розкладати цю перевірку по
    /// обробниках.
    pub fn wants_pointer(&self) -> bool {
        self.context.egui_wants_pointer_input()
    }

    /// Те саме для клавіатури: поле вводу з фокусом з'їдає натискання, і
    /// гра не має бачити «w» як команду, поки гравець пише число.
    pub fn wants_keyboard(&self) -> bool {
        self.context.egui_wants_keyboard_input()
    }

    /// Чи стоїть курсор над областю egui взагалі.
    ///
    /// Ширше за [`Ui::wants_pointer`]: та каже «егуй користується мишею»,
    /// ця — «миша над панеллю». Розвилка U1c називала цей варіант запасним;
    /// що з них правильне, вирішує вимір, а не смак.
    pub fn pointer_over_panel(&self) -> bool {
        self.context.is_pointer_over_egui()
    }

    /// Малює інтерфейс поверх уже намальованого кадру.
    ///
    /// Повертає [`egui::PlatformOutput`] — те, що egui просить зробити
    /// **платформу**: змінити курсор, покласти щось у буфер обміну. Рушій
    /// цього не робить сам, бо це вже про вікно; у знімка воно просто
    /// пропадає, і це правильно.
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        viewport: Viewport,
        input: egui::RawInput,
        build: impl FnMut(&mut egui::Ui),
    ) -> egui::PlatformOutput {
        self.context.set_pixels_per_point(viewport.scale);

        let output = self.context.run_ui(input, build);
        let primitives = self
            .context
            .tessellate(output.shapes, output.pixels_per_point);

        // Текстури приїжджають навіть тоді, коли не намальовано нічого: атлас
        // шрифта — теж текстура (U1a). Одна може приїхати кількома клаптями,
        // звідси внутрішній цикл.
        let mut deltas = output.textures_delta;
        for (id, patches) in &deltas.set {
            for patch in patches {
                self.renderer
                    .update_texture(&gpu.device, &gpu.queue, *id, patch);
            }
        }

        let descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [viewport.width, viewport.height],
            pixels_per_point: viewport.scale,
        };
        self.renderer
            .update_buffers(&gpu.device, &gpu.queue, encoder, &primitives, &descriptor);

        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // Саме `Load`: сцену вже намальовано, і прохід
                        // інтерфейсу її не стирає.
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                multiview_mask: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.renderer
                .render(&mut pass.forget_lifetime(), &primitives, &descriptor);
        }

        for id in &deltas.free {
            self.renderer.free_texture(id);
        }
        // `TexturesDelta` падає в `Drop`, якщо її не застосували, — і це
        // корисно: мовчазно пропущена текстура виглядала б як зниклий текст.
        deltas.clear();

        output.platform_output
    }
}

/// Збирач вводу з вікна (ROADMAP-UI.md, U2b).
///
/// Обгортка над `egui-winit`, і саме тому гра його не бачить: інтерфейс
/// приходить із рушія цілком — і малювання, і ввід, — інакше в грі з'явилася б
/// друга залежність на ту саму бібліотеку, а разом із нею й спосіб отримати
/// дві її версії.
///
/// Тут же й межа знання: `WindowInput` знає про вікно, [`Ui`] — ні. Тому
/// знімок без вікна лишається можливим і після того, як з'явилось вікно.
pub struct WindowInput {
    state: egui_winit::State,
}

impl WindowInput {
    pub fn new(ui: &Ui, window: &winit::window::Window) -> WindowInput {
        WindowInput {
            state: egui_winit::State::new(
                ui.context().clone(),
                egui::ViewportId::ROOT,
                window,
                Some(window.scale_factor() as f32),
                None,
                None,
            ),
        }
    }

    /// Віддає подію egui й каже, чи вона спожита.
    ///
    /// `true` означає «гра цієї події не бачить». Питати треба **до** того, як
    /// подія піде далі, і рівно в одному місці — правило 4.
    pub fn on_window_event(
        &mut self,
        window: &winit::window::Window,
        event: &winit::event::WindowEvent,
    ) -> bool {
        self.state.on_window_event(window, event).consumed
    }

    /// Ввід, накопичений з попереднього кадру.
    pub fn take(&mut self, window: &winit::window::Window) -> egui::RawInput {
        self.state.take_egui_input(window)
    }

    /// Те, що egui просить зробити платформу: курсор, буфер обміну.
    pub fn apply(&mut self, window: &winit::window::Window, output: egui::PlatformOutput) {
        self.state.handle_platform_output(window, output);
    }
}
