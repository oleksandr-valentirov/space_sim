//! What the game does with the past: the window in revolutions (N5a) and leg
//! retirement (N3a).
//!
//! Retirement discards samples for good (invariant 5), so what to check is
//! not "did it get smaller" -- that is plain to see -- but **what exactly
//! survived**. Three things per leg hold up the save (`leg::restart_at`):
//! `entry`, the last sample and `step_out`. Lose one and the game loads into
//! a different trajectory, and no count-based test would notice.

use game::mission;
use game::world::{World, RAW_LEGS_BEHIND};

const DAYS: f64 = 60.0;

fn flown(retire: bool) -> World {
    let mut world = mission::world(&mission::default_asset()).expect("the world builds");
    world.set_history_trimming(if retire { Some(RAW_LEGS_BEHIND) } else { None });
    world.run_to_day(mission::start().t + DAYS * 86400.0, 1.0, 8);
    world
}

/// The save does not change with retirement -- byte for byte.
///
/// The main check of the step. J6 promises the save reproduces the game
/// bitwise, and it rests on three things in a leg; retirement throws away
/// everything else. A difference here would mean it touched those three.
#[test]
fn the_save_is_the_same_file_with_retirement_and_without() {
    let directory = std::env::temp_dir().join("space_sim_retire_test");
    std::fs::create_dir_all(&directory).expect("the directory is created");

    let with = directory.join("with.save");
    let without = directory.join("without.save");

    game::save::write_world(&flown(true), &with).expect("the save is written");
    game::save::write_world(&flown(false), &without).expect("the save is written");

    let a = std::fs::read(&with).expect("the save is read");
    let b = std::fs::read(&without).expect("the save is read");
    assert_eq!(a, b, "retirement changed the save");
}

/// Samples drop several fold, and the number is written down right here.
///
/// Without this check the previous one would be green for a retirement that
/// does nothing.
#[test]
fn retirement_costs_the_history_most_of_its_samples() {
    let retired = flown(true).vessels()[0].trajectory.sample_count();
    let whole = flown(false).vessels()[0].trajectory.sample_count();

    // Two thirds, not "half", and the threshold is measured rather than
    // chosen: on the halo orbit retirement leaves 1207 samples out of 2304,
    // because that curve really does bend along its whole length. On a low
    // orbit the gain is larger -- but that is a number of the fleet fixture,
    // and it lives in ROADMAP, not in a test threshold.
    assert!(
        retired * 3 <= whole * 2,
        "retirement left {retired} of {whole} samples -- that is no retirement"
    );
}

/// The window around the cursor stays raw.
///
/// `state_at` on "now" answers bitwise the same as without retirement: the
/// cursor stands inside the window, and none of its samples saw retirement.
#[test]
fn the_window_around_the_cursor_keeps_every_sample() {
    let retired = flown(true);
    let whole = flown(false);

    let now = retired.clock().t();
    assert_eq!(
        now,
        whole.clock().t(),
        "the runs should have reached the same instant"
    );

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
            "{name} at the cursor: {x:e} against {y:e} -- the window is not raw"
        );
    }
}

/// Leg ends do not move: `entry`, `t1`, `step_out` and the **last sample**.
///
/// Checked on every leg, not the first: retirement works from the beginning,
/// and an error at the window boundary would be an error of exactly one leg.
#[test]
fn every_leg_keeps_the_three_things_the_save_stands_on() {
    let retired = flown(true);
    let whole = flown(false);

    let a = retired.vessels()[0].trajectory.legs();
    let b = whole.vessels()[0].trajectory.legs();
    assert_eq!(a.len(), b.len(), "retirement lost a leg");

    for (index, (mine, theirs)) in a.iter().zip(b).enumerate() {
        assert_eq!(
            mine.entry.t.to_bits(),
            theirs.entry.t.to_bits(),
            "leg {index}: entry moved"
        );
        assert_eq!(mine.t1.to_bits(), theirs.t1.to_bits(), "leg {index}: t1");
        assert_eq!(
            mine.step_out.to_bits(),
            theirs.step_out.to_bits(),
            "leg {index}: step_out"
        );

        let last = mine.samples.last().expect("a leg with no samples");
        let expected = theirs.samples.last().expect("a leg with no samples");
        assert_eq!(
            last.state.t.to_bits(),
            expected.state.t.to_bits(),
            "leg {index}: the last sample -- restart_at stands on it"
        );
        assert_eq!(
            last.state.r.x.to_bits(),
            expected.state.r.x.to_bits(),
            "leg {index}: the last sample, r.x"
        );
    }
}

/// The main number of N5a: the history plateaus and **stops growing**.
///
/// WARNING: the fixture here is the station, and that is not a detail. The
/// first version of the test took halo 1151 and was green for the wrong
/// reason: its geocentric radius vector goes round once per lunar month, so
/// twenty revolutions are twenty months -- longer than the whole fixture. The
/// plateau the test saw was the **end of the mission**, not the window (1128
/// samples on day 75 and the same on day 90, with ten legs out of ten
/// possible).
///
/// The station on a low orbit takes an hour and a half per revolution, so
/// twenty revolutions are a day and a bit, and the window really does cut.
/// The oracle is the same run without trimming: without it the history grows,
/// with it it plateaus.
#[test]
fn the_history_stops_growing_once_the_window_is_full() {
    let counts = |trim: bool| {
        let mut world = mission::fleet(&mission::default_asset(), 1).expect("the fleet builds");
        world.set_history_trimming(if trim { Some(RAW_LEGS_BEHIND) } else { None });
        let start = mission::start().t;

        let mut out = Vec::new();
        for days in [20.0, 40.0, 80.0] {
            world.run_to_day(start + days * 86400.0, 1.0, 8);
            // Vessel 1 is the station; vessel 0 is the halo, whose revolution
            // is a lunar month.
            out.push(world.vessels()[1].trajectory.sample_count());
        }
        out
    };

    let windowed = counts(true);
    let whole = counts(false);

    // Without the window the history grows -- otherwise the check below would
    // prove nothing.
    assert!(
        whole[2] > whole[0] * 2,
        "without the window the history should grow: {whole:?}"
    );

    // With the window, a plateau: doubling the run does not add even a leg.
    let slack = game::world::LEG;
    assert!(
        windowed[2] <= windowed[1] + slack,
        "the history kept growing: {windowed:?}"
    );
    assert!(
        windowed[2] * 4 < whole[2],
        "the window cut too little: {windowed:?} against {whole:?}"
    );
}

/// Memory is named by the same number the debt speaks in.
///
/// Not a separate count in the test: the player sees exactly
/// `history_bytes`, and the two numbers have no right to diverge.
#[test]
fn the_predicted_size_is_the_sample_count_times_the_debts_number() {
    let world = flown(true);
    let trajectory = &world.vessels()[0].trajectory;
    assert_eq!(trajectory.history_bytes(), trajectory.sample_count() * 104);
}
