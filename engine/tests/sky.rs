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
        colour: engine::frame::COLOUR,
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

/// Погляд на лімб: камера на висоті `altitude` над термінатором, дивиться
/// точно на горизонт — у бік Сонця (`towards_sun`) або від нього.
///
/// Над **термінатором**, бо там Сонце горизонтальне: той самий погляд уперед
/// дає освітлений лімб, той самий назад — нічний. Дві сцени з одного числа.
fn limb(altitude: f64, towards_sun: bool) -> Scene {
    let sun = sun_direction();
    let up = unit(cross(sun, [0.0, 0.0, 1.0]));
    let distance = EARTH + altitude;
    let eye = up.map(|v| v * distance);

    // Кут западання горизонту з цієї висоти — точно, а не приблизно: саме він
    // ставить лімб у центр кадру, а не десь.
    let (sin, cos) = (EARTH / distance).acos().sin_cos();
    let sign = if towards_sun { 1.0 } else { -1.0 };
    let direction = [
        cos * sun[0] * sign - sin * up[0],
        cos * sun[1] * sign - sin * up[1],
        cos * sun[2] * sign - sin * up[2],
    ];
    let target = [
        eye[0] + direction[0] * 1.0e6,
        eye[1] + direction[1] * 1.0e6,
        eye[2] + direction[2] * 1.0e6,
    ];

    let mut scene = Scene::new(Camera::look_at(eye, target, up));
    scene.bodies.push(earth(true));
    scene
}

