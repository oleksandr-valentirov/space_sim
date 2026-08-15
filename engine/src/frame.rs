//! Що саме малюється. Про вікно тут не знають нічого — лише про текстуру,
//! у яку писати, її формат і розмір.
//!
//! Формат у конструкторі, а не в `draw`, бо пайплайн прив'язаний до формату
//! цілі. Вікно й знімок мають різні формати, тож у них різні [`Frame`] —
//! і це та сама причина, з якої вони дають трохи різні пікселі: поверхня
//! вікна sRGB, ціль знімка лінійна. Кольори в шейдері однакові; те, що з
//! ними робить апаратура на записі, — ні.
//!
//! ## Чому тут сфера, а не трикутник (ROADMAP I1)
//!
//! До I1 кадр малював трикутник F2, а все виміряне на етапі F — reversed-Z,
//! camera-relative, реальний масштаб — жило в окремих самодостатніх шляхах
//! (`depth_probe`, `sphere_render`, `flight_probe`), кожен зі своїм
//! пайплайном і своєю depth-текстурою. Тобто жодна з цих властивостей не
//! була перевірена на тому шляху, яким кадр справді потрапляє у вікно, а
//! `--perf-probe` міряв стелю синхронізації, а не сцену.
//!
//! Ті шляхи лишаються: вони міряють по одному твердженню кожен і роблять це
//! краще, ніж міг би інтерактивний кадр. Спільним у них лишається все, що
//! робить арифметику: `sphere::Mesh`, `camera::Camera`, `depth`, той самий
//! `sphere.wgsl`.

use crate::camera::Camera;
use crate::cubesphere::{self, Patch};
use crate::depth;
use crate::gpu::Gpu;
use crate::scene::Scene;
use crate::sphere;

/// Колір очищення. Не чорний навмисно: чорний кадр і кадр, якого не було,
/// виглядають однаково, і перевірка «щось намалювалось» на чорному нічого
/// не варта.
pub const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.02,
    g: 0.03,
    b: 0.08,
    a: 1.0,
};

/// Той самий колір у байтах — для звірки знімка (лінійна ціль).
pub const CLEAR_BYTES: [u8; 3] = [5, 8, 20];

/// WGSL, згенерований зі `shaders/sphere.slang`.
///
/// Вбудовується в бінарник, а не читається з диска: шейдер — частина
/// програми, а не ассет, який можна підмінити. Генерується
/// `scripts/build_shaders.sh` і комітиться (ROADMAP F2).
const PATCH_WGSL: &str = include_str!("../shaders/patch.wgsl");

/// Те саме для ламаних (ROADMAP J1).
const LINE_WGSL: &str = include_str!("../shaders/line.wgsl");

pub const FOV_Y: f64 = std::f64::consts::PI / 3.0;

const LIGHT_DIR: [f32; 3] = [0.4, 0.4, 0.82];
const COLOUR: [f32; 4] = [0.2, 0.6, 0.9, 1.0];

/// Висота камери за замовчуванням, метри над поверхнею.
///
/// 10⁷ м — єдина точка з таблиці F5, де диск силуету цілком влазить у кадр
/// (при 60° поля зору сфера заповнює все аж до ~3·10⁶ м). Отже кадр за
/// замовчуванням — той, у якому покриття можна звірити з аналітичною
/// формулою, а не лише сказати «видно сферу».
pub const DEFAULT_ALTITUDE_M: f64 = 1.0e7;

/// Погляд на планету з [`DEFAULT_ALTITUDE_M`], уздовж осі x.
///
/// Та сама геометрія, що в [`crate::flight_probe`], і це навмисно: знімок
/// без вікна перевіряється тим самим оракулом
/// [`crate::flight_probe::expected_coverage`].
pub fn default_camera() -> Camera {
    let distance = sphere::EARTH_RADIUS_M + DEFAULT_ALTITUDE_M;
    Camera::look_at([distance, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0])
}

