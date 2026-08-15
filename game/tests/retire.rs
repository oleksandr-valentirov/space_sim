//! Що робить гра з минулим: вікно в обертах (N5a) і пенсія ланок (N3a).
//!
//! Пенсія викидає семпли назавжди (інваріант 5), тож перевіряти треба не «чи
//! стало менше» — це видно й так, — а **що саме вціліло**. Три речі на ланку
//! тримають сейв (`leg::restart_at`): `entry`, останній семпл і `step_out`.
//! Якщо загубиться котрась, гра завантажиться в іншу траєкторію — і жоден
//! тест на кількість цього не побачить.

use game::mission;
use game::world::{World, RAW_LEGS_BEHIND};

const DAYS: f64 = 60.0;

fn flown(retire: bool) -> World {
    let mut world = mission::world(&mission::default_asset()).expect("світ будується");
    world.set_history_trimming(if retire { Some(RAW_LEGS_BEHIND) } else { None });
    world.run_to_day(mission::start().t + DAYS * 86400.0, 1.0, 8);
    world
}

/// Сейв не змінюється від пенсії — байт у байт.
///
/// Це головна перевірка кроку. J6 обіцяє, що сейв відтворює гру бітово, а
/// стоїть він на трьох речах у ланці; пенсія викидає все інше. Різниця тут
/// означала б, що вона зачепила ті три.
#[test]
fn the_save_is_the_same_file_with_retirement_and_without() {
    let directory = std::env::temp_dir().join("space_sim_retire_test");
    std::fs::create_dir_all(&directory).expect("каталог створюється");

    let with = directory.join("with.save");
    let without = directory.join("without.save");

    game::save::write_world(&flown(true), &with).expect("сейв пишеться");
    game::save::write_world(&flown(false), &without).expect("сейв пишеться");

    let a = std::fs::read(&with).expect("сейв читається");
    let b = std::fs::read(&without).expect("сейв читається");
    assert_eq!(a, b, "пенсія змінила сейв");
}

/// Семплів меншає в рази, і число записане тут же.
///
/// Без цієї перевірки попередня була б зелена й на пенсії, яка нічого не
/// робить.
#[test]
fn retirement_costs_the_history_most_of_its_samples() {
    let retired = flown(true).vessels()[0].trajectory.sample_count();
    let whole = flown(false).vessels()[0].trajectory.sample_count();

    // Дві третини, а не «вдвічі», і поріг тут виміряний, а не обраний: на
    // halo-орбіті пенсія лишає 1207 семплів із 2304, бо ця крива справді
    // гнеться на всій довжині. На низькій орбіті виграш більший — але це вже
    // число фікстури флоту, і живе воно в ROADMAP, а не в порозі тесту.
    assert!(
        retired * 3 <= whole * 2,
        "пенсія лишила {retired} з {whole} семплів — це не пенсія"
    );
}

/// Вікно навколо курсора лишається сирим.
///
/// `state_at` на «зараз» відповідає бітово так само, як без пенсії: курсор
/// стоїть усередині вікна, і жоден його семпл пенсію не бачив.
#[test]
fn the_window_around_the_cursor_keeps_every_sample() {
    let retired = flown(true);
    let whole = flown(false);

    let now = retired.clock().t();
    assert_eq!(now, whole.clock().t(), "прогони мали дійти до однієї миті");

    let a = retired.vessels()[0].trajectory.state_at(now);
    let b = whole.vessels()[0].trajectory.state_at(now);

    for (name, x, y) in [
        ("t", a.t, b.t),
        ("r.x", a.r.x, b.r.x),
        ("r.y", a.r.y, b.r.y),
        ("r.z", a.r.z, b.r.z),
        ("v.x", a.v.x, b.v.x),
        ("v.y", a.v.y, b.v.y),
        ("v.z", a.v.z, b.v.z),
    ] {
        assert_eq!(
            x.to_bits(),
            y.to_bits(),
            "{name} на курсорі: {x:e} проти {y:e} — вікно не сире"
        );
    }
}

