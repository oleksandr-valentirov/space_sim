//! Зшита планета не має дірок і на екрані (ROADMAP-PLANETS.md, R2c).
//!
//! Топологія (`tests/lod.rs`) уже довела, що жодне ребро сітки не лишилося
//! без пари. Цього мало, і причина названа в плані кроку: патч може бути
//! зшитий бездоганно й усе одно провалитись, якщо рівень обрано в один бік
//! для геометрії й в інший — для набору індексів. Тоді дірки в сітці немає,
//! а в кадрі є.
//!
//! Тому тут — друга половина перевірки, і саме в тому порядку, який вимагає
//! правило 7 етапу R: спершу число на CPU, потім знімок. Знімок тут
//! **детектор**, а не оракул: якщо він знайде піксель неба, причина буде в
//! R2a або R2b, і шукати її треба там.
//!
//! ## Наскільки гострий цей детектор — виміряно, а не припущено
//!
//! Дві мутації прогнані руками, і **обидві лишили знімки зеленими**:
//!
//! 1. **зшивання вимкнене цілком** (`cubesphere::indices` ігнорує маску).
//!    Топологічний тест червоніє одразу, знімок — ні, і причина числом:
//!    найширша щілина такого стику — **0.060 пікселя** (тест унизу). Ширшою
//!    вона бути й не може: рівно цю величину критерій R2a тримає під допуском
//!    в один піксель, інакше він поділив би патч далі;
//!    
//! 2. **видимий патч не намальовано взагалі** — доки не було R3. Крізь дірку
//!    видно було не небо, а **той бік тієї самої планети**: відсікання задніх
//!    граней вимкнене свідомо (`cull_mode: None`), і дальня півсфера
//!    малювалась. Пікселі міняли відтінок, але кольору неба не набували.
//!
//!    З horizon culling (R3a) те, що за лімбом, більше не малюється, і та
//!    сама мутація дає **23310 пікселів неба** з 65536. Прогнано ще раз саме
//!    там, як і було записано наперед.
//!
//! Отже детектор гострий рівно на одне: **видимий патч, якого немає в кадрі**.
//! Тріщину між рівнями він не бачить і не побачить — вона в шістнадцяту
//! частину пікселя. Оракулом кроку лишається рівність вершин (правило 5).
//!
//! ## Чому вісім, і чому саме з кутів
//!
//! Кут куба — єдине місце кубосфери, де сходяться **три** патчі замість
//! чотирьох, і єдине, де сусідство переходить одразу через два ребра куба.
//! Восьми кутів рівно стільки, скільки їх є; менше означало б покластися на
//! те, що грані симетричні, а знак у [`engine::cubesphere`] має лише
//! нерухома вісь — тобто три грані з шести дзеркальні.
//!
//! ## Чому «нуль пікселів неба» — це весь кадр
//!
//! З висоти 100 км кутовий радіус диска Землі — `asin(R/(R+h)) = 79.9°`, а
//! півдіагональ квадратного кадру при полі зору 60° — близько 40°. Диск
//! накриває кадр цілком, тож «усередині силуету» й «у кадрі» — це те саме,
//! і межу силуету не доводиться ні шукати, ні наближати.

use engine::gpu::Gpu;
use engine::scene::{Body, Scene, TileSet};
use engine::shot::{self, Shot};
use engine::{camera::Camera, frame, sphere};

const SIZE: u32 = 256;
const ALTITUDE_M: f64 = 1.0e5;

fn gpu() -> Option<Gpu> {
    Gpu::for_tests()
}

/// Скільки пікселів кадру лишилися кольором очищення.
///
/// Рівність точна, без допуску: колір неба — це рівно ті байти, які записав
/// `LoadOp::Clear`, а найтемніший піксель планети (нічний бік, `shade = 0.05`)
/// дає `[3, 8, 11]` проти `[5, 8, 20]` неба. Допуск тут лише розмив би межу,
/// яка й так точна.
fn sky_pixels(shot: &Shot) -> usize {
    let mut sky = 0;
    for y in 0..shot.height {
        for x in 0..shot.width {
            let p = shot.pixel(x, y);
            if [p[0], p[1], p[2]] == frame::CLEAR_BYTES {
                sky += 1;
            }
        }
    }
    sky
}

/// Сцена: одне тіло радіуса Землі, камера над заданим напрямком.
fn looking_down(direction: [f64; 3], altitude: f64) -> Scene {
    let length =
        (direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2])
            .sqrt();
    let distance = sphere::EARTH_RADIUS_M + altitude;
    let eye = [
        direction[0] / length * distance,
        direction[1] / length * distance,
        direction[2] / length * distance,
    ];
    // Вертикаль кадру — будь-яка, аби не вздовж погляду. Напрямок кута куба
    // ніколи не паралельний осі x, тож ця вистачає на всі вісім.
    let camera = Camera::look_at(eye, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);

    let mut scene = Scene::new(camera);
    scene.bodies.push(Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: sphere::EARTH_RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: TileSet::Smooth,
    });
    scene
}

