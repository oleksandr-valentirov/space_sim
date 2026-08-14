//! Ігрові типи не змінюють жодного біта (ROADMAP J1).
//!
//! Це вся перевірка кроку. Між `prop_run` і тим, що бачить гравець, тепер
//! стоїть шар: ланки, стор, снапшот, сцена. Кожен із них міг би тихо щось
//! зіпсувати — переплутати порядок семплів, загубити останній, продовжити не
//! з того стану, — і жодна з цих помилок не падає: усі дають правдоподібну
//! криву.
//!
//! Тому оракул — прогін H5 (`engine::live`), і звірка бітова. Ланка там 64
//! семпли, тут 256, і це навмисно: якби числа збігалися, перевірка була б
//! тавтологією. Те, що вони різні, а результат бітово той самий, і є
//! твердженням «робота міряється ланками, а ланка на числа не впливає»
//! (CLAUDE.md, інваріант 9; виміряно в H1).

use engine::live;
use game::mission;
use game::snapshot::WorldSnapshot;
use game::world::World;

/// Усі семпли всіх апаратів підряд.
fn samples(snapshot: &WorldSnapshot) -> Vec<game::leg::Sample> {
    snapshot
        .vessels
        .iter()
        .flat_map(|v| v.legs.iter().flat_map(|leg| leg.samples.iter().copied()))
        .collect()
}

fn finished_world() -> World {
    let mut world = mission::world(&mission::default_asset()).expect("світ будується");
    world.run_to_horizon(8);
    world
}

/// Головна перевірка J1: те саме, що H5, до останнього біта.
#[test]
fn the_game_computes_what_the_direct_run_computes() {
    let world = finished_world();
    let snapshot = world.snapshot();
    let mine = samples(&snapshot);

    let reference =
        live::propagate(&mission::start(), mission::DAYS, &live::repo_asset()).expect("прогін H5");

    assert_eq!(
        mine.len(),
        reference.samples.len(),
        "{} семплів проти {} у прямого прогону",
        mine.len(),
        reference.samples.len()
    );
    assert!(
        snapshot.vessels[0].legs.len() != reference.legs,
        "ланки мають бути різного розміру, інакше звірка нічого не доводить: \
         {} проти {}",
        snapshot.vessels[0].legs.len(),
        reference.legs
    );

    for (i, (mine, theirs)) in mine.iter().zip(reference.samples.iter()).enumerate() {
        let pairs = [
            ("t", mine.state.t, theirs.t),
            ("r.x", mine.state.r.x, theirs.vessel[0]),
            ("r.y", mine.state.r.y, theirs.vessel[1]),
            ("r.z", mine.state.r.z, theirs.vessel[2]),
            ("v.x", mine.state.v.x, theirs.velocity[0]),
            ("v.y", mine.state.v.y, theirs.velocity[1]),
            ("v.z", mine.state.v.z, theirs.velocity[2]),
            ("earth.x", mine.earth[0], theirs.earth[0]),
            ("moon.x", mine.moon[0], theirs.moon[0]),
        ];
        for (name, a, b) in pairs {
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "семпл {i}, {name}: {a:e} проти {b:e}"
            );
        }
    }
}

/// Скільки роботи за тік — не впливає на числа взагалі.
///
/// Це J1-версія головної перевірки J2. Там роль бюджету гратиме частота
/// кадрів; тут вона вже грає ту саму роль, тільки без годинника, і саме тому
/// цю перевірку варто мати до того, як час з'явиться: якщо вона впаде тоді,
/// підозрюваних буде двоє.
#[test]
fn the_size_of_a_tick_does_not_change_the_numbers() {
    let run = |budget: usize| {
        let mut world = mission::world(&mission::default_asset()).expect("світ будується");
        let ticks = world.run_to_horizon(budget);
        (ticks, samples(&world.snapshot()))
    };

    let (slow_ticks, slow) = run(1);
    let (fast_ticks, fast) = run(1000);

    assert!(
        slow_ticks > fast_ticks,
        "по одній ланці за тік мало вийти більше тіків: {slow_ticks} проти {fast_ticks}"
    );
    assert_eq!(slow.len(), fast.len(), "різна кількість семплів");

    for (i, (a, b)) in slow.iter().zip(fast.iter()).enumerate() {
        assert_eq!(a.state.t.to_bits(), b.state.t.to_bits(), "семпл {i}: час");
        assert_eq!(a.state.r.x.to_bits(), b.state.r.x.to_bits(), "семпл {i}: x");
        assert_eq!(
            a.state.v.z.to_bits(),
            b.state.v.z.to_bits(),
            "семпл {i}: vz"
        );
    }
}

/// Ланки зшиваються без повторених і без загублених вершин.
///
/// `prop_run` не семплює початкову точку, тож кінець однієї ланки й початок
/// наступної — сусідні кроки, а не той самий. Помилка тут дала б ламану з
/// подвоєними вершинами або з дірками, і жодна з них не видна оком.
#[test]
fn legs_stitch_without_seams() {
    let world = finished_world();
    let vessel = &world.vessels()[0];
    let legs = vessel.trajectory.legs();

    assert!(legs.len() > 1, "потрібно щонайменше дві ланки");

    for pair in legs.windows(2) {
        let (before, after) = (&pair[0], &pair[1]);

        assert_eq!(
            before.t1.to_bits(),
            after.t0.to_bits(),
            "між ланками розрив: {} проти {}",
            before.t1,
            after.t0
        );

        let last = before.samples.last().expect("ланка не порожня");
        let first = after.samples.first().expect("ланка не порожня");
        assert!(
            first.state.t > last.state.t,
            "перший семпл наступної ланки не пізніший за останній попередньої: \
             {} проти {}",
            first.state.t,
            last.state.t
        );
    }

    // Останній семпл — це кінець місії, а не «десь близько».
    let last = legs.last().unwrap().samples.last().unwrap();
    assert_eq!(
        last.state.t.to_bits(),
        vessel.horizon_end.to_bits(),
        "місія скінчилася на {} замість {}",
        last.state.t,
        vessel.horizon_end
    );
}

/// Крок, з яким ланка закінчилася, — не нуль і переноситься далі.
///
/// Без нього перезапуск із межі ланки дав би іншу траєкторію (H1: ×70 роботи
/// й 1.9 мм розбіжності), а на ньому стоїть увесь каскадний перерахунок J3.
#[test]
fn every_leg_carries_the_step_it_ended_with() {
    let world = finished_world();
    let legs = world.vessels()[0].trajectory.legs();

    for (i, leg) in legs.iter().enumerate() {
        assert!(
            leg.step_out > 0.0 && leg.step_out.is_finite(),
            "ланка {i} лишила крок {}",
            leg.step_out
        );
        assert!(
            leg.step_out <= mission::H_MAX_S,
            "ланка {i} лишила крок {} понад стелю {}",
            leg.step_out,
            mission::H_MAX_S
        );
    }
}
