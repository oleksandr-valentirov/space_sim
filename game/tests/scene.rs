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

// ---------------------------------------------------------------------------
// Тіла в сцені (ROADMAP-PLANETS.md, R1c)

/// Сцена несе тіла як **дані**: центр, розмір, поворот.
///
/// Оракул — не пікселі (R1c нічого ще не малює по-новому), а три твердження
/// про числа, кожне з яких ловить свою помилку:
///
/// 1. Земля рівно в початку координат — кадр геоцентричний, і якби віднімання
///    робилося не від неї, вона поїхала б на 1.5·10¹¹ м;
/// 2. Місяць за 3.6–4.1·10⁸ м від неї — тобто це справді Місяць, а не
///    баріцентрична позиція, яку забули перевести;
/// 3. Земля повернута, а її поворот змінюється з часом — інакше в сцену
///    приїхала б одиниця, яку ніхто б не помітив, доки на планеті не з'явиться
///    рельєф.
#[test]
fn the_scene_carries_the_bodies_as_data() {
    use game::world::{EARTH, MOON};

    let mut world = mission::world(&mission::default_asset()).expect("світ");
    let orbit = Orbit::at_altitude(mission::CAMERA_ALTITUDE_M);

    let scene = view::build(&world.snapshot(), orbit.camera());
    assert_eq!(scene.bodies.len(), 2, "у фікстурі два тіла з розміром");

    let earth = scene.bodies[0];
    let moon = scene.bodies[1];

    // 1. Земля — початок координат кадру.
    assert_eq!(earth.centre, [0.0, 0.0, 0.0]);
    assert!(
        (earth.radius_m - 6.371e6).abs() < 1.0e4,
        "радіус Землі з ассета: {}",
        earth.radius_m
    );

    // 2. Місяць — на відстані Місяця.
    let distance =
        (moon.centre[0].powi(2) + moon.centre[1].powi(2) + moon.centre[2].powi(2)).sqrt();
    println!(
        "  Місяць за {:.4e} м, радіус {:.4e} м",
        distance, moon.radius_m
    );
    assert!(
        (3.6e8..4.1e8).contains(&distance),
        "Місяць опинився за {distance:.3e} м — це не орбіта Місяця"
    );
    assert!(
        (moon.radius_m - 1.7374e6).abs() < 1.0e4,
        "радіус Місяця з ассета: {}",
        moon.radius_m
    );

    // 3. Поворот є, він одиничний за довжиною й змінюється з часом.
    let length = |q: [f64; 4]| (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    assert!((length(earth.orientation) - 1.0).abs() < 1e-9);
    assert_ne!(
        earth.orientation,
        [1.0, 0.0, 0.0, 0.0],
        "Земля приїхала неповернутою — орієнтацію десь загубили"
    );

    // Через кілька годин поворот інший, і саме Землі: Місяць за той самий час
    // повертається помітно менше (доба проти місяця).
    // Шість годин по годиннику світу. Спершу порахувати прогноз, інакше
    // курсор упреться в горизонт і нікуди не зрушить.
    world.tick(64);
    let want = world.snapshot().t + 6.0 * 3600.0;
    while world.snapshot().t < want {
        world.step(6.0 * 3600.0 / mission::DEFAULT_WARP, 64);
    }
    let later = view::build(&world.snapshot(), orbit.camera());
    assert_ne!(
        later.bodies[0].orientation, earth.orientation,
        "за шість годин Земля не повернулася"
    );

    let turned = |a: [f64; 4], b: [f64; 4]| {
        // Кут між двома кватерніонами: 2·acos|⟨a, b⟩|.
        let d = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]).abs();
        2.0 * d.clamp(-1.0, 1.0).acos()
    };
    let earth_turn = turned(earth.orientation, later.bodies[0].orientation);
    let moon_turn = turned(moon.orientation, later.bodies[1].orientation);
    println!(
        "  за 6 год: Земля на {:.3}°, Місяць на {:.3}°",
        earth_turn.to_degrees(),
        moon_turn.to_degrees()
    );
    assert!(
        earth_turn > moon_turn * 10.0,
        "Земля повернулася на {:.3}°, Місяць на {:.3}° — за шість годин \
         різниця мала б бути в десятки разів",
        earth_turn.to_degrees(),
        moon_turn.to_degrees()
    );

    // Індекси тіл лишилися в грі, а не поїхали в рушій: `Body` про них не
    // знає взагалі, і саме тому цей рядок тут — як нагадування, а не як
    // перевірка.
    assert_eq!([EARTH, MOON], [3, 4]);
}
