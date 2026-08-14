//! Нитка нічого не змінила, крім того, хто рахує (ROADMAP J4).
//!
//! Найважливіше тут — те, чого перевіряти **не** довелося. Гонки даних немає
//! не тому, що ми її шукали, а тому, що спільного мутабельного стану немає
//! взагалі: світ належить нитці, назовні йдуть незмінні снапшоти, всередину —
//! команди каналом (PROJECT.md §6). Перевіряти лишається дві речі: що числа
//! ті самі й що читач не блокується.
//!
//! Числа мусять збігтися **бітово** з однонитковим прогоном J3, і це не
//! самоочевидно: нитка міряє власний `dt`, крутиться зі своїм тіком і робить
//! іншу кількість роботи за прохід. Усе це J2 уже оголосив безпечним; тут
//! воно перевіряється в тій формі, у якій справді працюватиме.

use std::time::{Duration, Instant};

use game::clock::{Stall, MAX_WARP};
use game::mission;
use game::plan::{Frame, Manoeuvre, Plan};
use game::sim::{Command, Event, Sim};
use game::snapshot::WorldSnapshot;
use game::world::{PlanRejected, VesselId};

const DAY: f64 = 86400.0;

/// Скільки чекати на нитку, перш ніж визнати тест зламаним.
///
/// Місія на максимальному warp — це близько секунди; десять означає «щось
/// стало», а не «машина повільна».
const PATIENCE: Duration = Duration::from_secs(10);

fn spawn(demo_plan: bool) -> Sim {
    Sim::spawn(mission::default_asset(), demo_plan).expect("нитка піднімається")
}

/// Крутить нитку на максимальному warp, доки місія не скінчиться.
fn run_to_end(sim: &Sim) -> std::sync::Arc<WorldSnapshot> {
    sim.send(Command::SetWarp(MAX_WARP));

    let deadline = Instant::now() + PATIENCE;
    loop {
        let snapshot = sim.snapshot();
        if snapshot.stall == Some(Stall::MissionEnd) {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "нитка не довела місію до кінця за {PATIENCE:?}: доба {:.2}, stall {:?}",
            (snapshot.t - mission::start().t) / DAY,
            snapshot.stall
        );
        std::thread::yield_now();
    }
}

fn samples_of(snapshot: &WorldSnapshot) -> Vec<core_rs::State> {
    snapshot
        .vessels
        .iter()
        .flat_map(|v| v.legs.iter().flat_map(|leg| leg.samples.iter()))
        .map(|s| s.state)
        .collect()
}

/// Головна перевірка J4: та сама траєкторія, до останнього біта.
#[test]
fn the_thread_computes_what_one_thread_computes() {
    let threaded = samples_of(&run_to_end(&spawn(true)));

    // Оракул — той самий світ, порахований на цій нитці, без каналів і
    // публікацій.
    let mut world = mission::world_with_demo_plan(&mission::default_asset()).expect("світ");
    world.run_to_end(1.0, 8);
    let plain: Vec<core_rs::State> = world.vessels()[0]
        .trajectory
        .legs()
        .iter()
        .flat_map(|leg| leg.samples.iter())
        .map(|s| s.state)
        .collect();

    assert_eq!(
        threaded.len(),
        plain.len(),
        "{} семплів з нитки проти {} однониткових",
        threaded.len(),
        plain.len()
    );
    assert!(threaded.len() > 1000, "замало семплів, щоб щось доводити");

    for (i, (a, b)) in threaded.iter().zip(plain.iter()).enumerate() {
        for (name, p, q) in [
            ("t", a.t, b.t),
            ("r.x", a.r.x, b.r.x),
            ("r.y", a.r.y, b.r.y),
            ("r.z", a.r.z, b.r.z),
            ("v.x", a.v.x, b.v.x),
            ("v.y", a.v.y, b.v.y),
            ("v.z", a.v.z, b.v.z),
        ] {
            assert_eq!(
                p.to_bits(),
                q.to_bits(),
                "семпл {i}, {name}: {p:e} проти {q:e}"
            );
        }
    }
}

