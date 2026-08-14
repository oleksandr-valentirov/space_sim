//! Лінія, яку ви бачили, і є лінія, якою полетите (ROADMAP J5).
//!
//! Прев'ю рахує інша нитка, з іншим пропагатором, у власному викидному світі.
//! Обіцянка при цьому — не «схоже», а **бітово те саме**, що потім порахує
//! `Sim`. Без неї планувальник маневрів безглуздий: гравець обирав би за
//! однією кривою, а летів іншою (PROJECT.md §8, «флайт-планер»).
//!
//! Найлегший спосіб цю обіцянку зламати — почати прогін не звідти: не з межі
//! ланки, або з «обери крок сам». H1 виміряв, що це інша траєкторія, а не
//! просто повільніша.

use std::sync::Arc;
use std::time::{Duration, Instant};

use game::leg::{restart_at, Leg};
use game::mission;
use game::plan::{Frame, Manoeuvre, Plan};
use game::planner::{Planner, Preview, Request};
use game::sim::{Command, Event, Sim};
use game::world::VesselId;

const DAY: f64 = 86400.0;
const PATIENCE: Duration = Duration::from_secs(10);

fn burn_at(t: f64) -> Plan {
    let mut plan = Plan::new();
    plan.insert(Manoeuvre {
        t,
        dv: [-8.0, 0.0, 0.0],
        frame: Frame::Vnb {
            body: game::world::EARTH,
        },
    });
    plan
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !done() {
        assert!(Instant::now() < deadline, "не дочекалися: {what}");
        std::thread::yield_now();
    }
}

/// Головна перевірка J5: прев'ю з іншої нитки — це майбутній політ, бітово.
#[test]
fn a_preview_is_bit_identical_to_the_flight_that_follows() {
    let sim = Sim::spawn(mission::world(&mission::default_asset()).expect("світ"))
        .expect("нитка симуляції");

    // Спершу дати курсору відійти від старту, тоді спинити. Пауза потрібна
    // тесту, а не конструкції: інакше точка перезапуску встигла б утекти в
    // минуле між снапшотом і комітом. А відійти треба тому, що горизонт
    // тримається за курсором — біля старту після нього просто не лишилося б
    // ланок, які можна звірити.
    let cursor_target = mission::start().t + 20.0 * DAY;
    sim.send(Command::SetWarp(game::clock::MAX_WARP));
    wait_until("курсор дійде до 20-ї доби", || {
        sim.snapshot().t >= cursor_target
    });
    sim.send(Command::TogglePause);

    let burn_t = mission::start().t + 30.0 * DAY;
    wait_until("горизонт дійде до маневру", || {
        sim.snapshot().vessels[0].computed_to > burn_t
    });

    let snapshot = sim.snapshot();
    let vessel = &snapshot.vessels[0];

    // Точка перезапуску — та сама функція, якою скористається `Sim`, коли
    // прийме план. У цьому й сенс: одна функція, а не два однакові правила.
    let restart = restart_at(&vessel.legs, vessel.start, burn_t);
    assert!(
        restart.step > 0.0,
        "перезапуск із «обери крок сам» — прев'ю почалося б не з того місця"
    );

    let plan = burn_at(burn_t);
    let planner = Planner::spawn(sim.ephemeris(), mission::config()).expect("нитка планувальника");
    planner.request(Request {
        id: 1,
        vessel: VesselId(0),
        from: restart.state,
        step: restart.step,
        plan: plan.clone(),
        horizon_end: vessel.horizon_end,
    });

    let mut preview: Option<Preview> = None;
    wait_until("прев'ю", || {
        preview = planner.latest();
        preview.is_some()
    });
    let preview = preview.expect("щойно перевірили");
    assert_eq!(preview.id, 1);
    assert!(
        preview.legs.len() >= 2,
        "прев'ю з {} ланок — замало, щоб щось звіряти",
        preview.legs.len()
    );

    // А тепер той самий план — по-справжньому.
    sim.send(Command::CommitPlan {
        vessel: VesselId(0),
        plan,
    });
    wait_until("відповідь про план", || {
        sim.events()
            .iter()
            .any(|e| matches!(e, Event::PlanCommitted { .. }))
    });

    // Скільки ланок політ порахує після точки перезапуску, вирішує горизонт,
    // а той тримається за КУРСОРОМ, який стоїть на паузі. Прев'ю ж рахує свої
    // чотири ланки від самої точки перезапуску, тобто заглядає далі. Звіряємо
    // перекриття — його достатньо, і воно чесне: розбіжність показалася б уже
    // на першій ланці.
    let flown_after = |snapshot: &game::snapshot::WorldSnapshot| -> Vec<Arc<Leg>> {
        snapshot.vessels[0]
            .legs
            .iter()
            .filter(|leg| leg.entry.t >= restart.state.t)
            .cloned()
            .collect()
    };

    // Чекати на кількість ланок недостатньо, і це варте окремого слова:
    // до коміту їх уже було досить, тож перевірка проходила б на СТАРІЙ
    // траєкторії. Ознака, що перерахунок таки стався, одна — ланка, що
    // закінчується рівно на маневрі; до коміту такої не було й бути не могло.
    wait_until("політ перерахує хвіст", || {
        let snapshot = sim.snapshot();
        snapshot.vessels[0].legs.iter().any(|leg| leg.t1 == burn_t)
            && flown_after(&snapshot).len() >= 2
    });

    let after = sim.snapshot();
    let flown = flown_after(&after);

    println!(
        "  перезапуск на добі {:.3}, маневр на {:.3}",
        (restart.state.t - mission::start().t) / DAY,
        (burn_t - mission::start().t) / DAY
    );

    let overlap = preview.legs.len().min(flown.len());
    assert!(
        overlap >= 2,
        "звіряти нема чого: прев'ю {} ланок, політ {}",
        preview.legs.len(),
        flown.len()
    );

    for (i, (shown, flew)) in preview
        .legs
        .iter()
        .zip(flown.iter())
        .take(overlap)
        .enumerate()
    {
        assert_eq!(
            shown.samples.len(),
            flew.samples.len(),
            "ланка {i}: {} семплів у прев'ю проти {} у польоті",
            shown.samples.len(),
            flew.samples.len()
        );
        assert_eq!(
            shown.step_out.to_bits(),
            flew.step_out.to_bits(),
            "ланка {i}: різний крок на виході"
        );

        for (j, (a, b)) in shown.samples.iter().zip(flew.samples.iter()).enumerate() {
            for (name, p, q) in [
                ("t", a.state.t, b.state.t),
                ("r.x", a.state.r.x, b.state.r.x),
                ("r.y", a.state.r.y, b.state.r.y),
                ("r.z", a.state.r.z, b.state.r.z),
                ("v.x", a.state.v.x, b.state.v.x),
                ("v.y", a.state.v.y, b.state.v.y),
                ("v.z", a.state.v.z, b.state.v.z),
            ] {
                assert_eq!(
                    p.to_bits(),
                    q.to_bits(),
                    "ланка {i}, семпл {j}, {name}: {p:e} проти {q:e}"
                );
            }
        }
    }

    println!(
        "  звірено {overlap} ланок, {} семплів",
        preview.legs[..overlap]
            .iter()
            .map(|l| l.samples.len())
            .sum::<usize>()
    );
}

