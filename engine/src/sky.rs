//! Небо й повітря на GPU: таблиці Hillaire 2020 (ROADMAP-ATMOSPHERE.md).
//!
//! Тут живуть **таблиці**, а не картинка. Розділення не косметичне: таблиці
//! різняться тим, як часто їх треба рахувати, і саме це визначає, де вони
//! стоять у кадрі (правило 5 етапу S):
//!
//! | таблиця | від чого залежить | як часто |
//! |---|---|---|
//! | пропускання | лише параметри повітря | раз на набір параметрів |
//! | багаторазове розсіювання | лише параметри повітря | раз на набір параметрів |
//! | небо (sky-view) | позиція камери + напрямок на Сонце | раз на кадр |
//! | аеральна перспектива | фрустум камери | раз на кадр, і не завжди |
//!
//! Решта рядків з'явиться разом зі своїми кроками; заводити їх наперед
//! CLAUDE.md прямо забороняє.
//!
//! ## Дві групи прив'язки: читане й писане
//!
//! Група 0 — те, що прохід читає, група 1 — те, що він пише. Поділ вимушений:
//! таблицю пропускання пише один прохід і читають усі наступні, а одна
//! bind-група, у якій та сама текстура стоїть і на запис, і на читання,
//! заборонена — wgpu бачить у ній гонку незалежно від того, що робить шейдер.
//! Тому проходу пропускання дістається **урізаний** макет групи 0, без самої
//! таблиці, і це не хитрість: від макета вимагається накривати те, що точка
//! входу читає, а не все, що є в модулі.
//!
//! ## Чому [`Sky::ensure`] подає роботу сам, а не в чужий encoder
//!
//! Бо це не робота кадру. Таблиця пропускання перераховується тоді, коли
//! змінилися параметри повітря, тобто практично ніколи; протягнута крізь
//! encoder кадру, вона виглядала б як щокадрова, і перший, хто прийде її
//! оптимізувати, витратить день. Таблиці, які **справді** рахуються щокадру,
//! підуть у кадровий encoder — і різниця між ними стане видима з коду.

use crate::atmosphere;
use crate::gpu::Gpu;
use crate::scene::Atmosphere;

/// WGSL, згенерований зі `shaders/sky.slang` (`scripts/build_shaders.sh`).
const SKY_WGSL: &str = include_str!("../shaders/sky.wgsl");

/// Формат таблиць. Half-float: пропускання лежить у `[0, 1]`, і одинадцяти
/// значущих бітів там вистачає з запасом — виміряно тестом S2, який звіряє
/// таблицю з оракулом у `f64`.
const LUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Скільки байтів займає `AirParams` у шейдері: чотири `float4`.
const AIR_BYTES: u64 = 80;

/// Скільки байтів займає `ViewParams` у шейдері: шість `float4`.
const VIEW_BYTES: u64 = 96;

/// Скільки байтів займає `PassParams`: один `float4`.
const PASS_BYTES: u64 = 16;

/// Крок між `PassParams` сусідніх діапазонів глибини в буфері.
///
/// 256 байтів — вирівнювання динамічного зсуву, якого вимагає wgpu на всіх
/// трьох цілях. Те саме число й з тієї самої причини, що `frame::PASS_STRIDE`.
const PASS_STRIDE: u64 = 256;

/// Скільки разів яскравіше стає небо перед записом у кадр.
///
/// **Стала, а не автоекспозиція**, і це рішення етапу: автоекспозиція
/// стосується всієї сцени, а не повітря, і без корабля в кадрі міряти її нема
/// на чому (ROADMAP-ATMOSPHERE.md, «чого етап S свідомо не робить»).
///
/// Число виміряне, а не підібране на око: яскравість у зеніті опівдні виходить
/// 0.048 на одиницю освітленості Сонця, і множник 8 ставить її на 0.38 —
/// денне небо, яке не впирається в одиницю навіть біля горизонту. Правити його
/// доведеться тоді ж, коли з'явиться автоекспозиція, і тим самим кроком.
pub const EXPOSURE: f32 = 8.0;

/// Розмір групи в `transmittance_main` — те саме, що в `[numthreads(8, 8, 1)]`.
const GROUP: u32 = 8;

/// Де стоїть камера відносно тіла з повітрям — усе, що прохід неба про неї знає.
///
/// Складається на CPU у `f64` і звужується один раз: віднімання центра тіла від
/// ока — те саме camera-relative, що й скрізь (F4). Осі екрана вже одиничні,
/// тангенси півкутів огляду приходять поруч, і разом вони дають промінь пікселя
/// без жодної оберненої матриці.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// Камера відносно центра тіла, метри, світові осі.
    pub eye: [f64; 3],
    /// Напрямок ДО Сонця, світові осі, одиничний.
    pub sun: [f32; 3],
    /// Осі екрана у світових координатах.
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub forward: [f32; 3],
    /// Тангенси півкутів огляду: горизонтального й вертикального.
    pub tan_half: [f32; 2],
}