#[derive(Clone, Copy)]
struct Uniforms {
    projection: depth::Matrix,
    light_dir: [f32; 4],
    colour: [f32; 4],
}

impl Uniforms {
    /// Розкладка вручну — та сама причина, що в `sphere_render` (CLAUDE.md,
    /// інваріант 1: наш `unsafe` живе лише в `core-rs`, тут його й не треба).
    fn to_bytes(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(96);
        for column in self.projection {
            for value in column {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        for value in self.light_dir.iter().chain(self.colour.iter()) {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}

/// Буфер глибини разом із розміром, під який його зроблено.
struct Depth {
    view: wgpu::TextureView,
    width: u32,
    height: u32,
}

/// Пайплайн ламаних і буфери під них (ROADMAP J1).
///
/// Місткість росте за потребою й не спадає: прогноз довшає з кожним тіком, і
/// перевиділяти буфер щоразу означало б робити це щокадру.
struct Lines {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,

    position_buffer: wgpu::Buffer,
    colour_buffer: wgpu::Buffer,
    capacity: usize,

    position_bytes: Vec<u8>,
    colour_bytes: Vec<u8>,
}

pub struct Frame {
    /// Планета патчами (ROADMAP-PLANETS.md, R1d).
    planet: Planet,

    /// Створюється при першому кадрі й перестворюється, коли змінився
    /// розмір цілі. Живе тут, а не в `app`, з однієї причини: інакше та сама
    /// логіка була б і в [`crate::shot`], а два місця розходяться.
    depth: Option<Depth>,

    lines: Lines,
}

impl Frame {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Frame {
        Frame {
            planet: Planet::new(gpu, format),
            depth: None,
            lines: Lines::new(gpu, format),
        }
    }

    /// Ближня площина під поточну висоту камери.
    ///
    /// Не стала: камера рухається від поверхні до орбіти, а F5 показав, що
    /// near мусить лишати запас під найближчу вершину меша, а не впиратися
    /// в неї. Десята частина висоти дає той самий порядок запасу, що F5
    /// узяв руками (near = 1 м при прольоті на 10 м).
    ///
    /// Роздільність глибини від near не залежить узагалі — це виміряно на
    /// F3 (`Δz ≈ z·6·10⁻⁸`, near скорочується), тож тут немає чого
    /// підбирати заради z-fighting.
    fn near_for(&self, camera: &Camera) -> f64 {
        let p = camera.position();
        let distance = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        let altitude = distance - sphere::EARTH_RADIUS_M;
        (altitude / 10.0).max(0.1)
    }

    fn ensure_depth(&mut self, gpu: &Gpu, width: u32, height: u32) {
        if let Some(depth) = &self.depth {
            if depth.width == width && depth.height == height {
                return;
            }
        }

        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frame depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: depth::FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });

        self.depth = Some(Depth {
            view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
            width,
            height,
        });
    }

    /// Записує в `encoder` усе, що складає кадр.
    ///
    /// Один метод, а не пара «оновити / записати»: розділені, вони дають
    /// спосіб забути перше й отримати кадр з учорашньою камерою або з
    /// depth-текстурою чужого розміру. Тут це неможливо.
    ///
    /// Що саме малювати, каже [`Scene`] — і це вся межа між рушієм і грою
    /// (PROJECT.md §6). Сфера поки не в сцені, а тут: у ассеті немає радіусів
    /// тіл, тож перелічувати їх не з чого (`crate::scene`).
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        scene: &Scene,
    ) {
        let camera = &scene.camera;
        self.ensure_depth(gpu, width, height);

        let aspect = f64::from(width) / f64::from(height);
        let projection = depth::reversed_infinite(FOV_Y, aspect, self.near_for(camera));

        // Планета: камера віднімається раз на патч, у `double`, а поворот
        // їде в матриці (R1d). Кількість роботи на CPU більше не залежить
        // від кількості вершин — тільки від кількості патчів.
        self.planet.upload(gpu, scene, projection);

        // Ламані проходять той самий шлях, що вершини сфери: віднімання й
        // поворот у double, звуження до f32 останнім кроком. Інакше
        // траєкторія за 4·10⁸ м від камери тремтіла б, а сфера поруч — ні.
        self.lines.upload(gpu, scene, projection);

        let depth = self.depth.as_ref().expect("ensure_depth щойно її створив");

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("frame"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(CLEAR),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(depth::CLEAR),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            multiview_mask: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        self.planet.draw(&mut pass);
        self.lines.draw(&mut pass, scene);
    }
}

/// Планета патчами кубосфери (ROADMAP-PLANETS.md, R1d).
///
/// Заміна UV-сфери, і суть заміни не в формі, а в тому, **хто рахує
/// camera-relative**. Було: CPU щокадру проганяв кожну з 8385 вершин через
/// `camera.relative`. Стало: CPU віднімає камеру раз на патч — шість чисел
/// на грань замість тисячі, — а зсув вершини всередині патча в `f32` уже
/// лежить у буфері й не переписується взагалі.
///
/// Поворот при цьому переїхав у шейдер, у ту саму матрицю, що й проєкція:
/// перенесення зробило віднімання на CPU в `double`, повороту байдуже до
/// масштабу (`camera::Camera::view_rotation`).
///
/// Початки патчів їдуть **storage-буфером**, а не масивом uniform-буферів:
/// D3D12 останніх не дає взагалі, і PROJECT.md §7 уже поклав per-object дані
/// саме сюди.
struct Planet {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    origin_buffer: wgpu::Buffer,

    offset_buffer: wgpu::Buffer,
    normal_buffer: wgpu::Buffer,
    patch_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,

    /// Початки патчів у світових координатах тіла — рахуються раз, бо форма
    /// планети не змінюється; камера віднімається від них щокадру.
    origins: Vec<[f64; 3]>,
    origin_bytes: Vec<u8>,
}

/// Рівень, на якому малюється планета. Без LOD — його приносить R2.
///
/// Нуль означає «патч на грань»: шість патчів, 32 відрізки на бік, тобто той
/// самий кутовий крок силуету, що в UV-сфери 64×128 (32 сегменти на 90°).
/// Саме тому знімок до й після можна звіряти маскою, а не «схожістю».
const PLANET_LEVEL: u32 = 0;

impl Planet {
    fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Planet {
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("patch"),
                source: wgpu::ShaderSource::Wgsl(PATCH_WGSL.into()),
            });

        // Шість граней на нульовому рівні; на рівні L їх 6·4^L.
        let side = 1u32 << PLANET_LEVEL;
        let mut patches = Vec::new();
        for face in 0..cubesphere::FACES {
            for i in 0..side {
                for j in 0..side {
                    patches.push(Patch {
                        face,
                        level: PLANET_LEVEL,
                        i,
                        j,
                    });
                }
            }
        }

        // Геометрія збирається раз: зсуви й нормалі від камери не залежать,
        // а індекс патча — тим паче.
        let mut offset_bytes = Vec::new();
        let mut normal_bytes = Vec::new();
        let mut patch_bytes = Vec::new();
        let mut index_bytes = Vec::new();
        let mut origins = Vec::new();
        let mut base: u32 = 0;

        for (index, patch) in patches.iter().enumerate() {
            let mesh = patch.mesh(sphere::EARTH_RADIUS_M);
            origins.push(mesh.origin);

            for (offset, normal) in mesh.offsets.iter().zip(mesh.normals.iter()) {
                for value in offset {
                    offset_bytes.extend_from_slice(&value.to_le_bytes());
                }
                for value in normal {
                    normal_bytes.extend_from_slice(&value.to_le_bytes());
                }
                patch_bytes.extend_from_slice(&(index as u32).to_le_bytes());
            }

            for i in &mesh.indices {
                index_bytes.extend_from_slice(&(base + i).to_le_bytes());
            }
            base += mesh.offsets.len() as u32;
        }

        let index_count = (index_bytes.len() / 4) as u32;

        let vertex_buffer = |label: &str, bytes: &[u8]| {
            let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes.len() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            gpu.queue.write_buffer(&buffer, 0, bytes);
            buffer
        };

        let offset_buffer = vertex_buffer("patch offsets", &offset_bytes);
        let normal_buffer = vertex_buffer("patch normals", &normal_bytes);
        let patch_buffer = vertex_buffer("patch indices", &patch_bytes);

        let index_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("patch elements"),
            size: index_bytes.len() as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&index_buffer, 0, &index_bytes);

