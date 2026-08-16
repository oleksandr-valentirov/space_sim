//! Проліт повз Місяць по еліптичній орбіті — анімація (зонд етапу T).
//!
//! Той самий жанр, що [`crate::ship_demo`] і [`crate::moon_demo`], і з тієї
//! самої причини: показати **тим самим** [`Frame`], який іде у вікно, те, що
//! етап щойно зробив можливим. Тут воно показано разом, бо разом його ще
//! ніде не було видно:
//!
//! - колір поверхні з мозаїки LROC WAC і рельєф LOLA під самим кораблем;
//! - корпус із Blender (T5d) з GGX-матеріалом металу (T5c);
//! - **сяйво Місяця на тіньовий бік корпусу** (T6): воно міняється і з
//!   висотою — форм-фактор диска падає як `sin²θ`, — і з тим, над чим
//!   корабель летить, бо альбедо береться з асета;
//! - тонмапер (T5c3): відблиск на корпусі виходить за одиницю й без нього
//!   злипся б у білу пляму;
//! - **два тіла в кадрі з різницею відстаней у два порядки**: Місяць за
//!   тисячі кілометрів і Земля за 384 400 км (R1e й діапазони глибини V3).
//!
//!     cargo run --release -p engine -- --flyby-demo build/flyby.apng
//!
//! ## Композиція вибрана числами, а не оком
//!
//! Три вимоги: Місяць у центрі кадру, Земля на тлі, плавний рух. Кожна щось
//! визначає, і жодну не можна виконати «приблизно».
//!
//! **Місяць у центрі** означає, що камера дивиться на його центр, а не на
//! корабель, тобто це вже не [`crate::chase`]: камера третьої особи завжди
//! тримає в центрі апарат. Корабель відсунуто рівно на [`SHIP_OFF_AXIS`] —
//! око стоїть радіально над ним і трохи вбік, а кут між цим зсувом і радіусом
//! і є кут, під яким корабель видно від центра кадру.
//!
//! **Земля на тлі — це вимога до орбіти, а не до камери.** Місяць у
//! припливному захопленні дивиться на Землю довготою 0, і мозаїка в асеті це
//! відображає; отже камера, спрямована на центр Місяця, бачить за ним Землю
//! лише тоді, коли корабель летить над **зворотним** боком. Звідси лінія
//! апсид: апогей на довготі 180° − [`EARTH_MARGIN`], тобто за 20° від
//! антиземної точки.
//!
//! ⚠ **Знизу Земля в кадр не потрапляє взагалі, і це геометрія, а не вада
//! композиції.** Диск Місяця має кутовий радіус 63.7° на висоті 200 км —
//! ширший за весь кадр, — тож коли камера націлена на центр Місяця, поза
//! диском не лишається неба. Виміряна доріжка вздовж витка:
//!
//! | висота | Земля від осі | лімб | що в кадрі |
//! |---|---|---|---|
//! | 6000 км (апогей) | 19.7° | 13.0° | Земля з-за лімба |
//! | 5472 км | 9.6° | 13.9° | **покриття**: Земля за диском |
//! | 4280 км | 20.5° | 16.8° | Земля знову видима |
//! | 3048 км | 37.1° | 21.3° | Земля вийшла за край кадру |
//! | 200 км (перигей) | 159.8° | 63.7° | Земля позаду камери |
//!
//! Тобто Земля в кадрі приблизно чверть витка — у високій його частині, — і
//! всередині цієї чверті встигає **зайти за лімб і вийти назад**. Покриття
//! тут справжнє, а не зникнення з кадру, і його оракул — перетин двох кутів.
//!
//! Перигей лягає на видимий бік (довгота −20°) — там, де в мозаїці лежать
//! моря, тобто найбільший контраст альбедо, який має що показати правилу
//! матеріалу (T4) і сяйву (T6).
//!
//! **Плавність — це вибір параметра, за яким беруться кадри.** Рівномірно за
//! часом апарат майже стоїть біля апогею; рівномірно за ексцентричною
//! аномалією кутова швидкість стрибає вдвічі на перигеї (`dν/dE` там
//! `√((1+e)/(1−e))`). Кадри беруться рівномірно за **істинною аномалією**:
//! тоді напрямок на корабель від центра Місяця повзе зі сталою кутовою
//! швидкістю. «Верх» камери — стала нормаль орбіти, тож у русі немає ні
//! ривків, ні крену, і петля замикається без стрибка.
//!
//! ⚠ **Читати анімацію як швидкість не можна**: час у ній нерівномірний за
//! побудовою. Сама орбіта при цьому точна — задача двох тіл має замкнену
//! форму, і жодного інтегрування тут немає.
//!
//! Це свідомо не `prop_run`. Зонд показує **кадр**, і брати для нього
//! інтегратор означало б тягнути в анімацію ефемериду, рухомий Місяць і
//! вибір фрейму — тобто три речі, жодна з яких на картинку не впливає.
//! Оракул інтегратора живе окремо (`tests/live.rs`, `--live-probe`), і
//! підмінювати його анімацією не можна: анімація не перевіряє нічого.

