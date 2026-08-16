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
use crate::cull;
use crate::depth;
use crate::detail;
use crate::gpu::Gpu;
use crate::lod;
use crate::scene::{self, Body, Scene, TileSet};
use crate::sky::{self, Sky};
use crate::sphere;
use crate::tiles;

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

/// Відбір патчів у compute (ROADMAP-PLANETS.md, R6b).
const CULL_WGSL: &str = include_str!("../shaders/cull.wgsl");

/// Те саме для ламаних (ROADMAP J1).
const LINE_WGSL: &str = include_str!("../shaders/line.wgsl");
const SHIP_WGSL: &str = include_str!("../shaders/ship.wgsl");

pub const FOV_Y: f64 = std::f64::consts::PI / 3.0;

/// Напрямок ДО джерела світла, світові осі. Тимчасовий, як і саме освітлення.
///
/// Публічний рівно тому, що на нього спирається перевірка: рельєф видно лише
/// на освітленому боці, тож тест мусить знати, де той бік (R5c).
pub const LIGHT_DIR: [f32; 3] = [0.4, 0.4, 0.82];
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
        air: None,
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
    /// `x` — скільки одиниць одиничної сфери в одній одиниці зберігання
    /// висоти, тобто `scale_m / radius_m`. Решта — нулі.
    terrain: [f32; 4],
    /// Процедурний детайл (R7c): радіус тіла, множник нахилу
    /// (`Terrain::slope_rise`), довжина хвилі найгрубішої октави
    /// (`detail::base_m`) і пікселів на радіан.
    detail: [f32; 4],
}

