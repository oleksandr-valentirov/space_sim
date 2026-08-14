//! Маркери, знайдені скануванням, проти озброєної події (ROADMAP-UI.md, U3a).
//!
//! Оракул тут — **та сама подія, знайдена в ядрі пошуком кореня**: `prop_run`
//! з озброєним `Event::Periapsis` спиняється рівно на перицентрі, і скан
//! мусить показати той самий момент у межах кроку інтерполяції.
//!
//! Порівняння робиться **один раз, у тесті**, і ніколи в грі: озброєна подія
//! змінює послідовність кроків після себе, тож у грі вона змінила б
//! траєкторію заради маркера на екрані (ROADMAP «Фізика й пропагація»).
//!
//! Мутація, яку це ловить: «шукати екстремум у другий бік» дає апоцентри
//! замість перицентрів — і різниця тут не тонка, вона в пів оберту.

use core_rs::{Event, Propagator};
use game::mission;
use game::schedule::{self, Kind};

/// Проганяє місію, доки не набереться `legs` ланок, і повертає снапшот.
fn fly(legs: usize) -> game::snapshot::WorldSnapshot {
    let mut world = mission::world(&mission::default_asset()).expect("світ будується");

    for _ in 0..100_000 {
        world.step(1.0 / 60.0, 4);
        if world.snapshot().vessels[0].legs.len() >= legs {
            break;
        }
    }

    world.snapshot()
}

/// Перший перицентр скану збігається з тим, що дала озброєна подія.
#[test]
fn a_scanned_periapsis_matches_an_armed_one() {
    let snapshot = fly(6);
    let vessel = &snapshot.vessels[0];

    let markers = schedule::scan(&vessel.legs);
    let scanned = markers
        .iter()
        .find(|m| m.kind == Kind::Periapsis)
        .expect("на кількох ланках перицентр мусить бути");

    // Той самий проміжок, але з озброєною подією. Пропагатор власний: цей
    // прогін навмисно **не** той, яким живе світ.
    let eph = std::sync::Arc::new(
        core_rs::Ephemeris::load(&mission::default_asset()).expect("ассет читається"),
    );
    let mut prop = Propagator::new(eph, mission::config()).expect("пропагатор створюється");

    let mut step = 0.0;
    let run = prop
        .run(
            &vessel.start,
            None,
            scanned.t + 3600.0,
            &[Event::Periapsis {
                body: game::world::EARTH,
            }],
            &mut [],
            &mut step,
        )
        .expect("прогін має пройти");

    let armed = run.final_state.t;

    // Крок інтегратора тут — тисячі секунд, а скан уточнює час параболою по
    // трьох семплах. Допуск у хвилину — це чверть кроку, і він про
    // інтерполяцію, а не про запас про всяк випадок.
    assert!(
        (scanned.t - armed).abs() < 60.0,
        "скан дав {:.3}, озброєна подія {:.3} — різниця {:.3} с",
        scanned.t,
        armed,
        scanned.t - armed
    );
}

/// Перицентри й апоцентри чергуються, і перицентр ближчий за апоцентр.
///
/// Це та половина перевірки, яка ловить переплутані боки: тест лише на
/// «перицентр знайдено» пройшов би й на скані, що видає апоцентри під чужим
/// іменем — бо збіг з озброєною подією він перевіряє на одному моменті.
#[test]
fn the_two_kinds_alternate_and_mean_what_they_say() {
    let snapshot = fly(8);
    let markers = schedule::scan(&snapshot.vessels[0].legs);

    assert!(
        markers.len() >= 2,
        "на восьми ланках мало знайтися принаймні два екстремуми"
    );

    for pair in markers.windows(2) {
        assert_ne!(
            pair[0].kind, pair[1].kind,
            "два однакові екстремуми поспіль: {:?} і {:?}",
            pair[0], pair[1]
        );

        let (near, far) = match pair[0].kind {
            Kind::Periapsis => (pair[0], pair[1]),
            Kind::Apoapsis => (pair[1], pair[0]),
        };
        assert!(
            near.distance_m < far.distance_m,
            "перицентр на {:.0} м, апоцентр на {:.0} м",
            near.distance_m,
            far.distance_m
        );
    }
}

/// Ланка, коротша за три семпли, не дає маркерів і не падає.
#[test]
fn a_leg_too_short_to_have_a_middle_says_nothing() {
    use core_rs::{State, Stop, Vec3d};
    use game::leg::{Leg, Sample};

    let sample = Sample {
        state: State {
            t: 0.0,
            r: Vec3d {
                x: 7.0e6,
                y: 0.0,
                z: 0.0,
            },
            v: Vec3d {
                x: 0.0,
                y: 7500.0,
                z: 0.0,
            },
        },
        earth: [0.0; 3],
        moon: [0.0; 3],
    };

    let leg = Leg {
        entry: sample.state,
        t1: 1.0,
        step_out: 1.0,
        samples: vec![sample, sample],
        stop: Stop::BufferFull,
    };

    assert!(schedule::scan_leg(&leg).is_empty());
}
