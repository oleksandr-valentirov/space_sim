//! Оракул проріджування: пікселі, в обидва боки (ROADMAP.md, N2a).
//!
//! Проріджування — це викидання даних, тож перевірка мусить ловити дві різні
//! помилки, і жодна з них не видно в кількості вершин окремо:
//!
//! - **проріджування нічого не дало** — вершин стільки ж, тобто критерій не
//!   спрацював, а тест на «картинка та сама» був би зелений;
//! - **лінія змінила форму** — вершин менше, і саме тому тест на кількість
//!   був би зелений теж.
//!
//! Тому обидва твердження перевіряються поруч, на одній сцені: **менше вершин
//! і та сама картинка**.

use engine::frame;
use engine::gpu::Gpu;
use engine::orbit::Orbit;
use engine::shot::{self, Shot};
use game::frame_view::ViewFrame;
use game::{mission, view};

const SIZE: u32 = 512;

/// Скільки станцій. Три, а не тридцять: критерій працює на ламану, і флот тут
/// потрібен лише щоб у кадрі були обидва масштаби — низька орбіта й halo.
const STATIONS: usize = 3;

/// Скільки діб літати. Достатньо, щоб станція намотала сотні витків один на
/// одного: саме на такому сліді проріджування має що робити.
const DAYS: f64 = 8.0;

fn gpu() -> Option<Gpu> {
    Gpu::for_tests()
}

fn thinned(
    snapshot: &game::snapshot::WorldSnapshot,
    camera: engine::camera::Camera,
    height_px: u32,
) -> engine::scene::Scene {
    let mut cache = game::trail::Cache::new();
    let mut thinning = view::Thinning {
        cache: &mut cache,
        height_px,
    };
    view::build_thinned(snapshot, camera, &[], ViewFrame::Inertial, &mut thinning)
}

/// Флот без пенсії ланок (N3a).
///
/// Пенсія проріджує старі ланки тими самими хордами, тож із нею критерій
/// кадру дістає вже проріджене й економить удвічі менше (виміряно: 6039 → 3227
/// замість ×3 на сирих). Тут перевіряється **критерій**, а не сума двох
/// проріджувань, тож пенсія вимкнена; їхнє накладання — число N3a в ROADMAP.
fn flown() -> game::snapshot::WorldSnapshot {
    let mut world = mission::fleet(&mission::default_asset(), STATIONS).expect("флот будується");
    world.set_retirement(None);
    world.run_to_day(mission::start().t + DAYS * 86400.0, 1.0, 8);
    world.snapshot()
}

fn vertices(scene: &engine::scene::Scene) -> usize {
    scene.polylines.iter().map(|line| line.points.len()).sum()
}

/// Засвічені пікселі `a`, поруч з якими в `b` немає жодного засвіченого.
///
/// **Не «різні пікселі», і це не послаблення оракула, а виправлення його.**
/// Критерій дозволяє лінії зсунутися на пів пікселя, а лінія завтовшки в
/// піксель від зсуву на пів пікселя міняє більшість своїх пікселів — на
/// масштабі пари Земля-Місяць виміряно 358 змінених із 1226 засвічених при
/// формі, яка не змінилась. Тобто попіксельне порівняння тут міряло б
/// растеризацію, а не проріджування.
///
/// Твердження, яке справді треба перевірити: **лінія не поїхала**. Викинутий
/// виток лишив би в повному кадрі засвічені пікселі, поруч з якими в
/// прорідженому немає нічого, — і саме це рахується нижче.
fn unmatched(a: &Shot, b: &Shot) -> u64 {
    let mut count = 0;
    for y in 0..a.height {
        for x in 0..a.width {
            if !is_lit(a, x, y) {
                continue;
            }
            let near = (y.saturating_sub(1)..=(y + 1).min(b.height - 1)).any(|ny| {
                (x.saturating_sub(1)..=(x + 1).min(b.width - 1)).any(|nx| is_lit(b, nx, ny))
            });
            if !near {
                count += 1;
            }
        }
    }
    count
}

fn is_lit(shot: &Shot, x: u32, y: u32) -> bool {
    let p = shot.pixel(x, y);
    [p[0], p[1], p[2]] != frame::CLEAR_BYTES
}