/// Скільки байтів займає [`Uniforms`] у буфері.
const UNIFORM_BYTES: u64 = 192;

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
        for value in self
            .light_dir
            .iter()
            .chain(self.colour.iter())
            .chain(self.terrain.iter())
            .chain(self.detail.iter())
        {
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

/// Скільки проходів кадру буває найбільше.
///
/// Чотири — це чотири діапазони глибини з PROJECT.md §7 (зорі, scaled space,
/// поверхня, локальна сцена). Число стоїть тут, бо під нього виділяються
/// буфери uniform-ів: розкладка з динамічним зсувом мусить бути відома до
/// першого кадру, а не з'ясовуватись у ньому.
pub const MAX_PASSES: usize = 4;

/// Скільки порядків відстані тримає один буфер глибини — виміряно на F3
/// (`Δz ≈ z·6·10⁻⁸`, `near` скорочується). Саме це число, а не смак, вирішує,
/// коли заводити другий діапазон.
const DECADES_PER_PASS: f64 = 7.0;

/// Скільки байтів займає `PatchData` у шейдері (R7a).
///
/// Вісім слів: початок (три), номер тайла, вікно в тайлі (зсув-два й крок) і
/// одне слово запасу. Сім із них читаються; восьме існує тому, що `float3` у
/// Slang вимагає 16-байтового вирівнювання, і структура з семи слів однаково
/// займала б вісім.
const PATCH_DATA_BYTES: usize = 32;

/// Назви проходів за зростанням відстані. Нульовий — найближчий.
const PASS_LABELS: [&str; MAX_PASSES] = [
    "depth range 0",
    "depth range 1",
    "depth range 2",
    "depth range 3",
];

/// Скільки байтів займає матриця проєкції ламаних.
const LINE_UNIFORM_BYTES: u64 = 64;

/// Проєкція плюс напрямок на світило: 64 + 16 байтів. Колір сюди не входить
/// і не входитиме — він атрибут вершини, з тієї самої причини, що в ламаних
/// (записи в чергу відбуваються ДО проходу, тож із uniform виграв би
/// останній корабель).
const SHIP_UNIFORM_BYTES: u64 = 80;

/// Крок між uniform-ами сусідніх проходів у буфері.
///
/// 256 байтів — вирівнювання динамічного зсуву, якого вимагає wgpu на всіх
/// трьох цілях. Самі [`Uniforms`] коротші; решта кроку не використовується, і
/// платити за неї доводиться рівно тому, що альтернатива — буфер на прохід.
const PASS_STRIDE: u64 = 256;

/// Один прохід кадру — **дані**, а не гілка в коді (ROADMAP-PLANETS.md, R4a).
///
/// ## Чому це не «frame graph» у звичному сенсі, і чому так правильно
///
/// Класичний граф кадру існує заради двох речей: порядку проходів і бар'єрів
/// між ресурсами. Другого тут не потрібно взагалі — `wgpu` розставляє бар'єри
/// сам, за використанням ресурсів, і власний розв'язувач поверх нього був би
/// другою правдою про той самий стан. Лишається перше, а перше — це список.
///
/// Тому проходи стали списком структур, а не деревом залежностей: кадр
/// перестав знати, **скільки** їх, і саме це потрібно чотирьом діапазонам
/// глибини (R4b). CLAUDE.md прямо забороняє заводити структуру наперед; тут
/// другий читач приходить наступним кроком, а не «колись».
///
/// Що в проході змінне, а що ні:
///
/// - **проєкція своя**, бо діапазон — це пара площин;
/// - **глибина очищається завжди.** У цьому й суть поділу: два тіла в різних
///   діапазонах не змагаються за біти глибини взагалі, їх упорядковує
///   порядок проходів;
/// - **колір очищає лише перший.** Композиція йде back-to-front, тобто від
///   найдальшого діапазону до найближчого, і кожен наступний малює поверх.
#[derive(Clone, Copy, Debug)]
struct Pass {
    label: &'static str,
    projection: depth::Matrix,
    clear_colour: bool,
    /// Коефіцієнти зворотного перетворення глибини: `z_ndc = −A + B/z`
    /// (S5). Потрібні композиції аеральної перспективи, яка з глибини має
    /// дістати метри.
    depth_a: f64,
    depth_b: f64,
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

/// Пайплайн кораблів і буфери під них (етап V, крок V2).
///
/// Побудований за зразком [`Lines`], а не [`Planet`], і це вибір за розміром
/// задачі: у корабля півтори тисячі вершин, а не мільйони, тож camera-relative
/// на кожну щокадру коштує мікросекунди — рівно те, від чого планету довелося
/// рятувати зсувом по патчах (R1d), і рівно те, що для корабля дешевше за
/// другий uniform із динамічним зсувом.
///
/// **Геометрія — корабель одиничної висоти, спільний для всіх** — те саме
/// рішення, що «одинична сфера, спільна для всіх тіл» (R1e). Висота, поворот
/// і позиція конкретного корабля прикладаються на CPU у `f64`.
struct Ships {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,

    /// Індекси меша — сталі, тож завантажуються один раз. Вершини (позиції,
    /// нормалі, колір) переписуються щокадру: позиція камери, поворот корабля
    /// й колір усі змінні.
    index_buffer: wgpu::Buffer,
    index_count: u32,
    /// Вершин в одному кораблі. Виклик малювання на корабель зсуває
    /// `base_vertex` на цю величину.
    vertices_per_ship: usize,
    /// Корабель одиничної висоти в системі корабля — те, що масштабується й
    /// повертається на CPU.
    mesh: crate::sphere::Mesh,

    position_buffer: wgpu::Buffer,
    normal_buffer: wgpu::Buffer,
    colour_buffer: wgpu::Buffer,
    capacity: usize,

    position_bytes: Vec<u8>,
    normal_bytes: Vec<u8>,
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

    /// Кораблі (етап V). Порожній список кораблів у сцені не коштує нічого:
    /// ні завантаження, ні виклику малювання — саме на цьому стоїть те, що
    /// знімок зондів рушія лишився бітово тим самим.
    ships: Ships,

    /// План цього кадру: проходи в порядку малювання (R4a). Поле, а не
    /// змінна, щоб не виділяти вектор щокадру.
    passes: Vec<Pass>,

    /// Повітря: сталі таблиці, таблиця неба на цей кадр і сам прохід (етап S).
    ///
    /// Полем кадру, а не окремою підсистемою поруч: небо малюється тим самим
    /// проходом, що й усе інше, і ділить із ним і глибину, і ціль.
    sky: Sky,

    /// Скільки коштував прохід по вершинах ламаних в останньому `draw`, мс.
    ///
    /// Існує заради боргу D7 і читається зондом гри (`game::perf_probe`, N1):
    /// саме на цьому числі стоїть розвилка кроку — чи справді ламані найдорожчі
    /// в кадрі. Міряється завжди, а не за прапорцем: два `Instant::now` на кадр
    /// — це десятки наносекунд проти сотень мікросекунд самого проходу, і
    /// вимір, який умикають окремо, вмикають не тоді, коли він потрібен.
    lines_upload_ms: f64,
}

impl Frame {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Frame {
        Frame {
            planet: Planet::new(gpu, format),
            depth: None,
            lines: Lines::new(gpu, format),
            ships: Ships::new(gpu, format),
            sky: Sky::new(gpu, format),
            passes: Vec::with_capacity(MAX_PASSES),
            lines_upload_ms: 0.0,
        }
    }

    /// Скільки коштував прохід по вершинах ламаних в останньому [`Frame::draw`],
    /// мс (D7, N1).
    pub fn lines_upload_ms(&self) -> f64 {
        self.lines_upload_ms
    }

    /// Завантажити рельєф у кадр і дістати хендл на нього
    /// (ROADMAP-PLANETS.md, R5c).
    ///
    /// По **текстурі на тайл**, а не по шару спільного масиву: правило 6
    /// етапу R вимагає bindless, і різниця не термінологічна. У
    /// `texture_2d_array` спільний розмір і жорстка стеля шарів (256 у
    /// downlevel-лімітах, тобто менше, ніж тайлів у першому ж асеті), у
    /// bindless-масиву — ні того, ні того.
    ///
    /// Помилка тут гучна навмисно. Пристрій без bindless — це бекенд, який і
    /// так не ціль (PROJECT.md §7 називає Vulkan, D3D12, Metal), і мовчазне
    /// «намалюємо гладко» дало б планету без гір, яку ніхто не відрізнить від
    /// планети, чий асет не завантажився.
    pub fn load_terrain(
        &mut self,
        gpu: &Gpu,
        terrain: &tiles::Terrain,
    ) -> Result<scene::TerrainId, String> {
        if self.planet.terrain.is_none() {
            return Err(format!(
                "рельєф вимагає bindless-масиву текстур, а адаптер його не має: {}",
                gpu.describe()
            ));
        }
        let count = tiles::Terrain::count(terrain.levels);
        if count > MAX_TILES as usize {
            return Err(format!(
                "{count} тайлів проти стелі масиву {MAX_TILES} — підніміть MAX_TILES"
            ));
        }

        let side = tiles::STORED as u32;
        let mut views = Vec::with_capacity(count);
        for index in 0..count {
            let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("terrain tile"),
                size: wgpu::Extent3d {
                    width: side,
                    height: side,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                // Цілі зі знаком, як у самому тайлі: жодного перетворення між
                // асетом і текстурою, отже й жодного місця, де воно поїде.
                format: wgpu::TextureFormat::R16Sint,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            gpu.queue.write_texture(
                texture.as_image_copy(),
                terrain.tile_bytes(index),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(side * 2),
                    rows_per_image: Some(side),
                },
                wgpu::Extent3d {
                    width: side,
                    height: side,
                    depth_or_array_layers: 1,
                },
            );
            views.push(texture.create_view(&wgpu::TextureViewDescriptor::default()));
        }

        let borrowed: Vec<&wgpu::TextureView> = views.iter().collect();
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terrain tiles"),
            layout: self
                .planet
                .tile_layout
                .as_ref()
                .expect("макет масиву є рівно тоді, коли є пайплайн рельєфу"),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureViewArray(&borrowed),
            }],
        });

        self.planet.terrains.push(TerrainSlot {
            data: terrain.clone(),
            bind_group,
            scale_m: terrain.scale_m,
        });
        Ok(scene::TerrainId(self.planet.terrains.len() - 1))
    }

    /// Скільки патчів GPU справді намалював для кожного тіла останнього кадру
    /// (ROADMAP-PLANETS.md, R6b).
    ///
    /// Існує заради оракула, і це не приховується: R3 робив відбір на CPU не
    /// тому, що так простіше, а щоб R6b мав із чим звірити своє число. Читати
    /// це в кадрі не можна — тут `poll(Wait)`, тобто повна зупинка конвеєра.
    pub fn drawn_patches(&self, gpu: &Gpu) -> Result<Vec<u32>, String> {
        let mut out = Vec::with_capacity(self.planet.bodies.len());
        for body in &self.planet.bodies {
            let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("indirect readback"),
                size: 16,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("indirect readback"),
                });
            encoder.copy_buffer_to_buffer(&body.indirect_buffer, 0, &staging, 0, 16);
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
            out.push(u32::from_le_bytes(data[4..8].try_into().unwrap()));
            drop(data);
            staging.unmap();
        }
        Ok(out)
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

        // Корабель — та сама арифметика, і саме заради неї крок існує (V2).
        // Без цього рядка `near` виводилася з висоти над тілом: на орбіті
        // 400 км це 40 км, тобто корпус за десять метрів від камери
        // відсікався цілком, і кадр від третьої особи був порожнім.
        for ship in &scene.ships {
            let d = [
                ship.centre[0] - eye[0],
                ship.centre[1] - eye[1],
                ship.centre[2] - eye[2],
            ];
            altitude = altitude.min(length(d) - ship.extent_m);
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
            // `TEXTURE_BINDING` — заради композиції аеральної перспективи
            // (S5): вона читає глибину, щоб знати, доки шейдити повітря, і
            // робить це в окремому проході, де глибина вже не ціль.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
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
        Frame::plan(&mut self.passes, scene, aspect);

        // Повітря (етап S). Нічого не коштує, коли його немає: тіло без
        // атмосфери не запускає ні таблиць, ні проходу, і кадр лишається
        // бітово тим самим, що до етапу, — на цьому стоїть правило 4.
        // **Повітря — не завжди** (S5, S7). Уся робота з ним коштує стільки ж,
        // скільки б не було в сцені, тож пропускати її треба тоді, коли крізь
        // повітря нема чого дивитися. Умова одна на все: товщина шару в
        // пікселях кадру. Шар, тонший за піксель, не змінить жодного — ні
        // таблицею неба, ні об'ємом.
        //
        // Одна умова, а не дві, і це виміряно: на 10⁹ м об'єм уже пропускався,
        // а таблиця неба рахувалася, і разом із повноекранним проходом вона
        // коштувала 0.05 мс за диск повітря завширшки в шістнадцяту пікселя.
        // Двох режимів «видно наполовину» тут не існує.
        let focal = lod::focal_px(FOV_Y, f64::from(height));
        let air = Frame::air_view(scene, aspect).filter(|(atmosphere, bottom, view)| {
            Frame::shell_px(atmosphere, *bottom, view, focal) >= 1.0
        });
        let aerial = air.is_some();
        if let Some((atmosphere, bottom, view)) = &air {
            self.sky.ensure(gpu, atmosphere, *bottom);
            self.sky.prepare_view(gpu, encoder, view);
            self.sky.prepare_aerial(encoder);
        }

        // Планети: камера віднімається раз на патч, у `double`, а поворот
        // їде в матриці (R1d). Кількість роботи на CPU більше не залежить
        // від кількості вершин — тільки від кількості патчів і тіл.
        self.planet.upload(
            gpu,
            scene,
            &self.passes,
            f64::from(width),
            f64::from(height),
        );

        // Ламані проходять той самий шлях, що вершини сфери: віднімання й
        // поворот у double, звуження до f32 останнім кроком. Інакше
        // траєкторія за 4·10⁸ м від камери тремтіла б, а сфера поруч — ні.
        let upload_start = std::time::Instant::now();
        self.lines.upload(gpu, scene, &self.passes);
        self.lines_upload_ms = upload_start.elapsed().as_secs_f64() * 1000.0;

        // Кораблі: та сама дорога, що в ламаних, і той самий порядок —
        // віднімання в `double`, звуження останнім кроком.
        self.ships.upload(gpu, scene, &self.passes);

        // Відбір — до проходів кадру, окремим compute-проходом (R6b). Бар'єри
        // між ним і читанням `indirect` розставляє wgpu сам: він бачить, що
        // той самий буфер щойно писали.
        self.planet.cull(encoder);

        if aerial {
            let depth = self.depth.as_ref().expect("ensure_depth щойно її створив");
            self.sky.bind_depth(gpu, &depth.view, width, height);
            for (index, plan) in self.passes.iter().enumerate() {
                self.sky.set_range(gpu, index, plan.depth_a, plan.depth_b);
            }
        }

        let depth = self.depth.as_ref().expect("ensure_depth щойно її створив");

        for (index, plan) in self.passes.iter().enumerate() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(plan.label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if plan.clear_colour {
                            wgpu::LoadOp::Clear(CLEAR)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth.view,
                    depth_ops: Some(wgpu::Operations {
                        // Очищається на кожному проході: діапазони не
                        // змагаються за біти глибини, їх упорядковує порядок.
                        load: wgpu::LoadOp::Clear(depth::CLEAR),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                multiview_mask: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Небо — **першим у найдальшому діапазоні**, одразу після очищення
            // кольору. Далі геометрія лягає зверху за звичайним тестом глибини,
            // а ближчі діапазони домальовують поверх, бо колір вони не чистять.
            // Власного запису глибини прохід неба не робить: воно нескінченно
            // далеко, і сперечатися з ним нема про що.
            if index == 0 {
                if let Some((atmosphere, _, view)) = &air {
                    self.sky.draw(&mut pass, view.radius() < atmosphere.top_m);
                }
            }

            self.planet.draw(&mut pass, index);
            self.ships.draw(&mut pass, scene, index);
            self.lines.draw(&mut pass, scene, index);
            drop(pass);

            // Композиція — **окремим проходом одразу після свого діапазону**, і
            // окремим саме тому, що вона читає глибину: та сама текстура не
            // буває одночасно ціллю й ресурсом. А після свого — бо кожен
            // діапазон чистить глибину, тобто своя глибина є рівно тут.
            // Пікселі, у яких цей діапазон нічого не намалював, композиція
            // пропускає: нуль у reversed-Z означає «нескінченно далеко».
            if aerial {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("aerial composite"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    multiview_mask: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                self.sky.composite(&mut pass, index);
            }
        }
    }

    /// Скільки пікселів кадру займає товщина шару повітря — умова кроку S5.
    ///
    /// Число, а не «камера далеко»: питання не в тому, де камера, а в тому, чи
    /// видно повітря. З 10⁹ м шар у сто кілометрів тонший за соту пікселя, і
    /// об'єм 32×32×32 рахувався б заради нічого. Зблизька ж камера стоїть
    /// усередині повітря, відстань до поверхні прямує до нуля, і число росте
    /// само.
    ///
    /// Той самий вид критерію, що `lod::error_px`: екранна похибка, а не
    /// відстань у метрах. Відстань у метрах довелося б підбирати на кожен
    /// радіус тіла окремо.
    fn shell_px(air: &scene::Atmosphere, bottom: f64, view: &sky::View, focal: f64) -> f64 {
        let altitude = (view.radius() - bottom).max(1.0);
        air.thickness_m(bottom) / altitude * focal
    }

    /// Тіло з повітрям, найближче до камери, і камера відносно нього.
    ///
    /// **Найближче, а не перше в списку.** Тіл з атмосферою в сцені сьогодні
    /// одне (Земля, S1), але правило мусить бути назване до того, як їх стане
    /// два: далеке повітря камери не оточує, і небо їй малює те, всередині
    /// якого — або поруч із яким — вона стоїть.
    ///
    /// `None` означає «повітря в кадрі немає», і це не окремий випадок, а той
    /// самий кадр, що був до етапу S: жодна таблиця не рахується, прохід не
    /// подається, знімок лишається бітово тим самим.
    fn air_view(scene: &Scene, aspect: f64) -> Option<(scene::Atmosphere, f64, sky::View)> {
        let eye = scene.camera.position();
        let mut best: Option<(f64, &Body)> = None;
        for body in &scene.bodies {
            let Some(_) = body.air else { continue };
            let d = [
                body.centre[0] - eye[0],
                body.centre[1] - eye[1],
                body.centre[2] - eye[2],
            ];
            let distance = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if best.is_none_or(|(previous, _)| distance < previous) {
                best = Some((distance, body));
            }
        }
        let (_, body) = best?;
        let air = body.air?;

        let (right, up, forward) = scene.camera.axes();
        let sun = {
            let l = LIGHT_DIR;
            let length = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
            [l[0] / length, l[1] / length, l[2] / length]
        };
        let t = (FOV_Y / 2.0).tan();
        let narrow = |v: [f64; 3]| [v[0] as f32, v[1] as f32, v[2] as f32];

        Some((
            air,
            body.radius_m,
            sky::View {
                // Віднімання центра тіла від ока — у `f64`, як усе
                // camera-relative (F4). Звужується воно вже в `sky`.
                eye: [
                    eye[0] - body.centre[0],
                    eye[1] - body.centre[1],
                    eye[2] - body.centre[2],
                ],
                sun,
                right: narrow(right),
                up: narrow(up),
                forward: narrow(forward),
                tan_half: [(t * aspect) as f32, t as f32],
            },
        ))
    }

    /// Найдальша точка сцени: дальній край найдальшого тіла.
    ///
    /// Ламані сюди не входять, і це не недогляд: найдальший діапазон
    /// **нескінченний**, тож нічого за цією межею з кадру не зникає. Число
    /// вирішує лише, де поставити межі між проходами, а не що малювати.
    fn far_for(scene: &Scene) -> f64 {
        let eye = scene.camera.position();
        let mut far: f64 = 0.0;
        for body in &scene.bodies {
            let d = [
                body.centre[0] - eye[0],
                body.centre[1] - eye[1],
                body.centre[2] - eye[2],
            ];
            far = far.max((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() + body.radius_m);
        }
        far
    }

    /// Ближні площини діапазонів глибини цієї сцени, **від найближчого**.
    ///
    /// Довжина — скільки буде проходів; `[0]` — ближня площина кадру, `[1]` —
    /// межа між першим і другим діапазоном, і так далі. Питання до плану, і
    /// відповідь на нього не варта ні GPU, ні кадру — так само, як
    /// [`Frame::near_for`].
    ///
    /// Числа дістаються з тих самих `depth_a`/`depth_b`, якими користується
    /// композиція аеральної перспективи (`z_ndc = −A + B/z`), а не рахуються
    /// вдруге: `lo = B/(A+1)` однаково для скінченного діапазону й для
    /// нескінченного, у якого `A = 0`. Тобто перевірка питає те число, яким
    /// кадр справді малює, а не своє власне.
    ///
    /// Існує заради V3: там треба стверджувати і «діапазонів два», і «межа
    /// лягла саме сюди», а повторювати формулу поруч із нею — це два джерела
    /// правди про одну річ.
    pub fn depth_ranges(scene: &Scene, aspect: f64) -> Vec<f64> {
        let mut passes = Vec::new();
        Frame::plan(&mut passes, scene, aspect);
        passes
            .iter()
            .rev()
            .map(|plan| plan.depth_b / (plan.depth_a + 1.0))
            .collect()
    }

    /// План кадру: скільки діапазонів глибини й де їхні межі (R4a, R4b).
    ///
    /// ## Скільки проходів, і звідки взялося це число
    ///
    /// Не «завжди чотири». Прохід коштує повний перемальовок сцени, тож
    /// заводити його треба тоді, коли одного буфера справді не вистачає — а
    /// це межа, яку F3 виміряв: **сім порядків відстані** (`Δz ≈ z·6·10⁻⁸`).
    /// Звідси правило: один діапазон на кожні сім порядків розмаху сцени,
    /// не більше [`MAX_PASSES`] (PROJECT.md §7). Сцена зондів рушія має
    /// розмах 22.7 — тобто рівно один прохід, і кадр лишається тим самим
    /// бітово, яким був до R4b.
    ///
    /// ## Чим діапазони виправдані, а чим — ні
    ///
    /// **Не роздільністю глибини.** Це виміряно й записано числом:
    /// `depth::tests::a_finite_range_is_no_sharper_than_an_infinite_one`
    /// показує 4.0 м на 10⁸ м однаково для нескінченної проєкції й для
    /// скінченного діапазону з будь-якого його кінця. Причина — катастрофічне
    /// скорочення в `z_clip` біля далекої площини, яке з'їдає рівно те, що
    /// обіцяла викладка. Те саме стосується й самої межі: площина відсікання
    /// стоїть на тій самій арифметиці, тож нею не розділити двох поверхонь
    /// ближче, ніж `z·6·10⁻⁸`.
    ///
    /// Виправдані вони **scaled space** (PROJECT.md §7): правом малювати
    /// далеке на вигаданій відстані. Тіло за 10¹¹ м, намальоване як мала
    /// модель за 10⁶ м, зіткнулося б із реальною геометрією тієї відстані —
    /// і рятує від цього рівно окремий прохід із власною глибиною, а не
    /// точність. Того малювання ще немає; механізм для нього — є.
    ///
    /// Межі — **геометричні**: глибина міряє відношення, а не різницю, тож
    /// рівні частки логарифма й дають рівні частки роботи.
    ///
    /// Порядок — back-to-front, від найдальшого діапазону до найближчого:
    /// колір очищає перший, глибину — кожен.
    ///
    /// Без `self` навмисно — з тієї самої причини, що [`Frame::near_for`]: це
    /// чиста арифметика над сценою, і питати в неї «скільки тут проходів»
    /// не має вимагати ні GPU, ні кадру. Список приходить параметром, щоб
    /// пам'ять під нього виділялась один раз на життя кадру, а не щокадру.
    fn plan(passes: &mut Vec<Pass>, scene: &Scene, aspect: f64) {
        let near = Frame::near_for(scene);
        let far = Frame::far_for(scene);
        passes.clear();

        let span = (far / near).max(1.0);
        let count = ((span.log10() / DECADES_PER_PASS).ceil() as usize).clamp(1, MAX_PASSES);
        let ratio = span.powf(1.0 / count as f64);

        for k in (0..count).rev() {
            let lo = near * ratio.powi(k as i32);
            let last = k + 1 == count;
            // Найдальший — нескінченний: за ним не лишається нічого, що
            // намалює хтось інший.
            let (projection, depth_a, depth_b) = if last {
                (depth::reversed_infinite(FOV_Y, aspect, lo), 0.0, lo)
            } else {
                let hi = near * ratio.powi(k as i32 + 1);
                let span = hi - lo;
                (
                    depth::reversed_finite(FOV_Y, aspect, lo, hi),
                    lo / span,
                    lo * hi / span,
                )
            };
            passes.push(Pass {
                label: PASS_LABELS[k],
                projection,
                clear_colour: last,
                depth_a,
                depth_b,
            });
        }
    }
}

/// Планети патчами кубосфери (ROADMAP-PLANETS.md, R1d, R1e, R2c).
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
///
/// ## Що приніс LOD (R2c)
///
/// Набір патчів більше не сталий: [`crate::lod::select`] дає його щокадру,
/// на тіло. Звідси три рішення, і кожне випливає з правила 1 етапу R — патч
/// є одиницею всього:
///
/// - **геометрія патча кешується за самим патчем.** Зсуви вершин не залежать
///   ні від камери, ні від тіла (одинична сфера), тож патч, який уже був у
///   кадрі, не рахується вдруге. Без кеша LOD коштував би десятки тисяч
///   `tan` щокадру — рівно та ціна, яку R1d щойно прибрав;
/// - **індексні набори лежать усі шістнадцять поспіль** в одному буфері, і
///   зшивання вибирає діапазон, а не буфер. Вони не залежать ні від патча,
///   ні від тіла — це адресація сітки;
/// - **виклик малювання — на патч**, `base_vertex` вказує на його слот у
///   кеші. Дорого це стане тоді, коли патчів стануть тисячі, і відповідь на
///   це вже названа: R6 і `draw_indexed_indirect`.
struct Planet {
    /// Два пайплайни, а не гілка в шейдері: гладке тіло й тіло з рельєфом
    /// малюються різними програмами (R5c). Причина — уже спіймана пастка
    /// F6: рантайм-перемикач за uniform-ом у вершинній стадії на ACO читався
    /// мовчки неправильно. Друга причина простіша: у гладкого тіла немає
    /// тайла, і `textureLoad` за невизначеним індексом не мусить навіть
    /// потрапити в його програму.
    smooth: wgpu::RenderPipeline,
    terrain: Option<wgpu::RenderPipeline>,
    bind_layout: wgpu::BindGroupLayout,
    tile_layout: Option<wgpu::BindGroupLayout>,
    /// Відбір у compute (R6b): та сама арифметика, що в `crate::cull`, але
    /// на тих даних, які вже лежать на GPU.
    cull_pipeline: wgpu::ComputePipeline,
    cull_layout: wgpu::BindGroupLayout,
    /// Група висот для тіла **без** рельєфу.
    ///
    /// Обидва пайплайни ділять один макет, тож група 1 мусить бути
    /// прив'язана завжди — навіть у гладкої програми, яка до неї не
    /// звертається. Один тайл 1×1 з нулем: `PARTIALLY_BOUND` дозволив би й
    /// порожній масив, але порожній масив — це ще один шлях, який працює
    /// не всюди однаково, а один нульовий тексель коштує чотирьох байтів.
    no_tiles: Option<wgpu::BindGroup>,

    /// Завантажені рельєфи: по текстурі на тайл.
    terrains: Vec<TerrainSlot>,

    cache: PatchCache,

    /// По слоту на тіло сцени. Ростуть за потребою й не спадають — та сама
    /// причина, що в [`Lines`]: тіла в кадрі з'являються й зникають (Місяць
    /// за обрієм), а перестворювати буфери щокадру означало б платити за це
    /// щокадру.
    bodies: Vec<BodySlot>,

    /// Набори цього кадру — поле, а не змінна, щоб не виділяти вектор щокадру.
    selections: Vec<lod::Selection>,
}

/// Завантажений рельєф: по текстурі на тайл плюс сам тайлсет.
///
/// Текстура на тайл, а не один шар масиву на тайл: правило 6 етапу R вимагає
/// **bindless-масив**, а не `texture_2d_array`. Різниця не термінологічна —
/// у масиву шарів спільний розмір і жорстка стеля (256 у downlevel-лімітах),
/// у bindless-масиву ні того, ні того, і саме тому на нього й перейшли
/// (PROJECT.md §7, розвідка P0: 10⁶ елементів на цій машині).
struct TerrainSlot {
    data: tiles::Terrain,
    bind_group: wgpu::BindGroup,
    /// Метрів на одиницю зберігання — множник для вершинного зсуву.
    scale_m: f32,
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
    /// Кандидати на малювання: увесь вибраний набір, до відбору.
    candidate_buffer: wgpu::Buffer,
    /// Параметри відбору на цей кадр.
    cull_uniform: wgpu::Buffer,
    /// Аргументи `draw_indirect`; друге слово — лічильник вижилих.
    indirect_buffer: wgpu::Buffer,
    cull_bind_group: wgpu::BindGroup,
    /// Скільки кандидатів подано цього кадру — стільки треба груп compute.
    candidates: u32,
    bind_group: wgpu::BindGroup,
    /// Який рельєф зараз прив'язаний до цього слота.
    ///
    /// Bind-група тримає **посилання на текстури**, тож змінити рельєф тіла
    /// без її перестворення не можна. Поле тут саме для того, щоб не
    /// перестворювати її щокадру: тіла в сцені міняються рідко, а кадр іде
    /// шістдесят разів на секунду.
    terrain: Option<usize>,
}

/// Скільки вершин у сітці одного патча.
const PATCH_VERTICES: usize = (cubesphere::SIDE + 1) * (cubesphere::SIDE + 1);
/// Скільки вершин у списку трикутників патча — по три на трикутник, по два
/// трикутники на клітинку.
const PATCH_INDICES: usize = cubesphere::SIDE * cubesphere::SIDE * 6;

/// Скільки байтів займає `PatchVertex` у std430: два `vec3` з вирівнюванням 16.
const VERTEX_BYTES: u64 = 32;

/// Скільки байтів займає `Cone` у std430.
const CONE_BYTES: u64 = 32;

/// Скільки байтів займає `CullParams`: сім `vec4`.
const CULL_BYTES: u64 = 112;

/// Скільки патчів обробляє одна група compute — те саме число, що в
/// `[numthreads(64, 1, 1)]` у `shaders/cull.slang`.
const CULL_GROUP: u32 = 64;
/// З чого починається місткість кеша — далі вона тільки росте.
const MIN_PATCHES: usize = 64;

/// Скільки елементів оголошує bindless-масив висот.
///
/// Стеля макета, не кількість тайлів: `PARTIALLY_BOUND_BINDING_ARRAY` дозволяє
/// прив'язати менше. Число взяте з того, що вже є: тайлсет Місяця на п'яти
/// рівнях піраміди — 2046 тайлів (R5b), і 4096 лишає рівно один рівень запасу.
/// Апаратна межа на порядки вища (10⁶ на цій машині, розвідка P0), тож
/// упертися тут можна лише в асет, а не в GPU.
const MAX_TILES: u32 = 4096;

/// Кеш геометрії патчів: слот на патч, спільний для всіх тіл.
///
/// ## Чому кеш, а не перерахунок
///
/// Зсуви вершин патча не залежать ні від камери, ні від тіла: геометрія —
/// одинична сфера (R1e), а патч на ній стоїть нерухомо. Отже єдине, що
/// змінюється щокадру, — **які** патчі потрібні, і це рівно та задача, під
/// яку кеш і існує. Сусідні кадри ділять майже весь набір: LOD міняє його
/// по одному патчу, а не цілком.
///
/// ## Витіснення — курсором, а не історією звернень
///
/// Місткість тримається щонайменше вдвічі більшою за потребу кадру, тож
/// слот, не потрібний **цього** кадру, знайдеться завжди. Шукає його курсор,
/// що йде по колу: LRU тут дав би той самий результат за більші гроші, бо
/// набір міняється поступово, а не стрибками.
struct PatchCache {
    capacity: usize,
    slot: std::collections::HashMap<Patch, u32>,
    resident: Vec<Option<Patch>>,
    /// Номер кадру, у якому слот востаннє знадобився.
    stamp: Vec<u64>,
    /// Початок патча на **одиничній** сфері, за слотом.
    origins: Vec<[f64; 3]>,
    cursor: usize,
    frame: u64,

    /// Зсуви й нормалі всіх патчів кеша одним **storage**-буфером (R6a).
    ///
    /// Не вершинними атрибутами: зшивання рівнів — це підміна індексу вузла,
    /// а атрибути приходять уже вибраними. Читаючи вузол сам, шейдер робить
    /// підміну арифметикою, і шістнадцять індексних наборів разом із викликом
    /// малювання на патч зникають обидва.
    vertex_buffer: wgpu::Buffer,
    /// Конус кожного патча в системі **тіла** — те, з чого compute рахує
    /// відбір за лімбом (R6b).
    ///
    /// За слотом кеша, а не за позицією в наборі: конус не залежить ні від
    /// камери, ні від тіла, тож рахується раз при заселенні слота — там же,
    /// де й геометрія.
    cone_buffer: wgpu::Buffer,
}

impl PatchCache {
    fn new(gpu: &Gpu, capacity: usize) -> PatchCache {
        let vertices = (capacity * PATCH_VERTICES) as u64;

        PatchCache {
            capacity,
            slot: std::collections::HashMap::new(),
            resident: vec![None; capacity],
            stamp: vec![0; capacity],
            origins: vec![[0.0; 3]; capacity],
            cursor: 0,
            frame: 0,
            vertex_buffer: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("patch vertices"),
                size: vertices * VERTEX_BYTES,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            cone_buffer: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("patch cones"),
                size: capacity as u64 * CONE_BYTES,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        }
    }

    /// Слот патча — той, що вже є, або щойно зайнятий і заповнений.
    fn intern(&mut self, gpu: &Gpu, patch: Patch) -> u32 {
        if let Some(&slot) = self.slot.get(&patch) {
            self.stamp[slot as usize] = self.frame;
            return slot;
        }

        // Слот, не потрібний цього кадру. Він є завжди: місткість тримається
        // вдвічі більшою за потребу (див. `Planet::reserve`).
        let slot = loop {
            let candidate = self.cursor;
            self.cursor = (self.cursor + 1) % self.capacity;
            if self.stamp[candidate] < self.frame {
                break candidate;
            }
        };

        if let Some(old) = self.resident[slot] {
            self.slot.remove(&old);
        }

        let mesh = patch.mesh(1.0);
        let base = (slot * PATCH_VERTICES) as u64 * VERTEX_BYTES;

        // Розкладка `PatchVertex` у std430: два `vec3` з вирівнюванням 16,
        // тобто 32 байти на вершину з чотирма нулями в кожному хвості.
        // Виписано руками з тієї самої причини, що й `Uniforms::to_bytes`:
        // наш `unsafe` живе лише в `core-rs`.
        let mut bytes = Vec::with_capacity(PATCH_VERTICES * VERTEX_BYTES as usize);
        for (offset, normal) in mesh.offsets.iter().zip(mesh.normals.iter()) {
            for value in offset {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(&0.0f32.to_le_bytes());
            for value in normal {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(&0.0f32.to_le_bytes());
        }
        gpu.queue.write_buffer(&self.vertex_buffer, base, &bytes);

        // Конус — розкладка `Cone` у std430: `vec3` з вирівнюванням 16, потім
        // два `float`, з яких другий знову вирівняний на 16.
        let cone = patch.cone();
        let mut cone_bytes = Vec::with_capacity(CONE_BYTES as usize);
        for value in cone.axis {
            cone_bytes.extend_from_slice(&(value as f32).to_le_bytes());
        }
        cone_bytes.extend_from_slice(&(cone.cos_half as f32).to_le_bytes());
        cone_bytes.extend_from_slice(&(cone.sin_half as f32).to_le_bytes());
        cone_bytes.resize(CONE_BYTES as usize, 0);
        gpu.queue
            .write_buffer(&self.cone_buffer, slot as u64 * CONE_BYTES, &cone_bytes);

        self.resident[slot] = Some(patch);
        self.origins[slot] = mesh.origin;
        self.stamp[slot] = self.frame;
        self.slot.insert(patch, slot as u32);
        slot as u32
    }
}

impl Planet {
    fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Planet {
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("patch"),
                source: wgpu::ShaderSource::Wgsl(PATCH_WGSL.into()),
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
                            // Один буфер на тіло, зсув — на прохід (R4a).
                            // Інакше кількість буферів множилася б на
                            // кількість діапазонів, а вони — те, що міняється.
                            has_dynamic_offset: true,
                            min_binding_size: std::num::NonZeroU64::new(UNIFORM_BYTES),
                        },
                        count: None,
                    },
                    // 1 — початки патчів і номери тайлів (за слотом кеша),
                    // 2 — геометрія всіх патчів кеша,
                    // 3 — список того, що малюється цього кадру (за інстансом).
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
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
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

        // ⚠ Масив висот — **окрема група**, і це вимога wgpu, а не смак:
        // «bind groups may not contain both a binding array and a dynamically
        // offset buffer». Динамічний зсув у групі 0 вибирає прохід глибини
        // (R4a) і нікуди не подінеться, тож розійтися мусив масив.
        let tile_layout = gpu.bindless.then(|| {
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("patch tiles"),
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Sint,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: std::num::NonZeroU32::new(MAX_TILES),
                    }],
                })
        });

        let layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("patch"),
                bind_group_layouts: &[Some(&bind_layout), tile_layout.as_ref()],
                immediate_size: 0,
            });

        // Вершинних буферів більше немає взагалі (R6a): усе, що читає
        // вершинна стадія, приходить storage-буферами, а номер вершини
        // й номер інстансу дає сам конвеєр.
        let build = |vertex: &str, fragment: &str| {
            gpu.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("patch"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some(vertex),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some(fragment),
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
                })
        };

        let cull_module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("cull"),
                source: wgpu::ShaderSource::Wgsl(CULL_WGSL.into()),
            });

        let storage = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let cull_layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("cull"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    storage(1, true),
                    storage(2, true),
                    storage(3, true),
                    storage(4, false),
                    storage(5, false),
                ],
            });
        let cull_pipeline_layout =
            gpu.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("cull"),
                    bind_group_layouts: &[Some(&cull_layout)],
                    immediate_size: 0,
                });
        let cull_pipeline = gpu
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("cull"),
                layout: Some(&cull_pipeline_layout),
                module: &cull_module,
                entry_point: Some("cull_main"),
                compilation_options: Default::default(),
                cache: None,
            });

        let no_tiles = tile_layout.as_ref().map(|layout| {
            let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("no terrain"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R16Sint,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("no terrain"),
                layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureViewArray(&[&view]),
                }],
            })
        });

        let smooth = build("vertex_smooth", "fragment_smooth");
        let terrain = gpu
            .bindless
            .then(|| build("vertex_terrain", "fragment_terrain"));

        Planet {
            smooth,
            terrain,
            terrains: Vec::new(),
            bind_layout,
            tile_layout,
            cull_pipeline,
            cull_layout,
            no_tiles,
            cache: PatchCache::new(gpu, MIN_PATCHES),
            bodies: Vec::new(),
            selections: Vec::new(),
        }
    }

    /// Слот під тіло — свій uniform, свої початки патчів, своя bind-група.
    fn slot(&self, gpu: &Gpu) -> BodySlot {
        let uniform_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("patch uniforms"),
            size: PASS_STRIDE * MAX_PASSES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Вісім слів на патч: початок (три) плюс номер тайла, тоді вікно в
        // тайлі (зсув-два й крок) і одне слово запасу на вирівнювання
        // (R7a). Індексується слотом кеша, тобто буфер рівно такий, як кеш.
        let origin_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("patch origins"),
            size: (self.cache.capacity * PATCH_DATA_BYTES) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Список того, що справді малюється: його пише compute, а читає
        // вершинна стадія. Полем структури він не стає — обидві bind-групи
        // тримають його самі, а поле, якого ніхто не читає, гірше за свою
        // відсутність (CLAUDE.md).
        //
        // По вісім байтів на інстанс, і інстансів не більше за місткість
        // кеша: патч, якого немає в кеші, намалювати нема з чого.
        let draw_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("patch draws"),
            size: (self.cache.capacity * 8) as u64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let candidate_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("patch candidates"),
            size: (self.cache.capacity * 8) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cull_uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cull params"),
            size: CULL_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // `COPY_SRC` тут не для кадру, а для перевірки: R6b звіряє кількість
        // намальованих патчів із CPU-відбором, і прочитати її можна лише
        // звідси (`Frame::drawn_patches`).
        let indirect_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("patch indirect"),
            size: 16,
            usage: wgpu::BufferUsages::INDIRECT
                | wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let cull_bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cull"),
            layout: &self.cull_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: cull_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: candidate_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.cache.cone_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: origin_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: draw_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: indirect_buffer.as_entire_binding(),
                },
            ],
        });

        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("patch"),
            layout: &self.bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &uniform_buffer,
                        offset: 0,
                        size: std::num::NonZeroU64::new(UNIFORM_BYTES),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: origin_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.cache.vertex_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: draw_buffer.as_entire_binding(),
                },
            ],
        });

        BodySlot {
            uniform_buffer,
            origin_buffer,
            candidate_buffer,
            cull_uniform,
            indirect_buffer,
            cull_bind_group,
            candidates: 0,
            bind_group,
            terrain: None,
        }
    }

    /// Місткість кеша під потребу кадру — **вдвічі** більша за неї.
    ///
    /// Удвічі, а не рівно: витіснення шукає слот, не потрібний цього кадру, і
    /// при місткості впритул такого слота могло б не бути взагалі. Запас
    /// перетворює пошук на завжди успішний, і саме тому в ньому немає гілки
    /// «а якщо ні».
    ///
    /// Росте й не спадає, а зростання скидає кеш: буфери перестворюються, і
    /// те, що в них лежало, більше не за тими адресами. Платиться це один раз
    /// на кожне подвоєння за всю сесію.
    fn reserve(&mut self, gpu: &Gpu, needed: usize) {
        if needed * 2 <= self.cache.capacity {
            return;
        }
        let capacity = (needed * 2).next_power_of_two().max(MIN_PATCHES);
        self.cache = PatchCache::new(gpu, capacity);
        // Буфери початків були під стару місткість, а bind-групи — під ті
        // буфери. Слоти тіл перестворяться при наступному ж проході.
        self.bodies.clear();
    }

    /// Набори патчів, їхня геометрія й початки — усе, що кадр рахує на CPU.
    ///
    /// Оце і є прохід планети: вибір рівня на тіло, звіряння з кешем і
    /// віднімання камери від початку кожного патча, у `double`.
    fn upload(&mut self, gpu: &Gpu, scene: &Scene, passes: &[Pass], width_px: f64, height_px: f64) {
        let aspect = width_px / height_px;
        let camera = &scene.camera;
        let eye = camera.position();
        let focal = lod::focal_px(FOV_Y, height_px);

        // Вибір рівня — на тіло, і до всякого дотику до GPU: місткість кеша
        // мусить бути відома до першого `intern`.
        self.selections.clear();
        let mut needed = 0;
        for body in &scene.bodies {
            // **Стелю даних знято (R7a).** До цього кроку вибір не йшов
            // глибше за піраміду тайлів, і причина була не в кількості
            // деталей, а в адресації: патч глибшого рівня читав би тайл
            // предка за **своїми** локальними координатами, тобто не в тому
            // місці. Тепер вікно в тайлі приїжджає в `PatchData`
            // (`Terrain::window`), і глибший патч читає підпрямокутник предка
            // білінійно — рівно те, що `Terrain::height_m` рахує на CPU.
            //
            // Само по собі це нових висот не додає: інтерполяція між вузлами
            // LOLA — це та сама поверхня, лише дрібнішою сіткою. Сенс з'явиться
            // з процедурною деталлю (R7c), якій треба, куди сідати; передумова
            // ж мусила бути закрита окремо й з власним оракулом.
            let ceiling = lod::MAX_LEVEL;
            // **Рельєф входить у вибір рівня (R7c).** Без нього критерій
            // питає лише про стрілу прогину сфери, а сфера зблизька пласка:
            // на кілометрі над Місяцем клітинка виходила 2665 м, тобто 1662
            // пікселі завширшки, і в неї не влазив ні шум, ні сам DEM (вузол
            // 5330 м). Тіло без тайлів лишається гладким і рахується як
            // раніше — бітово.
            let terrain = match body.tiles {
                scene::TileSet::Loaded(id) => self.terrains.get(id.0).map(|slot| &slot.data),
                scene::TileSet::Smooth => None,
            };
            let selection = lod::select(
                &lod::Body {
                    centre: body.centre,
                    radius_m: body.radius_m,
                    rotation: rotation(body.orientation),
                    max_level: ceiling,
                },
                camera,
                focal,
                terrain,
            );
            needed += selection.patches.len();
            self.selections.push(selection);
        }
        self.reserve(gpu, needed);
        self.cache.frame += 1;

        while self.bodies.len() < scene.bodies.len() {
            let slot = self.slot(gpu);
            self.bodies.push(slot);
        }

        // Поворот вигляду однаковий для всіх тіл і всіх проходів — множиться
        // раз, а не на тіло й не на прохід.
        let view_rotation = camera.view_rotation();

        let mut origin_bytes: Vec<u8> = Vec::with_capacity(self.cache.capacity * PATCH_DATA_BYTES);
        let mut draw_bytes: Vec<u8> = Vec::with_capacity(self.cache.capacity * 8);

        for (index, body) in scene.bodies.iter().enumerate() {
            let rotation = rotation(body.orientation);
            let selection = &self.selections[index];

            // **Відбір переїхав у compute (R6b).** Сюди подається весь
            // вибраний набір; що з нього намалювати, вирішує GPU. CPU-шлях
            // (`crate::cull`) лишається — але як другий незалежний шлях до
            // того самого числа, тобто як оракул, а не як робота кадру.
            draw_bytes.clear();
            for (patch, &mask) in selection.patches.iter().zip(&selection.masks) {
                let slot = self.cache.intern(gpu, *patch);
                draw_bytes.extend_from_slice(&slot.to_le_bytes());
                draw_bytes.extend_from_slice(&u32::from(mask).to_le_bytes());
            }
            let candidates = (draw_bytes.len() / 8) as u32;
            self.bodies[index].candidates = candidates;
            if !draw_bytes.is_empty() {
                gpu.queue
                    .write_buffer(&self.bodies[index].candidate_buffer, 0, &draw_bytes);
            }

            // Параметри відбору: те саме, що рахує `cull::horizon` і
            // `cull::frustum`, лише один раз на тіло замість разу на патч.
            let in_body = lod::Body {
                rotation,
                ..lod::Body::still(body.centre, body.radius_m)
            };
            let to_eye = {
                let d = in_body.eye_in_body(eye);
                let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1.0);
                [d[0] / n, d[1] / n, d[2] / n]
            };
            let distance = {
                let d = [
                    eye[0] - body.centre[0],
                    eye[1] - body.centre[1],
                    eye[2] - body.centre[2],
                ];
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
            };
            let limb = cull::limb_cos(
                &cull::Body::smooth(body.centre, body.radius_m, rotation),
                distance,
            );

            let t = (FOV_Y / 2.0).tan();
            let (tx, ty) = (aspect * t, t);
            let mut vectors: Vec<[f32; 4]> = Vec::with_capacity(5);
            vectors.push([
                to_eye[0] as f32,
                to_eye[1] as f32,
                to_eye[2] as f32,
                limb as f32,
            ]);
            // Рядки повороту вигляду: `view_rotation` лежить стовпцями, тож
            // рядок — це однойменні компоненти трьох перших стовпців.
            let [right, up, back, _] = view_rotation;
            for row in [0, 1, 2] {
                vectors.push([right[row], up[row], back[row], 0.0]);
            }
            vectors.push([
                tx as f32,
                ty as f32,
                (1.0 / (1.0 + tx * tx).sqrt()) as f32,
                (1.0 / (1.0 + ty * ty).sqrt()) as f32,
            ]);

            let mut cull_bytes: Vec<u8> = Vec::with_capacity(CULL_BYTES as usize);
            for vector in &vectors {
                for value in vector {
                    cull_bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
            for value in [candidates, 0, 0, 0] {
                cull_bytes.extend_from_slice(&value.to_le_bytes());
            }
            for value in [body.radius_m as f32, 0.0, 0.0, 0.0] {
                cull_bytes.extend_from_slice(&value.to_le_bytes());
            }
            gpu.queue
                .write_buffer(&self.bodies[index].cull_uniform, 0, &cull_bytes);

            // Аргументи indirect: скільки вершин у патчі, і нуль інстансів —
            // лічильник, який compute нарощує атомарно.
            let mut args = Vec::with_capacity(16);
            for value in [PATCH_INDICES as u32, 0, 0, 0] {
                args.extend_from_slice(&value.to_le_bytes());
            }
            gpu.queue
                .write_buffer(&self.bodies[index].indirect_buffer, 0, &args);

            // Рельєф тіла, якщо він є: множник висоти й таблиця тайлів.
            let wanted = match body.tiles {
                TileSet::Smooth => None,
                TileSet::Loaded(id) => (id.0 < self.terrains.len()).then_some(id.0),
            };
            self.bodies[index].terrain = wanted;
            let terrain = wanted.and_then(|id| self.terrains.get(id));
            let height_scale = terrain
                .map(|t| t.scale_m / body.radius_m as f32)
                .unwrap_or(0.0);

            // Початки — на **слот**, а не на позицію в наборі: так номер
            // патча живе у вершинному буфері й не переписується щокадру.
            // Слоти, не зайняті цим тілом, лишаються нулями й не малюються.
            origin_bytes.clear();
            for (slot, origin) in self.cache.origins.iter().enumerate() {
                // Усе в `double`: поворот одиничного початку, множення на
                // радіус, зсув до центра тіла й віднімання камери. Звуження до
                // `f32` — останнім кроком, як завжди (ROADMAP F4).
                for k in 0..3 {
                    let turned = rotation[k][0] * origin[0]
                        + rotation[k][1] * origin[1]
                        + rotation[k][2] * origin[2];
                    let value = (body.centre[k] + body.radius_m * turned - eye[k]) as f32;
                    origin_bytes.extend_from_slice(&value.to_le_bytes());
                }
                // Четверте слово — номер тайла в bindless-масиві, а не
                // вирівнювальний нуль: `PatchData` у шейдері саме такий.
                //
                // Далі — **вікно в цьому тайлі** (R7a): зсув патча всередині
                // нього у вузлах і крок. Для патча, який має власний тайл, це
                // `(0, 0)` і `1`, тобто те саме, що робив точний `Load`. Для
                // глибшого — підпрямокутник предка, і рахує його
                // `Terrain::window`, той самий код, що й `height_m` на CPU.
                let (tile, origin_uv, step, delta) = match (terrain, self.cache.resident[slot]) {
                    (Some(t), Some(patch)) => {
                        let (index, origin_uv, step) = t.data.window(&patch);
                        (index as u32, origin_uv, step, t.data.delta_nodes(&patch))
                    }
                    _ => (0, [0.0, 0.0], 1.0, 1.0),
                };
                origin_bytes.extend_from_slice(&tile.to_le_bytes());
                origin_bytes.extend_from_slice(&(origin_uv[0] as f32).to_le_bytes());
                origin_bytes.extend_from_slice(&(origin_uv[1] as f32).to_le_bytes());
                origin_bytes.extend_from_slice(&(step as f32).to_le_bytes());
                // Восьме слово — крок центральної різниці у вузлах цього ж
                // тайла (R7c). Було вирівнювальним нулем; місце під нього тут
                // і трималося.
                origin_bytes.extend_from_slice(&(delta as f32).to_le_bytes());
            }
            let slot = &self.bodies[index];
            gpu.queue
                .write_buffer(&slot.origin_buffer, 0, &origin_bytes);

            let model = model_matrix(rotation, body.radius_m);
            for (k, plan) in passes.iter().enumerate() {
                let uniforms = Uniforms {
                    projection: depth::multiply(plan.projection, view_rotation),
                    model,
                    light_dir: [LIGHT_DIR[0], LIGHT_DIR[1], LIGHT_DIR[2], 0.0],
                    colour: COLOUR,
                    terrain: [height_scale, 0.0, 0.0, 0.0],
                    // Процедурний детайл (R7c). Гладке тіло дістає нулі: без
                    // тайлів нахилу нема звідки взяти, а деталь без нахилу —
                    // це рівний килим, тобто рівно те, чого крок не робить.
                    detail: match terrain {
                        Some(t) => [
                            body.radius_m as f32,
                            t.data.slope_rise() as f32,
                            detail::base_m(body.radius_m) as f32,
                            focal as f32,
                        ],
                        None => [0.0; 4],
                    },
                };
                gpu.queue.write_buffer(
                    &slot.uniform_buffer,
                    k as u64 * PASS_STRIDE,
                    &uniforms.to_bytes(),
                );
            }
        }
    }

    /// Відбір у compute: по групі на 64 кандидати, на тіло.
    fn cull(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("cull"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.cull_pipeline);
        for body in &self.bodies {
            if body.candidates == 0 {
                continue;
            }
            pass.set_bind_group(0, &body.cull_bind_group, &[]);
            pass.dispatch_workgroups(body.candidates.div_ceil(CULL_GROUP), 1, 1);
        }
    }

    fn draw(&self, pass: &mut wgpu::RenderPass<'_>, index: usize) {
        let offset = (index as u64 * PASS_STRIDE) as u32;

        // Виклик на **тіло**, а не на патч (R6a). Вершинних буферів немає
        // взагалі: геометрія, початки й список того, що малюється, приходять
        // storage-буферами, а номер вершини й номер інстансу дає конвеєр.
        for body in &self.bodies {
            if body.candidates == 0 {
                continue;
            }
            pass.set_pipeline(match (&self.terrain, body.terrain) {
                (Some(terrain), Some(_)) => terrain,
                _ => &self.smooth,
            });
            pass.set_bind_group(0, &body.bind_group, &[offset]);
            match body.terrain.and_then(|id| self.terrains.get(id)) {
                Some(slot) => pass.set_bind_group(1, &slot.bind_group, &[]),
                None => {
                    if let Some(empty) = &self.no_tiles {
                        pass.set_bind_group(1, empty, &[]);
                    }
                }
            }
            // Скільки інстансів — знає лише GPU: лічильник наростив compute.
            pass.draw_indirect(&body.indirect_buffer, 0);
        }
    }
}