impl View {
    /// Відстань камери від центра тіла.
    pub fn radius(&self) -> f64 {
        let e = self.eye;
        (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt()
    }

    /// Косинус зенітного кута Сонця в точці камери.
    pub fn sun_zenith_cos(&self) -> f64 {
        let r = self.radius().max(1.0);
        let e = self.eye;
        (e[0] * f64::from(self.sun[0])
            + e[1] * f64::from(self.sun[1])
            + e[2] * f64::from(self.sun[2]))
            / r
    }
}

pub struct Sky {
    transmittance_pipeline: wgpu::ComputePipeline,
    multiscatter_pipeline: wgpu::ComputePipeline,
    skyview_pipeline: wgpu::ComputePipeline,
    /// Два пайплайни, а не гілка в шейдері: камера всередині повітря читає
    /// таблицю, камера поза ним марширує. Вибір робить CPU — те саме рішення,
    /// що з гладким тілом і тілом з рельєфом (R5c).
    inside_pipeline: wgpu::RenderPipeline,
    outside_pipeline: wgpu::RenderPipeline,
    /// Аеральна перспектива (S5): об'єм у compute, композиція двома викликами.
    aerial_pipeline: wgpu::ComputePipeline,
    multiply_pipeline: wgpu::RenderPipeline,
    add_pipeline: wgpu::RenderPipeline,
    write_aerial: wgpu::BindGroup,
    /// Група композиції. Перестворюється разом із буфером глибини — вона
    /// тримає посилання на нього.
    composite: Option<Composite>,
    composite_layout: wgpu::BindGroupLayout,
    pass_buffer: wgpu::Buffer,
    /// Група 0 для малювання: обидві сталі таблиці, таблиця неба й параметри
    /// кадру, і все це видиме фрагментній стадії.
    read_draw: wgpu::BindGroup,

    /// Група 0 без самої таблиці пропускання — для проходу, який її пише.
    read_min: wgpu::BindGroup,
    /// Група 0 з таблицею пропускання — для всіх, хто її читає.
    read_full: wgpu::BindGroup,
    /// Група 0 з обома сталими таблицями — для того, хто рахує небо щокадру.
    read_frame: wgpu::BindGroup,
    /// Група 1 кожного проходу: рівно те, що він пише.
    write_transmittance: wgpu::BindGroup,
    write_multiscatter: wgpu::BindGroup,
    write_skyview: wgpu::BindGroup,

    air_buffer: wgpu::Buffer,
    view_buffer: wgpu::Buffer,
    sampler: wgpu::Sampler,

    transmittance: wgpu::Texture,
    multiscatter: wgpu::Texture,
    skyview: wgpu::Texture,
    skyview_view: wgpu::TextureView,
    aerial_inscatter_view: wgpu::TextureView,
    aerial_transmittance_view: wgpu::TextureView,

    /// Параметри, під які таблиці вже пораховані: саме повітря і радіус
    /// поверхні тіла, якому воно належить.
    ///
    /// Радіус окремо, бо в [`Atmosphere`] його немає: там лише верхня межа.
    /// Два тіла з однаковим повітрям і різними радіусами — різні атмосфери, і
    /// ключ мусить це бачити.
    current: Option<(Atmosphere, f64, [u32; 3])>,
}

/// Група композиції разом із розміром буфера глибини, під який її зроблено.
///
/// Окремою структурою, бо вона єдина в [`Sky`] залежить від розміру цілі:
/// глибина перестворюється при зміні розміру вікна, а bind-група тримає
/// посилання на неї.
struct Composite {
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

/// Опис одного запису макета — щоб чотири майже однакові макети не займали
/// сторінку.
fn storage_2d(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format: LUT_FORMAT,
            view_dimension: wgpu::TextureViewDimension::D2,
        },
        count: None,
    }
}

/// Таблиця на читання: `float4`, білінійна фільтрація.
fn sampled_2d_for(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn sampled_2d(binding: u32) -> wgpu::BindGroupLayoutEntry {
    sampled_2d_for(binding, wgpu::ShaderStages::COMPUTE)
}

/// Об'єм на запис.
fn storage_3d(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format: LUT_FORMAT,
            view_dimension: wgpu::TextureViewDimension::D3,
        },
        count: None,
    }
}

