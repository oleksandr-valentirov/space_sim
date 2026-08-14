//! Годинник не входить в інтегратор (ROADMAP J2).
//!
//! Це головна перевірка кроку й, мабуть, усього етапу. Твердження просте й
//! перевіряється просто: **прогнати ту саму місію з різною частотою кадрів,
//! різним warp і паузами посеред — і отримати бітово ту саму траєкторію.**
//!
//! Воно не самоочевидне. Досить одному `t_end` прийти від годинника — і
//! частота кадрів впишеться в послідовність кроків інтегратора, бо `prop_run`
//! приземляє останній крок рівно на `t_end` (CLAUDE.md, інваріант 9). Помилка
//! такого класу не падає: вона дає правильну на вигляд криву, яка просто трохи
//! інша на іншій машині — тобто зламаний детермінізм, знайдений через півроку
//! на чужому сейві.

use game::clock::Stall;
use game::leg::Sample;
use game::mission;

/// Проганяє місію послідовністю кадрів `dt` (циклічно) і повертає всі семпли.
fn run(pattern: &[f64], budget: usize) -> (Vec<Sample>, f64) {
    let mut world = mission::world(&mission::default_asset()).expect("світ будується");

    // Стеля на кількість кадрів — щоб зламаний тест падав, а не висів.
    for frame in 0..2_000_000 {
        world.step(pattern[frame % pattern.len()], budget);
        if world.clock().stall() == Some(Stall::MissionEnd) {
            break;
        }
    }

    let snapshot = world.snapshot();
    let samples = snapshot
        .vessels
        .iter()
        .flat_map(|v| v.legs.iter().flat_map(|leg| leg.samples.iter().copied()))
        .collect();

    (samples, snapshot.t)
}

fn assert_same(a: &[Sample], b: &[Sample], what: &str) {
    assert_eq!(a.len(), b.len(), "{what}: різна кількість семплів");
    assert!(!a.is_empty(), "{what}: нічого не пораховано");

    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        for (name, p, q) in [
            ("t", x.state.t, y.state.t),
            ("r.x", x.state.r.x, y.state.r.x),
            ("r.y", x.state.r.y, y.state.r.y),
            ("r.z", x.state.r.z, y.state.r.z),
            ("v.x", x.state.v.x, y.state.v.x),
            ("v.y", x.state.v.y, y.state.v.y),
            ("v.z", x.state.v.z, y.state.v.z),
        ] {
            assert_eq!(
                p.to_bits(),
                q.to_bits(),
                "{what}: семпл {i}, {name}: {p:e} проти {q:e}"
            );
        }
    }
}

/// П'ять кадрів на секунду проти п'ятисот — бітово те саме.
#[test]
fn the_frame_rate_does_not_reach_the_numbers() {
    let (slow, slow_t) = run(&[0.2], 4);
    let (fast, fast_t) = run(&[0.002], 4);

    assert_same(&slow, &fast, "5 fps проти 500 fps");

    // Обидва дійшли до кінця місії — інакше однакові семпли означали б лише,
    // що обидва зупинилися в тому самому місці зарано.
    let end = mission::start().t + mission::DAYS * 86400.0;
    assert_eq!(slow_t.to_bits(), end.to_bits(), "повільний не дійшов");
    assert_eq!(fast_t.to_bits(), end.to_bits(), "швидкий не дійшов");
}

/// Смикана частота кадрів — теж бітово те саме.
///
/// Рівна послідовність могла б випадково лягти в ту саму сітку; ця не може.
/// Числа взяті як просадки реальної гри: 60 fps із випадковими провалами до
/// 3 fps, включно з нулем (кадр без поступу часу теж мусить бути безпечним).
#[test]
fn a_stuttering_frame_rate_does_not_reach_them_either() {
    let steady = run(&[0.016], 4).0;
    let stutter = run(&[0.016, 0.33, 0.001, 0.0, 0.07, 0.016, 0.21], 4).0;

    assert_same(&steady, &stutter, "рівні кадри проти смиканих");
}

/// Warp і пауза — теж не змінюють нічого, крім швидкості курсора.
///
/// Той самий прогін, але з половинним warp і з паузою на третині шляху.
/// Пауза тут не декорація: вона зупиняє курсор, лишаючи тік працювати, і
/// якби горизонт рахувався «від часу», це було б рівно те місце, де воно
/// проявилося б.
#[test]
fn warp_and_pause_do_not_reach_them_either() {
    let plain = run(&[0.016], 4).0;

    let mut world = mission::world(&mission::default_asset()).expect("світ будується");
    world.clock_mut().set_warp(mission::DEFAULT_WARP / 2.0);

    let mut paused_once = false;
    for frame in 0..2_000_000 {
        // Пауза на п'ятдесят кадрів посеред місії.
        if !paused_once && frame == 1500 {
            world.clock_mut().toggle_pause();
        }
        if !paused_once && frame == 1550 {
            world.clock_mut().toggle_pause();
            paused_once = true;
        }
        // Warp удвічі вгору вже після паузи.
        if frame == 1600 {
            world.clock_mut().scale_warp(2.0);
        }

        world.step(0.016, 4);
        if world.clock().stall() == Some(Stall::MissionEnd) {
            break;
        }
    }

    assert!(
        paused_once,
        "пауза так і не спрацювала — місія коротша, ніж тест думає"
    );

    let snapshot = world.snapshot();
    let varied: Vec<Sample> = snapshot
        .vessels
        .iter()
        .flat_map(|v| v.legs.iter().flat_map(|leg| leg.samples.iter().copied()))
        .collect();

    assert_same(&plain, &varied, "сталий warp проти зміненого з паузою");
}

