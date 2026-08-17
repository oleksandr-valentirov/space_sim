//! Markers found by scanning, against an armed event (ROADMAP-UI.md, U3a).
//!
//! The oracle here is **the same event found in the core by root finding**:
//! `prop_run` with `Event::Periapsis` armed stops exactly at periapsis, and
//! the scan must show the same instant to within an interpolation step.
//!
//! The comparison is made **once, in the test**, and never in the game: an
//! armed event changes the sequence of steps after it, so in the game it
//! would change the trajectory for the sake of a marker on screen (ROADMAP,
//! "Фізика й пропагація").
//!
//! The mutation this catches: "look for the extremum the other way" gives
//! apoapses instead of periapses -- and the difference is not subtle, it is
//! half a revolution.

use core_rs::{Event, Propagator};
use game::mission;
use game::schedule::{self, Kind};

/// Runs the mission until `legs` legs have accumulated, and returns the
/// snapshot. The run has leg retirement off (N3a).
///
/// The scan looks for periapsis by fitting a parabola through **three
/// neighbouring samples**, while retirement thins old legs down to 8e5 m,
/// i.e. to chords hundreds of seconds long. Marker accuracy in the past does
/// fall from that (measured: 395 s against a tolerance of 60), and that is a
/// consequence of retirement, not a scan bug. What is checked here is the
/// scan itself, so retirement is off; the consequence for markers is written
/// down in ROADMAP, N3a.
fn fly(legs: usize) -> game::snapshot::WorldSnapshot {
    let mut world = mission::world(&mission::default_asset()).expect("the world builds");
    world.set_history_trimming(None);

    for _ in 0..100_000 {
        world.step(1.0 / 60.0, 4);
        if world.snapshot().vessels[0].legs.len() >= legs {
            break;
        }
    }

    world.snapshot()
}

/// The first periapsis of the scan matches the one the armed event gave.
#[test]
fn a_scanned_periapsis_matches_an_armed_one() {
    let snapshot = fly(6);
    let vessel = &snapshot.vessels[0];

    let markers = schedule::scan(&vessel.legs);
    let scanned = markers
        .iter()
        .find(|m| m.kind == Kind::Periapsis)
        .expect("across several legs there must be a periapsis");

    // The same span, but with the event armed. The propagator is its own: this
    // run is deliberately **not** the one the world lives on.
    let eph = std::sync::Arc::new(
        core_rs::Ephemeris::load(&mission::default_asset()).expect("the asset is read"),
    );
    let mut prop = Propagator::new(eph, mission::config()).expect("the propagator is created");

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
        .expect("the run should go through");

    let armed = run.final_state.t;

    // The integrator step here is thousands of seconds, and the scan refines
    // the time with a parabola through three samples. A tolerance of a minute
    // is a quarter of a step, and it is about interpolation rather than slack
    // just in case.
    assert!(
        (scanned.t - armed).abs() < 60.0,
        "the scan gave {:.3}, the armed event {:.3} -- a difference of {:.3} s",
        scanned.t,
        armed,
        scanned.t - armed
    );
}

/// Periapses and apoapses alternate, and the periapsis is the nearer one.
///
/// This is the half of the check that catches swapped sides: a test for
/// "periapsis found" would pass for a scan that reports apoapses under
/// another name, since it checks agreement with the armed event at one
/// instant only.
#[test]
fn the_two_kinds_alternate_and_mean_what_they_say() {
    let snapshot = fly(8);
    let markers = schedule::scan(&snapshot.vessels[0].legs);

    assert!(
        markers.len() >= 2,
        "across eight legs at least two extrema should have been found"
    );

    for pair in markers.windows(2) {
        assert_ne!(
            pair[0].kind, pair[1].kind,
            "two identical extrema in a row: {:?} and {:?}",
            pair[0], pair[1]
        );

        let (near, far) = match pair[0].kind {
            Kind::Periapsis => (pair[0], pair[1]),
            Kind::Apoapsis => (pair[1], pair[0]),
        };
        assert!(
            near.distance_m < far.distance_m,
            "periapsis at {:.0} m, apoapsis at {:.0} m",
            near.distance_m,
            far.distance_m
        );
    }
}