/// Висота, на якій промінь пікселя проходить найближче до центра тіла.
///
/// Це і є та висота, якій належить світло лімба: промінь дотичний, тож увесь
/// його шлях проходить біля неї. Рахується точно — `√(|eye|² − (eye·w)²)`, —
/// а не через кути в кадрі: другий спосіб мав би власну похибку, і вона
/// увійшла б у виміряну висоту шкали.
fn tangent_altitude(scene: &Scene, size: u32, column: u32, row: u32) -> f64 {
    let eye = scene.camera.position();
    let (right, up, forward) = scene.camera.axes();
    // Кадр квадратний, тож тангенс півкута однаковий по обох осях.
    let t = (engine::frame::FOV_Y / 2.0).tan();
    let ndc_x = 2.0 * (f64::from(column) + 0.5) / f64::from(size) - 1.0;
    let ndc_y = 1.0 - 2.0 * (f64::from(row) + 0.5) / f64::from(size);
    // Стовпець входить нарівні з рядком, і це не педантизм: лімб у кадрі
    // вигнутий, тож на краю кадру той самий рядок дотикається шару значно
    // нижче. Формула лише по рядку давала б висоту, якої в тому пікселі немає.
    let w = unit([
        forward[0] + right[0] * ndc_x * t + up[0] * ndc_y * t,
        forward[1] + right[1] * ndc_x * t + up[1] * ndc_y * t,
        forward[2] + right[2] * ndc_x * t + up[2] * ndc_y * t,
    ]);
    let along = eye[0] * w[0] + eye[1] * w[1] + eye[2] * w[2];
    let radius = eye[0] * eye[0] + eye[1] * eye[1] + eye[2] * eye[2];
    (radius - along * along).max(0.0).sqrt() - EARTH
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
///
/// ⚠ Байти декодуються (T5a). Ціль знімка кодує гамму, тож відношення байтів
/// стискає справжнє відношення яскравостей у корінь степеня 2.4: десятикратна
/// різниця виглядає як двох-з-половиною-кратна. Пороги нижче виміряні в
/// **лінійному** світлі, і саме там вони означають фізику розсіяння.
fn redness(pixel: [u8; 4]) -> f64 {
    let red = engine::srgb::byte_to_linear(pixel[0]);
    let blue = engine::srgb::byte_to_linear(pixel[2]);
    red / blue.max(1.0 / 255.0 / 12.92)
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

// ---------------------------------------------------------------------------
// S6 — лімб і тінь планети
// ---------------------------------------------------------------------------

/// Світна смуга на краю диска спадає з висотою шкали Релея.
///
/// Це і є оракул кроку, названий у ROADMAP-ATMOSPHERE.md: **товщина смуги
/// проти висоти шкали**, а не «схоже на фото з орбіти». Промінь, дотичний до
/// шару на висоті `h`, проходить майже весь шлях біля цієї висоти, тож у
/// прозорій частині атмосфери його яскравість пропорційна густині, тобто
/// `exp(−h/H)`. Отже e-складання смуги мусить дорівнювати `H` — 8 км, і жодне
/// інше число сюди не підходить.
///
/// Міряється у **прозорій** частині, від 35 до 55 км. Нижче смуга насичена:
/// дотичний промінь на десяти кілометрах має оптичну товщу в одиниці, і там
/// яскравість уже не пропорційна густині. Виміряно: у прозорій частині
/// e-складання 8.1 км, біля поверхні — 12.5 км, і друге число — це насичення,
/// а не інша фізика.
#[test]
fn the_limb_glow_falls_off_with_the_scale_height() {
    let Some(gpu) = Gpu::for_tests() else { return };

    // 1600 пікселів заради роздільності: стокілометровий шар на лімбі з 500 км
    // займає 2.2°, тобто при 60° поля огляду близько 54 рядків. Вісім
    // кілометрів висоти шкали — чотири рядки з них.
    const SIZE: u32 = 1600;
    let scene = limb(500_000.0, true);
    let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene).expect("кадр мав намалюватися");
    shot.write_png(std::path::Path::new("build/s6_limb_day.png"))
        .expect("знімок мав записатися");

    // Профіль: висота дотику проти яскравості понад фоном. Тільки над
    // поверхнею — нижче лімба вже поверхня, а не повітря.
    let mut profile: Vec<(f64, f64)> = Vec::new();
    for row in 0..SIZE {
        let altitude = tangent_altitude(&scene, SIZE, SIZE / 2, row);
        if altitude <= 5_000.0 || altitude > 120_000.0 {
            continue;
        }
        let blue = f64::from(shot.pixel(SIZE / 2, row)[2]) - f64::from(CLEAR_BYTES[2]);
        profile.push((altitude, blue.max(0.0)));
    }
    assert!(profile.len() > 30, "профіль замалий: {}", profile.len());
    // Знизу вгору.
    profile.sort_by(|a, b| a.0.total_cmp(&b.0));

    // Висоти, на яких яскравість перетинає два рівні, що відрізняються рівно
    // вдесятеро. Десять разів — це `ln 10 = 2.303` e-складань, тобто відстань
    // між ними ділиться на це число й дає висоту шкали.
    let crossing = |level: f64| -> Option<f64> {
        profile.windows(2).find_map(|pair| {
            let ((h0, v0), (h1, v1)) = (pair[0], pair[1]);
            (v0 >= level && v1 < level).then(|| h0 + (h1 - h0) * (v0 - level) / (v0 - v1))
        })
    };
    let high = crossing(50.0).expect("смуга ніде не яскравіша за 50");
    let low = crossing(5.0).expect("смуга ніде не тьмяніша за 5");
    assert!(low > high, "яскравість не спадає з висотою: {high} → {low}");

    let scale_height = (low - high) / 10.0f64.ln();
    let expected = f64::from(Atmosphere::EARTH.rayleigh_height_m);
    // Виміряно 8.1 км проти 8.0 в параметрах повітря. Допуск у півтора раза —
    // не через невпевненість у фізиці, а тому, що рівні 50 і 5 не строго в
    // прозорій частині: нижній край тягне насичення вгору.
    assert!(
        scale_height > expected / 1.5 && scale_height < expected * 1.5,
        "e-складання смуги {scale_height} м проти висоти шкали {expected} м \
         (перетини на {high} і {low} м)"
    );
}

/// Нічний бік лімба темний: над поверхнею не світиться нічого.
///
/// Тінь планети тут ніхто не малює окремо — вона виходить сама з того, що
/// промінь до Сонця з кожної точки повітря перевіряється на зустріч із
/// поверхнею (S3). Тест ловить рівно ту помилку, яка зробила б цю перевірку
/// зайвою: повітря, освітлене крізь планету.
#[test]
fn the_night_side_of_the_limb_does_not_glow() {
    let Some(gpu) = Gpu::for_tests() else { return };

    const SIZE: u32 = 800;
    let scene = limb(500_000.0, false);
    let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene).expect("кадр мав намалюватися");
    shot.write_png(std::path::Path::new("build/s6_limb_night.png"))
        .expect("знімок мав записатися");

    let mut checked = 0;
    for row in 0..SIZE {
        let altitude = tangent_altitude(&scene, SIZE, SIZE / 2, row);
        if !(1_000.0..100_000.0).contains(&altitude) {
            continue;
        }
        checked += 1;
        let pixel = shot.pixel(SIZE / 2, row);
        assert_eq!(
            &pixel[..3],
            &CLEAR_BYTES,
            "рядок {row} (висота {altitude:.0} м): {pixel:?} — нічне повітря світиться"
        );
    }
    assert!(checked > 10, "перевірено лише {checked} рядків шару");

    // І для контрасту — той самий лімб із того ж боку, але з Сонцем: там
    // світиться. Без цього тест вище пройшов би й на кадрі, де немає нічого.
    let day =
        shot::take_scene(&gpu, SIZE, SIZE, &limb(500_000.0, true)).expect("кадр мав намалюватися");
    let brightest = (0..SIZE)
        .filter(|&row| {
            (1_000.0..100_000.0).contains(&tangent_altitude(&scene, SIZE, SIZE / 2, row))
        })
        .map(|row| day.pixel(SIZE / 2, row)[2])
        .max()
        .expect("рядки є");
    assert!(
        brightest > CLEAR_BYTES[2] * 4,
        "денний лімб теж не світиться: {brightest}"
    );
}

