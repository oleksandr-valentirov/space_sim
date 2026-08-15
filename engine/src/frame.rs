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
use crate::scene::{Body, Scene, TileSet};
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

/// Сцена зондів рушія: одне тіло радіуса Землі в початку координат.
///
/// З R1e кадр малює **лише** те, що є в [`Scene::bodies`], і порожня сцена
/// означає порожнє небо. Тіло за замовчуванням не зникло — воно переїхало
/// сюди, у фікстуру зондів, і це саме те, чим воно завжди було: рушій не
/// знає, що таке Земля, і не має підставляти її нікому за спиною.
///
/// Гра свою сцену збирає сама (`game::view`), і цієї функції не кличе.
pub fn default_scene(camera: Camera) -> Scene {
    let mut scene = Scene::new(camera);
    scene.bodies.push(Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: sphere::EARTH_RADIUS_M,
        // Одиничний кватерніон: зонди міряють геометрію, і поворот у них
        // лише додав би до кожного числа привід сумніватися.
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: TileSet::Smooth,
    });
    scene
}

#[derive(Clone, Copy)]
struct Uniforms {
    projection: depth::Matrix,
    /// Поворот тіла, помножений на його радіус: геометрія патча — одинична
    /// сфера, спільна для всіх тіл (R1e).
    model: depth::Matrix,
    light_dir: [f32; 4],
    colour: [f32; 4],
}

/// Скільки байтів займає [`Uniforms`] у буфері.
const UNIFORM_BYTES: u64 = 160;

