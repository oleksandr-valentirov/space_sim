//! Сцена, яку гра дає рушієві, справді доїжджає до пікселів (ROADMAP J1).
//!
//! Тести `trajectory.rs` доводять, що числа правильні; ці — що вони
//! потрапляють у кадр. Без другого перше нічого не варте: порожня сцена й
//! правильна дають однаково «зелений тест», якщо не подивитися на пікселі.
//!
//! Оракул тут не аналітичний, і не може ним бути: форма halo-орбіти в
//! перспективі не має короткої формули. Тому перевіряються твердження, які
//! ламаються від реальних помилок — що лінія є, що вона зникає разом із
//! траєкторією, і що камера її рухає.

use engine::frame;
use engine::gpu::Gpu;
use engine::orbit::Orbit;
use engine::shot::{self, Shot};
use game::{mission, view};

const SIZE: u32 = 256;

fn gpu() -> Option<Gpu> {
    match Gpu::new(wgpu::Instance::default(), None) {
        Ok(gpu) => Some(gpu),
        Err(_) => {
            eprintln!("ПРОПУЩЕНО: немає адаптера wgpu (немає драйвера або GPU)");
            None
        }
    }
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

/// Порахований прогноз видно в кадрі, а непорахованого — ні.
///
/// Різниця між двома кадрами і є доказом: якби перший малював щось інше
/// (скажімо, саму планету), обидва числа були б однаково ненульові.
#[test]
fn the_prediction_appears_in_the_frame_and_only_when_it_exists() {
    let Some(gpu) = gpu() else { return };

    let camera = || Orbit::at_altitude(mission::CAMERA_ALTITUDE_M).camera();
    let mut world = mission::world(&mission::default_asset()).expect("світ будується");

    // Ще нічого не пораховано: у кадрі лише планета, і з мільярда метрів вона
    // займає кілька пікселів.
    let empty = shot::take_scene(&gpu, SIZE, SIZE, &view::build(&world.snapshot(), camera()))
        .expect("кадр");
    let empty_lit = lit(&empty);

    world.run_to_end(1.0, 8);
    let full = shot::take_scene(&gpu, SIZE, SIZE, &view::build(&world.snapshot(), camera()))
        .expect("кадр");
    let full_lit = lit(&full);

    assert!(
        empty_lit < 100,
        "порожній прогноз намалював {empty_lit} пікселів — це вже не сама планета"
    );
    assert!(
        full_lit > empty_lit + 500,
        "прогноз додав лише {} пікселів ({full_lit} проти {empty_lit})",
        full_lit - empty_lit
    );

    // PNG звідси не пишеться навмисно: `cargo test` запускає бінарник з
    // каталогу крейта, і файл ліг би в `game/build/`, а не там, де на нього
    // дивляться. Знімок робить `cargo run -p game -- --shot`.
}

/// Камера рухає ламану так само, як рухає планету.
///
/// Найдешевша перевірка того, що ламана йде тим самим шляхом camera-relative,
/// що й вершини сфери: якби вона проєктувалася окремо (скажімо, зі своїм
/// зсувом, як у `trajectory_render`), обертання камери її б не зачепило.
#[test]
fn the_camera_moves_the_prediction_too() {
    let Some(gpu) = gpu() else { return };

    let mut world = mission::world(&mission::default_asset()).expect("світ будується");
    world.run_to_end(1.0, 8);
    let snapshot = world.snapshot();

    let mut orbit = Orbit::at_altitude(mission::CAMERA_ALTITUDE_M);
    let before =
        shot::take_scene(&gpu, SIZE, SIZE, &view::build(&snapshot, orbit.camera())).expect("кадр");

    // Чверть оберту: орбіта лежить у площині, і збоку вона зобов'язана
    // виглядати інакше.
    orbit.drag(300.0, 0.0);
    let after =
        shot::take_scene(&gpu, SIZE, SIZE, &view::build(&snapshot, orbit.camera())).expect("кадр");

    let differing = (0..SIZE)
        .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
        .filter(|&(x, y)| before.pixel(x, y) != after.pixel(x, y))
        .count();

    assert!(
        differing > 200,
        "обертання камери змінило лише {differing} пікселів — ламана її не слухає"
    );
}
