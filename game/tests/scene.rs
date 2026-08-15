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
    // Спільний помічник рушія: він же вирішує, чи пропуск дозволений
    // (`SPACE_SIM_REQUIRE_GPU`, U6c), і друкує назву адаптера в лог.
    Gpu::for_tests()
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

// ---------------------------------------------------------------------------
// Тіла в пікселях (ROADMAP-PLANETS.md, R1e)

/// Око, з якого Земля й Місяць рівновіддалені.
///
/// Точка на серединному перпендикулярі до відрізка Земля-Місяць: відстань до
/// обох однакова **за побудовою**. Тоді видимі розміри відносяться рівно як
/// радіуси, без поправки на дальність, — інакше довелося б доводити, скільки
/// саме дальність з'їла з різниці, а це вже не оракул, а підгонка.
fn eye_beside(earth: [f64; 3], moon: [f64; 3], distance: f64) -> [f64; 3] {
    let line = sub(moon, earth);
    let mid = [
        earth[0] + line[0] / 2.0,
        earth[1] + line[1] / 2.0,
        earth[2] + line[2] / 2.0,
    ];
    // Убік від лінії тіл — будь-куди, аби перпендикулярно.
    let away = unit(cross(line, [0.0, 0.0, 1.0]));
    [
        mid[0] + away[0] * distance,
        mid[1] + away[1] * distance,
        mid[2] + away[2] * distance,
    ]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn length(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn unit(v: [f64; 3]) -> [f64; 3] {
    let n = length(v);
    [v[0] / n, v[1] / n, v[2] / n]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Радіус диска тіла в пікселях, коли воно **в центрі** кадру.
///
/// `asin(R/d)` — точний кут силуету опуклої сфери (F5), далі тангенс через
/// половину поля зору: та сама арифметика, що в проєкційній матриці.
///
/// Тільки в центрі: збоку сфера проєктується в еліпс (тим помітніший, чим
/// далі від осі), і кругла формула там просто не про ту фігуру. Саме тому
/// розміри міряються трьома кадрами, а не одним.
fn disc_radius_px(radius_m: f64, distance_m: f64, height: u32) -> f64 {
    let half_angle = (radius_m / distance_m).asin();
    half_angle.tan() / (frame::FOV_Y / 2.0).tan() * f64::from(height) / 2.0
}

fn is_lit(shot: &Shot, x: u32, y: u32) -> bool {
    let p = shot.pixel(x, y);
    [p[0], p[1], p[2]] != frame::CLEAR_BYTES
}

/// Скільки світлих пікселів у квадраті навколо точки, і де їхній центр ваги.
fn blob(shot: &Shot, centre: [f32; 2], half: f64) -> (u64, [f64; 2]) {
    let mut count = 0u64;
    let mut sum = [0.0f64; 2];
    for y in 0..shot.height {
        for x in 0..shot.width {
            if !is_lit(shot, x, y) {
                continue;
            }
            if (f64::from(x) - f64::from(centre[0])).abs() > half
                || (f64::from(y) - f64::from(centre[1])).abs() > half
            {
                continue;
            }
            count += 1;
            sum[0] += f64::from(x) + 0.5;
            sum[1] += f64::from(y) + 0.5;
        }
    }
    let middle = if count == 0 {
        [0.0, 0.0]
    } else {
        [sum[0] / count as f64, sum[1] / count as f64]
    };
    (count, middle)
}

/// Обидва тіла зі снапшоту — на своїх місцях і свого розміру.
///
/// Це те, чого не закрив R1c: сцена вже несла два тіла, а кадр і далі малював
/// одну сферу радіуса Землі в початку координат.
///
/// Кадрів три, і це не марнотратство. Око в усіх одне — на серединному
/// перпендикулярі, звідки тіла рівновіддалені. Перший кадр дивиться між ними
/// й відповідає на «де вони»: центр ваги диска проти проєкції центра тіла,
/// плюс порожнє небо навколо. Другий і третій дивляться на кожне тіло окремо
/// й відповідають на «якого вони розміру»: рівно в центрі кадру силует —
/// круг, і його радіус має точну формулу. Збоку той самий силует — еліпс
/// (виміряно: 35×26 пікселів за 41° від осі), і круглий оракул там міряв би
/// не те.
#[test]
fn both_bodies_land_where_they_are_and_at_the_size_they_are() {
    let Some(gpu) = gpu() else { return };

    const WIDTH: u32 = 2048;
    const HEIGHT: u32 = 1024;
    /// Стільки, щоб обидва тіла влізли в кадр і жодне не торкнулося краю.
    const DISTANCE_M: f64 = 2.2e8;

    let world = mission::world(&mission::default_asset()).expect("світ");
    let snapshot = world.snapshot();

    // Позиції — з тієї самої сцени, яку побачить кадр: інакше перевірялися б
    // дві різні миті.
    let probe = view::build(&snapshot, Orbit::at_altitude(1.0e6).camera());
    let (earth, moon) = (probe.bodies[0], probe.bodies[1]);

    let eye = eye_beside(earth.centre, moon.centre, DISTANCE_M);
    let d_earth = length(sub(earth.centre, eye));
    let d_moon = length(sub(moon.centre, eye));
    assert!(
        (d_earth - d_moon).abs() / d_earth < 1e-12,
        "око не рівновіддалене: {d_earth:.6e} проти {d_moon:.6e}"
    );

    // Погляд між тілами, «вгору» — перпендикулярно до їхньої лінії, щоб вона
    // лягла горизонтально й кожне тіло мало свою половину кадру.
    let line = sub(moon.centre, earth.centre);
    let mid = [
        earth.centre[0] + line[0] / 2.0,
        earth.centre[1] + line[1] / 2.0,
        earth.centre[2] + line[2] / 2.0,
    ];
    let up = unit(cross(sub(mid, eye), line));
    let together = view::build(&snapshot, engine::camera::Camera::look_at(eye, mid, up));
    let taken = shot::take_scene(&gpu, WIDTH, HEIGHT, &together).expect("кадр");

    // Де вони. Радіус диска тут потрібен лише як розмір вікна, а не як оракул:
    // збоку силует еліптичний, і вікно взяте вдвічі більшим за круг саме тому.
    let mut windows = Vec::new();
    for (name, body) in [("Земля", earth), ("Місяць", moon)] {
        let distance = length(sub(body.centre, eye));
        let centre = together
            .camera
            .to_screen(frame::FOV_Y, WIDTH, HEIGHT, body.centre)
            .expect("тіло попереду камери");
        let half = 3.0 * disc_radius_px(body.radius_m, distance, HEIGHT) + 8.0;

        let (count, middle) = blob(&taken, centre, half);
        println!(
            "  {name}: центр ваги ({:.1}, {:.1}) проти проєкції ({:.1}, {:.1}), {count} пікселів",
            middle[0], middle[1], centre[0], centre[1]
        );
        assert!(count > 0, "{name}: у кадрі немає жодного пікселя тіла");
        assert!(
            (middle[0] - f64::from(centre[0])).hypot(middle[1] - f64::from(centre[1])) < 1.5,
            "{name} намальований не там, де його проєкція"
        );
        windows.push((centre, half));
    }

    // Поза двома вікнами — порожнє небо: у сцені без прогнозу малювати більше
    // нема чого, і жоден силует за своє вікно не виліз.
    let mut outside = 0u64;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            if !is_lit(&taken, x, y) {
                continue;
            }
            let inside = windows.iter().any(|(c, half)| {
                (f64::from(x) - f64::from(c[0])).abs() <= *half
                    && (f64::from(y) - f64::from(c[1])).abs() <= *half
            });
            if !inside {
                outside += 1;
            }
        }
    }
    assert_eq!(outside, 0, "поза тілами світиться {outside} пікселів");

    // Якого вони розміру. Те саме око, погляд просто на тіло — і силует стає
    // кругом, для якого формула точна.
    let mut drawn = Vec::new();
    for (name, body) in [("Земля", earth), ("Місяць", moon)] {
        let distance = length(sub(body.centre, eye));
        let scene = view::build(
            &snapshot,
            engine::camera::Camera::look_at(eye, body.centre, up),
        );
        let shot = shot::take_scene(&gpu, WIDTH, HEIGHT, &scene).expect("кадр");

        let expected = disc_radius_px(body.radius_m, distance, HEIGHT);
        let centre = [WIDTH as f32 / 2.0, HEIGHT as f32 / 2.0];
        let (count, _) = blob(&shot, centre, 2.0 * expected + 8.0);

        // Радіус із площі, а не з габариту: площа збирає весь диск, тож
        // дискретизація краю входить у неї коренем, а не в повний зріст.
        let measured = (count as f64 / std::f64::consts::PI).sqrt();
        println!("  {name} у центрі кадру: радіус {measured:.2} проти {expected:.2} px");
        assert!(
            (measured - expected).abs() < 1.0,
            "{name}: диск радіуса {measured:.2} px замість {expected:.2} px"
        );
        drawn.push((measured, body.radius_m));
    }

    // І головне число кроку: розміри відносяться як радіуси. Відстань з нього
    // випала — вона однакова, і саме заради цього око стоїть, де стоїть.
    let ratio = drawn[0].0 / drawn[1].0;
    let real = drawn[0].1 / drawn[1].1;
    println!("  розміри: {ratio:.3} проти {real:.3} за радіусами");
    assert!(
        (ratio - real).abs() / real < 0.05,
        "диски відносяться як {ratio:.3}, а радіуси як {real:.3}"
    );
}

/// Повернуте тіло виглядає так само — і це не порожнє твердження.
///
/// Гладка сфера свого повороту показати **не може**: і силует, і нормаль у
/// кожній точці переходять самі в себе. Тому перевіряється рівно те, що тут
/// узагалі можна перевірити, і воно варте перевірки: поворот застосовано
/// **однаково** до геометрії й до нормалей. Застосований до однієї з них — і
/// кадр змінюється відразу: патчі роз'їжджаються або освітлення сповзає.
///
/// Виміряно, а не проголошено. Чверть оберту навколо x міняє 2382 пікселі на
/// одну одиницю яскравості (інша діагональ трикутників у сітці) і 36 пікселів
/// помітно — **усі 36 лежать за 0.1 пікселя від краю силуету**, де сітка
/// кубосфери й справді не симетрична до повороту. Контроль поруч: зсув центра
/// на один радіус міняє помітно 144414 пікселів, тобто в чотири тисячі разів
/// більше.
///
/// Побачити поворот **очима** можна буде з R5, коли в тіла з'явиться поверхня;
/// доти оракул орієнтації — числа R1c (полюс, RA нульового меридіана,
/// швидкість обертання), а не пікселі.
#[test]
fn turning_a_smooth_sphere_moves_only_the_edge_of_its_silhouette() {
    let Some(gpu) = gpu() else { return };

    const SIDE: u32 = 512;
    const ALTITUDE_M: f64 = 1.0e7;
    /// Різниця, більша за цю, — вже не округлення інтерполяції.
    const NOTABLE: i32 = 4;

    let world = mission::world(&mission::default_asset()).expect("світ");
    let snapshot = world.snapshot();

    let scene = |orientation: Option<[f64; 4]>, shift: f64| {
        let mut scene = view::build(&snapshot, Orbit::at_altitude(ALTITUDE_M).camera());
        if let Some(q) = orientation {
            scene.bodies[0].orientation = compose(q, scene.bodies[0].orientation);
        }
        scene.bodies[0].centre[1] += shift;
        scene
    };

    let base = scene(None, 0.0);
    let earth = base.bodies[0];
    let radius_px = disc_radius_px(
        earth.radius_m,
        length(sub(earth.centre, base.camera.position())),
        SIDE,
    );
    let taken = shot::take_scene(&gpu, SIDE, SIDE, &base).expect("кадр");

    // Помітні розбіжності разом із тим, як далеко вони від краю силуету.
    let notable = |other: &Shot| {
        let mut count = 0u64;
        let mut furthest = 0.0f64;
        for y in 0..SIDE {
            for x in 0..SIDE {
                let (a, b) = (taken.pixel(x, y), other.pixel(x, y));
                let difference = (0..3)
                    .map(|k| (i32::from(a[k]) - i32::from(b[k])).abs())
                    .max()
                    .expect("три канали");
                if difference <= NOTABLE {
                    continue;
                }
                count += 1;
                let dx = f64::from(x) + 0.5 - f64::from(SIDE) / 2.0;
                let dy = f64::from(y) + 0.5 - f64::from(SIDE) / 2.0;
                furthest = furthest.max((dx.hypot(dy) - radius_px).abs());
            }
        }
        (count, furthest)
    };

    // Чверть оберту навколо трьох осей, а не однієї: переставлена компонента
    // кватерніона збіглася б сама із собою на осі, яку вгадали.
    let half = std::f64::consts::FRAC_PI_4;
    for (name, axis) in [
        ("x", [1.0, 0.0, 0.0]),
        ("y", [0.0, 1.0, 0.0]),
        ("z", [0.0, 0.0, 1.0]),
    ] {
        let turn = [
            half.cos(),
            half.sin() * axis[0],
            half.sin() * axis[1],
            half.sin() * axis[2],
        ];
        let turned = shot::take_scene(&gpu, SIDE, SIDE, &scene(Some(turn), 0.0)).expect("кадр");

        let (count, furthest) = notable(&turned);
        println!(
            "  поворот на 90° навколо {name}: помітних пікселів {count}, \
             найдальший за {furthest:.2} px від краю силуету"
        );
        assert!(
            count < 100,
            "поворот навколо {name} змінив {count} пікселів помітно — геометрія \
             й нормалі поїхали різними шляхами"
        );
        assert!(
            furthest < 1.0,
            "поворот навколо {name} змінив піксель за {furthest:.2} px від краю \
             силуету — це вже не край"
        );
    }

    // Контроль: те саме порівняння на зсуві в один радіус. Без нього «нічого
    // не змінилося» означало б лише те, що порівняння сліпе.
    let shifted = shot::take_scene(&gpu, SIDE, SIDE, &scene(None, earth.radius_m)).expect("кадр");
    let (count, _) = notable(&shifted);
    println!("  зсув на один радіус: помітних пікселів {count}");
    assert!(
        count > 50_000,
        "зсув на радіус змінив лише {count} пікселів — порівняння нічого не бачить"
    );
}

/// Добуток кватерніонів `[w, x, y, z]`: спершу `b`, потім `a`.
fn compose(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [aw, ax, ay, az] = a;
    let [bw, bx, by, bz] = b;
    [
        aw * bw - ax * bx - ay * by - az * bz,
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
    ]
}

// ---------------------------------------------------------------------------
// Обертовий фрейм (ROADMAP-UI.md, U6a2)

/// Синодичні координати з `view` — ті самі, що дає формула рушія, звірена з C.
///
/// Оракул тут не «петля виглядає замкненою», і саме тому він вартий чогось.
/// `engine::trajectory::rotating_position` звірена з `frame_from_inertial`
/// (C, `core/frame.h`) на 1345 семплах фікстури з розбіжністю 3.48·10⁻⁷ (F6);
/// якщо перетворення гри збігається з нею на всій живій траєкторії, воно
/// збігається і з ядром — транзитивно, без другої фікстури.
///
/// Різниця між ними рівно одна й навмисна: рушій віддає безрозмірні одиниці
/// CR3BP (поділені на `L` **своєї** миті), а гра множить їх на теперішню
/// відстань Земля-Місяць. Тому Місяць кожного семпла лягає туди, де Місяць
/// зараз, — і саме це тримає картинку нерухомою, поки `L` гуляє в межах
/// 3.63–4.06·10⁸ м.
#[test]
fn the_rotating_frame_agrees_with_the_formula_checked_against_c() {
    use game::frame_view::ViewFrame;

    let mut world = mission::world(&mission::default_asset()).expect("світ");
    world.tick(16);
    let snapshot = world.snapshot();

    let camera = || Orbit::at_altitude(mission::CAMERA_ALTITUDE_M).camera();
    let scene = view::build_in(&snapshot, camera(), ViewFrame::Rotating);

    // Той самий сталий масштаб, яким гра множить безрозмірні координати.
    let scale = game::frame_view::SYNODIC_SCALE_M;

    // Те, що мала б дати гра: формула рушія на тих самих семплах і тих самих
    // нормалях, помножена на масштаб.
    let vessel = &snapshot.vessels[0];
    let mut expected: Vec<[f64; 3]> = Vec::new();
    for leg in &vessel.legs {
        let normals = view::plane_normals(&leg.samples);
        for (index, sample) in leg.samples.iter().enumerate() {
            if sample.state.t <= snapshot.t {
                continue;
            }
            // Рушій чекає **одиничну** нормаль (`fill_axes` її нормує), а
            // `plane_normals` віддає `d × ḋ` як є — гра нормує сама, всередині
            // базису. Різні контракти на ту саму величину, і мовчазна
            // невідповідність тут дала б 10²³ м різниці.
            let n = normals[index];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            let p = engine::trajectory::rotating_position(
                [sample.state.r.x, sample.state.r.y, sample.state.r.z],
                sample.earth,
                sample.moon,
                [n[0] / len, n[1] / len, n[2] / len],
            );
            expected.push([p[0] * scale, p[1] * scale, p[2] * scale]);
        }
    }
    assert!(
        expected.len() > 500,
        "прогноз надто короткий: {}",
        expected.len()
    );

    // Прогноз — найдовша ламана; історії на початку місії немає.
    let drawn = scene
        .polylines
        .iter()
        .max_by_key(|p| p.points.len())
        .expect("у сцені є ламані")
        .points
        .clone();
    assert_eq!(
        drawn.len(),
        expected.len(),
        "кількість точок розійшлася — порівнюються різні ламані"
    );

    let mut worst = 0.0f64;
    for (a, b) in drawn.iter().zip(expected.iter()) {
        let e = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        worst = worst.max(e);
    }
    println!(
        "  {} точок, найгірша розбіжність із формулою рушія {worst:.3e} м",
        drawn.len()
    );

    // Метр на 4·10⁸ — це 2.5·10⁻⁹ відносно, тобто рівень самої формули, а не
    // помилка перетворення. `μ` у гри з ассета, у рушія — константа, і на
    // цьому рівні вони теж мали б збігтися.
    assert!(
        worst < 1.0,
        "розбіжність {worst:.3e} м — це вже інша формула, а не інша арифметика"
    );

    // Друге число, заради якого карту й вмикають: у синодичному фреймі та сама
    // траєкторія займає втричі менше місця, бо з неї прибрано обертання пари.
    let spread = |points: &[[f64; 3]]| {
        let mut worst: f64 = 0.0;
        for p in points {
            for q in points.iter().step_by(points.len() / 32 + 1) {
                let d =
                    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
                worst = worst.max(d);
            }
        }
        worst
    };
    let inertial = view::build_in(&snapshot, camera(), ViewFrame::Inertial);
    let inertial_points = inertial
        .polylines
        .iter()
        .max_by_key(|p| p.points.len())
        .expect("у сцені є ламані")
        .points
        .clone();
    let (a, b) = (spread(&inertial_points), spread(&drawn));
    println!("  розмах: інерціально {a:.4e} м, синодично {b:.4e} м");
    assert!(
        b < 0.5 * a,
        "синодичний розмах {b:.3e} проти інерціального {a:.3e} — фрейм нічого \
         не прибрав"
    );
}

/// Тіла в синодичному фреймі стоять там, де їм належить.
///
/// Земля за `−μ·L` від початку координат, Місяць за `(1 − μ)·L`, обидва на осі
/// x — це визначення фрейму, і воно ж перевіряє, що тіла пройшли **те саме**
/// перетворення, що й ламані. Тіло, залишене в інерціальних координатах,
/// висіло б окремо від траєкторії навколо себе.
#[test]
fn the_pair_sits_on_the_axis_in_the_rotating_frame() {
    use game::frame_view::ViewFrame;

    let world = mission::world(&mission::default_asset()).expect("світ");
    let snapshot = world.snapshot();
    let camera = Orbit::at_altitude(mission::CAMERA_ALTITUDE_M).camera();

    let scene = view::build_in(&snapshot, camera, ViewFrame::Rotating);
    let (earth, moon) = (scene.bodies[0], scene.bodies[1]);

    // Масштаб — теперішня відстань Земля-Місяць, тобто саме те, чим фрейм
    // нормує все інше.
    let l = moon.centre[0] - earth.centre[0];
    let mu = -earth.centre[0] / l;
    println!(
        "  Земля {:?}, Місяць {:?}, L = {l:.4e} м, μ = {mu:.9}",
        earth.centre, moon.centre
    );

    assert!(
        (3.6e8..4.1e8).contains(&l),
        "відстань між тілами {l:.3e} м — це не орбіта Місяця"
    );
    assert!(
        (mu - 0.0121505856).abs() < 1e-6,
        "барицентр стоїть за μ = {mu:.9}, а мало б за 0.01215",
    );
    for (name, body) in [("Земля", earth), ("Місяць", moon)] {
        assert!(
            body.centre[1].abs() < 1.0 && body.centre[2].abs() < 1.0,
            "{name} зійшла з осі x: {:?}",
            body.centre
        );
    }
}

/// У синодичному фреймі Місяць стоїть; в інерціальному — за три доби йде геть.
///
/// Це і є та властивість, заради якої карта переходить в обертову систему,
/// перевірена в пікселях, а не в числах: камера націлена на Місяць у мить A і
/// не рухається, а світ проживає три доби. У синодичному фреймі кадр B
/// збігається з кадром A; в інерціальному Місяць за той самий час проходить
/// близько 36° орбіти — тобто чверть мільярда метрів — і з поля зору
/// завширшки 5.8·10⁷ м зникає цілком.
///
/// Ламані зі сцени навмисно прибрані: вони рухаються в обох фреймах (апарат
/// летить, прогноз довшає), і без цього кадр міряв би дві речі одразу.
#[test]
fn the_moon_stands_still_in_the_rotating_frame_and_leaves_the_inertial_one() {
    use game::frame_view::ViewFrame;
    use game::world::{EARTH, MOON};

    let Some(gpu) = gpu() else { return };

    const SIDE: u32 = 512;
    /// Звідки дивитись на Місяць: 5·10⁷ м дають диск близько 30 пікселів.
    const DISTANCE_M: f64 = 5.0e7;
    const DAYS: f64 = 3.0;

    let mut world = mission::world(&mission::default_asset()).expect("світ");
    world.tick(16);
    let before = world.snapshot();

    // Камера на кожен фрейм своя — націлена туди, де Місяць у мить A саме в
    // цьому фреймі. Спільної камери тут бути не може: координати різні.
    let aim = |frame: ViewFrame| -> engine::camera::Camera {
        let scene = view::build_in(&before, Orbit::at_altitude(1.0e9).camera(), frame);
        let moon = scene.bodies[1].centre;
        // Збоку від лінії Земля-Місяць, щоб Земля не влізла в кадр.
        let side = [-moon[1], moon[0], 0.0];
        let n = (side[0] * side[0] + side[1] * side[1]).sqrt();
        let eye = [
            moon[0] + side[0] / n * DISTANCE_M,
            moon[1] + side[1] / n * DISTANCE_M,
            moon[2],
        ];
        engine::camera::Camera::look_at(eye, moon, [0.0, 0.0, 1.0])
    };

    // Три доби світу. Прогноз спершу порахований, інакше курсор упреться в
    // горизонт і нікуди не зрушить.
    let want = before.t + DAYS * 86400.0;
    while world.snapshot().t < want {
        world.step(DAYS * 86400.0 / mission::DEFAULT_WARP, 64);
    }
    let after = world.snapshot();

    // За цей час Місяць справді пройшов те, що мав, — інакше «зник з кадру»
    // нічого не доводило б.
    let moon_at = |snapshot: &game::snapshot::WorldSnapshot| {
        let body = |index: i32| {
            snapshot
                .bodies
                .iter()
                .find(|b| b.body == index)
                .expect("тіло у снапшоті")
        };
        let (earth, moon) = (body(EARTH), body(MOON));
        [
            moon.position[0] - earth.position[0],
            moon.position[1] - earth.position[1],
            moon.position[2] - earth.position[2],
        ]
    };
    let (a, b) = (moon_at(&before), moon_at(&after));
    let travelled = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    println!("  за {DAYS} доби Місяць пройшов {travelled:.3e} м");
    assert!(
        travelled > 1.0e8,
        "Місяць пройшов лише {travelled:.3e} м — світ не рухався"
    );

    for frame in [ViewFrame::Rotating, ViewFrame::Inertial] {
        let camera = aim(frame);
        let shoot = |snapshot: &game::snapshot::WorldSnapshot, camera| {
            let mut scene = view::build_in(snapshot, camera, frame);
            // Тільки тіла: ламані рухаються в будь-якому фреймі.
            scene.polylines.clear();
            shot::take_scene(&gpu, SIDE, SIDE, &scene).expect("кадр")
        };

        let first = shoot(&before, camera);
        let second = shoot(&after, aim(frame));

        let differing = (0..SIDE)
            .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
            .filter(|&(x, y)| first.pixel(x, y) != second.pixel(x, y))
            .count();
        let (lit_first, lit_second) = (lit(&first), lit(&second));

        println!("  {frame:?}: диск {lit_first} → {lit_second} пікселів, різних {differing}");
        // Диск Місяця з 5·10⁷ м — 15 пікселів радіуса, тобто близько 730
        // пікселів площі. Менше означало б, що камера дивиться не туди.
        assert!(
            lit_first > 500,
            "{frame:?}: у першому кадрі лише {lit_first} пікселів — Місяця не видно"
        );

        match frame {
            // Стоїть: той самий диск на тих самих пікселях. Допуск — край
            // силуету, той самий, що виміряв R1e (36 пікселів на поворот).
            ViewFrame::Rotating => assert!(
                differing < 100,
                "у синодичному фреймі за три доби змінилося {differing} пікселів — \
                 Місяць не стоїть"
            ),
            // Пішов: у кадрі, націленому на його вчорашнє місце, лишилось небо.
            ViewFrame::Inertial => {
                assert!(
                    lit_second == 0,
                    "інерціально Місяць лишив у кадрі {lit_second} пікселів — \
                     він мав піти цілком"
                );
                // Змінився кожен піксель, який був диском, — не «багато», а
                // рівно весь Місяць.
                assert!(
                    differing as u64 >= lit_first,
                    "інерціально змінилося {differing} пікселів із {lit_first} — \
                     диск зник не цілком"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Рельєф у грі (D12)

/// Гра вмикає рельєф тому тілу, для якого його завантажили, — і тільки йому.
///
/// Друга половина тут головна. `attach_terrain`, який ставить `Loaded` усім
/// тілам, пройшов би перевірку «Місяць має рельєф» і зіпсував би Землю, для
/// якої DEM у репозиторії немає взагалі: вона малювалася б місячними
/// висотами. Тому перевіряються обидва тіла, а не одне.
#[test]
fn the_game_attaches_terrain_to_the_moon_and_leaves_the_earth_smooth() {
    use engine::scene::{TerrainId, TileSet};
    use game::world::{EARTH, MOON};

    let mut world = mission::world(&mission::default_asset()).expect("світ");
    world.tick(8);
    let snapshot = world.snapshot();

    let camera = || Orbit::at_altitude(mission::CAMERA_ALTITUDE_M).camera();
    let mut scene = view::build(&snapshot, camera());

    // До виклику — гладкі всі, тобто те, що гра малювала до D12.
    assert!(
        scene.bodies.iter().all(|b| b.tiles == TileSet::Smooth),
        "сцена приїхала з рельєфом ще до того, як його ввімкнули"
    );

    // Хендл тут вигаданий навмисно: перевіряється, кому його поставили, а не
    // що в ньому лежить, — а справжній вимагав би GPU з bindless.
    let id = TerrainId(0);
    view::attach_terrain(&mut scene, &snapshot, MOON, id);

    // Порядок тіл у сцені той самий, що в снапшоті, без тих, що без радіуса.
    let with_radius: Vec<i32> = snapshot
        .bodies
        .iter()
        .filter(|b| b.radius_m > 0.0)
        .map(|b| b.body)
        .collect();
    assert_eq!(
        with_radius.len(),
        scene.bodies.len(),
        "правило порядку тіл розійшлося між build і attach_terrain"
    );

    for (body, drawn) in with_radius.iter().zip(&scene.bodies) {
        let expected = if *body == MOON {
            TileSet::Loaded(id)
        } else {
            TileSet::Smooth
        };
        assert_eq!(
            drawn.tiles, expected,
            "тіло {body} отримало не той набір тайлів"
        );
    }

    // І окремо — що Земля справді була в кадрі. Без цього рядка перевірка
    // «Земля гладка» пройшла б і на сцені, у якій Землі немає.
    assert!(
        with_radius.contains(&EARTH) && with_radius.contains(&MOON),
        "у сцені мали бути обидва тіла, а є {with_radius:?}"
    );
}

/// Тіло, якого в сцені немає, не валить виклик.
///
/// Сцена без Місяця — законний стан (ассет без нього, тіло без радіуса), і
/// `attach_terrain` мусить це пережити: рельєф — оздоблення, а не інваріант.
#[test]
fn attaching_terrain_to_a_body_that_is_not_there_does_nothing() {
    use engine::scene::TerrainId;

    let mut world = mission::world(&mission::default_asset()).expect("світ");
    world.tick(8);
    let snapshot = world.snapshot();

    let camera = Orbit::at_altitude(mission::CAMERA_ALTITUDE_M).camera();
    let mut scene = view::build(&snapshot, camera);
    let before: Vec<_> = scene.bodies.iter().map(|b| b.tiles).collect();

    // 99 — тіла з таким номером в ассеті немає.
    view::attach_terrain(&mut scene, &snapshot, 99, TerrainId(0));

    let after: Vec<_> = scene.bodies.iter().map(|b| b.tiles).collect();
    assert_eq!(before, after, "невідоме тіло змінило чужі тайли");
}

/// Повітря є в Землі й немає в Місяця (ROADMAP-ATMOSPHERE.md, S1).
///
/// Тіло без атмосфери — це не «ще не зробили», а факт про Місяць, і рушій
/// має право на ньому економити. Тест ловить рівно ту помилку, яку легко
/// зробити при першому викликачі: повісити повітря на всі тіла ассета.
#[test]
fn the_earth_carries_air_and_the_moon_does_not() {
    let mut world = mission::world(&mission::default_asset()).expect("світ будується");
    world.run_to_day(mission::start().t + 2.0 * 86400.0, 1.0, 8);
    let snapshot = world.snapshot();
    let scene = view::build(&snapshot, Orbit::at_altitude(1.0e9).camera());

    // Порядок тіл у сцені — той самий, що в снапшоті, з пропуском
    // безрозмірних (`view::attach_terrain` спирається на це саме правило).
    let mut with_air = 0;
    for body in &scene.bodies {
        if body.air.is_some() {
            with_air += 1;
        }
    }
    assert_eq!(
        with_air,
        1,
        "повітря має бути рівно в одного тіла з {}",
        scene.bodies.len()
    );

    let earth = scene
        .bodies
        .iter()
        .find(|b| b.air.is_some())
        .expect("щойно порахували");
    let air = earth.air.expect("щойно перевірили");
    // Верхня межа стоїть над радіусом **із ассета**, а не над константою.
    assert!(
        (air.thickness_m(earth.radius_m) - engine::scene::Atmosphere::EARTH_THICKNESS_M).abs()
            < 1.0,
        "шар {} м над радіусом {}",
        air.thickness_m(earth.radius_m),
        earth.radius_m
    );
}