/// Курсор ніколи не обганяє пораховане.
///
/// Перевіряється щокадру на максимальному warp і з однією ланкою на кадр,
/// тобто в найгірших умовах, які гра може створити сама.
#[test]
fn the_cursor_never_outruns_what_is_computed() {
    let mut world = mission::world(&mission::default_asset()).expect("світ будується");
    world.clock_mut().set_warp(game::clock::MAX_WARP);

    for _ in 0..400 {
        world.step(0.016, 1);
        let snapshot = world.snapshot();

        for vessel in &snapshot.vessels {
            assert!(
                snapshot.t <= vessel.computed_to,
                "курсор на {} обігнав пораховане до {}",
                snapshot.t,
                vessel.computed_to
            );
        }

        if snapshot.stall == Some(Stall::MissionEnd) {
            return;
        }
    }

    panic!("на максимальному warp місія мала скінчитися за 400 кадрів");
}

/// Світ, який нічого не рахує, не має права рухати час.
///
/// Це і є механізм стелі warp у чистому вигляді: не число в коді, а те, що
/// курсору нікуди йти. Бюджет нуль — крайній випадок «інтегратор не встигає»,
/// і саме тому він тут: **на цій місії warp у пропускну здатність не
/// упирається взагалі.** Одна ланка — це близько одинадцяти діб траєкторії, а
/// кадр на максимальному warp — 1.85 доби; інтегратор випереджає годинник
/// ушестеро навіть на стелі й з однією ланкою на кадр. Отже спровокувати
/// [`Stall::Horizon`] чесною роботою тут неможливо, і це не хиба тесту, а
/// виміряна властивість: на вільному польоті пропускної здатності ядра
/// (3·10⁶ кроків/с, ROADMAP I3) забагато. Упреться воно на тязі та в
/// атмосфері — там крок фіксований і малий.
#[test]
fn a_world_that_computes_nothing_cannot_move_its_clock() {
    let mut world = mission::world(&mission::default_asset()).expect("світ будується");
    world.clock_mut().set_warp(game::clock::MAX_WARP);

    let before = world.clock().t();
    for _ in 0..10 {
        world.step(0.016, 0);
    }

    assert_eq!(
        world.clock().t().to_bits(),
        before.to_bits(),
        "час пішов уперед без жодної порахованої ланки"
    );
    assert_eq!(
        world.clock().stall(),
        Some(Stall::Horizon),
        "час стоїть, але не каже чому"
    );
}

/// Інтерпольований стан лежить на траєкторії, а не поруч із нею.
///
/// Оракул — сама траєкторія: у моменти семплів інтерполяція зобов'язана
/// віддати рівно семпл (це ловить помилку в базисі Ерміта), а посередині —
/// не далі за те, на скільки крива встигає відхилитися від хорди.
#[test]
fn the_interpolated_state_lies_on_the_trajectory() {
    let mut world = mission::world(&mission::default_asset()).expect("світ будується");
    world.run_to_end(1.0, 8);

    let vessel = &world.vessels()[0];
    let trajectory = &vessel.trajectory;

    let mut worst_at_sample = 0.0f64;
    let mut worst_chord = 0.0f64;

    let all: Vec<Sample> = trajectory
        .legs()
        .iter()
        .flat_map(|leg| leg.samples.iter().copied())
        .collect();

    for pair in all.windows(2) {
        let (a, b) = (pair[0].state, pair[1].state);

        // У вузлі — рівно вузол.
        let at = trajectory.state_at(a.t);
        worst_at_sample = worst_at_sample.max(distance(at.r, a.r));

        // Посередині — між хордою й кубікою. Хорда тут і є мірою: якщо
        // інтерполяція гірша за неї, вона не інтерполяція.
        let mid_t = 0.5 * (a.t + b.t);
        let mid = trajectory.state_at(mid_t);
        let chord = [
            0.5 * (a.r.x + b.r.x),
            0.5 * (a.r.y + b.r.y),
            0.5 * (a.r.z + b.r.z),
        ];
        worst_chord = worst_chord.max(distance(
            mid.r,
            core_rs::Vec3d {
                x: chord[0],
                y: chord[1],
                z: chord[2],
            },
        ));
    }

    println!("  у вузлі: {worst_at_sample:e} м, від хорди: {worst_chord:e} м");

    // У вузлі Ерміт точний за побудовою; лишається тільки заокруглення.
    assert!(
        worst_at_sample < 1e-6,
        "у моменти семплів інтерполяція має віддавати сам семпл, а промахнулась \
         на {worst_at_sample:e} м"
    );

    // Відхилення від хорди мусить бути помітним, інакше кубіка вироджена в
    // пряму — тобто швидкості в неї не входять, і помилку не було б видно.
    assert!(
        worst_chord > 1.0,
        "кубіка збіглася з хордою ({worst_chord:e} м) — швидкості в інтерполяцію \
         не потрапили"
    );
}

fn distance(a: core_rs::Vec3d, b: core_rs::Vec3d) -> f64 {
    let d = [a.x - b.x, a.y - b.y, a.z - b.z];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}