use std::path::Path;

use crate::camera::Camera;
use crate::frame::{self, Frame};
use crate::gpu::Gpu;
use crate::scene::{Body, Scene, Ship, TerrainId, TileSet};
use crate::{demo, ship, ship_demo, shot, sphere, tiles};

/// Радіус Місяця, метри — той самий, що в решті зондів.
const RADIUS_M: f64 = 1_737_400.0;

/// Гравітаційний параметр Місяця, м³/с² (DE440).
const MU: f64 = 4.902_800_118e12;

/// Висоти апогею й перигею над поверхнею, метри.
const APOAPSIS_M: f64 = 6_000_000.0;
const PERIAPSIS_M: f64 = 200_000.0;

/// Середня відстань до Землі, метри.
const EARTH_RANGE_M: f64 = 384_400_000.0;

/// На скільки апогей відведений від антиземної точки, радіани.
///
/// **Виміряне число, не смак.** Земля має бути далі за лімб (кутовий радіус
/// диска в апогеї 12.9°) і ближче за півкут камери по вертикалі (30°).
/// Двадцять градусів лишають запас з обох боків — і саме цей запас з'їдає
/// зниження: на висоті 3.3·10⁶ м диск доростає до 20°, і Земля ховається.
const EARTH_MARGIN: f64 = 0.35;

/// Нахилення орбіти до екватора Місяця, радіани.
///
/// Не нуль і не 90°: полярна орбіта пройшла б над полюсами, де мозаїка WAC
/// знята при найгірших кутах, а екваторіальна — уздовж одного пояса. Поворот
/// іде **навколо лінії апсид**, тож апогей і перигей лишаються на екваторі —
/// а на їхніх довготах стоїть уся композиція.
const INCLINATION: f64 = 0.52;

/// Кут світила від перигею, радіани.
///
/// 70°, тобто низьке сонце над точкою найнижчого прольоту: саме там рельєф
/// дає найдовші тіні. Світило над головою зробило б поверхню пласкою.
const SOLAR_ZENITH: f64 = 1.22;

/// Скільки габаритів корпусу від камери до корабля.
const RANGES: f64 = 3.2;

/// На який кут корабель відведений від центра кадру, радіани.
///
/// Центр кадру зайнятий Місяцем, тож корабель мусить стояти збоку — але в
/// кадрі: 0.28 рад це 16°, трохи більше за половину півкута камери.
const SHIP_OFF_AXIS: f64 = 0.28;

/// Хвилина відео: кадрів рівно стільки, скільки їх у хвилині при [`FPS`].
///
/// Один повний виток за анімацію, тобто 8.39 години орбітального часу на
/// шістдесят секунд. Менше кадрів дало б ту саму траєкторію швидше — це
/// `--frames`.
pub const FRAMES: u32 = 3600;
pub const FPS: u16 = 60;

/// Велика піввісь і ексцентриситет із двох висот.
fn elements() -> (f64, f64) {
    let apo = RADIUS_M + APOAPSIS_M;
    let peri = RADIUS_M + PERIAPSIS_M;
    (0.5 * (apo + peri), (apo - peri) / (apo + peri))
}

/// Ексцентрична аномалія за істинною — точна форма, без ітерацій.
fn eccentric_from_true(true_anomaly: f64) -> f64 {
    let (_, e) = elements();
    2.0 * (((1.0 - e) / (1.0 + e)).sqrt() * (0.5 * true_anomaly).tan()).atan()
}

