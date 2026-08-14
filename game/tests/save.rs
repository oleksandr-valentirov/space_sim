//! Зберегти, завантажити, продовжити — і не помітити (ROADMAP J6).
//!
//! PROJECT.md §4, правило 4, нарешті перевіряється, а не декларується:
//! **стан інтегратора входить у сейв.** Твердження перевірки одне —
//! продовження після завантаження бітово дорівнює продовженню без нього.
//!
//! Це рівно те, чого правило 4 і вимагає, і найлегший спосіб його зламати —
//! забути крок. Тоді сейв не падає й навіть виглядає правильним: траєкторія
//! після завантаження правдоподібна, просто **інша**, а в N-body розбіжність
//! росте експоненційно.

use core_rs::State;
use game::mission;
use game::plan::{Frame, Manoeuvre, Plan};
use game::save::Save;
use game::world::{VesselId, World};

const DAY: f64 = 86400.0;

fn plan_at(start_t: f64) -> Plan {
    let mut plan = Plan::new();
    plan.insert(Manoeuvre {
        t: start_t + 15.0 * DAY,
        dv: [-6.0, 0.0, 0.0],
        frame: Frame::Vnb {
            body: game::world::EARTH,
        },
    });
    plan.insert(Manoeuvre {
        t: start_t + 45.0 * DAY,
        dv: [0.0, 2.5, 0.0],
        frame: Frame::Inertial,
    });
    plan
}

fn planned_world() -> World {
    let mut world = mission::world(&mission::default_asset()).expect("світ будується");
    world
        .commit_plan(VesselId(0), plan_at(mission::start().t))
        .expect("план у майбутньому");
    world
}

fn samples_after(world: &World, t: f64) -> Vec<State> {
    world.vessels()[0]
        .trajectory
        .legs()
        .iter()
        .flat_map(|leg| leg.samples.iter())
        .map(|s| s.state)
        .filter(|s| s.t > t)
        .collect()
}

fn assert_same(a: &[State], b: &[State], what: &str) {
    assert_eq!(a.len(), b.len(), "{what}: різна кількість семплів");
    assert!(!a.is_empty(), "{what}: нічого не пораховано");

    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        for (name, p, q) in [
            ("t", x.t, y.t),
            ("r.x", x.r.x, y.r.x),
            ("r.y", x.r.y, y.r.y),
            ("r.z", x.r.z, y.r.z),
            ("v.x", x.v.x, y.v.x),
            ("v.y", x.v.y, y.v.y),
            ("v.z", x.v.z, y.v.z),
        ] {
            assert_eq!(
                p.to_bits(),
                q.to_bits(),
                "{what}: семпл {i}, {name}: {p:e} проти {q:e}"
            );
        }
    }
}

/// Головна перевірка J6.
#[test]
fn saving_and_loading_changes_nothing() {
    let start = mission::start();

    // Прогін без збереження — еталон.
    let mut plain = planned_world();
    plain.run_to_end(1.0, 8);
    let reference = samples_after(&plain, start.t + 30.0 * DAY);

    // Той самий прогін, але на 30-й добі його зберігають і піднімають наново.
    let mut interrupted = planned_world();
    interrupted.run_to_day(start.t + 30.0 * DAY, 1.0, 8);

    let saved = Save::of(&interrupted);
    let cut = saved.vessels[0].tip.t;
    assert!(
        saved.vessels[0].step > 0.0,
        "у сейві крок нуль — тоді перевіряти нема чого"
    );

    let text = saved.to_text();
    let mut loaded = Save::from_text(&text)
        .expect("сейв читається")
        .into_world(interrupted.ephemeris(), mission::config())
        .expect("світ із сейву");

    loaded.run_to_end(1.0, 8);

    // Порівнюємо все, що після точки збереження: до неї в завантаженого світу
    // історії немає за побудовою (§4: траєкторія в сейв не входить).
    let after_reload = samples_after(&loaded, cut);
    let after_plain = samples_after(&plain, cut);

    println!(
        "  збережено на добі {:.3}, звірено {} семплів",
        (cut - start.t) / DAY,
        after_reload.len()
    );
    assert!(
        after_reload.len() > 500,
        "після точки збереження мало лишитися багато семплів"
    );

    assert_same(&after_plain, &after_reload, "після завантаження");

    // Точка збереження — не пізніша за курсор і не сам курсор: це остання
    // межа ланки перед ним. Саме тому завантажена гра не стрибає ні вперед
    // (у непорахований прогноз), ні в довільну точку, з якої продовжити
    // бітово неможливо.
    let cursor = start.t + 30.0 * DAY;
    assert!(
        cut <= cursor,
        "збереглися попереду курсора: {cut} > {cursor}"
    );
    assert!(cut > start.t, "збереглися на самому старті");
    assert!(!reference.is_empty());
}