/// A leg too short to have three samples yields no markers and does not panic.
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

// ---------------------------------------------------------------------------
// Seeking to an event (U3b)

/// Seeking moves the cursor; it is not a second run.
///
/// The step's oracle verbatim: after `SeekTo` the number of computed legs has
/// **not** grown, and the trajectory is bitwise the same. Had seeking
/// integrated, both numbers would move -- which is why both are checked, not
/// just "the cursor jumped".
#[test]
fn seeking_moves_the_cursor_and_computes_nothing() {
    let mut world = mission::world(&mission::default_asset()).expect("the world builds");

    // Compute some forecast without moving the cursor far: the default warp is
    // large, so the frames taken are tiny.
    for _ in 0..200 {
        world.step(1.0 / 6000.0, 4);
    }

    let before = world.snapshot();
    let legs_before = world.legs_computed();
    assert!(
        legs_before > 0,
        "nothing was computed -- then \"did not grow\" means nothing either"
    );
    let markers = schedule::scan(&before.vessels[0].legs);

    let target = markers
        .iter()
        .find(|m| m.t > before.t)
        .expect("there must be an event ahead of the cursor")
        .t;

    world
        .seek_to(target)
        .expect("the event lies inside the computed span");

    let after = world.snapshot();
    assert_eq!(
        world.legs_computed(),
        legs_before,
        "seeking computed legs -- that is, it integrated"
    );
    assert_eq!(after.t, target, "the cursor did not land on the event");

    // Bitwise equality of the trajectory: the same legs, the same samples.
    let samples = |snapshot: &game::snapshot::WorldSnapshot| -> Vec<game::leg::Sample> {
        snapshot.vessels[0]
            .legs
            .iter()
            .flat_map(|leg| leg.samples.iter().copied())
            .collect()
    };
    let (a, b) = (samples(&before), samples(&after));
    assert_eq!(a.len(), b.len(), "the sample count changed");
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        for (name, p, q) in [
            ("t", x.state.t, y.state.t),
            ("r.x", x.state.r.x, y.state.r.x),
            ("v.x", x.state.v.x, y.state.v.x),
        ] {
            assert_eq!(
                p.to_bits(),
                q.to_bits(),
                "sample {i}, {name}: {p:e} against {q:e}"
            );
        }
    }
}

/// The cursor never goes back, and the refusal is visible.
#[test]
fn seeking_backwards_is_refused_out_loud() {
    use game::world::SeekRejected;

    let mut world = mission::world(&mission::default_asset()).expect("the world builds");
    for _ in 0..200 {
        world.step(1.0 / 6000.0, 4);
    }

    let now = world.snapshot().t;
    assert!(
        now > 0.0,
        "the cursor should have moved, or the check is empty"
    );

    assert_eq!(world.seek_to(now - 1.0), Err(SeekRejected::Backwards));
    assert_eq!(
        world.snapshot().t,
        now,
        "the refusal moved the cursor anyway"
    );
}

/// And forward, no further than what is computed: seeking has no right to
/// turn into a `t_end` (CLAUDE.md, invariant 9).
#[test]
fn seeking_past_the_forecast_is_refused() {
    let mut world = mission::world(&mission::default_asset()).expect("the world builds");
    for _ in 0..200 {
        world.step(1.0 / 6000.0, 4);
    }

    let snapshot = world.snapshot();
    let beyond = snapshot.vessels[0].computed_to + 1.0;
    let legs_before = world.legs_computed();

    assert!(world.seek_to(beyond).is_err());
    assert_eq!(world.snapshot().t, snapshot.t);
    assert_eq!(
        world.legs_computed(),
        legs_before,
        "a refusal must compute nothing"
    );
}