impl Uniforms {
    /// Розкладка вручну — та сама причина, що в `sphere_render` (CLAUDE.md,
    /// інваріант 1: наш `unsafe` живе лише в `core-rs`, тут його й не треба).
    fn to_bytes(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(UNIFORM_BYTES as usize);
        for matrix in [self.projection, self.model] {
            for column in matrix {
                for value in column {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        for value in self.light_dir.iter().chain(self.colour.iter()) {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}

/// Матриця повороту з кватерніона `[w, x, y, z]`, у `f64`.
///
/// Рядками: `m[row][col]`, тобто `m · v` — звичайне множення на вектор-стовпець.
/// У `f64`, бо цією ж матрицею повертаються початки патчів, а вони йдуть у
/// віднімання камери — те єдине місце, де `f32` коштував би планету на
/// півметра не там (ROADMAP F4).
pub(crate) fn rotation(q: [f64; 4]) -> [[f64; 3]; 3] {
    let [w, x, y, z] = q;
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - w * z),
            2.0 * (x * z + w * y),
        ],
        [
            2.0 * (x * y + w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - w * x),
        ],
        [
            2.0 * (x * z - w * y),
            2.0 * (y * z + w * x),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

/// Поворот тіла разом із його радіусом — те, що шейдер застосує до зсуву
/// вершини всередині патча.
///
/// `f32` тут безпечний з тієї самої причини, що й самі зсуви: масштаб виносить
/// множник, а не додає до великого числа мале. Одинична сфера, помножена на
/// 6.4·10⁶, дає ту саму **відносну** похибку, що й вершина, порахована одразу
/// в метрах, — тобто десяток сантиметрів на Землі, як і до R1e.
fn model_matrix(rotation: [[f64; 3]; 3], radius: f64) -> depth::Matrix {
    let mut m = [[0.0f32; 4]; 4];
    for (col, column) in m.iter_mut().enumerate().take(3) {
        for (row, value) in column.iter_mut().enumerate().take(3) {
            *value = (rotation[row][col] * radius) as f32;
        }
    }
    m[3][3] = 1.0;
    m
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
    /// Висота — над **найближчим** тілом сцени, а не над Землею в початку
    /// координат (R1e): камера біля Місяця, яка міряє висоту від Землі,
    /// отримала б near у 4·10⁷ м і зрізала б увесь Місяць.
    ///
    /// Роздільність глибини від near не залежить узагалі — це виміряно на
    /// F3 (`Δz ≈ z·6·10⁻⁸`, near скорочується), тож тут немає чого
    /// підбирати заради z-fighting.
    /// Без `self` навмисно: це чиста арифметика над сценою, і перевіряти її
    /// не має вимагати ні GPU, ні кадру.
    fn near_for(scene: &Scene) -> f64 {
        let eye = scene.camera.position();
        let length = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();

        let mut altitude = f64::INFINITY;
        for body in &scene.bodies {
            let d = [
                body.centre[0] - eye[0],
                body.centre[1] - eye[1],
                body.centre[2] - eye[2],
            ];
            altitude = altitude.min(length(d) - body.radius_m);
        }

        // Порожнє небо: міряти висоту нема над чим, лишається відстань до
        // початку координат — там-таки й ламані, якщо вони є.
        if !altitude.is_finite() {
            altitude = length(eye);
        }

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
    /// (PROJECT.md §6), тепер разом із тілами: з R1e кадр малює те, що лежить
    /// у [`Scene::bodies`], а не одну сферу радіуса Землі в початку координат.
    /// Порожній список тіл означає порожнє небо.
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        scene: &Scene,
    ) {
        self.ensure_depth(gpu, width, height);

        let aspect = f64::from(width) / f64::from(height);
        let projection = depth::reversed_infinite(FOV_Y, aspect, Frame::near_for(scene));

        // Планети: камера віднімається раз на патч, у `double`, а поворот
        // їде в матриці (R1d). Кількість роботи на CPU більше не залежить
        // від кількості вершин — тільки від кількості патчів і тіл.
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

/// Планети патчами кубосфери (ROADMAP-PLANETS.md, R1d, R1e).
///
/// Заміна UV-сфери, і суть заміни не в формі, а в тому, **хто рахує
/// camera-relative**. Було: CPU щокадру проганяв кожну з 8385 вершин через
/// `camera.relative`. Стало: CPU віднімає камеру раз на патч — шість чисел
/// на грань замість тисячі, — а зсув вершини всередині патча в `f32` уже
/// лежить у буфері й не переписується взагалі.
///
/// Поворот вигляду при цьому переїхав у шейдер, у ту саму матрицю, що й
/// проєкція: перенесення зробило віднімання на CPU в `double`, повороту
/// байдуже до масштабу (`camera::Camera::view_rotation`).
///
/// **Геометрія — одинична сфера, спільна для всіх тіл** (R1e). Радіус і
/// поворот конкретного тіла приходять другою матрицею, а початок його патча
/// рахується на CPU у `f64`: `центр + R·(q·s) − око`. Це головне, чого крок
/// не мав права зламати: множення на радіус у `f32` після віднімання камери
/// повернуло б катастрофу скорочення на низькій орбіті.
///
/// Початки патчів їдуть **storage-буфером**, а не масивом uniform-буферів:
/// D3D12 останніх не дає взагалі, і PROJECT.md §7 уже поклав per-object дані
/// саме сюди.
struct Planet {
    pipeline: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,

    offset_buffer: wgpu::Buffer,
    normal_buffer: wgpu::Buffer,
    patch_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,

    /// Початки патчів на **одиничній** сфері — рахуються раз, бо форма
    /// кубосфери від тіла не залежить: радіус і поворот множаться на них
    /// щокадру, разом із відніманням камери.
    origins: Vec<[f64; 3]>,
    origin_bytes: Vec<u8>,

    /// По слоту на тіло сцени. Ростуть за потребою й не спадають — та сама
    /// причина, що в [`Lines`]: тіла в кадрі з'являються й зникають (Місяць
    /// за обрієм), а перестворювати буфери щокадру означало б платити за це
    /// щокадру.
    slots: Vec<BodySlot>,
    /// Скільки слотів малювати цього кадру — стільки ж, скільки тіл у сцені.
    drawn: usize,
}

/// Те, чим одне тіло відрізняється від іншого на GPU: своя матриця й свої
/// початки патчів.
///
/// Виклик малювання на тіло — свідома ціна R1e. Якщо десятки тіл виявляться
/// дорогими, відповідь не «повернути одне тіло», а R6: патчі всіх тіл одним
/// буфером і `draw_indirect` (ROADMAP-PLANETS.md).
struct BodySlot {
    uniform_buffer: wgpu::Buffer,
    origin_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
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
            // Одинична сфера: розмір тіла — множник у матриці моделі, а не
            // друга копія тих самих вершин (R1e).
            let mesh = patch.mesh(1.0);
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
            bind_layout,
            offset_buffer,
            normal_buffer,
            patch_buffer,
            index_buffer,
            index_count,
            origins,
            origin_bytes,
            slots: Vec::new(),
            drawn: 0,
        }
    }

    /// Слот під тіло — свій uniform, свої початки патчів, своя bind-група.
    fn slot(&self, gpu: &Gpu) -> BodySlot {
        let uniform_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("patch uniforms"),
            size: UNIFORM_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Чотири числа на патч, а не три: вирівнювання vec4 у std430, і
        // четверте лишається нулем.
        let origin_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("patch origins"),
            size: (self.origins.len() * 16) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("patch"),
            layout: &self.bind_layout,
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

        BodySlot {
            uniform_buffer,
            origin_buffer,
            bind_group,
        }
    }

    /// Початки патчів відносно камери й матриці на цей кадр — по тілу.
    ///
    /// Оце і є весь CPU-прохід планети: шість віднімань у `double` на тіло
    /// замість восьми з половиною тисяч.
    fn upload(&mut self, gpu: &Gpu, scene: &Scene, projection: depth::Matrix) {
        let camera = &scene.camera;
        let eye = camera.position();

        while self.slots.len() < scene.bodies.len() {
            let slot = self.slot(gpu);
            self.slots.push(slot);
        }
        self.drawn = scene.bodies.len();

        // Поворот вигляду однаковий для всіх тіл — множиться раз, а не на тіло.
        let view = depth::multiply(projection, camera.view_rotation());

        for (body, slot) in scene.bodies.iter().zip(&self.slots) {
            let rotation = rotation(body.orientation);

            self.origin_bytes.clear();
            for origin in &self.origins {
                // Усе в `double`: поворот одиничного початку, множення на
                // радіус, зсув до центра тіла й віднімання камери. Звуження до
                // `f32` — останнім кроком, як завжди (ROADMAP F4).
                for k in 0..3 {
                    let turned = rotation[k][0] * origin[0]
                        + rotation[k][1] * origin[1]
                        + rotation[k][2] * origin[2];
                    let value = (body.centre[k] + body.radius_m * turned - eye[k]) as f32;
                    self.origin_bytes.extend_from_slice(&value.to_le_bytes());
                }
                self.origin_bytes.extend_from_slice(&0.0f32.to_le_bytes());
            }
            gpu.queue
                .write_buffer(&slot.origin_buffer, 0, &self.origin_bytes);

            let uniforms = Uniforms {
                projection: view,
                model: model_matrix(rotation, body.radius_m),
                light_dir: [LIGHT_DIR[0], LIGHT_DIR[1], LIGHT_DIR[2], 0.0],
                colour: COLOUR,
            };
            gpu.queue
                .write_buffer(&slot.uniform_buffer, 0, &uniforms.to_bytes());
        }
    }

    fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.drawn == 0 {
            return;
        }

        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, self.offset_buffer.slice(..));
        pass.set_vertex_buffer(1, self.normal_buffer.slice(..));
        pass.set_vertex_buffer(2, self.patch_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);

        // Виклик на тіло: спільна геометрія, різні матриця й початки патчів.
        for slot in &self.slots[..self.drawn] {
            pass.set_bind_group(0, &slot.bind_group, &[]);
            pass.draw_indexed(0..self.index_count, 0, 0..1);
        }
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
                        // Змішування ввімкнене саме тут і саме для ламаних:
                        // PROJECT.md §7 вимагає крив нульової швидкості
                        // «напівпрозорим шаром», а це і є ламані з альфою
                        // менше одиниці (U6b3). Решта ламаних мають альфу 1.0
                        // і від цього не змінюються — перевірено знімком.
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Кватерніон із `[w, x, y, z]` повертає так, як обіцяє його `w`.
    ///
    /// Оракул — не «матриця схожа на матрицю», а образ осей при повороті на
    /// 90° навколо z: `x → y`, `y → −x`, `z → z`. Саме це ловить переставлений
    /// `w` (R1c: спряжений кватерніон лишається одиничним і обертає так само
    /// добре, просто в інший бік).
    #[test]
    fn a_quarter_turn_about_z_takes_x_to_y() {
        let half = std::f64::consts::FRAC_PI_4;
        let q = [half.cos(), 0.0, 0.0, half.sin()];
        let m = rotation(q);

        let apply = |v: [f64; 3]| {
            [
                m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
                m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
                m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
            ]
        };

        let close = |a: [f64; 3], b: [f64; 3]| {
            for k in 0..3 {
                assert!((a[k] - b[k]).abs() < 1e-12, "{a:?} проти {b:?}");
            }
        };

        close(apply([1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]);
        close(apply([0.0, 1.0, 0.0]), [-1.0, 0.0, 0.0]);
        close(apply([0.0, 0.0, 1.0]), [0.0, 0.0, 1.0]);
    }

    /// Матриця моделі робить із зсуву те саме, що поворот із радіусом.
    ///
    /// Дві реалізації того самого перетворення тут навмисно поруч — як
    /// `Camera::rotate` і `Camera::view_rotation` (R1d): CPU рахує нею початки
    /// патчів, GPU — зсуви вершин, і розбіжність між ними дала б планету,
    /// зшиту з двох різних планет.
    #[test]
    fn the_model_matrix_scales_and_turns_the_same_way_the_origins_do() {
        let q = [0.923_880, 0.220_942, 0.220_942, 0.220_942];
        let radius = sphere::EARTH_RADIUS_M;
        let m = rotation(q);
        let model = model_matrix(m, radius);

        // Зсув на одиничній сфері — того ж порядку, що справжні зсуви патча.
        let offset = [0.31, -0.42, 0.17];

        for row in 0..3 {
            let by_cpu =
                radius * (m[row][0] * offset[0] + m[row][1] * offset[1] + m[row][2] * offset[2]);
            // Так само, як шейдер: стовпці матриці на компоненти вектора.
            let by_matrix = model[0][row] as f64 * offset[0]
                + model[1][row] as f64 * offset[1]
                + model[2][row] as f64 * offset[2];

            // Півметра на 6.4·10⁶ м — це `f32` матриці, і нічого понад те.
            assert!(
                (by_cpu - by_matrix).abs() < 0.5,
                "рядок {row}: {by_cpu} проти {by_matrix}"
            );
        }
    }

    /// Ближня площина міряється від найближчого тіла, а не від початку
    /// координат (R1e).
    ///
    /// Оракул — Місяць: камера за 100 км над ним, а Земля за 4·10⁸ м. Висота
    /// над Землею дала б near у мільйони метрів, тобто кадр, у якому Місяця
    /// просто немає.
    #[test]
    fn the_near_plane_follows_the_nearest_body() {
        let moon_centre = [4.0e8, 0.0, 0.0];
        let moon_radius = 1.7374e6;
        let altitude = 1.0e5;

        let eye = [moon_centre[0] - moon_radius - altitude, 0.0, 0.0];
        let camera = Camera::look_at(eye, moon_centre, [0.0, 0.0, 1.0]);

        let mut scene = Scene::new(camera);
        scene.bodies.push(Body {
            centre: [0.0, 0.0, 0.0],
            radius_m: sphere::EARTH_RADIUS_M,
            orientation: [1.0, 0.0, 0.0, 0.0],
            tiles: TileSet::Smooth,
        });
        scene.bodies.push(Body {
            centre: moon_centre,
            radius_m: moon_radius,
            orientation: [1.0, 0.0, 0.0, 0.0],
            tiles: TileSet::Smooth,
        });

        let near = Frame::near_for(&scene);
        assert!(
            (near - altitude / 10.0).abs() < 1.0,
            "near {near} м — це не десята частина висоти над Місяцем"
        );
    }
}
