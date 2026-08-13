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
use crate::depth;
use crate::gpu::Gpu;
use crate::sphere::{self, Mesh};

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
const SPHERE_WGSL: &str = include_str!("../shaders/sphere.wgsl");

pub const FOV_Y: f64 = std::f64::consts::PI / 3.0;

/// Сітка сфери. 64×128 — те саме, на чому міряли F5, тобто числа звідти
/// лишаються порівнюваними.
const LAT_SEGMENTS: u32 = 64;
const LON_SEGMENTS: u32 = 128;

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

pub struct Frame {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,

    mesh: Mesh,
    position_buffer: wgpu::Buffer,
    normal_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,

    /// Створюється при першому кадрі й перестворюється, коли змінився
    /// розмір цілі. Живе тут, а не в `app`, з однієї причини: інакше та сама
    /// логіка була б і в [`crate::shot`], а два місця розходяться.
    depth: Option<Depth>,

    /// Байти позицій, що переписуються щокадру. Тримається між кадрами, щоб
    /// не виділяти 8385 × 12 байтів шістдесят разів на секунду.
    position_bytes: Vec<u8>,
}

impl Frame {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Frame {
        let mesh = sphere::generate(sphere::EARTH_RADIUS_M, LAT_SEGMENTS, LON_SEGMENTS);

        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("sphere"),
                source: wgpu::ShaderSource::Wgsl(SPHERE_WGSL.into()),
            });

        let bind_layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("frame"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
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
                label: Some("frame"),
                bind_group_layouts: &[Some(&bind_layout)],
                immediate_size: 0,
            });

        // Атрибути мусять пережити виклик створення пайплайна, тож масиви
        // живуть тут, а не всередині виразу.
        let position_attrs = [wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 0,
            shader_location: 0,
        }];
        let normal_attrs = [wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 0,
            shader_location: 1,
        }];

        let pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("frame"),
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
                            array_stride: 12,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &normal_attrs,
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
                    // Без відсікання граней — той самий вибір, що в
                    // `sphere_render`: коректність тримається на тесті
                    // глибини (сфера опукла), а не на вгаданому порядку
                    // обходу вершин.
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

        let position_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame positions"),
            size: (mesh.positions.len() * 12) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Нормалі не залежать від камери, тож пишуться раз.
        let mut normal_bytes = Vec::with_capacity(mesh.normals.len() * 12);
        for n in &mesh.normals {
            for value in n {
                normal_bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        let normal_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame normals"),
            size: normal_bytes.len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&normal_buffer, 0, &normal_bytes);

        let index_bytes: Vec<u8> = mesh.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let index_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame indices"),
            size: index_bytes.len() as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&index_buffer, 0, &index_bytes);

        let uniform_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame uniforms"),
            size: 96,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let position_bytes = Vec::with_capacity(mesh.positions.len() * 12);

        Frame {
            pipeline,
            bind_group,
            uniform_buffer,
            mesh,
            position_buffer,
            normal_buffer,
            index_buffer,
            depth: None,
            position_bytes,
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
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        camera: &Camera,
    ) {
        self.ensure_depth(gpu, width, height);

        // Camera-relative щокадру, на кожну вершину: віднімання й поворот у
        // double, звуження до f32 — останній крок (ROADMAP F4, F5). Для
        // 8385 вершин налагоджувальної сфери це прийнятно; мільйони вершин
        // LOD у M4 доведеться зсувати по патчах, і саме `--perf-probe`
        // покаже, коли межа настане.
        self.position_bytes.clear();
        for &p in &self.mesh.positions {
            let rel = camera.relative(p);
            for value in rel {
                self.position_bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        gpu.queue
            .write_buffer(&self.position_buffer, 0, &self.position_bytes);

        let aspect = f64::from(width) / f64::from(height);
        let uniforms = Uniforms {
            projection: depth::reversed_infinite(FOV_Y, aspect, self.near_for(camera)),
            light_dir: [LIGHT_DIR[0], LIGHT_DIR[1], LIGHT_DIR[2], 0.0],
            colour: COLOUR,
        };
        gpu.queue
            .write_buffer(&self.uniform_buffer, 0, &uniforms.to_bytes());

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

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.position_buffer.slice(..));
        pass.set_vertex_buffer(1, self.normal_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.mesh.indices.len() as u32, 0, 0..1);
    }
}