/// Прев'ю починається з межі ланки, а не «де апарат зараз».
///
/// Ця перевірка є окремо, бо саме тут обіцянка ламається найтихіше: прогін із
/// довільної точки дає правдоподібну криву, яка просто не та.
#[test]
fn starting_a_preview_from_the_wrong_step_gives_a_different_line() {
    let sim = Sim::spawn(mission::world(&mission::default_asset()).expect("світ"))
        .expect("нитка симуляції");
    sim.send(Command::TogglePause);

    let burn_t = mission::start().t + 30.0 * DAY;
    wait_until("горизонт", || {
        sim.snapshot().vessels[0].computed_to > burn_t
    });

    let snapshot = sim.snapshot();
    let vessel = &snapshot.vessels[0];
    let restart = restart_at(&vessel.legs, vessel.start, burn_t);
    let plan = burn_at(burn_t);

    let planner = Planner::spawn(sim.ephemeris(), mission::config()).expect("планувальник");

    let ask = |id: u64, step: f64| -> Preview {
        planner.request(Request {
            id,
            vessel: VesselId(0),
            from: restart.state,
            step,
            plan: plan.clone(),
            horizon_end: vessel.horizon_end,
        });
        let mut got = None;
        wait_until("прев'ю", || {
            got = planner.latest();
            got.as_ref().is_some_and(|p| p.id == id)
        });
        got.expect("щойно перевірили")
    };

    let right = ask(1, restart.step);
    let wrong = ask(2, 0.0);

    let count = |p: &Preview| p.legs.iter().map(|l| l.samples.len()).sum::<usize>();
    println!(
        "  з перенесеним кроком: {} семплів; з «обери сам»: {}",
        count(&right),
        count(&wrong)
    );

    assert_ne!(
        count(&right),
        count(&wrong),
        "прогін з «обери крок сам» дав рівно те саме — тоді перенесення кроку \
         нічого не значить, і H1 виміряв щось інше"
    );
}

/// Застарілі прев'ю не доходять до викликача.
///
/// Гравець тягне вузол — запити летять десятками за секунду. Актуальний
/// завжди останній, і саме він мусить дійти; решта нікому не потрібні.
#[test]
fn only_the_latest_request_is_answered() {
    let sim = Sim::spawn(mission::world(&mission::default_asset()).expect("світ"))
        .expect("нитка симуляції");
    sim.send(Command::TogglePause);

    let burn_t = mission::start().t + 30.0 * DAY;
    wait_until("горизонт", || {
        sim.snapshot().vessels[0].computed_to > burn_t
    });

    let snapshot = sim.snapshot();
    let vessel = &snapshot.vessels[0];
    let restart = restart_at(&vessel.legs, vessel.start, burn_t);

    let planner = Planner::spawn(sim.ephemeris(), mission::config()).expect("планувальник");

    // Двадцять запитів поспіль, як від тягнення миші: кожен зі своїм Δv.
    for id in 1..=20u64 {
        let mut plan = Plan::new();
        plan.insert(Manoeuvre {
            t: burn_t,
            dv: [-(id as f64), 0.0, 0.0],
            frame: Frame::Inertial,
        });
        planner.request(Request {
            id,
            vessel: VesselId(0),
            from: restart.state,
            step: restart.step,
            plan,
            horizon_end: vessel.horizon_end,
        });
    }

    let mut last = None;
    wait_until("останнє прев'ю", || {
        if let Some(preview) = planner.latest() {
            last = Some(preview);
        }
        last.as_ref().is_some_and(|p| p.id == 20)
    });

    let last = last.expect("щойно перевірили");
    assert_eq!(last.id, 20, "дійшло не останнє прев'ю");
    assert!(!last.legs.is_empty(), "останнє прев'ю порожнє");
}