        let uniform_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("patch uniforms"),
            size: 96,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Чотири числа на патч, а не три: вирівнювання vec4 у std430, і
        // четверте лишається нулем.
        let origin_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("patch origins"),
            size: (patches.len() * 16) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("patch"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("patch"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: origin_buffer.as_entire_binding(),
                },
            ],
        });

        let layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("patch"),
                bind_group_layouts: &[Some(&bind_layout)],
                immediate_size: 0,
            });

        let offset_attrs = [wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 0,
            shader_location: 0,
        }];
        let normal_attrs = [wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 0,
            shader_location: 1,
        }];
        let patch_attrs = [wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Uint32,
            offset: 0,
            shader_location: 2,
        }];

        let pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("patch"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vertex_main"),
                    compilation_options: Default::default(),
                    buffers: &[
                        Some(wgpu::VertexBufferLayout {
                            array_stride: 12,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &offset_attrs,
                        }),
                        Some(wgpu::VertexBufferLayout {
                            array_stride: 12,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &normal_attrs,
                        }),
                        Some(wgpu::VertexBufferLayout {
                            array_stride: 4,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &patch_attrs,
                        }),
                    ],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fragment_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    // Без відсікання граней — той самий вибір, що був у
                    // сфери: коректність тримається на тесті глибини, а не
                    // на вгаданому порядку обходу вершин.
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: depth::FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(depth::COMPARE),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });

        let origin_bytes = Vec::with_capacity(patches.len() * 16);

        Planet {
            pipeline,
            bind_group,
            uniform_buffer,
            origin_buffer,
            offset_buffer,
            normal_buffer,
            patch_buffer,
            index_buffer,
            index_count,
            origins,
            origin_bytes,
        }
    }

    /// Початки патчів відносно камери й матриця на цей кадр.
    ///
    /// Оце і є весь CPU-прохід планети: шість віднімань у `double` замість
    /// восьми з половиною тисяч.
    fn upload(&mut self, gpu: &Gpu, scene: &Scene, projection: depth::Matrix) {
        let camera = &scene.camera;
        let eye = camera.position();

        self.origin_bytes.clear();
        for origin in &self.origins {
            // Віднімання в `double`, звуження — останнім кроком, як завжди
            // (ROADMAP F4). Поворот тут НЕ робиться: він у матриці, бо його
            // однаково доведеться застосувати й до зсуву вершини.
            for k in 0..3 {
                let value = (origin[k] - eye[k]) as f32;
                self.origin_bytes.extend_from_slice(&value.to_le_bytes());
            }
            self.origin_bytes.extend_from_slice(&0.0f32.to_le_bytes());
        }
        gpu.queue
            .write_buffer(&self.origin_buffer, 0, &self.origin_bytes);

        let uniforms = Uniforms {
            projection: depth::multiply(projection, camera.view_rotation()),
            light_dir: [LIGHT_DIR[0], LIGHT_DIR[1], LIGHT_DIR[2], 0.0],
            colour: COLOUR,
        };
        gpu.queue
            .write_buffer(&self.uniform_buffer, 0, &uniforms.to_bytes());
    }

    fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.offset_buffer.slice(..));
        pass.set_vertex_buffer(1, self.normal_buffer.slice(..));
        pass.set_vertex_buffer(2, self.patch_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.index_count, 0, 0..1);
    }
}

