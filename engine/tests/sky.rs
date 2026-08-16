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

/// Спостерігач на висоті `altitude`, погляд під `depression` градусів **нижче**
/// горизонталі: поверхня тягнеться від кількох кілометрів під ногами до
/// горизонту, тобто той самий ґрунт видно на всіх відстанях одразу.
fn looking_down(altitude: f64, depression: f64, air: bool) -> Scene {
    let sun = sun_direction();
    let side = unit(cross(sun, [0.0, 0.0, 1.0]));
    // Не в підсонячній точці й не на термінаторі: поверхня яскраво освітлена,
    // але Сонце не за спиною.
    let up = unit([sun[0] + side[0], sun[1] + side[1], sun[2] + side[2]]);
    let eye = up.map(|v| v * (EARTH + altitude));
    let forward = unit(cross(up, side));
    let (sin, cos) = (-depression.to_radians()).sin_cos();
    let direction = [
        cos * forward[0] + sin * up[0],
        cos * forward[1] + sin * up[1],
        cos * forward[2] + sin * up[2],
    ];
    let target = [
        eye[0] + direction[0] * 1.0e4,
        eye[1] + direction[1] * 1.0e4,
        eye[2] + direction[2] * 1.0e4,
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
        // Рахується **порожній простір**, а не весь кадр: диск планети змінює
        // ще й аеральна перспектива (S5), і вона накриває його цілком — то
        // інше твердження й інший тест.
        if b == CLEAR_BYTES && a != b {
            changed += 1;
        }
        // **Тільки порожній простір.** Там, де щось намальовано, повітря має
        // повне право затемнити: аеральна перспектива (S5) множить кадр на
        // пропускання, і диск планети крізь сто кілометрів повітря справді
        // тьмяніший. А от порожнє небо повітря лише підсвічує — там воно
        // нічого не заступає, і піксель, що потемнів, означав би заміщення
        // замість додавання.
        if b == CLEAR_BYTES && a.iter().zip(b).any(|(x, y)| x < y) {
            darker += 1;
        }
    }

    assert_eq!(darker, 0, "{darker} пікселів порожнього неба потемніли");
    // Виміряно: 852 пікселі з 230 400, тобто 0.37% кадру. Шар у 100 км на
    // радіусі 6371 км з десяти мегаметрів — це смуга завширшки два-три пікселі
    // вздовж диска, і саме такий порядок тут і має бути.
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

// ---------------------------------------------------------------------------
// S5 — аеральна перспектива
// ---------------------------------------------------------------------------

/// Той самий ґрунт на різній відстані: контраст падає, серпанок росте.
///
/// Обидва числа з одного знімка, і це важливо — камера, освітлення й поверхня
/// в ньому однакові скрізь, різна лише **відстань**: унизу кадру ґрунт за
/// вісім кілометрів, під горизонтом — за сімдесят.
///
/// ## Чому контраст міряється в червоному
///
/// Бо поверхня в рушії поки що синя (`frame::COLOUR`), тобто майже того самого
/// відтінку, що й серпанок. У синьому ослаблення й підсвітка майже
/// компенсують одне одного, і різниця там не про повітря, а про збіг двох
/// плейсхолдерів. У червоному вони розходяться найсильніше — там і видно те,
/// що аеральна перспектива робить.
///
/// Це не підганяння: другий тест того самого знімка — **серпанок**, тобто
/// повна різниця з кадром без повітря по всіх трьох каналах. Він росте
/// монотонно, і в ньому синій бере участь нарівні.
#[test]
fn the_same_ground_loses_contrast_and_gains_haze_with_distance() {
    let Some(gpu) = Gpu::for_tests() else { return };

    let with_air = render(&gpu, &looking_down(5_000.0, 12.0, true), "ground_haze");
    let bare = render(&gpu, &looking_down(5_000.0, 12.0, false), "ground_bare");

    // Горизонт — перший зверху рядок, у якому з'явилася поверхня. Шукається,
    // а не рахується: він залежить і від висоти, і від кута погляду, і
    // порахований другий раз розійшовся б із першим.
    let column = WIDTH / 2;
    let horizon = (0..HEIGHT)
        .find(|&y| bare.pixel(column, y)[1] > 50)
        .expect("поверхня має бути в кадрі");
    assert!(
        horizon > 20 && horizon < HEIGHT - 40,
        "горизонт у рядку {horizon}"
    );
    // Небо трохи вище горизонту — те, у що поверхня перетворюється з відстанню.
    let sky = with_air.pixel(column, horizon - 3);

    // Знизу вгору, тобто від близького до далекого.
    let mut rows: Vec<u32> = (horizon + 4..HEIGHT - 4).step_by(12).collect();
    rows.reverse();
    assert!(
        rows.len() >= 6,
        "замало рядків для порівняння: {}",
        rows.len()
    );

    let mut previous_contrast = 1000;
    let mut previous_haze = -1000;
    let (mut first_contrast, mut last_contrast) = (0, 0);
    let (mut first_haze, mut last_haze) = (0, 0);
    for (index, &y) in rows.iter().enumerate() {
        let pixel = with_air.pixel(column, y);
        let plain = bare.pixel(column, y);
        let contrast = i32::from(pixel[0]) - i32::from(sky[0]);
        let contrast = contrast.abs();
        let haze: i32 = (0..3)
            .map(|c| (i32::from(pixel[c]) - i32::from(plain[c])).abs())
            .sum();

        // Допуск в одиницю — це один крок восьмибітного кольору, тобто
        // найдрібніше, що взагалі можна записати в кадр. Без нього тест ловив
        // би не фізику, а округлення.
        assert!(
            contrast <= previous_contrast + 1,
            "рядок {y}: контраст {contrast} проти {previous_contrast} ближче — з відстанню він мав би падати"
        );
        assert!(
            haze >= previous_haze - 2,
            "рядок {y}: серпанок {haze} проти {previous_haze} ближче — з відстанню він мав би рости"
        );
        previous_contrast = contrast;
        previous_haze = haze;
        if index == 0 {
            first_contrast = contrast;
            first_haze = haze;
        }
        last_contrast = contrast;
        last_haze = haze;
    }

    // І це не «майже не змінилося». Виміряно: контраст 63 → 44, серпанок
    // 6 → 41 між вісьмома кілометрами й сімдесятьма.
    assert!(
        last_contrast * 4 < first_contrast * 3,
        "контраст упав лише з {first_contrast} до {last_contrast}"
    );
    assert!(
        last_haze > first_haze * 3,
        "серпанок виріс лише з {first_haze} до {last_haze}"
    );
}