/// Читач ніколи не чекає на письменника.
///
/// Це і є вся причина, з якої снапшоти — `arc-swap`, а не `Mutex`: під
/// мьютексом кадр чекав би на тік симуляції, і 60 fps трималися б рівно доти,
/// доки нитка не візьметься за довгу ланку.
///
/// Вимір робиться при насиченій нитці (максимальний warp), тобто в тих
/// умовах, у яких мьютекс і почав би блокувати.
#[test]
fn reading_a_snapshot_never_waits_for_the_writer() {
    let sim = spawn(false);
    sim.send(Command::SetWarp(MAX_WARP));

    let mut worst = Duration::ZERO;
    let mut reads = 0u32;

    let until = Instant::now() + Duration::from_millis(300);
    while Instant::now() < until {
        let at = Instant::now();
        let snapshot = sim.snapshot();
        worst = worst.max(at.elapsed());
        reads += 1;
        // Снапшот справді читається, а не оптимізується геть.
        assert!(snapshot.t.is_finite());
    }

    println!("  {reads} читань, найгірше {worst:?}");

    // Поріг навмисно щедрий: на завантаженому CI-раннері планувальник може
    // відібрати нитку будь-коли, і тест має ловити блокування, а не
    // планувальник. Реально це десятки наносекунд.
    assert!(
        worst < Duration::from_millis(50),
        "найдовше читання снапшоту тривало {worst:?} — читач на когось чекає"
    );
    assert!(reads > 1000, "читань замало, щоб вимір щось означав");
}

/// На команду приходить подія, і саме та.
///
/// Канал назад існує заради дискретного: снапшот не сказав би, що план
/// відхилено, — він показав би просто нічого не змінилося.
#[test]
fn a_command_is_answered_by_an_event() {
    let sim = spawn(false);
    sim.send(Command::TogglePause);

    let start = mission::start();
    let mut future = Plan::new();
    future.insert(Manoeuvre {
        t: start.t + 60.0 * DAY,
        dv: [1.0, 0.0, 0.0],
        frame: Frame::Inertial,
    });
    sim.send(Command::CommitPlan {
        vessel: VesselId(0),
        plan: future,
    });

    // Маневр у момент старту — це минуле: курсор стоїть саме там.
    let mut past = Plan::new();
    past.insert(Manoeuvre {
        t: start.t,
        dv: [1.0, 0.0, 0.0],
        frame: Frame::Inertial,
    });
    sim.send(Command::CommitPlan {
        vessel: VesselId(0),
        plan: past,
    });

    let mut seen = Vec::new();
    let deadline = Instant::now() + PATIENCE;
    while seen.len() < 2 {
        seen.extend(sim.events());
        assert!(
            Instant::now() < deadline,
            "нитка не відповіла за {PATIENCE:?}: {seen:?}"
        );
        std::thread::yield_now();
    }

    assert!(
        matches!(
            seen[0],
            Event::PlanCommitted {
                vessel: VesselId(0),
                from: Some(_)
            }
        ),
        "перша відповідь мала бути про прийнятий план: {:?}",
        seen[0]
    );
    assert_eq!(
        seen[1],
        Event::PlanRejected {
            vessel: VesselId(0),
            why: PlanRejected::InThePast
        },
        "маневр у момент курсора мав бути відхилений"
    );
}

/// Пауза, надіслана каналом, доходить і зупиняє курсор.
#[test]
fn pause_reaches_the_thread_and_stops_the_cursor() {
    let sim = spawn(false);
    sim.send(Command::SetWarp(MAX_WARP));

    // Дати часу зрушити, щоб «стоїть» не означало «ще не почав».
    let deadline = Instant::now() + PATIENCE;
    while sim.snapshot().t <= mission::start().t {
        assert!(Instant::now() < deadline, "курсор так і не зрушив");
        std::thread::yield_now();
    }

    sim.send(Command::TogglePause);

    // Дочекатися, доки пауза долетить.
    let deadline = Instant::now() + PATIENCE;
    while sim.snapshot().stall != Some(Stall::Paused) {
        assert!(Instant::now() < deadline, "пауза не дійшла");
        std::thread::yield_now();
    }

    let stopped = sim.snapshot().t;
    std::thread::sleep(Duration::from_millis(50));
    let still = sim.snapshot();

    assert_eq!(
        still.t.to_bits(),
        stopped.to_bits(),
        "курсор рухався на паузі: {} -> {}",
        stopped,
        still.t
    );
    assert_eq!(still.stall, Some(Stall::Paused));
}

/// Нитка спиняється разом із ручкою.
///
/// Без цього процес із закритим вікном лишався б із живою ниткою, яка й далі
/// рахує; помітно це стало б лише в диспетчері задач.
#[test]
fn dropping_the_handle_stops_the_thread() {
    let sim = spawn(false);
    sim.send(Command::SetWarp(MAX_WARP));

    let deadline = Instant::now() + PATIENCE;
    while sim.snapshot().t <= mission::start().t {
        assert!(Instant::now() < deadline, "курсор так і не зрушив");
        std::thread::yield_now();
    }

    // `Drop` шле Shutdown і чекає на нитку. Якби вона не виходила, тест не
    // впав би — він завис би, і саме тому тут стеля терпіння всього тесту.
    let at = Instant::now();
    drop(sim);
    let took = at.elapsed();

    println!("  нитка спинилася за {took:?}");
    assert!(
        took < Duration::from_secs(1),
        "нитка виходила {took:?} — Shutdown її не будить"
    );
}