impl Ships {
    fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Ships {
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("ship"),
                source: wgpu::ShaderSource::Wgsl(SHIP_WGSL.into()),
            });

        let bind_layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("ship"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        // Зсув на прохід, як у ламаних і патчів.
                        has_dynamic_offset: true,
                        min_binding_size: std::num::NonZeroU64::new(SHIP_UNIFORM_BYTES),
                    },
                    count: None,
                }],
            });

        let layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("ship"),
                bind_group_layouts: &[Some(&bind_layout)],
                immediate_size: 0,
            });

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
        let colour_attrs = [wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 0,
            shader_location: 2,
        }];

        let pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("ship"),
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
                // Без відсікання граней, з тієї самої причини, що у сфери:
                // корпус замкнений, і найближчу поверхню вибирає тест
                // глибини. Оболонки корабля до того ж перетинаються
                // (стабілізатор входить у корпус), тож правильного «зовні»
                // для спільного об'єму не існує взагалі.
                primitive: wgpu::PrimitiveState {
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

        let uniform_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ship uniforms"),
            size: PASS_STRIDE * MAX_PASSES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ship"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniform_buffer,
                    offset: 0,
                    size: std::num::NonZeroU64::new(SHIP_UNIFORM_BYTES),
                }),
            }],
        });

        // Одинична висота: масштаб прикладає CPU разом із поворотом.
        let mesh = crate::ship::generate(1.0);
        let index_bytes: Vec<u8> = mesh.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let index_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ship indices"),
            size: index_bytes.len() as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&index_buffer, 0, &index_bytes);

        let index_count = mesh.indices.len() as u32;
        let vertices_per_ship = mesh.positions.len();
        let (position_buffer, normal_buffer, colour_buffer) =
            Ships::buffers(gpu, vertices_per_ship);

        Ships {
            pipeline,
            bind_group,
            uniform_buffer,
            index_buffer,
            index_count,
            vertices_per_ship,
            mesh,
            position_buffer,
            normal_buffer,
            colour_buffer,
            capacity: vertices_per_ship,
            position_bytes: Vec::new(),
            normal_bytes: Vec::new(),
            colour_bytes: Vec::new(),
        }
    }

    fn buffers(gpu: &Gpu, vertices: usize) -> (wgpu::Buffer, wgpu::Buffer, wgpu::Buffer) {
        let make = |label: &str, stride: usize| {
            gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (vertices * stride) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        (
            make("ship positions", 12),
            make("ship normals", 12),
            make("ship colours", 16),
        )
    }

    /// Позиції — camera-relative у `f64`, нормалі — повернуті в світ.
    ///
    /// Світова позиція вершини будується як `центр + R·(h·s)`, тобто в тому
    /// самому порядку, що початок патча (R1d): множення на висоту йде **до**
    /// віднімання камери, і жодне мале число не додається до великого двічі.
    fn upload(&mut self, gpu: &Gpu, scene: &Scene, passes: &[Pass]) {
        if scene.ships.is_empty() {
            return;
        }

        let needed = scene.ships.len() * self.vertices_per_ship;
        if needed > self.capacity {
            self.capacity = needed.next_power_of_two();
            let (position, normal, colour) = Ships::buffers(gpu, self.capacity);
            self.position_buffer = position;
            self.normal_buffer = normal;
            self.colour_buffer = colour;
        }

        self.position_bytes.clear();
        self.normal_bytes.clear();
        self.colour_bytes.clear();

        for ship in &scene.ships {
            let r = rotation(ship.orientation);
            let turn = |v: [f64; 3]| {
                [
                    r[0][0] * v[0] + r[0][1] * v[1] + r[0][2] * v[2],
                    r[1][0] * v[0] + r[1][1] * v[1] + r[1][2] * v[2],
                    r[2][0] * v[0] + r[2][1] * v[1] + r[2][2] * v[2],
                ]
            };

            for (local, normal) in self.mesh.positions.iter().zip(&self.mesh.normals) {
                let offset = turn([
                    local[0] * ship.height_m,
                    local[1] * ship.height_m,
                    local[2] * ship.height_m,
                ]);
                let world = [
                    ship.centre[0] + offset[0],
                    ship.centre[1] + offset[1],
                    ship.centre[2] + offset[2],
                ];
                for value in scene.camera.relative(world) {
                    self.position_bytes.extend_from_slice(&value.to_le_bytes());
                }

                let n = turn([
                    f64::from(normal[0]),
                    f64::from(normal[1]),
                    f64::from(normal[2]),
                ]);
                for value in n {
                    self.normal_bytes
                        .extend_from_slice(&(value as f32).to_le_bytes());
                }

                for value in ship.colour {
                    self.colour_bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
        }

        let mut uniform_bytes = Vec::with_capacity(SHIP_UNIFORM_BYTES as usize);
        for (k, plan) in passes.iter().enumerate() {
            uniform_bytes.clear();
            for column in plan.projection {
                for value in column {
                    uniform_bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
            for value in [LIGHT_DIR[0], LIGHT_DIR[1], LIGHT_DIR[2], 0.0] {
                uniform_bytes.extend_from_slice(&value.to_le_bytes());
            }
            gpu.queue
                .write_buffer(&self.uniform_buffer, k as u64 * PASS_STRIDE, &uniform_bytes);
        }

        gpu.queue
            .write_buffer(&self.position_buffer, 0, &self.position_bytes);
        gpu.queue
            .write_buffer(&self.normal_buffer, 0, &self.normal_bytes);
        gpu.queue
            .write_buffer(&self.colour_buffer, 0, &self.colour_bytes);
    }

    fn draw(&self, pass: &mut wgpu::RenderPass<'_>, scene: &Scene, index: usize) {
        if scene.ships.is_empty() {
            return;
        }

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[(index as u64 * PASS_STRIDE) as u32]);
        pass.set_vertex_buffer(0, self.position_buffer.slice(..));
        pass.set_vertex_buffer(1, self.normal_buffer.slice(..));
        pass.set_vertex_buffer(2, self.colour_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);

        // Виклик на корабель: індекси спільні, зсуває їх `base_vertex`.
        for k in 0..scene.ships.len() {
            let base = (k * self.vertices_per_ship) as i32;
            pass.draw_indexed(0..self.index_count, base, 0..1);
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
                        // Як і в патчів: зсув на прохід, а не буфер на прохід.
                        has_dynamic_offset: true,
                        min_binding_size: std::num::NonZeroU64::new(LINE_UNIFORM_BYTES),
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
            size: PASS_STRIDE * MAX_PASSES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("line"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniform_buffer,
                    offset: 0,
                    size: std::num::NonZeroU64::new(LINE_UNIFORM_BYTES),
                }),
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

    fn upload(&mut self, gpu: &Gpu, scene: &Scene, passes: &[Pass]) {
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

        let mut uniform_bytes = Vec::with_capacity(LINE_UNIFORM_BYTES as usize);
        for (k, plan) in passes.iter().enumerate() {
            uniform_bytes.clear();
            for column in plan.projection {
                for value in column {
                    uniform_bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
            gpu.queue
                .write_buffer(&self.uniform_buffer, k as u64 * PASS_STRIDE, &uniform_bytes);
        }
        gpu.queue
            .write_buffer(&self.position_buffer, 0, &self.position_bytes);
        gpu.queue
            .write_buffer(&self.colour_buffer, 0, &self.colour_bytes);
    }

    fn draw(&self, pass: &mut wgpu::RenderPass<'_>, scene: &Scene, index: usize) {
        if scene.vertex_count() == 0 {
            return;
        }

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[(index as u64 * PASS_STRIDE) as u32]);
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
            air: None,
        });
        scene.bodies.push(Body {
            centre: moon_centre,
            radius_m: moon_radius,
            orientation: [1.0, 0.0, 0.0, 0.0],
            tiles: TileSet::Smooth,
            air: None,
        });

        let near = Frame::near_for(&scene);
        assert!(
            (near - altitude / 10.0).abs() < 1.0,
            "near {near} м — це не десята частина висоти над Місяцем"
        );
    }

    /// Умова аеральної перспективи — товщина шару в пікселях (S5).
    ///
    /// Три точки, і кожна називає свій випадок. Зблизька число велике й об'єм
    /// потрібен; на 10⁹ м воно менше за соту пікселя, тобто об'єм 32×32×32
    /// рахувався б заради нічого. Між ними є висота, на якій воно рівно
    /// одиниця, і вона виводиться з формули: `товщина · фокус`, тобто
    /// 6.24·10⁷ м для стокілометрового шару в кадрі 1280×720.
    ///
    /// Виміряно, скільки це коштує: 0.49 мс проти 0.23 мс на 6·10⁷ і 6.5·10⁷ м
    /// відповідно, тобто об'єм подвоює кадр там, де він ще потрібен, і не
    /// коштує нічого там, де вже ні.
    /// Ближня площина відходить від корабля, а не від тіла під ним.
    ///
    /// Це і є весь крок V2 одним числом. До нього `near` виводилася з висоти
    /// над найближчим тілом: на орбіті 400 км вона ставала 40 км, тобто все,
    /// що ближче за сорок кілометрів, зникало з кадру — а корабель стоїть за
    /// п'ятнадцять метрів.
    #[test]
    fn the_near_plane_lets_the_ship_in() {
        let altitude = 400_000.0;
        let radius = sphere::EARTH_RADIUS_M;
        let eye = [radius + altitude, 0.0, 0.0];
        let camera = Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);

        let mut scene = Scene::new(camera);
        scene.bodies.push(scene::Body {
            centre: [0.0, 0.0, 0.0],
            radius_m: radius,
            orientation: [1.0, 0.0, 0.0, 0.0],
            tiles: scene::TileSet::Smooth,
            air: None,
        });

        let without = Frame::near_for(&scene);
        assert!(
            without > 1000.0,
            "без корабля near мала лишитись величиною висоти: {without}"
        );

        // Корабель за п'ятнадцять метрів перед камерою, тобто трохи нижче.
        let distance = 15.0;
        scene.ships.push(scene::Ship {
            centre: [eye[0] - distance, 0.0, 0.0],
            orientation: [1.0, 0.0, 0.0, 0.0],
            height_m: crate::ship::DEFAULT_HEIGHT_M,
            extent_m: 0.5 * crate::ship::DEFAULT_HEIGHT_M,
            colour: [0.7, 0.7, 0.75, 1.0],
        });

        let with = Frame::near_for(&scene);
        let hull = distance - 0.5 * crate::ship::DEFAULT_HEIGHT_M;
        assert!(
            with < hull,
            "near {with} не пропускає корпус, найближча точка якого за {hull} м"
        );
    }

    #[test]
    fn the_aerial_volume_is_skipped_when_the_air_is_thinner_than_a_pixel() {
        let air = scene::Atmosphere::EARTH.with_surface(sphere::EARTH_RADIUS_M);
        let bottom = sphere::EARTH_RADIUS_M;
        let focal = lod::focal_px(FOV_Y, 720.0);

        let at = |altitude: f64| {
            let view = sky::View {
                eye: [bottom + altitude, 0.0, 0.0],
                sun: [1.0, 0.0, 0.0],
                right: [0.0, 1.0, 0.0],
                up: [0.0, 0.0, 1.0],
                forward: [-1.0, 0.0, 0.0],
                tan_half: [1.0, 0.577],
            };
            Frame::shell_px(&air, bottom, &view, focal)
        };

        // Всередині повітря — на порядки більше за піксель.
        assert!(at(1.0e4) > 1000.0, "{}", at(1.0e4));
        // З 10⁹ м — соті пікселя: шейдити крізь повітря нема чого.
        assert!(at(1.0e9) < 0.1, "{}", at(1.0e9));
        // Межа рівно там, де каже формула.
        let threshold = air.thickness_m(bottom) * focal;
        assert!((at(threshold) - 1.0).abs() < 1.0e-9, "{}", at(threshold));
        assert!(at(threshold * 1.01) < 1.0 && at(threshold * 0.99) > 1.0);
    }

    /// Корабель за метри й планета за мільйони метрів в одному кадрі — це
    /// **два** діапазони глибини, і це перша сцена, у якій їх більше одного
    /// (V3).
    ///
    /// Число, з якого воно виходить, виміряно на F3: один буфер тримає сім
    /// порядків. Тут розмах `far/near` = 1.15·10⁷, тобто 7.06 порядка — на
    /// шість сотих більше за той, що вміщається в один прохід. Тому сцена
    /// зондів рушія (розмах 22.7) лишається однопрохідною, а ця — ні.
    ///
    /// Межа перевіряється не «приблизно там»: вона мусить лягти в порожнечу
    /// **між** корпусом і поверхнею, і обидва краї названі числами сцени, а не
    /// константами тесту.
    #[test]
    fn a_ship_over_a_planet_needs_two_depth_ranges() {
        let scene = crate::ship_demo::scene_at(0, crate::ship_demo::FRAMES);
        let ranges = Frame::depth_ranges(&scene, 16.0 / 9.0);

        assert_eq!(ranges.len(), 2, "діапазонів мало б бути два: {ranges:?}");

        let eye = scene.camera.position();
        let range = |p: [f64; 3]| {
            let d = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
        };
        let ship = &scene.ships[0];
        let hull_far = range(ship.centre) + ship.extent_m;
        let body = &scene.bodies[0];
        let surface = range(body.centre) - body.radius_m;

        // Ближня площина — те саме число, що дає `near_for`; межа — наступна.
        assert!(
            (ranges[0] - Frame::near_for(&scene)).abs() < 1.0e-9,
            "ближня площина плану розійшлася з near_for: {ranges:?}"
        );
        let boundary = ranges[1];
        assert!(
            boundary > hull_far,
            "межа {boundary} м ріже корпус, який тягнеться до {hull_far} м"
        );
        assert!(
            boundary < surface,
            "межа {boundary} м лежить під поверхнею, до якої {surface} м"
        );
    }

    /// Розвилка кроку V3, закрита арифметикою, а не одним кадром: **межа
    /// діапазонів не потрапляє в корпус ніколи**.
    ///
    /// Доведення коротке, і саме тому воно варте тесту, а не коментаря. Другий
    /// діапазон з'являється лише при розмаху понад 10⁷, тож найменше можливе
    /// відношення сусідніх меж — `√10⁷ ≈ 3162` (три й чотири діапазони роблять
    /// його ще більшим). Межа стоїть на `near·3162`, а `near` — це десята
    /// частина відстані до найближчої точки корпусу. Тобто межа не ближча за
    /// 316 висот, а корпус тягнеться щонайбільше на два своїх габарити.
    /// Зійтися вони могли б хіба тоді, коли камера впритул до корпусу — а там
    /// `near` впирається в поріг 0.1 м і межа все одно лишається за сотні
    /// метрів.
    ///
    /// Перевірка — свіп, а не одна точка, і напрямок на камеру навмисно
    /// несиметричний: фікстура, що стоїть рівно над центром грані куба, вже
    /// ховала дві помилки поспіль (D13, D14).
    #[test]
    fn the_range_boundary_never_falls_inside_the_hull() {
        let radius = sphere::EARTH_RADIUS_M;
        let height = crate::ship::DEFAULT_HEIGHT_M;
        let extent = 0.5 * height;

        // Косий напрямок: жодна вісь не збігається з віссю світу.
        let dir = {
            let v: [f64; 3] = [0.37, -0.51, 0.77];
            let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            [v[0] / n, v[1] / n, v[2] / n]
        };

        // Від «камера торкається корпусу» до «корабель — крапка в кадрі», і
        // висоти орбіти теж різні: від дотику до поверхні до геостаціонарної.
        for altitude in [1.0e3, 4.0e5, 3.6e7] {
            let centre = [
                dir[0] * (radius + altitude),
                dir[1] * (radius + altitude),
                dir[2] * (radius + altitude),
            ];
            for step in 0..40 {
                let distance = extent * 1.001 * 1.3_f64.powi(step);
                let eye = [
                    centre[0] + dir[0] * distance,
                    centre[1] + dir[1] * distance,
                    centre[2] + dir[2] * distance,
                ];
                let camera = Camera::look_at(eye, centre, [0.0, 0.0, 1.0]);

                let mut scene = Scene::new(camera);
                scene.bodies.push(Body {
                    centre: [0.0, 0.0, 0.0],
                    radius_m: radius,
                    orientation: [1.0, 0.0, 0.0, 0.0],
                    tiles: TileSet::Smooth,
                    air: None,
                });
                scene.ships.push(scene::Ship {
                    centre,
                    orientation: [1.0, 0.0, 0.0, 0.0],
                    height_m: height,
                    extent_m: extent,
                    colour: [0.7, 0.7, 0.75, 1.0],
                });

                let ranges = Frame::depth_ranges(&scene, 16.0 / 9.0);

                // Ближня площина пропускає корпус — те саме твердження, що
                // в V2, але тепер на всьому свіпі, а не в одній точці.
                let near = Frame::near_for(&scene);
                let hull_near = (distance - extent).max(0.0);
                assert!(
                    near <= hull_near.max(0.1),
                    "висота {altitude}, відстань {distance}: near {near} ріже корпус з {hull_near}"
                );

                let Some(&boundary) = ranges.get(1) else {
                    continue;
                };
                let hull_far = distance + extent;
                assert!(
                    boundary > hull_far,
                    "висота {altitude}, відстань {distance}: межа {boundary} впала в корпус,
                     який тягнеться до {hull_far} м"
                );
            }
        }
    }
}