/// Об'єм на читання, з тривимірною фільтрацією.
fn sampled_3d(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D3,
            multisampled: false,
        },
        count: None,
    }
}

/// Об'єм аеральної перспективи: 32×32×32.
fn aerial_texture(gpu: &Gpu, label: &str) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: atmosphere::AERIAL_SIZE,
            height: atmosphere::AERIAL_SIZE,
            depth_or_array_layers: atmosphere::AERIAL_SIZE,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: LUT_FORMAT,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

/// Текстура таблиці: пишеться compute, читається шейдерами, читається назад
/// перевіркою.
///
/// `COPY_SRC` тут заради оракула, і це не приховується: перевірки етапу S —
/// звірка таблиці з `engine::atmosphere`, а прочитати її можна лише звідси. Та
/// сама причина, що в `indirect_buffer` (R6b).
fn lut_texture(gpu: &Gpu, label: &str, width: u32, height: u32) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: LUT_FORMAT,
        usage: wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

impl Sky {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Sky {
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("sky"),
                source: wgpu::ShaderSource::Wgsl(SKY_WGSL.into()),
            });

        let air_entry = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: std::num::NonZeroU64::new(AIR_BYTES),
            },
            count: None,
        };
        let sampler_entry = wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };

        let read_min_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("sky read (no luts)"),
                    entries: &[air_entry, sampler_entry],
                });
        let read_full_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("sky read"),
                    entries: &[air_entry, sampler_entry, sampled_2d(2)],
                });
        let view_entry = wgpu::BindGroupLayoutEntry {
            binding: 4,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: std::num::NonZeroU64::new(VIEW_BYTES),
            },
            count: None,
        };
        let read_frame_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("sky read (frame)"),
                    entries: &[
                        air_entry,
                        sampler_entry,
                        sampled_2d(2),
                        sampled_2d(3),
                        view_entry,
                    ],
                });
        let write_transmittance_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("sky write transmittance"),
                    entries: &[storage_2d(0)],
                });
        let write_multiscatter_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("sky write multiscatter"),
                    entries: &[storage_2d(1)],
                });
        // Малювання: та сама група 0, але видима ФРАГМЕНТНІЙ стадії й з
        // таблицею неба замість слота, у який її пишуть.
        let fragment = wgpu::ShaderStages::FRAGMENT;
        let read_draw_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("sky read (draw)"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            visibility: fragment,
                            ..air_entry
                        },
                        wgpu::BindGroupLayoutEntry {
                            visibility: fragment,
                            ..sampler_entry
                        },
                        sampled_2d_for(2, fragment),
                        sampled_2d_for(3, fragment),
                        wgpu::BindGroupLayoutEntry {
                            visibility: fragment,
                            ..view_entry
                        },
                        sampled_2d_for(5, fragment),
                    ],
                });
        let write_skyview_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("sky write skyview"),
                    entries: &[storage_2d(2)],
                });
        let write_aerial_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("sky write aerial"),
                    entries: &[storage_3d(3), storage_3d(4)],
                });

        // Композиція читає глибину як текстуру, обидва об'єми й параметри
        // діапазону. Повітря їй не потрібне взагалі: вона нічого не рахує, лише
        // вибирає з уже порахованого, і макет це показує — `air` тут немає.
        let composite_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("sky composite"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            visibility: fragment,
                            ..sampler_entry
                        },
                        wgpu::BindGroupLayoutEntry {
                            visibility: fragment,
                            ..view_entry
                        },
                        sampled_3d(6),
                        sampled_3d(7),
                        wgpu::BindGroupLayoutEntry {
                            binding: 8,
                            visibility: fragment,
                            ty: wgpu::BindingType::Texture {
                                // Глибина читається `textureLoad`, без
                                // фільтрації: проміжне значення між двома
                                // поверхнями не належить жодній.
                                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 9,
                            visibility: fragment,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                // Зсув на діапазон глибини, а не буфер на
                                // діапазон — те саме рішення, що в патчів (R4a).
                                has_dynamic_offset: true,
                                min_binding_size: std::num::NonZeroU64::new(PASS_BYTES),
                            },
                            count: None,
                        },
                    ],
                });

        let compute = |label: &str,
                       read: &wgpu::BindGroupLayout,
                       write: &wgpu::BindGroupLayout,
                       entry: &str| {
            let layout = gpu
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some(label),
                    bind_group_layouts: &[Some(read), Some(write)],
                    immediate_size: 0,
                });
            gpu.device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(label),
                    layout: Some(&layout),
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                })
        };
        let transmittance_pipeline = compute(
            "transmittance",
            &read_min_layout,
            &write_transmittance_layout,
            "transmittance_main",
        );
        let multiscatter_pipeline = compute(
            "multiscatter",
            &read_full_layout,
            &write_multiscatter_layout,
            "multiscatter_main",
        );
        let skyview_pipeline = compute(
            "skyview",
            &read_frame_layout,
            &write_skyview_layout,
            "skyview_main",
        );
        let aerial_pipeline = compute(
            "aerial",
            &read_frame_layout,
            &write_aerial_layout,
            "aerial_main",
        );

        // Прохід неба малює повноекранний трикутник без вершинних буферів і без
        // запису глибини: він іде першим у найдальшому діапазоні, і все, що
        // після нього, лягає зверху за звичайним тестом глибини.
        let draw_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("sky draw"),
                bind_group_layouts: &[Some(&read_draw_layout)],
                immediate_size: 0,
            });
        let draw = |label: &str, entry: &str| {
            gpu.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&draw_layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("vertex_sky"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            // **Додавання, а не заміщення.** Повітря світиться,
                            // а не закриває: те, що за ним, лишається видимим.
                            // Заміщення видно було одразу — нічний край лімба
                            // з орбіти вигризав із фону чорну дугу, бо там
                            // розсіювати нема чого, і нуль ставав кольором.
                            //
                            // Повна композиція — `фон·T + L`, тобто фон іще й
                            // гаситься повітрям. Другий множник з'явиться разом
                            // з аеральною перспективою (S5), і саме там він
                            // потрібен: поки за небом немає нічого, крім кольору
                            // очищення, гасити нема чого.
                            blend: Some(wgpu::BlendState {
                                color: wgpu::BlendComponent {
                                    src_factor: wgpu::BlendFactor::One,
                                    dst_factor: wgpu::BlendFactor::One,
                                    operation: wgpu::BlendOperation::Add,
                                },
                                alpha: wgpu::BlendComponent::REPLACE,
                            }),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState {
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: crate::depth::FORMAT,
                        depth_write_enabled: Some(false),
                        depth_compare: Some(wgpu::CompareFunction::Always),
                        stencil: wgpu::StencilState::default(),
                        bias: wgpu::DepthBiasState::default(),
                    }),
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                })
        };
        let inside_pipeline = draw("sky inside", "fragment_sky_inside");
        let outside_pipeline = draw("sky outside", "fragment_sky_outside");

        // Композиція малює в кадр без буфера глибини взагалі: вона читає його
        // як текстуру, а бути одночасно ціллю й ресурсом та сама текстура не
        // може.
        let composite_pipeline_layout =
            gpu.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("sky composite"),
                    bind_group_layouts: &[Some(&composite_layout)],
                    immediate_size: 0,
                });
        let composite_draw = |label: &str, entry: &str, blend: wgpu::BlendState| {
            gpu.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&composite_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("vertex_sky"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: Some(blend),
                            write_mask: wgpu::ColorWrites::COLOR,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState {
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                })
        };
        // `dst · T`: джерело множиться на нуль, ціль — на джерело.
        let multiply_pipeline = composite_draw(
            "aerial multiply",
            "fragment_aerial_multiply",
            wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Zero,
                    dst_factor: wgpu::BlendFactor::Src,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::REPLACE,
            },
        );
        // `dst + L`.
        let add_pipeline = composite_draw(
            "aerial add",
            "fragment_aerial_add",
            wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::REPLACE,
            },
        );

        let air_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("air params"),
            size: AIR_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("view params"),
            size: VIEW_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let pass_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("depth range params"),
            size: PASS_STRIDE * crate::frame::MAX_PASSES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Затискання по краях, а не повтор: таблиця — це функція, визначена на
        // відрізку, і за його межами продовжувати її колом означало б читати
        // зеніт замість горизонту.
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sky luts"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let transmittance = lut_texture(
            gpu,
            "transmittance lut",
            atmosphere::TRANSMITTANCE_WIDTH,
            atmosphere::TRANSMITTANCE_HEIGHT,
        );
        let multiscatter = lut_texture(
            gpu,
            "multiscatter lut",
            atmosphere::MULTISCATTER_SIZE,
            atmosphere::MULTISCATTER_SIZE,
        );
        let skyview = lut_texture(
            gpu,
            "skyview lut",
            atmosphere::SKYVIEW_WIDTH,
            atmosphere::SKYVIEW_HEIGHT,
        );
        let transmittance_view = transmittance.create_view(&wgpu::TextureViewDescriptor::default());
        let multiscatter_view = multiscatter.create_view(&wgpu::TextureViewDescriptor::default());
        let skyview_view = skyview.create_view(&wgpu::TextureViewDescriptor::default());
        let aerial_inscatter = aerial_texture(gpu, "aerial inscatter");
        let aerial_transmittance = aerial_texture(gpu, "aerial transmittance");
        let aerial_inscatter_view =
            aerial_inscatter.create_view(&wgpu::TextureViewDescriptor::default());
        let aerial_transmittance_view =
            aerial_transmittance.create_view(&wgpu::TextureViewDescriptor::default());

        let read_min = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky read (no luts)"),
            layout: &read_min_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: air_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let read_full = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky read"),
            layout: &read_full_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: air_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&transmittance_view),
                },
            ],
        });
        let read_frame = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky read (frame)"),
            layout: &read_frame_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: air_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&transmittance_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&multiscatter_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: view_buffer.as_entire_binding(),
                },
            ],
        });
        let write_transmittance = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky write transmittance"),
            layout: &write_transmittance_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&transmittance_view),
            }],
        });
        let write_multiscatter = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky write multiscatter"),
            layout: &write_multiscatter_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&multiscatter_view),
            }],
        });

        let read_draw = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky read (draw)"),
            layout: &read_draw_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: air_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&transmittance_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&multiscatter_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: view_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&skyview_view),
                },
            ],
        });
        let write_skyview = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky write skyview"),
            layout: &write_skyview_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&skyview_view),
            }],
        });

        let write_aerial = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky write aerial"),
            layout: &write_aerial_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&aerial_inscatter_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&aerial_transmittance_view),
                },
            ],
        });

        Sky {
            transmittance_pipeline,
            multiscatter_pipeline,
            skyview_pipeline,
            inside_pipeline,
            outside_pipeline,
            aerial_pipeline,
            multiply_pipeline,
            add_pipeline,
            write_aerial,
            composite: None,
            composite_layout,
            pass_buffer,
            read_draw,
            read_min,
            read_full,
            read_frame,
            write_transmittance,
            write_multiscatter,
            write_skyview,
            air_buffer,
            view_buffer,
            sampler,
            transmittance,
            multiscatter,
            skyview,
            skyview_view,
            aerial_inscatter_view,
            aerial_transmittance_view,
            current: None,
        }
    }

    /// Таблиці під це повітря — порахувати, якщо вони ще не під нього.
    ///
    /// Повертає `true`, якщо рахувати таки довелося. Значення потрібне не
    /// кадру, а перевірці: «таблиці не перераховуються щокадру» — твердження,
    /// яке треба вміти перевірити, а не лише написати в коментарі.
    ///
    /// Порядок проходів тут — залежність за даними: розсіювання читає
    /// пропускання. Бар'єр між ними ставить wgpu сам, за використанням
    /// ресурсів; окремі проходи потрібні лише тому, що всередині одного
    /// проходу порядок груп не гарантований.
    pub fn ensure(&mut self, gpu: &Gpu, air: &Atmosphere, bottom_m: f64, albedo: [f32; 3]) -> bool {
        // Альбедо входить у ключ разом із повітрям: воно міняє **таблиці**, а
        // не кадр, тож перебудова мусить статися рівно тоді, коли воно
        // змінилось. Порівняння бітове — це вхід, а не результат виміру.
        let key = (*air, bottom_m, albedo.map(f32::to_bits));
        if self.current == Some(key) {
            return false;
        }

        gpu.queue
            .write_buffer(&self.air_buffer, 0, &air_bytes(air, bottom_m, albedo));

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("sky luts"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("transmittance"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.transmittance_pipeline);
            pass.set_bind_group(0, &self.read_min, &[]);
            pass.set_bind_group(1, &self.write_transmittance, &[]);
            pass.dispatch_workgroups(
                atmosphere::TRANSMITTANCE_WIDTH.div_ceil(GROUP),
                atmosphere::TRANSMITTANCE_HEIGHT.div_ceil(GROUP),
                1,
            );
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("multiscatter"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.multiscatter_pipeline);
            pass.set_bind_group(0, &self.read_full, &[]);
            pass.set_bind_group(1, &self.write_multiscatter, &[]);
            let groups = atmosphere::MULTISCATTER_SIZE.div_ceil(GROUP);
            pass.dispatch_workgroups(groups, groups, 1);
        }
        gpu.queue.submit([encoder.finish()]);

        self.current = Some(key);
        true
    }

    /// Небо під цю камеру — **у чужий encoder**, бо це робота кадру.
    ///
    /// Різниця з [`Sky::ensure`] тут головна й видима з підпису: сталі таблиці
    /// подають роботу самі й майже ніколи, а ця йде туди ж, куди й проходи
    /// кадру, тобто щокадру. Хто прийде оптимізувати, побачить це з коду.
    pub fn prepare_view(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, view: &View) {
        // Глибину об'єму аеральної перспективи рахуємо тут, а не в кадрі: вона
        // залежить від повітря, а повітря знає лише `Sky`. Кадру довелося б
        // тягнути ту саму формулу другим примірником.
        let span = match self.current {
            Some((air, bottom, _)) => atmosphere::aerial_span(&air, bottom, view.radius()),
            None => (0.0, 1.0),
        };
        gpu.queue
            .write_buffer(&self.view_buffer, 0, &view_bytes(view, span));

        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("skyview"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.skyview_pipeline);
        pass.set_bind_group(0, &self.read_frame, &[]);
        pass.set_bind_group(1, &self.write_skyview, &[]);
        pass.dispatch_workgroups(
            atmosphere::SKYVIEW_WIDTH.div_ceil(GROUP),
            atmosphere::SKYVIEW_HEIGHT.div_ceil(GROUP),
            1,
        );
    }

    /// Намалювати небо повноекранним трикутником.
    ///
    /// `inside` вирішує викликач — він знає, де верхня межа повітря; вибір
    /// пайплайна на CPU, а не гілка в шейдері, з тієї самої причини, що в
    /// патчів (R5c).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, inside: bool) {
        pass.set_pipeline(if inside {
            &self.inside_pipeline
        } else {
            &self.outside_pipeline
        });
        pass.set_bind_group(0, &self.read_draw, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Об'єм аеральної перспективи під цю камеру — теж у чужий encoder.
    ///
    /// Кличеться лише тоді, коли повітря в кадрі справді видно: умову рахує
    /// викликач ([`crate::frame::Frame`]), бо вона про кадр, а не про повітря.
    pub fn prepare_aerial(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("aerial"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.aerial_pipeline);
        pass.set_bind_group(0, &self.read_frame, &[]);
        pass.set_bind_group(1, &self.write_aerial, &[]);
        // По потоку на промінь, не на тексель: шари одного стовпця лежать на
        // одному промені й рахуються одним проходом уздовж нього.
        let groups = atmosphere::AERIAL_SIZE.div_ceil(GROUP);
        pass.dispatch_workgroups(groups, groups, 1);
    }

    /// Група композиції під цей буфер глибини — створити, якщо розмір змінився.
    ///
    /// Окремо від решти груп рівно тому, що вона єдина залежить від розміру
    /// цілі: глибина перестворюється при зміні вікна, а група тримає посилання
    /// на неї.
    pub fn bind_depth(&mut self, gpu: &Gpu, depth: &wgpu::TextureView, width: u32, height: u32) {
        if let Some(composite) = &self.composite {
            if composite.width == width && composite.height == height {
                return;
            }
        }
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky composite"),
            layout: &self.composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: self.view_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&self.aerial_inscatter_view),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&self.aerial_transmittance_view),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(depth),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.pass_buffer,
                        offset: 0,
                        size: std::num::NonZeroU64::new(PASS_BYTES),
                    }),
                },
            ],
        });
        self.composite = Some(Composite {
            bind_group,
            width,
            height,
        });
    }

    /// Записати, як діапазон глибини `index` перетворює `z_ndc` назад у метри.
    ///
    /// `a` і `b` — коефіцієнти `z_ndc = −A + B/z`; звідки вони беруться,
    /// написано в `crate::depth`, а рахує їх кадр: він єдиний знає межі
    /// діапазонів.
    pub fn set_range(&self, gpu: &Gpu, index: usize, a: f64, b: f64) {
        let mut bytes = Vec::with_capacity(PASS_BYTES as usize);
        for value in [a as f32, b as f32, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        gpu.queue
            .write_buffer(&self.pass_buffer, index as u64 * PASS_STRIDE, &bytes);
    }

    /// Композиція: `кадр · T + L` двома викликами.
    ///
    /// Двома, а не одним: за один прохід це вимагало б dual-source blending —
    /// ще однієї фічі пристрою й `@blend_src` у WGSL, якого компілятор Slang не
    /// друкує. Два повноекранні трикутники коштують менше, ніж залежність від
    /// того й того.
    pub fn composite(&self, pass: &mut wgpu::RenderPass<'_>, index: usize) {
        let Some(composite) = &self.composite else {
            return;
        };
        let offset = (index as u64 * PASS_STRIDE) as u32;
        pass.set_pipeline(&self.multiply_pipeline);
        pass.set_bind_group(0, &composite.bind_group, &[offset]);
        pass.draw(0..3, 0..1);
        pass.set_pipeline(&self.add_pipeline);
        pass.set_bind_group(0, &composite.bind_group, &[offset]);
        pass.draw(0..3, 0..1);
    }

    /// Вигляд таблиці неба — для того, хто малюватиме нею кадр (S4b).
    pub fn skyview_view(&self) -> &wgpu::TextureView {
        &self.skyview_view
    }

    /// Таблиця неба назад у пам'ять — оракул S4.
    pub fn read_skyview(&self, gpu: &Gpu) -> Result<Vec<[f32; 4]>, String> {
        read_lut(
            gpu,
            &self.skyview,
            atmosphere::SKYVIEW_WIDTH,
            atmosphere::SKYVIEW_HEIGHT,
        )
    }

    /// Таблиця пропускання назад у пам'ять — оракул S2.
    pub fn read_transmittance(&self, gpu: &Gpu) -> Result<Vec<[f32; 4]>, String> {
        read_lut(
            gpu,
            &self.transmittance,
            atmosphere::TRANSMITTANCE_WIDTH,
            atmosphere::TRANSMITTANCE_HEIGHT,
        )
    }

    /// Таблиця багаторазового розсіювання назад у пам'ять — оракул S3.
    ///
    /// RGB — `ψ`, альфа — найбільший канал `f`, тобто те число, від якого
    /// залежить збіжність ряду.
    pub fn read_multiscatter(&self, gpu: &Gpu) -> Result<Vec<[f32; 4]>, String> {
        read_lut(
            gpu,
            &self.multiscatter,
            atmosphere::MULTISCATTER_SIZE,
            atmosphere::MULTISCATTER_SIZE,
        )
    }
}

