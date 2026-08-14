//! План перетворюється на послідовність `prop_run` (ROADMAP J3).
//!
//! Три твердження, і кожне про свою помилку:
//!
//!   1. **машинерія нічого не додає від себе** — план, виконаний світом,
//!      бітово дорівнює тим самим викликам `prop_run`, зшитим руками;
//!   2. **правка не торкається минулого** — жоден біт до маневру не зрушив;
//!   3. **перерахунок коштує рівно хвіст** — і це число, а не намір.
//!
//! ## Чого тут немає: оракула Lambert
//!
//! ROADMAP обіцяв зовнішній оракул — дві імпульси з Lambert (G1), які
//! приводять апарат у цільову точку. Він відкладений, і не через складність:
//! `lambert_solve` живе в C і на межу ще не винесений (`core-sys` має рівно
//! шість функцій). Винести його — це окремий крок етапу D з власним
//! C-оракулом і бітовою звіркою, а не рядок у тесті плану.
//!
//! Що від цього втрачено, варто назвати чесно: **фізика імпульсу тут
//! перевіряється не проти зовнішньої задачі.** Але сам імпульс — це
//! `v += Δv`, тобто три додавання; єдине, де в ньому можна помилитися, — це
//! базис фрейму, і його перевіряють юніт-тести `plan.rs` проти явно
//! виписаних векторів. Ризик, який лишається непокритим, — не в арифметиці, а
//! в тому, чи має обраний Δv фізичний сенс; на це й потрібен Lambert, і він
//! приїде разом із флайт-планером M3.

use core_rs::{Ephemeris, PropConfig, Propagator, State};
use game::leg::Sample;
use game::mission;
use game::plan::{Frame, Manoeuvre, Plan};
use game::world::{PlanRejected, VesselId, World, LEG};
use std::sync::Arc;

const DAY: f64 = 86400.0;

/// План із трьох маневрів: уздовж швидкості, поперек площини, назовні.
///
/// Різні фрейми навмисно: якби всі три були інерціальні, тест не сказав би
/// нічого про перетворення базису, а саме воно тут єдине нетривіальне.
fn three_burns(start_t: f64) -> Plan {
    let mut plan = Plan::new();
    plan.insert(Manoeuvre {
        t: start_t + 10.0 * DAY,
        dv: [12.0, 0.0, 0.0],
        frame: Frame::Vnb {
            body: game::world::EARTH,
        },
    });
    plan.insert(Manoeuvre {
        t: start_t + 25.0 * DAY,
        dv: [0.0, -3.5, 0.0],
        frame: Frame::Vnb {
            body: game::world::EARTH,
        },
    });
    plan.insert(Manoeuvre {
        t: start_t + 40.0 * DAY,
        dv: [0.7, 0.0, -1.2],
        frame: Frame::Inertial,
    });
    plan
}

fn world_with(plan: Plan) -> World {
    let mut world = mission::world(&mission::default_asset()).expect("світ будується");
    world
        .commit_plan(VesselId(0), plan)
        .expect("план у майбутньому");
    world.run_to_end(1.0, 8);
    world
}

fn samples(world: &World) -> Vec<Sample> {
    world.vessels()[0]
        .trajectory
        .legs()
        .iter()
        .flat_map(|leg| leg.samples.iter().copied())
        .collect()
}

