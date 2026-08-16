//! The game types change no bit (ROADMAP J1).
//!
//! This is the whole check for the step. Between `prop_run` and what the
//! player sees there is now a layer: legs, storage, snapshot, scene. Each of
//! them could quietly spoil something -- shuffle the sample order, lose the
//! last one, continue from the wrong state -- and none of those errors fails:
//! all of them give a plausible curve.
//!
//! So the oracle is the H5 run (`engine::live`), and the comparison is
//! bitwise. A leg there is 64 samples, here 256, deliberately: if the numbers
//! matched, the check would be a tautology. That they differ while the result
//! is bit-identical is the claim "work is measured in legs, and a leg does not
//! affect the numbers" (CLAUDE.md, invariant 9; measured in H1).

use engine::live;
use game::mission;
use game::snapshot::WorldSnapshot;
use game::world::World;

/// Every sample of every vessel, in order.
fn samples(snapshot: &WorldSnapshot) -> Vec<game::leg::Sample> {
    snapshot
        .vessels
        .iter()
        .flat_map(|v| v.legs.iter().flat_map(|leg| leg.samples.iter().copied()))
        .collect()
}

/// A world without leg retirement (N3a).
///
/// This is J1's main comparison -- sample for sample against the direct H5 run
/// -- and retirement discards samples. It changes no bit of what was computed
/// but changes what remains; comparing with it would mean comparing storage
/// rather than physics.
fn finished_world() -> World {
    let mut world = mission::world(&mission::default_asset()).expect("the world builds");
    world.set_history_trimming(None);
    world.run_to_end(1.0, 8);
    world
}

/// J1's main check: the same as H5, to the last bit.
#[test]
fn the_game_computes_what_the_direct_run_computes() {
    let world = finished_world();
    let snapshot = world.snapshot();
    let mine = samples(&snapshot);

    let reference =
        live::propagate(&mission::start(), mission::DAYS, &live::repo_asset()).expect("the H5 run");

    assert_eq!(
        mine.len(),
        reference.samples.len(),
        "{} samples against {} from the direct run",
        mine.len(),
        reference.samples.len()
    );
    assert!(
        snapshot.vessels[0].legs.len() != reference.legs,
        "the legs must differ in size, or the comparison proves nothing: \
         {} against {}",
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
                "sample {i}, {name}: {a:e} against {b:e}"
            );
        }
    }
}

/// How much work per tick does not affect the numbers at all.
///
/// The leg budget is how a frame bounds latency; it decides **when** a piece
/// of prediction appears and never what its numbers are. Its counterpart is
/// `tests/time.rs`, where the same is checked from the clock's side.
#[test]
fn the_size_of_a_tick_does_not_change_the_numbers() {
    let run = |budget: usize| {
        let mut world = mission::world(&mission::default_asset()).expect("the world builds");
        world.run_to_end(1.0, budget);
        samples(&world.snapshot())
    };

    let slow = run(1);
    let fast = run(1000);

    assert!(!slow.is_empty(), "nothing was computed");
    assert_eq!(slow.len(), fast.len(), "different sample counts");

    for (i, (a, b)) in slow.iter().zip(fast.iter()).enumerate() {
        assert_eq!(a.state.t.to_bits(), b.state.t.to_bits(), "sample {i}: time");
        assert_eq!(
            a.state.r.x.to_bits(),
            b.state.r.x.to_bits(),
            "sample {i}: x"
        );
        assert_eq!(
            a.state.v.z.to_bits(),
            b.state.v.z.to_bits(),
            "sample {i}: vz"
        );
    }
}

/// The legs stitch without repeated or lost vertices.
///
/// `prop_run` does not sample the initial point, so one leg's end and the
/// next's start are adjacent steps rather than the same one. An error here
/// would give a polyline with doubled vertices or with holes, and neither is
/// visible by eye.
#[test]
fn legs_stitch_without_seams() {
    let world = finished_world();
    let vessel = &world.vessels()[0];
    let legs = vessel.trajectory.legs();

    assert!(legs.len() > 1, "at least two legs are needed");

    for pair in legs.windows(2) {
        let (before, after) = (&pair[0], &pair[1]);

        assert_eq!(
            before.t1.to_bits(),
            after.entry.t.to_bits(),
            "a gap between legs: {} against {}",
            before.t1,
            after.entry.t
        );

        let last = before.samples.last().expect("the leg is not empty");
        let first = after.samples.first().expect("the leg is not empty");
        assert!(
            first.state.t > last.state.t,
            "the next leg's first sample is not later than the previous leg's \
             last: {} against {}",
            first.state.t,
            last.state.t
        );
    }

    // The last sample is the mission's end rather than "somewhere near".
    let last = legs.last().unwrap().samples.last().unwrap();
    assert_eq!(
        last.state.t.to_bits(),
        vessel.horizon_end.to_bits(),
        "the mission ended at {} instead of {}",
        last.state.t,
        vessel.horizon_end
    );
}

/// The step a leg ended with is non-zero and is carried onwards.
///
/// Without it, restarting from a leg boundary would give a different
/// trajectory (H1: 70x the work and 1.9 mm of divergence), and the whole J3
/// cascade recomputation rests on it.
#[test]
fn every_leg_carries_the_step_it_ended_with() {
    let world = finished_world();
    let legs = world.vessels()[0].trajectory.legs();

    for (i, leg) in legs.iter().enumerate() {
        assert!(
            leg.step_out > 0.0 && leg.step_out.is_finite(),
            "leg {i} left step {}",
            leg.step_out
        );
        assert!(
            leg.step_out <= mission::H_MAX_S,
            "leg {i} left step {}, above the ceiling {}",
            leg.step_out,
            mission::H_MAX_S
        );
    }
}
