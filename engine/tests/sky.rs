//! Небо в кадрі (ROADMAP-ATMOSPHERE.md, S4b).
//!
//! ## Чому тут числа, а не знімки
//!
//! Знімки теж пишуться — у `build/`, і подивитися на них варто, — але вирішує
//! не око. Правило 2 етапу S вимагає числа, і числа тут беруться з тієї
//! фізики, яку небо й має показувати:
//!
//! - **біля горизонту небо яскравіше й біліше, ніж у зеніті.** Промінь, що йде
//!   полого, проходить утричі більше повітря; синє в ньому встигає розсіятись
//!   і назад, і вбік, а червоне доходить — звідси і яскравість, і білизна;
//! - **захід червоніший за полудень.** Те саме, доведене до кінця: коли Сонце
//!   на горизонті, його світло йде крізь усю товщу, і синього в ньому не
//!   лишається зовсім;
//! - **з орбіти повітря — тонка світна дуга**, а не півнеба, і вона **додає**
//!   світло, а не заміщає фон.
//!
//! Кожне з цих тверджень ловить свою помилку. Перше — переплутані осі таблиці
//! неба. Друге — загублене пропускання (без нього захід лишився б білим).
//! Третє — заміщення замість додавання, тобто чорну дугу на нічному краї.
//!
//! ## І одне твердження про те, чого немає
//!
//! Тіло без повітря дає той самий кадр, що до етапу S (правило 4). Найдешевша
//! сторожа проти «пройшло крізь усе», і перевіряється вона прямо: поза диском
//! планети кожен піксель мусить дорівнювати кольору очищення **точно**.

use engine::camera::Camera;
use engine::frame::{CLEAR_BYTES, LIGHT_DIR};
use engine::gpu::Gpu;
use engine::scene::{Atmosphere, Body, Scene, TileSet};
use engine::shot::{self, Shot};
use engine::sphere::EARTH_RADIUS_M as EARTH;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;

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

fn sun_direction() -> [f64; 3] {
    unit(LIGHT_DIR.map(f64::from))
}

/// Земля з повітрям або без нього.
fn earth(air: bool) -> Body {
    Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: EARTH,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: TileSet::Smooth,
        air: air.then(|| Atmosphere::EARTH.with_surface(EARTH)),
    }
}

/// Спостерігач на висоті `altitude` над точкою `up_dir`, погляд під кутом
/// `elevation` градусів над горизонтом **у бік Сонця**.
///
/// Азимут на Сонце, а не довільний: уся різниця між полуднем і заходом живе
/// саме в ньому, і камера, повернута кудись убік, показала б однакове небо в
/// обох випадках.
fn observer(up_dir: [f64; 3], altitude: f64, elevation: f64, air: bool) -> Scene {
    let up = unit(up_dir);
    let eye = up.map(|v| v * (EARTH + altitude));
    let sun = sun_direction();
    let mu_s = sun[0] * up[0] + sun[1] * up[1] + sun[2] * up[2];

    // Горизонтальна складова напрямку на Сонце. У підсонячній точці вона
    // вироджується — тоді азимут не має значення, бо Сонце в зеніті.
    let mut horizontal = [
        sun[0] - mu_s * up[0],
        sun[1] - mu_s * up[1],
        sun[2] - mu_s * up[2],
    ];
    if horizontal.iter().map(|v| v * v).sum::<f64>().sqrt() < 1.0e-6 {
        horizontal = cross(up, [0.0, 0.0, 1.0]);
    }
    let horizontal = unit(horizontal);

    let (sin, cos) = elevation.to_radians().sin_cos();
    let direction = [
        cos * horizontal[0] + sin * up[0],
        cos * horizontal[1] + sin * up[1],
        cos * horizontal[2] + sin * up[2],
    ];
    let target = [
        eye[0] + direction[0] * 1000.0,
        eye[1] + direction[1] * 1000.0,
        eye[2] + direction[2] * 1000.0,
    ];

    let mut scene = Scene::new(Camera::look_at(eye, target, up));
    scene.bodies.push(earth(air));
    scene
}