fn assert_same(a: &[Sample], b: &[Sample], upto: usize, what: &str) {
    for i in 0..upto {
        let (x, y) = (a[i].state, b[i].state);
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

/// Головна перевірка J3: світ робить рівно те, що зроблять руки.
///
/// Оракул — послідовність викликів `prop_run` через `core-rs`, зшита в тесті:
/// пропагувати до моменту запалення, додати Δv, продовжити. Це та сама
/// фізика, але **інша машинерія** — без стору, без снапшотів, без індексу
/// застосованих маневрів і без каскаду. Розбіжність означала б помилку саме в
/// них, і жодна з таких помилок не падає сама: усі дають правдоподібну криву.
#[test]
fn the_world_flies_the_plan_a_hand_stitched_run_would_fly() {
    let start = mission::start();
    let plan = three_burns(start.t);
    let world = world_with(plan.clone());
    let mine = samples(&world);

    // --- оракул, руками ---
    let eph = Arc::new(Ephemeris::load(&mission::default_asset()).expect("ассет"));
    let cfg = PropConfig {
        tol_m: mission::TOL_M,
        h_max_s: mission::H_MAX_S,
        ..PropConfig::default()
    };
    let mut prop = Propagator::new(eph.clone(), cfg).expect("пропагатор");

    let mission_end = start.t + mission::DAYS * DAY;
    let mut state = start;
    let mut step = 0.0;
    let mut buffer = vec![State::default(); LEG];
    let mut theirs: Vec<State> = Vec::new();

    // Межі сегментів: моменти маневрів, тоді кінець місії. Саме це й робить
    // світ, тільки він бере їх з плану по індексу.
    let boundaries: Vec<f64> = plan
        .manoeuvres()
        .iter()
        .map(|m| m.t)
        .chain(std::iter::once(mission_end))
        .collect();

    for (index, boundary) in boundaries.iter().enumerate() {
        loop {
            let run = prop
                .run(&state, None, *boundary, &[], &mut buffer, &mut step)
                .expect("прогін");
            theirs.extend_from_slice(&buffer[..run.filled]);
            state = run.final_state;
            if run.stop == core_rs::Stop::ReachedEnd {
                break;
            }
        }

        // Імпульс — після сегмента, з тим самим фреймом і тим самим кроком.
        if let Some(m) = plan.get(index) {
            let body = m
                .frame_body()
                .map(|id| eph.body_state(id, state.t).expect("тіло"));
            let dv = m.dv_inertial(&state, body.as_ref());
            state.v.x += dv[0];
            state.v.y += dv[1];
            state.v.z += dv[2];
        }
    }

    assert_eq!(
        mine.len(),
        theirs.len(),
        "{} семплів проти {} у зшитого руками",
        mine.len(),
        theirs.len()
    );
    assert!(mine.len() > 1000, "замало семплів, щоб щось доводити");

    for (i, (a, b)) in mine.iter().zip(theirs.iter()).enumerate() {
        for (name, p, q) in [
            ("t", a.state.t, b.t),
            ("r.x", a.state.r.x, b.r.x),
            ("r.y", a.state.r.y, b.r.y),
            ("r.z", a.state.r.z, b.r.z),
            ("v.x", a.state.v.x, b.v.x),
            ("v.y", a.state.v.y, b.v.y),
            ("v.z", a.state.v.z, b.v.z),
        ] {
            assert_eq!(
                p.to_bits(),
                q.to_bits(),
                "семпл {i}, {name}: {p:e} проти {q:e}"
            );
        }
    }
}

/// Той самий план двічі — бітово те саме.
#[test]
fn a_plan_replayed_gives_the_same_bits() {
    let start = mission::start();
    let first = samples(&world_with(three_burns(start.t)));
    let second = samples(&world_with(three_burns(start.t)));

    assert_eq!(first.len(), second.len());
    assert_same(&first, &second, first.len(), "повтор плану");
}

/// Маневр справді щось міняє, і саме там, де сказано.
///
/// Без цієї перевірки всі бітові звірки вище були б однаково правдиві для
/// плану, який ніхто не виконав.
#[test]
fn the_burn_changes_the_trajectory_and_only_after_itself() {
    let start = mission::start();
    let burn_t = start.t + 10.0 * DAY;

    let plain = samples(&world_with(Plan::new()));
    let burned = samples(&world_with(three_burns(start.t)));

    let before: Vec<usize> = (0..plain.len().min(burned.len()))
        .filter(|&i| plain[i].state.t < burn_t)
        .collect();
    assert!(!before.is_empty(), "до маневру мали бути семпли");

    assert_same(
        &plain,
        &burned,
        *before.last().unwrap() + 1,
        "до першого маневру",
    );

    // А після — розходяться, і помітно: 12 м/с на нестійкій орбіті це багато.
    let last = plain.len().min(burned.len()) - 1;
    let miss = distance(plain[last].state, burned[last].state);
    println!("  розбіжність у кінці місії: {miss:e} м");
    assert!(
        miss > 1.0e6,
        "маневр на 12 м/с мав розвести траєкторії далі, ніж на {miss:e} м"
    );
}

/// Правка маневру не зрушує жодного біта до нього — і коштує лише хвіст.
///
/// Це і є каскадний перерахунок з PROJECT.md §6, у двох твердженнях одразу:
/// що саме зберігається й скільки це коштує.
///
/// Курсор навмисно лишається на 20-й добі: правити можна тільки майбутнє, і
/// це не обмеження тесту, а сама конструкція — саме тому історія й
/// недоторканна.
#[test]
fn editing_a_manoeuvre_costs_only_the_tail() {
    let start = mission::start();
    let plan = three_burns(start.t);

    let mut world = mission::world(&mission::default_asset()).expect("світ будується");
    world
        .commit_plan(VesselId(0), plan.clone())
        .expect("план у майбутньому");
    world.run_to_day(start.t + 20.0 * DAY, 1.0, 8);

    let before_edit = samples(&world);
    let legs_before = world.vessels()[0].trajectory.legs().len();
    let horizon_before = world.vessels()[0].trajectory.computed_to();

    // Другий маневр — на 25-й добі — уже в порахованому, а після нього є ще
    // що перераховувати. Третій не годиться: горизонт зупиняється рівно на
    // ньому (він і є межею сегмента), тож правка не викинула б нічого, і
    // вимір вийшов би порожнім.
    let second = *plan.get(1).expect("три маневри");
    assert!(
        horizon_before > second.t,
        "горизонт {horizon_before} не перейшов за маневр {}",
        second.t
    );

    let mut edited = Plan::new();
    for (i, m) in plan.manoeuvres().iter().enumerate() {
        if i == 1 {
            edited.insert(Manoeuvre {
                dv: [0.0, 5.0, 0.0],
                ..second
            });
        } else {
            edited.insert(*m);
        }
    }

    let from = world
        .commit_plan(VesselId(0), edited)
        .expect("правка в майбутньому")
        .expect("план змінився");
    assert_eq!(
        from.to_bits(),
        second.t.to_bits(),
        "перерахунок не з маневру"
    );

    let legs_kept = world.vessels()[0].trajectory.legs().len();
    let cost_before = world.legs_computed();

    // dt = 0: курсор стоїть, працює лише горизонт. Так міряється рівно
    // вартість перерахунку, без домішки нової роботи попереду.
    loop {
        let done = world.step(0.0, 8);
        if done.legs == 0 {
            break;
        }
    }
    let recomputed = world.legs_computed() - cost_before;

    let after_edit = samples(&world);

    let kept_samples = before_edit.iter().take_while(|s| s.state.t <= from).count();
    assert!(
        kept_samples > 500,
        "до другого маневру мало бути багато семплів, а є {kept_samples}"
    );
    assert_same(
        &before_edit,
        &after_edit,
        kept_samples,
        "до правленого маневру",
    );

    let horizon_after = world.vessels()[0].trajectory.computed_to();
    println!(
        "  ланок було {legs_before}, лишилося {legs_kept}, перераховано \
         {recomputed}; збережено семплів {kept_samples}; горизонт {:.1} -> {:.1} доби",
        (horizon_before - start.t) / DAY,
        (horizon_after - start.t) / DAY
    );

    // Головне число кроку: перерахунок торкнувся лише хвоста. Рівності тут
    // вимагати не можна — з іншим Δv межі «буфер заповнився» лягають інакше,
    // тож ланок може вийти на одну більше або менше.
    assert!(
        recomputed as usize <= legs_before - legs_kept + 1,
        "перераховано {recomputed} ланок, а відкинуто лише {}",
        legs_before - legs_kept
    );
    assert!(
        legs_kept > 0 && (recomputed as usize) < legs_before,
        "перерахунок з'їв увесь прогноз: {recomputed} проти {legs_before} ланок"
    );

    // Горизонт відновився — але не обов'язково в ту саму секунду, і це не
    // недогляд: він міряється в ЛАНКАХ, а межі «буфер заповнився» після
    // іншого Δv лягають в інших місцях. Вимагати тієї самої секунди означало б
    // вимагати, щоб інша траєкторія мала ту саму густину кроків.
    assert!(
        horizon_after >= horizon_before,
        "прогноз після перерахунку коротший, ніж був: {horizon_after} проти \
         {horizon_before}"
    );
}

/// Правити минуле не можна, і це відмова, а не тиха згода.
#[test]
fn a_manoeuvre_in_the_past_is_refused() {
    let start = mission::start();
    let mut world = mission::world(&mission::default_asset()).expect("світ будується");

    // Довести курсор до тридцятої доби.
    world.run_to_day(start.t + 30.0 * DAY, 1.0, 8);
    assert!(world.clock().t() >= start.t + 30.0 * DAY);

    let mut past = Plan::new();
    past.insert(Manoeuvre {
        t: start.t + 20.0 * DAY,
        dv: [1.0, 0.0, 0.0],
        frame: Frame::Inertial,
    });

    assert_eq!(
        world.commit_plan(VesselId(0), past),
        Err(PlanRejected::InThePast),
        "маневр на 20-й добі при курсорі на 30-й мав бути відхилений"
    );

    // А в майбутньому — приймається.
    let mut future = Plan::new();
    future.insert(Manoeuvre {
        t: start.t + 60.0 * DAY,
        dv: [1.0, 0.0, 0.0],
        frame: Frame::Inertial,
    });
    assert!(world.commit_plan(VesselId(0), future).is_ok());
}

/// Скільки коштує скинути крок інтегратора на маневрі замість перенести.
///
/// ROADMAP J3 вимагав виміряти обидва числа, а не обрати навмання. Порівняння
/// робиться напряму через `core-rs`, без світу: тут питання не про
/// машинерію, а про поведінку контролера кроку після розриву швидкості.
#[test]
fn carrying_the_step_through_a_burn_against_resetting_it() {
    let start = mission::start();
    let eph = Arc::new(Ephemeris::load(&mission::default_asset()).expect("ассет"));
    let cfg = PropConfig {
        tol_m: mission::TOL_M,
        h_max_s: mission::H_MAX_S,
        ..PropConfig::default()
    };

    let burn_t = start.t + 10.0 * DAY;
    let end_t = burn_t + 5.0 * DAY;

    let fly = |reset: bool| -> (usize, State) {
        let mut prop = Propagator::new(eph.clone(), cfg).expect("пропагатор");
        let mut buffer = vec![State::default(); LEG];
        let mut step = 0.0;
        let mut state = start;
        let mut count = 0;

        for boundary in [burn_t, end_t] {
            loop {
                let run = prop
                    .run(&state, None, boundary, &[], &mut buffer, &mut step)
                    .expect("прогін");
                count += run.filled;
                state = run.final_state;
                if run.stop == core_rs::Stop::ReachedEnd {
                    break;
                }
            }

            if boundary == burn_t {
                // Той самий імпульс в обох випадках; різниця лише в кроці.
                state.v.x += 12.0;
                if reset {
                    step = 0.0;
                }
            }
        }

        (count, state)
    };

    let (carried_steps, carried) = fly(false);
    let (reset_steps, reset) = fly(true);

    let miss = distance(carried, reset);
    println!(
        "  перенесений крок: {carried_steps} кроків; скинутий: {reset_steps}; \
         розбіжність у кінці: {miss:e} м"
    );

    // Обидві траєкторії правильні в межах допуску — питання лише в ціні.
    assert!(
        miss < 1.0e3,
        "два способи вести крок дали різні траєкторії ({miss:e} м) — це вже не \
         про вартість"
    );
    assert!(
        carried_steps <= reset_steps,
        "перенесення кроку виявилось дорожчим: {carried_steps} проти {reset_steps}"
    );
}

/// Δv застосовується рівно в момент маневру, і рівно один раз.
///
/// Ланка мусить закінчитися бітово на часі маневру, а стрибок швидкості між
/// її останнім семплом і `entry` наступної — дорівнювати |Δv| з точністю
/// заокруглення. Подвійне застосування (найімовірніша помилка в обліку
/// індексу) дало б рівно вдвічі більше.
#[test]
fn the_impulse_lands_at_the_instant_and_happens_once() {
    let start = mission::start();
    let burn_t = start.t + 10.0 * DAY;

    let mut plan = Plan::new();
    plan.insert(Manoeuvre {
        t: burn_t,
        dv: [12.0, 0.0, 0.0],
        frame: Frame::Inertial,
    });

    let world = world_with(plan);
    let legs = world.vessels()[0].trajectory.legs();

    let at = legs
        .iter()
        .position(|leg| leg.t1 == burn_t)
        .expect("жодна ланка не закінчилася рівно на маневрі");

    let before = legs[at].samples.last().expect("ланка не порожня").state;
    let after = legs[at + 1].entry;

    assert_eq!(before.t.to_bits(), after.t.to_bits(), "розрив у часі");
    assert_eq!(
        before.r.x.to_bits(),
        after.r.x.to_bits(),
        "позиція стрибнула"
    );

    let jump = (
        after.v.x - before.v.x,
        after.v.y - before.v.y,
        after.v.z - before.v.z,
    );
    let magnitude = (jump.0 * jump.0 + jump.1 * jump.1 + jump.2 * jump.2).sqrt();

    assert!(
        (magnitude - 12.0).abs() < 1e-9,
        "стрибок швидкості {magnitude} м/с замість 12 — маневр застосовано не раз"
    );
    assert!(
        (jump.0 - 12.0).abs() < 1e-9,
        "імпульс пішов не туди: {jump:?}"
    );
}

fn distance(a: State, b: State) -> f64 {
    let d = [a.r.x - b.r.x, a.r.y - b.r.y, a.r.z - b.r.z];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}