/// Один шейдер з поверхні й з орбіти: на межі повітря обидва шляхи сходяться.
///
/// Правило 3 етапу S — «один шейдер з поверхні й з орбіти», — і це його
/// найгостріша перевірка. Камера всередині повітря читає таблицю неба, камера
/// поза ним марширує промінь; це два різні пайплайни, і межа між ними — рівно
/// верхня межа атмосфери. Кілометр по обидва боки від неї мусить дати той
/// самий кадр, інакше в грі на цій висоті блимне шов.
///
/// Виміряно: **8 одиниць з 255**, тобто 3%, і причина в них названа. Це не
/// крок марша — піднімати його з 16 до 48 не міняє нічого взагалі; це кутова
/// роздільність таблиці неба, у якої біля горизонту один тексель накриває
/// помітну дугу. Тобто шов не зникне від точнішого інтегрування, і зменшити
/// його можна лише більшою таблицею — а це вже питання ціни, не правильності.
#[test]
fn the_two_paths_meet_at_the_top_of_the_air() {
    let Some(gpu) = Gpu::for_tests() else { return };

    // Десять метрів по обидва боки, а не кілометр, і це не перестраховка.
    // Камери на різній висоті бачать лімб трохи по-різному — кут западання
    // горизонту та масштаб висот у кадрі залежать від неї, — і на кілометрі ця
    // геометрія дає більше, ніж могла б дати різниця шляхів. На десяти метрах
    // вона зникає: горизонт зсувається на чотири тисячні пікселя.
    const SIZE: u32 = 320;
    let thickness = Atmosphere::EARTH_THICKNESS_M;
    let inside = shot::take_scene(&gpu, SIZE, SIZE, &limb(thickness - 10.0, true))
        .expect("кадр мав намалюватися");
    let outside = shot::take_scene(&gpu, SIZE, SIZE, &limb(thickness + 10.0, true))
        .expect("кадр мав намалюватися");

    // Порівнюється **небо**, а не весь кадр: рядки, у яких промінь проходить
    // повітрям над поверхнею. Нижче лімба видно ґрунт, і там обидві камери
    // малюють його тим самим шляхом (аеральна перспектива, S5) — різниця в
    // кілька одиниць є, але вона про те, що камери таки на різній висоті, а не
    // про шов між шляхами. Тут перевіряється шов.
    let scene = limb(thickness, true);
    let mut worst = 0i32;
    let mut worst_at = (0u32, 0u32);
    let mut rows = 0;
    for row in 0..SIZE {
        if !(5_000.0..90_000.0).contains(&tangent_altitude(&scene, SIZE, SIZE / 2, row)) {
            continue;
        }
        rows += 1;
        for column in 0..SIZE {
            // Висота — за самим пікселем, а не за рядком: на краю кадру лімб
            // вигнутий, і той самий рядок там уже в поверхні.
            if !(5_000.0..90_000.0).contains(&tangent_altitude(&scene, SIZE, column, row)) {
                continue;
            }
            for c in 0..3 {
                let difference = (i32::from(inside.pixel(column, row)[c])
                    - i32::from(outside.pixel(column, row)[c]))
                .abs();
                if difference > worst {
                    worst = difference;
                    worst_at = (column, row);
                }
            }
        }
    }
    assert!(rows > 5, "перевірено лише {rows} рядків неба");
    assert!(
        worst <= 12,
        "шов на межі повітря: {worst} одиниць у пікселі {worst_at:?}"
    );
}
