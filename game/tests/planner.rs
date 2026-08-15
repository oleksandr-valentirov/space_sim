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

use game::clock::Stall;
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
    sim.send(Command::SetWarp(game::clock::MAX_WARP));
    wait_until("курсор відійде від старту", || {
        sim.snapshot().t >= mission::start().t + 15.0 * DAY
    });
    sim.send(Command::TogglePause);
    wait_until("пауза дійде", || {
        sim.snapshot().stall == Some(Stall::Paused)
    });

    // Момент маневру береться від того, де курсор СПРАВДІ спинився, а не від
    // круглого числа. Команда долітає не миттєво, і на максимальному warp
    // кожен тік — це майже дві доби; фіксована 30-та доба означала б, що на
    // повільнішій машині курсор устигає її проїхати, і план відхиляється як
    // «у минулому». Саме так цей тест і впав на macOS, пройшовши на Linux.
    let cursor = sim.snapshot().t;
    let burn_t = cursor + 5.0 * DAY;
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
        params: None,
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
    // Відмова тут — це не «ще не прийшло», а провал; чекати на неї до кінця
    // терпіння означало б сховати причину за таймаутом.
    wait_until("відповідь про план", || {
        for event in sim.events() {
            match event {
                Event::PlanCommitted { .. } => return true,
                Event::PlanRejected { why, .. } => panic!("план відхилено: {why:?}"),
                _ => {}
            }
        }
        false
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
            params: None,
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
            params: None,
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

/// Запит, що **перервав** прогін, сам без відповіді не лишається.
///
/// Перевірка вище шле пачку одним махом, і тому нічого не доводить про цей
/// випадок: усі двадцять уже лежать у каналі, коли нитка бере перший, і їх
/// забирає звичайне вичерпування черги. Тут інакше — другий запит прилітає
/// **посеред** прогону першого, тобто його бачить саме перевірка скасування.
/// Вона повідомлення з каналу виймає, і питання рівно одне: куди воно потім
/// дінеться.
///
/// Так виглядає кінець тягнення вузла: гравець відпустив мишу, полетів
/// останній запит, і після нього не буде жодного. Якщо його з'їдає
/// скасування, нитка засинає на `recv()`, а на екрані лишається лінія
/// передостаннього положення — назавжди.
///
/// Пауза тут потрібна саме тесту: прев'ю рахується близько 50 мс (вимір цієї
/// сесії), а перевірка каналу трапляється між ланками, тож п'яти мілісекунд
/// досить, щоб другий запит застав перший у роботі, і мало, щоб той устиг
/// добігти до кінця.
#[test]
fn the_request_that_cancelled_the_work_is_answered_too() {
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

    let ask = |id: u64| {
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
            params: None,
            horizon_end: vessel.horizon_end,
        });
    };

    ask(1);
    std::thread::sleep(Duration::from_millis(5));
    ask(2);

    let mut last = None;
    wait_until("прев'ю на другий запит", || {
        if let Some(preview) = planner.latest() {
            last = Some(preview);
        }
        last.as_ref().is_some_and(|p| p.id == 2)
    });

    assert_eq!(last.expect("щойно перевірили").id, 2);
}