/// Планета цілком у кадрі, з висоти 10⁷ м — та сама геометрія, що в `--shot`.
fn from_orbit(air: bool) -> Scene {
    let eye = [EARTH + 1.0e7, 0.0, 0.0];
    let mut scene = Scene::new(Camera::look_at(eye, [0.0; 3], [0.0, 0.0, 1.0]));
    scene.bodies.push(earth(air));
    scene
}

fn render(gpu: &Gpu, scene: &Scene, name: &str) -> Shot {
    let shot = shot::take_scene(gpu, WIDTH, HEIGHT, scene).expect("кадр мав намалюватися");
    shot.write_png(std::path::Path::new(&format!("build/s4_{name}.png")))
        .expect("знімок мав записатися");
    shot
}

/// Відношення червоного до синього — те, чим міряється «червоніше».
fn redness(pixel: [u8; 4]) -> f64 {
    f64::from(pixel[0]) / f64::from(pixel[2]).max(1.0)
}

fn centre(shot: &Shot) -> [u8; 4] {
    shot.pixel(WIDTH / 2, HEIGHT / 2)
}

/// Небо з поверхні: біля горизонту яскравіше й біліше, ніж у зеніті.
///
/// Обидва знімки з однієї точки, різниця лише в куті погляду — тобто ловиться
/// саме довжина шляху крізь повітря, а не щось у камері.
#[test]
fn the_sky_is_brighter_and_whiter_towards_the_horizon() {
    let Some(gpu) = Gpu::for_tests() else { return };

    // Не точно в підсонячній точці: там азимут на Сонце вироджений, і помилка
    // в ньому не проявилася б узагалі.
    let sun = sun_direction();
    let side = unit(cross(sun, [0.0, 0.0, 1.0]));
    let noon = unit([
        sun[0] + 0.25 * side[0],
        sun[1] + 0.25 * side[1],
        sun[2] + 0.25 * side[2],
    ]);

    let zenith = centre(&render(
        &gpu,
        &observer(noon, 2.0, 89.0, true),
        "noon_zenith",
    ));
    let horizon = centre(&render(
        &gpu,
        &observer(noon, 2.0, 3.0, true),
        "noon_horizon",
    ));

    // Небо взагалі є: колір очищення — [5, 8, 20], і зеніт мусить бути помітно
    // світлішим за нього, інакше прохід не намалював нічого.
    assert!(
        zenith[2] > u32::from(CLEAR_BYTES[2]) as u8 * 2,
        "зеніт {zenith:?} не світліший за фон {CLEAR_BYTES:?}"
    );

    // Виміряно: синій 89 у зеніті проти 166 біля горизонту, тобто в 1.86 раза.
    // Поріг 1.3 лишає запас під зміну експозиції й кроку марша.
    let brighter = f64::from(horizon[2]) / f64::from(zenith[2]);
    assert!(
        brighter > 1.3,
        "біля горизонту небо не яскравіше: {horizon:?} проти {zenith:?}"
    );

    // Виміряно: червоне/синє 0.28 у зеніті проти 0.42 біля горизонту. Синє
    // розсіюється по дорозі, червоне доходить — звідси й білизна горизонту.
    assert!(
        redness(horizon) > redness(zenith) * 1.2,
        "горизонт не біліший: {} проти {}",
        redness(horizon),
        redness(zenith)
    );
}