/// Кінці ланок не рухаються: `entry`, `t1`, `step_out` і **останній семпл**.
///
/// Перевіряється на кожній ланці, а не на першій: пенсія йде від початку, і
/// помилка на межі вікна була б помилкою рівно однієї ланки.
#[test]
fn every_leg_keeps_the_three_things_the_save_stands_on() {
    let retired = flown(true);
    let whole = flown(false);

    let a = retired.vessels()[0].trajectory.legs();
    let b = whole.vessels()[0].trajectory.legs();
    assert_eq!(a.len(), b.len(), "пенсія загубила ланку");

    for (index, (mine, theirs)) in a.iter().zip(b).enumerate() {
        assert_eq!(
            mine.entry.t.to_bits(),
            theirs.entry.t.to_bits(),
            "ланка {index}: entry поїхав"
        );
        assert_eq!(mine.t1.to_bits(), theirs.t1.to_bits(), "ланка {index}: t1");
        assert_eq!(
            mine.step_out.to_bits(),
            theirs.step_out.to_bits(),
            "ланка {index}: step_out"
        );

        let last = mine.samples.last().expect("ланка без семплів");
        let expected = theirs.samples.last().expect("ланка без семплів");
        assert_eq!(
            last.state.t.to_bits(),
            expected.state.t.to_bits(),
            "ланка {index}: останній семпл — на ньому стоїть restart_at"
        );
        assert_eq!(
            last.state.r.x.to_bits(),
            expected.state.r.x.to_bits(),
            "ланка {index}: останній семпл, r.x"
        );
    }
}

/// Головне число N5a: історія виходить на полицю й **перестає рости**.
///
/// ⚠ **Фікстура тут — станція, і це не деталь.** Перша версія тесту брала
/// halo 1151 і була зелена з хибної причини: її геоцентричний радіус-вектор
/// обходить коло раз на місячний місяць, тож двадцять обертів — це двадцять
/// місяців, більше за всю фікстуру. Полиця, яку тест бачив, була **кінцем
/// місії**, а не вікном (1128 семплів на 75-й добі й стільки ж на 90-й, при
/// десяти ланках із десяти можливих).
///
/// Станція на низькій орбіті робить оберт за півтори години, тож двадцять
/// обертів — доба з гаком, і вікно справді ріже. Оракул — той самий прогін
/// без різання: без нього історія росте, з ним виходить на полицю.
#[test]
fn the_history_stops_growing_once_the_window_is_full() {
    let counts = |trim: bool| {
        let mut world = mission::fleet(&mission::default_asset(), 1).expect("флот будується");
        world.set_history_trimming(if trim { Some(RAW_LEGS_BEHIND) } else { None });
        let start = mission::start().t;

        let mut out = Vec::new();
        for days in [20.0, 40.0, 80.0] {
            world.run_to_day(start + days * 86400.0, 1.0, 8);
            // Апарат 1 — станція; нульовий це halo, у якого оберт місячний.
            out.push(world.vessels()[1].trajectory.sample_count());
        }
        out
    };

    let windowed = counts(true);
    let whole = counts(false);

    // Без вікна історія росте — інакше перевірка нижче нічого не доводила б.
    assert!(
        whole[2] > whole[0] * 2,
        "без вікна історія мала рости: {whole:?}"
    );

    // З вікном — полиця: подвоєння прогону не додає навіть ланки.
    let slack = game::world::LEG;
    assert!(
        windowed[2] <= windowed[1] + slack,
        "історія росла далі: {windowed:?}"
    );
    assert!(
        windowed[2] * 4 < whole[2],
        "вікно зрізало замало: {windowed:?} проти {whole:?}"
    );
}

/// Пам'ять названа тим самим числом, яким говорить борг.
///
/// Не окремий підрахунок у тесті: гравець побачить саме `history_bytes`, і
/// розійтися ці два числа не мають права.
#[test]
fn the_predicted_size_is_the_sample_count_times_the_debts_number() {
    let world = flown(true);
    let trajectory = &world.vessels()[0].trajectory;
    assert_eq!(trajectory.history_bytes(), trajectory.sample_count() * 104);
}