/// Вісім кутів куба, і в жодному з них крізь планету не видно неба.
#[test]
fn no_sky_shows_through_the_planet_from_any_cube_corner() {
    let Some(gpu) = gpu() else { return };

    let out = std::path::Path::new("build/r2c");
    for &x in &[-1.0, 1.0] {
        for &y in &[-1.0, 1.0] {
            for &z in &[-1.0, 1.0] {
                let scene = looking_down([x, y, z], ALTITUDE_M);
                let shot =
                    shot::take_scene(&gpu, SIZE, SIZE, &scene).expect("кадр мав намалюватися");
                let sky = sky_pixels(&shot);

                let name = format!(
                    "corner_{}{}{}.png",
                    if x > 0.0 { 'p' } else { 'm' },
                    if y > 0.0 { 'p' } else { 'm' },
                    if z > 0.0 { 'p' } else { 'm' }
                );
                // Знімок лягає на диск незалежно від результату: коли він
                // колись стане червоним, дивитися буде на що.
                let _ = shot.write_png(&out.join(&name));

                println!("  {name}: {sky} пікселів неба");
                assert_eq!(
                    sky, 0,
                    "{name}: крізь планету видно небо в {sky} пікселях — це \
                     тріщина, і причина її в R2a або R2b"
                );
            }
        }
    }
}

/// Контроль: детектор неба таки бачить небо.
///
/// Без нього попередній тест був би зелений і на кадрі, зафарбованому суцільно
/// будь-чим, і на зламаному порівнянні кольорів. Тут камера відходить на ту
/// висоту, з якої диск свідомо не накриває кадру, і небо мусить з'явитися —
/// приблизно стільки, скільки лишає від кадру `asin(R/(R+h))`.
#[test]
fn the_sky_detector_does_see_the_sky() {
    let Some(gpu) = gpu() else { return };

    let scene = looking_down([1.0, 1.0, 1.0], frame::DEFAULT_ALTITUDE_M);
    let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene).expect("кадр мав намалюватися");
    let sky = sky_pixels(&shot);
    let all = (SIZE * SIZE) as usize;

    println!(
        "  з {:.1e} м: {sky} пікселів неба з {all} ({:.3} кадру)",
        frame::DEFAULT_ALTITUDE_M,
        sky as f64 / all as f64
    );
    assert!(
        sky > all / 4 && sky < all,
        "з {:.1e} м небо зайняло {sky} пікселів з {all} — детектор міряє не те",
        frame::DEFAULT_ALTITUDE_M
    );
}

/// **Скільки саме пікселів коштує незшитий стик — і чому знімок про це мовчить.**
///
/// Число, на якому стоїть перша з двох мутацій зі вступу модуля.
///
/// Незшитий край лишає T-подібний стик: непарний вузол дрібнішого патча
/// стирчить із хорди грубішого рівно на стрілу прогину своєї клітинки. Але
/// саме цю величину критерій R2a і тримає під допуском в один піксель — інакше
/// він поділив би патч далі. Отже щілина не може бути ширшою за піксель за
/// побудовою, а щілина в частку пікселя не накриває жодного центру фрагмента:
/// небо крізь неї не видно, хоч тріщина є.
///
/// Тому оракулом кроку лишається рівність вершин (правило 5), а знімок ловить
/// грубе — не ту грань, не той діапазон індексів, загублений патч. Тут же —
/// число, яке пояснює, чому саме так, і сторож на випадок, якщо критерій
/// колись відпустить допуск.
#[test]
fn an_unstitched_joint_would_be_thinner_than_a_pixel() {
    use engine::cubesphere::{Edge, EDGES, SIDE};
    use engine::lod::{self, Body as LodBody};

    let focal = lod::focal_px(frame::FOV_Y, f64::from(SIZE));
    let scene = looking_down([1.0, 1.0, 1.0], ALTITUDE_M);
    let eye = scene.camera.position();
    let radius = sphere::EARTH_RADIUS_M;
    let selection = lod::select(
        &LodBody::still([0.0, 0.0, 0.0], radius),
        &scene.camera,
        focal,
        None,
    );

    let node = |patch: &engine::cubesphere::Patch, edge: Edge, k: usize| match edge {
        Edge::AMin => patch.vertex(0, k, radius),
        Edge::AMax => patch.vertex(SIDE, k, radius),
        Edge::BMin => patch.vertex(k, 0, radius),
        Edge::BMax => patch.vertex(k, SIDE, radius),
    };

    let mut worst_px: f64 = 0.0;
    let mut joints = 0;
    for (patch, &mask) in selection.patches.iter().zip(&selection.masks) {
        for edge in EDGES {
            if mask & edge.bit() == 0 {
                continue;
            }
            for k in (1..SIDE).step_by(2) {
                let here = node(patch, edge, k);
                let before = node(patch, edge, k - 1);
                let after = node(patch, edge, k + 1);
                // Хорда грубішого сусіда проходить через парні вузли; від неї
                // й міряється виступ непарного.
                let gap = (0..3)
                    .map(|c| (here[c] - (before[c] + after[c]) / 2.0).powi(2))
                    .sum::<f64>()
                    .sqrt();
                let range = (0..3)
                    .map(|c| (here[c] - eye[c]).powi(2))
                    .sum::<f64>()
                    .sqrt();
                worst_px = worst_px.max(gap / range * focal);
                joints += 1;
            }
        }
    }

    println!(
        "  {} патчів, {joints} стиків; найширша щілина без зшивання \
         {worst_px:.3} пікселя при допуску {:.1}",
        selection.patches.len(),
        lod::TOLERANCE_PX
    );

    assert!(joints > 0, "у наборі немає жодного стику рівнів");
    assert!(
        worst_px > 0.0,
        "щілина нульова ще до зшивання — стик міряється не там"
    );
    assert!(
        worst_px <= lod::TOLERANCE_PX,
        "щілина {worst_px:.3} пікселя ширша за допуск {:.1} — знімок мусив би \
         її побачити, а він не бачить",
        lod::TOLERANCE_PX
    );
}