/// Таблиця з GPU назад у пам'ять.
///
/// Читати це в кадрі не можна: тут `poll(Wait)`, тобто повна зупинка
/// конвеєра. Існує рівно заради перевірки, як і
/// [`crate::frame::Frame::drawn_patches`].
///
/// Рядок-major, `[r, g, b, a]` на тексель, уже розпакований із half-float.
fn read_lut(
    gpu: &Gpu,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Vec<[f32; 4]>, String> {
    // Вісім байтів на тексель; і 256, і 32 текселі в рядку дають кратне 256,
    // тобто вирівнювання `copy_texture_to_buffer` виконується саме собою й
    // окремого доповнення не треба. Перевіряється, а не мається на увазі:
    // наступна таблиця може виявитися іншої ширини.
    let bytes_per_row = width * 8;
    assert_eq!(
        bytes_per_row % 256,
        0,
        "рядок таблиці {width}×{height} не вирівняний на 256 байтів"
    );

    let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lut readback"),
        size: u64::from(bytes_per_row * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("lut readback"),
        });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);

    let slice = staging.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    gpu.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .map_err(|e| format!("не дочекалися GPU: {e}"))?;
    let data = slice
        .get_mapped_range()
        .map_err(|e| format!("буфер не відобразився: {e}"))?;

    let mut out = Vec::with_capacity((width * height) as usize);
    for texel in data.chunks_exact(8) {
        let mut rgba = [0.0f32; 4];
        for (channel, half) in rgba.iter_mut().zip(texel.chunks_exact(2)) {
            *channel = from_half(u16::from_le_bytes([half[0], half[1]]));
        }
        out.push(rgba);
    }
    drop(data);
    staging.unmap();
    Ok(out)
}