/// Стан на ексцентричній аномалії: позиція й швидкість, світові осі.
///
/// Замкнена форма задачі двох тіл: перифокальна площина, поворот на
/// нахилення навколо лінії апсид, поворот усієї орбіти навколо полярної осі
/// на [`EARTH_MARGIN`]. Швидкість потрібна не фізиці, а **орієнтації**: ніс
/// корабля дивиться вздовж неї.
fn state_at(e_anomaly: f64) -> ([f64; 3], [f64; 3]) {
    let (a, e) = elements();
    let (sin_e, cos_e) = e_anomaly.sin_cos();

    // Перифокальна система: перигей на осі `+x`.
    let r = a * (1.0 - e * cos_e);
    let plane = [a * (cos_e - e), a * (1.0 - e * e).sqrt() * sin_e];

    // Похідна тієї самої параметризації: `dE/dt = n·a/r`, де `n = √(μ/a³)`.
    let n = (MU / (a * a * a)).sqrt();
    let rate = n * a / r;
    let speed = [-a * sin_e * rate, a * (1.0 - e * e).sqrt() * cos_e * rate];

    // Нахилення — навколо `x`, тобто навколо лінії апсид: апогей і перигей
    // лишаються на екваторі, а на їхніх довготах стоїть композиція.
    let (sin_i, cos_i) = INCLINATION.sin_cos();
    let lift = |v: [f64; 2]| [v[0], v[1] * cos_i, v[1] * sin_i];

    // І поворот усієї орбіти навколо `z`: перигей їде на довготу
    // `−EARTH_MARGIN`, апогей — на `180° − EARTH_MARGIN`.
    let (sin_arg, cos_arg) = (-EARTH_MARGIN).sin_cos();
    let turn = |v: [f64; 3]| {
        [
            v[0] * cos_arg - v[1] * sin_arg,
            v[0] * sin_arg + v[1] * cos_arg,
            v[2],
        ]
    };
    (turn(lift(plane)), turn(lift(speed)))
}

fn unit(v: [f64; 3]) -> [f64; 3] {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / n, v[1] / n, v[2] / n]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Нормаль площини орбіти — стала, і саме тому камера не хитається.
fn orbit_normal() -> [f64; 3] {
    let (position, velocity) = state_at(0.3);
    unit(cross(position, velocity))
}

/// Куди від Місяця стоїть Земля.
///
/// Довгота 0: Місяць у припливному захопленні дивиться на Землю саме нею, і
/// мозаїка в асеті це відображає. Тобто напрямок не вибраний, а заданий тим,
/// що вже лежить у тайлах.
fn earth_centre() -> [f64; 3] {
    [EARTH_RANGE_M, 0.0, 0.0]
}

/// Напрямок на світило — виведений з орбіти, а не поставлений на око.
///
/// Береться в площині орбіти під кутом [`SOLAR_ZENITH`] до перигею, з боку
/// **підльоту**: корабель знижується над освітленою поверхнею, проходить
/// перигей при низькому світилі, а далі йде до термінатора, за яким сяйво на
/// корпусі гасне (T6).
fn sun() -> [f64; 3] {
    let (periapsis, ahead) = state_at(0.0);
    let p = unit(periapsis);
    let a = unit(ahead);
    let (sin_z, cos_z) = SOLAR_ZENITH.sin_cos();
    unit([
        cos_z * p[0] - sin_z * a[0],
        cos_z * p[1] - sin_z * a[1],
        cos_z * p[2] - sin_z * a[2],
    ])
}