/// Скільки пікселів не є фоном.
fn lit(shot: &Shot) -> u64 {
    let mut count = 0;
    for y in 0..shot.height {
        for x in 0..shot.width {
            let p = shot.pixel(x, y);
            if [p[0], p[1], p[2]] != frame::CLEAR_BYTES {
                count += 1;
            }
        }
    }
    count
}

/// Три масштаби камери, як вимагає перевірка кроку: навколоземний, уся пара
/// Земля-Місяць, уся місія.
fn scales() -> [(&'static str, f64); 3] {
    [
        ("навколоземний", 2.0e7),
        ("пара Земля-Місяць", 5.0e8),
        ("уся місія", mission::CAMERA_ALTITUDE_M),
    ]
}

#[test]
fn thinning_drops_vertices_and_keeps_the_picture() {
    let Some(gpu) = gpu() else { return };
    let snapshot = flown();

    for (name, altitude) in scales() {
        let camera = || Orbit::at_altitude(altitude).camera();

        let full = view::build_in(&snapshot, camera(), ViewFrame::Inertial);
        let thin = thinned(&snapshot, camera(), SIZE);

        // Перше твердження: вершин не більшає ніде, а на масштабі всієї місії
        // меншає в рази.
        //
        // Порогу «вдвічі» на кожному масштабі тут свідомо немає, і це не
        // послаблення тесту, а те, що виміряв сам критерій: з 2·10⁷ м орбіта
        // станції займає 155 пікселів, і при 18 семплах на виток стріла
        // прогину між сусідніми — 1.2 пікселя. Тобто вузли там **потрібні**,
        // і критерій, який їх викинув би, був би зламаний. Проріджування живе
        // на масштабі карти, де виток менший за піксель.
        assert!(
            vertices(&thin) <= vertices(&full),
            "{name}: {} → {} вершин, проріджування додало вершин",
            vertices(&full),
            vertices(&thin)
        );
        if altitude >= mission::CAMERA_ALTITUDE_M {
            assert!(
                vertices(&thin) * 2 <= vertices(&full),
                "{name}: {} → {} вершин, це не проріджування",
                vertices(&full),
                vertices(&thin)
            );
        }

        let full_shot = shot::take_scene(&gpu, SIZE, SIZE, &full).expect("кадр");
        let thin_shot = shot::take_scene(&gpu, SIZE, SIZE, &thin).expect("кадр");

        // Друге: лінія не поїхала — в обидва боки. Один бік ловить викинуту
        // деталь, другий — домальовану; допуск у частках засвічених, бо на
        // різних масштабах слід займає різну площу.
        let lit_full = lit(&full_shot).max(1);
        let lit_thin = lit(&thin_shot).max(1);
        let lost = unmatched(&full_shot, &thin_shot);
        let gained = unmatched(&thin_shot, &full_shot);
        assert!(
            lost * 100 <= lit_full * 2,
            "{name}: {lost} засвічених пікселів із {lit_full} лишилися без пари — \
             проріджування з'їло деталь"
        );
        assert!(
            gained * 100 <= lit_thin * 2,
            "{name}: {gained} пікселів із {lit_thin} з'явилися там, де їх не було"
        );

        // І третє, без якого друге можна пройти порожнім кадром.
        assert!(
            lit_thin * 4 >= lit_full,
            "{name}: у прорідженому кадрі {lit_thin} засвічених проти {lit_full} — слід зник"
        );
    }
}

/// Критерій екранний, отже від роздільності він **мусить** залежати: на
/// більшому кадрі пів пікселя — менша величина в метрах, і вершин лишається
/// більше.
///
/// Тест на те, що критерій справді дивиться на екран, а не на метри: якби
/// допуск був у метрах, обидва числа збіглися б.
#[test]
fn a_bigger_frame_keeps_more_vertices() {
    let snapshot = flown();
    let camera = || Orbit::at_altitude(mission::CAMERA_ALTITUDE_M).camera();

    let small = thinned(&snapshot, camera(), 360);
    let large = thinned(&snapshot, camera(), 1440);

    assert!(
        vertices(&large) > vertices(&small),
        "640×360 лишив {} вершин, 2560×1440 — {}: критерій не екранний",
        vertices(&small),
        vertices(&large)
    );
}