/// Параметри повітря в розкладці `AirParams` із `sky.slang`.
///
/// Виписано руками з тієї самої причини, що й `Uniforms::to_bytes` у кадрі:
/// наш `unsafe` живе лише в `core-rs` (CLAUDE.md, інваріант 1).
///
/// **Радіуси звужуються до `f32` тут, і це не те звуження, якого бояться.**
/// Правило «світові координати ніколи не в float» (F4) стосується позицій, у
/// яких камера віднімається від великого числа; радіус тіла в неї не входить
/// — з нього рахують висоту над поверхнею, а `6.371·10⁶` у `f32` має крок
/// 0.5 м, тобто помилку в шістнадцять мільйонних від висоти шкали.
fn air_bytes(air: &Atmosphere, bottom_m: f64, albedo: [f32; 3]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(AIR_BYTES as usize);
    let mut push = |values: [f32; 4]| {
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    };
    push([
        air.rayleigh_scattering[0],
        air.rayleigh_scattering[1],
        air.rayleigh_scattering[2],
        air.rayleigh_height_m,
    ]);
    push([
        air.mie_scattering,
        air.mie_absorption,
        air.mie_height_m,
        air.mie_g,
    ]);
    push([
        air.ozone_absorption[0],
        air.ozone_absorption[1],
        air.ozone_absorption[2],
        0.0,
    ]);
    push([
        air.ozone_centre_m,
        air.ozone_width_m,
        bottom_m as f32,
        air.top_m as f32,
    ]);
    // Середнє альбедо поверхні під цим небом (T7h); `w` — запас.
    push([albedo[0], albedo[1], albedo[2], 0.0]);
    bytes
}