/// Кватерніон `[w, x, y, z]`, що переводить корабельний `+Z` у `forward`, а
/// корабельний `+Y` — якнайближче до `up`.
fn look_along(forward: [f64; 3], up: [f64; 3]) -> [f64; 4] {
    let z = unit(forward);
    let x = unit(cross(up, z));
    let y = cross(z, x);

    // Стовпці матриці — образи корабельних осей у світі; далі стандартне
    // перетворення матриці в кватерніон через найбільший слід.
    let m = [[x[0], y[0], z[0]], [x[1], y[1], z[1]], [x[2], y[2], z[2]]];
    let trace = m[0][0] + m[1][1] + m[2][2];
    if trace > 0.0 {
        let s = 0.5 / (trace + 1.0).sqrt();
        [
            0.25 / s,
            (m[2][1] - m[1][2]) * s,
            (m[0][2] - m[2][0]) * s,
            (m[1][0] - m[0][1]) * s,
        ]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = 2.0 * (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt();
        [
            (m[2][1] - m[1][2]) / s,
            0.25 * s,
            (m[0][1] + m[1][0]) / s,
            (m[0][2] + m[2][0]) / s,
        ]
    } else if m[1][1] > m[2][2] {
        let s = 2.0 * (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt();
        [
            (m[0][2] - m[2][0]) / s,
            (m[0][1] + m[1][0]) / s,
            0.25 * s,
            (m[1][2] + m[2][1]) / s,
        ]
    } else {
        let s = 2.0 * (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt();
        [
            (m[1][0] - m[0][1]) / s,
            (m[0][2] + m[2][0]) / s,
            (m[1][2] + m[2][1]) / s,
            0.25 * s,
        ]
    }
}

/// Сцена на кадр номер `k` з `frames`.
///
/// Будується з нуля щокадру навмисно: сцена — це дані, і зонд, який тримав би
/// її між кадрами, перевіряв би свій кеш, а не кадр.
pub fn scene_at(k: u32, frames: u32, tiles: TileSet, earth: TileSet, extent: f64) -> Scene {
    let t = f64::from(k) / f64::from(frames.max(2));

    // Рівномірно за **істинною** аномалією: від апогею через перигей і назад.
    // Саме вона дає сталу кутову швидкість у кадрі, тобто плавність.
    let true_anomaly = std::f64::consts::PI * (2.0 * t - 1.0);
    let (position, velocity) = state_at(eccentric_from_true(true_anomaly));

    let up = unit(position);
    let normal = orbit_normal();
    let ship = Ship {
        centre: position,
        orientation: look_along(velocity, up),
        height_m: ship::DEFAULT_HEIGHT_M,
        extent_m: extent * ship::DEFAULT_HEIGHT_M,
        colour: [0.72, 0.74, 0.78, 1.0],
        roughness: ship::HULL_ROUGHNESS,
        metallic: ship::HULL_METALLIC,
    };

    // Око — над кораблем і трохи вбік від площини орбіти. Кут між цим зсувом
    // і радіусом дорівнює куту, під яким корабель видно від центра кадру:
    // камера дивиться на центр Місяця, а корабель від неї — рівно назад по
    // зсуву.
    let (sin_off, cos_off) = SHIP_OFF_AXIS.sin_cos();
    let offset = [
        cos_off * up[0] + sin_off * normal[0],
        cos_off * up[1] + sin_off * normal[1],
        cos_off * up[2] + sin_off * normal[2],
    ];
    let range = RANGES * ship.extent_m;
    let eye = [
        position[0] + range * offset[0],
        position[1] + range * offset[1],
        position[2] + range * offset[2],
    ];
    // «Верх» кадру — нормаль орбіти, стала на всю анімацію: будь-який верх,
    // виведений з положення, крутив би кадр разом із рухом.
    let camera = Camera::look_at(eye, [0.0, 0.0, 0.0], normal);

    let mut scene = Scene::new(camera);
    scene.sun = sun();
    scene.bodies.push(Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles,
        // Сірий, а не синій колір фікстур: без колірного асета Місяць має
        // лишитись Місяцем.
        colour: [0.55, 0.55, 0.56, 1.0],
        air: None,
    });
    // Земля — друге тіло сцени, за два порядки далі. Гладка й без повітря:
    // з такої відстані диск має 1.9°, і ні рельєф, ні шар повітря в ньому не
    // мають де проявитись (умова S5 однаково пропустила б повітря).
    scene.bodies.push(Body {
        centre: earth_centre(),
        radius_m: sphere::EARTH_RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: earth,
        colour: frame::COLOUR,
        air: None,
    });
    scene.ships.push(ship);
    scene
}

/// Малює `frames` кадрів і складає їх в анімований PNG.
pub fn render(gpu: &Gpu, width: u32, height: u32, frames: u32, path: &Path) -> Result<(), String> {
    let mut frame = Frame::new(gpu, shot::FORMAT);
    let surface = load_surface(gpu, &mut frame)?;
    let earth = match load_earth(gpu, &mut frame) {
        Some(id) => TileSet::Loaded(id),
        None => TileSet::Smooth,
    };

    // Корпус з асета, якщо він скукований; інакше заглушка V1.
    let hull = ship_demo::hull();
    if let Some(model) = &hull {
        frame.load_ship(gpu, model);
    }
    let extent = hull.as_ref().map_or(ship_demo::STUB_EXTENT, |m| m.extent);

    report();

    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("flyby demo"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shot::FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .set_animated(frames, 0)
        .map_err(|e| format!("APNG: {e}"))?;
    encoder
        .set_frame_delay(1, FPS)
        .map_err(|e| format!("APNG: {e}"))?;
    let mut writer = encoder.write_header().map_err(|e| format!("APNG: {e}"))?;

    for k in 0..frames {
        let scene = scene_at(k, frames, TileSet::Loaded(surface), earth, extent);
        let mut commands = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("flyby demo"),
            });
        frame.draw(gpu, &mut commands, &view, width, height, &scene);
        let shot = shot::read_back(gpu, commands, &texture, width, height)?;
        writer
            .write_image_data(&shot.pixels)
            .map_err(|e| format!("APNG: {e}"))?;
    }

    writer.finish().map_err(|e| format!("APNG: {e}"))?;
    Ok(())
}

/// Друкує елементи орбіти й кути композиції — щоб число в кадрі можна було
/// звірити з числом.
fn report() {
    let (a, e) = elements();
    let period = 2.0 * std::f64::consts::PI * (a * a * a / MU).sqrt();
    let speed = |r: f64| (MU * (2.0 / r - 1.0 / a)).sqrt();
    println!(
        "орбіта: {:.0} × {:.0} км над поверхнею, a = {:.1} км, e = {:.4}, нахилення {:.0}°",
        PERIAPSIS_M / 1000.0,
        APOAPSIS_M / 1000.0,
        a / 1000.0,
        e,
        INCLINATION.to_degrees()
    );
    println!(
        "  період {:.2} год; швидкість {:.0} м/с у перигеї, {:.0} м/с в апогеї",
        period / 3600.0,
        speed(RADIUS_M + PERIAPSIS_M),
        speed(RADIUS_M + APOAPSIS_M)
    );
    println!(
        "  Земля на {:.0}° від осі погляду; диск Місяця — {:.1}° в апогеї, {:.1}° у перигеї",
        EARTH_MARGIN.to_degrees(),
        (RADIUS_M / (RADIUS_M + APOAPSIS_M)).asin().to_degrees(),
        (RADIUS_M / (RADIUS_M + PERIAPSIS_M)).asin().to_degrees()
    );
}

/// Рельєф і колір Місяця з готових асетів.
fn load_surface(gpu: &Gpu, frame: &mut Frame) -> Result<TerrainId, String> {
    let bytes = std::fs::read(demo::TERRAIN_ASSET)
        .map_err(|e| format!("{}: {e}\nполікувати: make cook-dem", demo::TERRAIN_ASSET))?;
    let terrain = tiles::Terrain::from_bytes(&bytes)?;

    let bytes = std::fs::read(demo::COLOUR_ASSET)
        .map_err(|e| format!("{}: {e}\nполікувати: make cook-colour", demo::COLOUR_ASSET))?;
    let colour = tiles::Colour::from_bytes(&bytes)?;
    frame.load_surface(gpu, &terrain, Some(&colour))
}

/// Поверхня Землі — **друге** тіло з тайлами в одному кадрі (T7g).
///
/// Мовчки повертає `None`, коли асета немає: він поза git (Q5), а зонд має
/// малюватись і без нього — тоді Земля лишається гладкою кулею, як була до
/// цього кроку. Це та сама поблажливість, що в `game::app::load_surface`, і
/// та сама причина: відсутній ассет не є поламаним рушієм.
fn load_earth(gpu: &Gpu, frame: &mut Frame) -> Option<TerrainId> {
    let terrain = tiles::Terrain::from_bytes(&std::fs::read(EARTH_TERRAIN_ASSET).ok()?).ok()?;
    let colour = tiles::Colour::from_bytes(&std::fs::read(EARTH_COLOUR_ASSET).ok()?).ok()?;
    frame.load_surface(gpu, &terrain, Some(&colour)).ok()
}

/// Скукована поверхня Землі (T7d, T7e).
const EARTH_TERRAIN_ASSET: &str = "assets/earth.dem";
const EARTH_COLOUR_ASSET: &str = "assets/earth.col";

#[cfg(test)]
mod tests {
    use super::*;

    fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    /// Кут між двома напрямками, радіани.
    fn angle(a: [f64; 3], b: [f64; 3]) -> f64 {
        dot(unit(a), unit(b)).clamp(-1.0, 1.0).acos()
    }

    /// Висоти в апогеї й перигеї — ті, що замовлені.
    #[test]
    fn the_orbit_has_the_two_altitudes_it_promises() {
        let radius = |e_anomaly: f64| {
            let (p, _) = state_at(e_anomaly);
            (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt()
        };
        let peri = radius(0.0) - RADIUS_M;
        let apo = radius(std::f64::consts::PI) - RADIUS_M;
        println!("  перигей {peri:.1} м, апогей {apo:.1} м");
        assert!((peri - PERIAPSIS_M).abs() < 1.0);
        assert!((apo - APOAPSIS_M).abs() < 1.0);
    }

    /// Швидкість — справді похідна позиції, а не окрема формула.
    ///
    /// Дві незалежні дороги до одного числа: замкнена форма проти
    /// центральної різниці за часом. Вони мусять зійтися, інакше ніс корабля
    /// дивиться не туди, куди він летить.
    #[test]
    fn the_velocity_is_the_derivative_of_the_position() {
        let (a, e) = elements();
        let n = (MU / (a * a * a)).sqrt();
        let time = |anomaly: f64| (anomaly - e * anomaly.sin()) / n;
        let anomaly_at = |t: f64| {
            let mut anomaly = n * t;
            for _ in 0..64 {
                let f = anomaly - e * anomaly.sin() - n * t;
                anomaly -= f / (1.0 - e * anomaly.cos());
            }
            anomaly
        };

        for probe in [0.3, 1.1, 2.4, 3.0] {
            let t0 = time(probe);
            let step = 1.0e-3;
            let (before, _) = state_at(anomaly_at(t0 - step));
            let (after, _) = state_at(anomaly_at(t0 + step));
            let (_, velocity) = state_at(probe);
            for k in 0..3 {
                let numeric = (after[k] - before[k]) / (2.0 * step);
                assert!(
                    (numeric - velocity[k]).abs() < 1e-3 * velocity[k].abs().max(1.0),
                    "аномалія {probe}, вісь {k}: {numeric} проти {}",
                    velocity[k]
                );
            }
        }
    }

    /// Ніс корабля дивиться вздовж швидкості.
    #[test]
    fn the_ship_points_where_it_flies() {
        for k in [0u32, 37, 180, 300] {
            let scene = scene_at(k, FRAMES, TileSet::Smooth, TileSet::Smooth, 0.647);
            let ship = &scene.ships[0];
            let true_anomaly =
                std::f64::consts::PI * (2.0 * f64::from(k) / f64::from(FRAMES) - 1.0);
            let (_, velocity) = state_at(eccentric_from_true(true_anomaly));
            let r = crate::frame::rotation(ship.orientation);
            // Образ корабельного `+Z` — третій стовпець матриці повороту.
            let nose = [r[0][2], r[1][2], r[2][2]];
            let along = dot(nose, unit(velocity));
            println!("  кадр {k}: ніс уздовж швидкості на {along:.6}");
            assert!(along > 0.999_999, "ніс дивиться не вздовж швидкості");
        }
    }

    /// Місяць у центрі кадру, а корабель — рівно збоку від нього.
    ///
    /// Два твердження одним числом: центр тіла лежить на осі погляду, а
    /// корабель — на [`SHIP_OFF_AXIS`] від неї, тобто в кадрі й не поверх
    /// центра.
    #[test]
    fn the_moon_is_in_the_middle_and_the_ship_beside_it() {
        for k in [0u32, 600, 1200, 1800, 2400, 3000] {
            let scene = scene_at(k, FRAMES, TileSet::Smooth, TileSet::Smooth, 0.647);
            let eye = scene.camera.position();
            let to_moon = [-eye[0], -eye[1], -eye[2]];
            let to_ship = {
                let c = scene.ships[0].centre;
                [c[0] - eye[0], c[1] - eye[1], c[2] - eye[2]]
            };
            let off = angle(to_moon, to_ship);
            println!(
                "  кадр {k}: корабель на {:.2}° від центра",
                off.to_degrees()
            );
            assert!(
                (off - SHIP_OFF_AXIS).abs() < 0.02,
                "корабель поїхав з місця: {off} проти {SHIP_OFF_AXIS}"
            );
            // Півкут кадру по вертикалі — 30°, і корабель мусить бути в ньому.
            assert!(off < 0.5 * frame::FOV_Y);
        }
    }

    /// Земля виходить з-за лімба в апогеї, ховається за диском невдовзі по
    /// ньому й зникає з кадру на зниженні.
    ///
    /// Три різні причини, і тест розрізняє саме їх, а не «Земля десь є»:
    /// **за диском** — коли кут до неї менший за кутовий радіус лімба;
    /// **поза кадром** — коли він більший за півкут камери; **видима** — між
    /// ними. Остання перевірка й записує геометричну межу композиції: знизу
    /// диск ширший за кадр, тож неба поза ним не лишається взагалі.
    #[test]
    fn the_earth_clears_the_limb_high_up_and_hides_behind_it_low_down() {
        let separation = |k: u32| {
            let scene = scene_at(k, FRAMES, TileSet::Smooth, TileSet::Smooth, 0.647);
            let eye = scene.camera.position();
            let to_moon = [-eye[0], -eye[1], -eye[2]];
            let earth = earth_centre();
            let to_earth = [earth[0] - eye[0], earth[1] - eye[1], earth[2] - eye[2]];
            let distance = (eye[0] * eye[0] + eye[1] * eye[1] + eye[2] * eye[2]).sqrt();
            (angle(to_moon, to_earth), (RADIUS_M / distance).asin())
        };

        // Апогей: Земля за лімбом і в кадрі.
        let (apart, limb) = separation(0);
        println!(
            "  апогей: Земля на {:.1}°, лімб на {:.1}°",
            apart.to_degrees(),
            limb.to_degrees()
        );
        assert!(apart > limb, "в апогеї Земля за диском Місяця");
        assert!(apart < 0.5 * frame::FOV_Y, "в апогеї Земля поза кадром");

        // Невдовзі по апогею траса проходить під Землею — покриття.
        let (apart, limb) = separation(180);
        println!(
            "  кадр 180: Земля на {:.1}°, лімб на {:.1}°",
            apart.to_degrees(),
            limb.to_degrees()
        );
        assert!(apart < limb, "покриття зникло — Земля не зайшла за диск");

        // Перигей: Земля позаду камери, бо камера дивиться на центр Місяця,
        // а корабель уже над видимим боком.
        let (apart, limb) = separation(FRAMES / 2);
        println!(
            "  перигей: Земля на {:.1}°, лімб на {:.1}°",
            apart.to_degrees(),
            limb.to_degrees()
        );
        assert!(
            apart > 0.5 * frame::FOV_Y,
            "у перигеї Земля не може бути в кадрі"
        );
        // І це не «не влізла»: диск ширший за півкут камери, тобто неба поза
        // ним у кадрі немає взагалі.
        assert!(limb > 0.5 * frame::FOV_Y, "лімб мав би накрити весь кадр");
    }

    /// Рух плавний: кутовий крок камери між кадрами не гуляє.
    ///
    /// Оракул числовий, а не «виглядає добре». Рівномірність за **істинною**
    /// аномалією саме це й дає: за ексцентричною відношення найбільшого кроку
    /// до найменшого було б `√((1+e)/(1−e))` ≈ 2, а за часом — на порядки.
    /// Останній крок перевіряється окремо: петля мусить замикатися.
    #[test]
    fn the_camera_moves_without_jerks() {
        let direction = |k: u32| {
            let scene = scene_at(k % FRAMES, FRAMES, TileSet::Smooth, TileSet::Smooth, 0.647);
            unit(scene.camera.position())
        };
        let mut steps = Vec::with_capacity(FRAMES as usize);
        for k in 0..FRAMES {
            steps.push(angle(direction(k), direction(k + 1)));
        }
        let smallest = steps.iter().copied().fold(f64::INFINITY, f64::min);
        let largest = steps.iter().copied().fold(0.0, f64::max);
        println!(
            "  крок камери: {:.4}° … {:.4}°, відношення {:.4}",
            smallest.to_degrees(),
            largest.to_degrees(),
            largest / smallest
        );
        assert!(
            largest / smallest < 1.05,
            "кутова швидкість гуляє в {:.2} рази",
            largest / smallest
        );
        assert!(
            steps[FRAMES as usize - 1] < 1.05 * smallest,
            "петля з розривом"
        );
    }
}