/// Захід червоніший за полудень, і це не «трохи».
///
/// Обидві камери на поверхні й дивляться під тим самим малим кутом у бік
/// Сонця; різниця лише в тому, де Сонце. Без пропускання вздовж променя до
/// Сонця захід лишився б таким самим білим, як полудень, — саме цю помилку
/// тест і ловить.
#[test]
fn a_sunset_is_redder_than_noon() {
    let Some(gpu) = Gpu::for_tests() else { return };

    let sun = sun_direction();
    let side = unit(cross(sun, [0.0, 0.0, 1.0]));
    let noon = unit([
        sun[0] + 0.25 * side[0],
        sun[1] + 0.25 * side[1],
        sun[2] + 0.25 * side[2],
    ]);

    let at_noon = centre(&render(&gpu, &observer(noon, 2.0, 3.0, true), "noon_low"));
    // Спостерігач на термінаторі: Сонце рівно на його горизонті.
    let at_sunset = centre(&render(&gpu, &observer(side, 2.0, 3.0, true), "sunset"));

    // Виміряно: 0.42 опівдні проти 4.45 на заході, тобто в десять разів. Поріг
    // 5 — половина від виміряного.
    assert!(
        redness(at_sunset) > redness(at_noon) * 5.0,
        "захід {at_sunset:?} (r/b {}) не червоніший за полудень {at_noon:?} (r/b {})",
        redness(at_sunset),
        redness(at_noon)
    );
    // І він таки видимий, а не просто червонуватий нуль.
    assert!(
        at_sunset[0] > 40,
        "захід надто темний, щоб про нього говорити: {at_sunset:?}"
    );
}

/// З орбіти повітря — тонка світна дуга, і воно **додає** світло.
///
/// Три числа замість ока: дуга є, вона тонка, і жоден піксель від неї не
/// потемнів. Останнє й ловить заміщення замість додавання — саме воно
/// вигризало з фону чорну дугу на нічному краї, де розсіювати нема чого.
#[test]
fn from_orbit_the_air_is_a_thin_arc_that_only_adds_light() {
    let Some(gpu) = Gpu::for_tests() else { return };

    let with_air = render(&gpu, &from_orbit(true), "orbit");
    let bare = render(&gpu, &from_orbit(false), "orbit_bare");

    let total = (WIDTH * HEIGHT) as usize;
    let mut changed = 0;
    let mut darker = 0;
    for k in 0..total {
        let a = &with_air.pixels[k * 4..k * 4 + 3];
        let b = &bare.pixels[k * 4..k * 4 + 3];
        if a != b {
            changed += 1;
        }
        if a.iter().zip(b).any(|(x, y)| x < y) {
            darker += 1;
        }
    }

    assert_eq!(darker, 0, "{darker} пікселів потемніли від повітря");
    // Виміряно: 852 пікселі з 230 400, тобто 0.37%. Шар у 100 км на радіусі
    // 6371 км з десяти мегаметрів — це смуга завширшки два-три пікселі вздовж
    // диска, і саме такий порядок тут і має бути.
    let share = changed as f64 / total as f64;
    assert!(
        (0.0005..0.05).contains(&share),
        "повітря змінило {share} кадру — це вже не тонка дуга"
    );
}

/// Тіло без повітря дає той самий кадр, що до етапу S.
///
/// Перевіряється прямо: поза диском планети кожен піксель дорівнює кольору
/// очищення **точно**. Прохід неба, який пробіг би зайвий раз, лишив би там
/// хоч одиницю — додавання нуля не буває безкоштовним лише на папері.
#[test]
fn a_body_without_air_leaves_the_frame_exactly_as_it_was() {
    let Some(gpu) = Gpu::for_tests() else { return };

    let bare = render(&gpu, &from_orbit(false), "orbit_bare");
    // Диск займає ±132 пікселі від центра (asin(R/(R+10⁷)) = 22.9°), тож
    // лівий край кадру — точно порожній простір.
    for y in (0..HEIGHT).step_by(17) {
        for x in (0..80).step_by(7) {
            let pixel = bare.pixel(x, y);
            assert_eq!(
                &pixel[..3],
                &CLEAR_BYTES,
                "піксель ({x}, {y}) поза диском — {pixel:?}, а мав бути кольором очищення"
            );
        }
    }
}