/// Викинути крок із сейву — і гра стане іншою.
///
/// Це та сама перевірка зубів, що в H1 і J3, але з найгіршою ціною: тут
/// різниця не в роботі, а в тому, що завантажена гра летить не туди, куди
/// летіла збережена.
#[test]
fn dropping_the_step_from_a_save_gives_a_different_game() {
    let start = mission::start();

    let mut world = planned_world();
    world.run_to_day(start.t + 30.0 * DAY, 1.0, 8);

    let mut honest = Save::of(&world);
    let cut = honest.vessels[0].tip.t;
    let step = honest.vessels[0].step;

    let mut careless = Save::of(&world);
    careless.vessels[0].step = 0.0;

    let run = |save: Save| -> Vec<State> {
        let mut world = save
            .into_world(
                core_rs::Ephemeris::load(&mission::default_asset())
                    .map(std::sync::Arc::new)
                    .expect("ассет"),
                mission::config(),
            )
            .expect("світ");
        world.run_to_end(1.0, 8);
        samples_after(&world, cut)
    };

    honest.vessels[0].step = step;
    let with_step = run(honest);
    let without = run(careless);

    println!(
        "  з кроком: {} семплів; без кроку: {}",
        with_step.len(),
        without.len()
    );

    assert_ne!(
        with_step.len(),
        without.len(),
        "сейв без кроку дав рівно ту саму траєкторію — тоді правило 4 з \
         PROJECT.md §4 нічого не означає, а H1 виміряв щось інше"
    );
}

/// Маневр у момент збереження не виконується вдруге й не губиться.
///
/// Найтихіша помилка сейву: стан до й після імпульсу мають **однаковий час**.
/// Правило «застосувати все, що не пізніше» виконало б маневр удвічі,
/// «застосувати все, що раніше» — жодного разу. Тому `applied` лежить у файлі
/// числом, а точка перезапуску завжди доімпульсна.
#[test]
fn a_manoeuvre_at_the_save_point_is_flown_exactly_once() {
    let start = mission::start();
    let burn_t = start.t + 15.0 * DAY;

    let mut world = planned_world();
    // Курсор трохи ЗА маневр: тоді остання межа ланки перед ним — це рівно
    // момент маневру, бо на маневрі ланка й закінчується.
    world.run_to_day(burn_t + 0.5 * DAY, 1.0, 8);

    let saved = Save::of(&world);
    assert_eq!(
        saved.vessels[0].tip.t.to_bits(),
        burn_t.to_bits(),
        "тест хотів зберегтися рівно на межі, що збігається з маневром"
    );
    assert_eq!(
        saved.vessels[0].applied, 0,
        "стан на межі — доімпульсний, тож маневр іще не застосований"
    );

    // Пост-імпульсний стан з оригінального світу: ланка, що ПОЧИНАЄТЬСЯ на
    // маневрі.
    let original = world.vessels()[0]
        .trajectory
        .legs()
        .iter()
        .find(|leg| leg.entry.t == burn_t)
        .expect("після маневру мала початися нова ланка")
        .entry;

    let mut loaded = Save::from_text(&saved.to_text())
        .expect("читається")
        .into_world(world.ephemeris(), mission::config())
        .expect("світ із сейву");
    loaded.tick(4);

    let resumed = loaded.vessels()[0]
        .trajectory
        .legs()
        .iter()
        .find(|leg| leg.entry.t == burn_t)
        .expect("після завантаження маневр мав виконатися")
        .entry;

    for (name, a, b) in [
        ("v.x", original.v.x, resumed.v.x),
        ("v.y", original.v.y, resumed.v.y),
        ("v.z", original.v.z, resumed.v.z),
    ] {
        assert_eq!(
            a.to_bits(),
            b.to_bits(),
            "{name} після завантаження {b:e} проти {a:e} — маневр виконано не \
             один раз"
        );
    }
}

/// Текст сейву читається назад бітово, і коментарі йому не заважають.
#[test]
fn the_text_round_trips_bit_for_bit() {
    let mut world = planned_world();
    world.run_to_day(mission::start().t + 10.0 * DAY, 1.0, 8);

    let saved = Save::of(&world);
    let text = saved.to_text();

    // Десяткові значення в рядках — для ока; парсер читає біти.
    assert!(text.contains('#'), "у сейві немає коментарів для читача");

    let back = Save::from_text(&text).expect("читається");

    assert_eq!(back.t.to_bits(), saved.t.to_bits());
    assert_eq!(back.warp.to_bits(), saved.warp.to_bits());
    assert_eq!(back.vessels.len(), saved.vessels.len());

    let (a, b) = (&saved.vessels[0], &back.vessels[0]);
    assert_eq!(a.name, b.name);
    assert_eq!(a.step.to_bits(), b.step.to_bits());
    assert_eq!(a.horizon_end.to_bits(), b.horizon_end.to_bits());
    assert_eq!(a.applied, b.applied);
    assert_eq!(a.tip.r.x.to_bits(), b.tip.r.x.to_bits());
    assert_eq!(a.tip.v.z.to_bits(), b.tip.v.z.to_bits());
    assert_eq!(a.tip.t.to_bits(), b.tip.t.to_bits());
    assert_eq!(a.plan, b.plan);
}

/// Чужий формат не читається мовчки.
#[test]
fn a_file_that_is_not_a_save_is_refused() {
    assert!(Save::from_text("щось інше\nt 0\n").is_err());
    assert!(Save::from_text("").is_err());
    assert!(Save::from_text("space_sim save v1\n").is_err(), "немає 't'");
}