/// Параметри камери в розкладці `ViewParams` із `sky.slang`.
fn view_bytes(view: &View, aerial_span: (f64, f64)) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(VIEW_BYTES as usize);
    let mut push = |values: [f32; 4]| {
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    };
    push([
        view.radius() as f32,
        view.sun_zenith_cos() as f32,
        EXPOSURE,
        aerial_span.1 as f32,
    ]);
    push([
        view.eye[0] as f32,
        view.eye[1] as f32,
        view.eye[2] as f32,
        aerial_span.0 as f32,
    ]);
    push([view.sun[0], view.sun[1], view.sun[2], 0.0]);
    push([
        view.right[0],
        view.right[1],
        view.right[2],
        view.tan_half[0],
    ]);
    push([view.up[0], view.up[1], view.up[2], view.tan_half[1]]);
    push([view.forward[0], view.forward[1], view.forward[2], 0.0]);
    bytes
}

/// Half-float у `f32`.
///
/// Десять рядків замість залежності: `half` уже є в дереві транзитивно, але
/// прямою залежністю рушія стала б заради однієї функції, потрібної лише
/// перевірці. Формат IEEE 754 binary16 описаний повністю — знак, п'ять бітів
/// порядку зі зсувом 15, десять бітів мантиси.
fn from_half(bits: u16) -> f32 {
    let exponent = (bits >> 10) & 0x1f;
    let mantissa = bits & 0x3ff;
    let magnitude = match exponent {
        // Нуль і субнормальні: значення `mantissa · 2⁻²⁴`.
        0 => f32::from(mantissa) * (1.0 / 16_777_216.0),
        // Нескінченність і NaN — у таблиці їх бути не може, але мовчки
        // перетворити їх на скінченне число означало б сховати помилку.
        0x1f if mantissa == 0 => f32::INFINITY,
        0x1f => f32::NAN,
        _ => f32::from_bits((u32::from(exponent) + (127 - 15)) << 23 | u32::from(mantissa) << 13),
    };
    if bits & 0x8000 != 0 {
        -magnitude
    } else {
        magnitude
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Розпакування half-float — на числах, які легко перевірити руками.
    #[test]
    fn half_floats_unpack_to_the_numbers_they_encode() {
        assert_eq!(from_half(0x0000), 0.0);
        assert_eq!(from_half(0x3c00), 1.0);
        assert_eq!(from_half(0x4000), 2.0);
        assert_eq!(from_half(0xc000), -2.0);
        assert_eq!(from_half(0x3800), 0.5);
        // Найменше нормальне: 2⁻¹⁴.
        assert_eq!(from_half(0x0400), 2.0f32.powi(-14));
        // Найбільше субнормальне: (1023/1024)·2⁻¹⁴.
        assert!((from_half(0x03ff) - 1023.0 / 1024.0 * 2.0f32.powi(-14)).abs() < 1.0e-12);
        assert!(from_half(0x7c00).is_infinite());
        assert!(from_half(0x7e00).is_nan());
    }

    /// Розкладка `AirParams` — та сама, що в шейдері: двадцять чисел,
    /// і кожне на своєму місці.
    #[test]
    fn the_air_params_land_where_the_shader_reads_them() {
        let air = Atmosphere::EARTH;
        // Альбедо навмисно різне по каналах: однакове не відрізнило б
        // переставлені місцями компоненти.
        let bytes = air_bytes(&air, 6_371_000.0, [0.11, 0.22, 0.33]);
        assert_eq!(bytes.len() as u64, AIR_BYTES);

        let word = |k: usize| f32::from_le_bytes(bytes[k * 4..k * 4 + 4].try_into().unwrap());
        assert_eq!(word(0), air.rayleigh_scattering[0]);
        assert_eq!(word(3), air.rayleigh_height_m);
        assert_eq!(word(4), air.mie_scattering);
        assert_eq!(word(7), air.mie_g);
        assert_eq!(word(8), air.ozone_absorption[0]);
        assert_eq!(word(12), air.ozone_centre_m);
        assert_eq!(word(14), 6_371_000.0);
        assert_eq!(word(15), air.top_m as f32);
        assert_eq!(word(16), 0.11);
        assert_eq!(word(17), 0.22);
        assert_eq!(word(18), 0.33);
    }
}