impl Lines {
    fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Lines {
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("line"),
                source: wgpu::ShaderSource::Wgsl(LINE_WGSL.into()),
            });

        let bind_layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("line"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("line"),
                bind_group_layouts: &[Some(&bind_layout)],
                immediate_size: 0,
            });

        let position_attrs = [wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 0,
            shader_location: 0,
        }];
        let colour_attrs = [wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 0,
            shader_location: 1,
        }];

        let pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("line"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vertex_main"),
                    compilation_options: Default::default(),
                    buffers: &[
                        Some(wgpu::VertexBufferLayout {
                            array_stride: 12,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &position_attrs,
                        }),
                        Some(wgpu::VertexBufferLayout {
                            array_stride: 16,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &colour_attrs,
                        }),
                    ],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fragment_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::LineStrip,
                    ..Default::default()
                },
                // Глибина спільна зі сферою, і запис теж увімкнений: ділянка
                // траєкторії за планетою мусить зникати за лімбом. Це не
                // косметика — саме по ній видно, з якого боку апарат.
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: depth::FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(depth::COMPARE),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });

        let uniform_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("line uniforms"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("line"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // Порожні буфери робити не можна (валідація), тож одна вершина —
        // місткість, з якої починаємо. Вона одразу ж і виросте.
        let (position_buffer, colour_buffer) = Lines::buffers(gpu, 1);

        Lines {
            pipeline,
            bind_group,
            uniform_buffer,
            position_buffer,
            colour_buffer,
            capacity: 1,
            position_bytes: Vec::new(),
            colour_bytes: Vec::new(),
        }
    }

    fn buffers(gpu: &Gpu, vertices: usize) -> (wgpu::Buffer, wgpu::Buffer) {
        let make = |label: &str, stride: usize| {
            gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (vertices * stride) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        (make("line positions", 12), make("line colours", 16))
    }

    fn upload(&mut self, gpu: &Gpu, scene: &Scene, projection: depth::Matrix) {
        let vertices = scene.vertex_count();
        if vertices == 0 {
            return;
        }

        if vertices > self.capacity {
            // Подвоєння, а не «рівно скільки треба»: прогноз росте ланка за
            // ланкою, і буфер під точний розмір перевиділявся б на кожному
            // тіку симуляції.
            self.capacity = vertices.next_power_of_two();
            let (position_buffer, colour_buffer) = Lines::buffers(gpu, self.capacity);
            self.position_buffer = position_buffer;
            self.colour_buffer = colour_buffer;
        }

        self.position_bytes.clear();
        self.colour_bytes.clear();
        for line in &scene.polylines {
            for &p in &line.points {
                for value in scene.camera.relative(p) {
                    self.position_bytes.extend_from_slice(&value.to_le_bytes());
                }
                for value in line.colour {
                    self.colour_bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
        }

        let mut uniform_bytes = Vec::with_capacity(64);
        for column in projection {
            for value in column {
                uniform_bytes.extend_from_slice(&value.to_le_bytes());
            }
        }

        gpu.queue
            .write_buffer(&self.uniform_buffer, 0, &uniform_bytes);
        gpu.queue
            .write_buffer(&self.position_buffer, 0, &self.position_bytes);
        gpu.queue
            .write_buffer(&self.colour_buffer, 0, &self.colour_bytes);
    }

    fn draw(&self, pass: &mut wgpu::RenderPass<'_>, scene: &Scene) {
        if scene.vertex_count() == 0 {
            return;
        }

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.position_buffer.slice(..));
        pass.set_vertex_buffer(1, self.colour_buffer.slice(..));

        // Один виклик на ламану: `LineStrip` з'єднав би останню вершину однієї
        // з першою наступної, і кадр отримав би відрізок, якого ніхто не
        // рахував.
        let mut first = 0u32;
        for line in &scene.polylines {
            let count = line.points.len() as u32;
            if count >= 2 {
                pass.draw(first..first + count, 0..1);
            }
            first += count;
        }
    }
}
