//! The clock does not enter the integrator (ROADMAP J2).
//!
//! The main check of the step and probably of the whole stage. The claim is
//! simple and checked simply: **run the same mission at different frame
//! rates, different warp and with pauses in the middle -- and get bitwise the
//! same trajectory.**
//!
//! It is not self-evident. One `t_end` coming from the clock is enough for
//! the frame rate to write itself into the sequence of integrator steps,
//! because `prop_run` lands the last step exactly on `t_end` (CLAUDE.md,
//! invariant 9). A bug of that class does not crash: it gives a plausible
//! curve that is merely a little different on another machine -- broken
//! determinism, found half a year later on someone else's save.

use game::clock::Stall;
use game::leg::Sample;
use game::mission;

/// Runs the mission with a cyclic pattern of frame `dt` and returns every
/// sample.
fn run(pattern: &[f64], budget: usize) -> (Vec<Sample>, f64) {
    let mut world = mission::world(&mission::default_asset()).expect("the world builds");

    // A ceiling on frames, so that a broken test fails instead of hanging.
    for frame in 0..2_000_000 {
        world.step(pattern[frame % pattern.len()], budget);
        if world.clock().stall() == Some(Stall::MissionEnd) {
            break;
        }
    }

    let snapshot = world.snapshot();
    let samples = snapshot
        .vessels
        .iter()
        .flat_map(|v| v.legs.iter().flat_map(|leg| leg.samples.iter().copied()))
        .collect();

    (samples, snapshot.t)
}

fn assert_same(a: &[Sample], b: &[Sample], what: &str) {
    assert_eq!(a.len(), b.len(), "{what}: different sample counts");
    assert!(!a.is_empty(), "{what}: nothing was computed");

    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        for (name, p, q) in [
            ("t", x.state.t, y.state.t),
            ("r.x", x.state.r.x, y.state.r.x),
            ("r.y", x.state.r.y, y.state.r.y),
            ("r.z", x.state.r.z, y.state.r.z),
            ("v.x", x.state.v.x, y.state.v.x),
            ("v.y", x.state.v.y, y.state.v.y),
            ("v.z", x.state.v.z, y.state.v.z),
        ] {
            assert_eq!(
                p.to_bits(),
                q.to_bits(),
                "{what}: sample {i}, {name}: {p:e} against {q:e}"
            );
        }
    }
}

/// Five frames per second against five hundred -- bitwise the same.
#[test]
fn the_frame_rate_does_not_reach_the_numbers() {
    let (slow, slow_t) = run(&[0.2], 4);
    let (fast, fast_t) = run(&[0.002], 4);

    assert_same(&slow, &fast, "5 fps against 500 fps");

    // Both reached the end of the mission -- otherwise identical samples would
    // only mean both stopped at the same place too early.
    let end = mission::start().t + mission::DAYS * 86400.0;
    assert_eq!(
        slow_t.to_bits(),
        end.to_bits(),
        "the slow one did not get there"
    );
    assert_eq!(
        fast_t.to_bits(),
        end.to_bits(),
        "the fast one did not get there"
    );
}

/// A stuttering frame rate is bitwise the same too.
///
/// An even sequence could fall into the same grid by chance; this one cannot.
/// The numbers are taken as the drops of a real game: 60 fps with random
/// dips to 3 fps, zero included (a frame with no time advance must be safe
/// too).
#[test]
fn a_stuttering_frame_rate_does_not_reach_them_either() {
    let steady = run(&[0.016], 4).0;
    let stutter = run(&[0.016, 0.33, 0.001, 0.0, 0.07, 0.016, 0.21], 4).0;

    assert_same(&steady, &stutter, "even frames against stuttering ones");
}

/// Warp and pause change nothing either, beyond the cursor's speed.
///
/// The same run, but at half warp and with a pause a third of the way
/// through. The pause is not decoration: it stops the cursor while the tick
/// keeps running, and if the horizon were computed "from the time", this is
/// exactly where that would show.
#[test]
fn warp_and_pause_do_not_reach_them_either() {
    let plain = run(&[0.016], 4).0;

    let mut world = mission::world(&mission::default_asset()).expect("the world builds");
    world.clock_mut().set_warp(mission::DEFAULT_WARP / 2.0);

    let mut paused_once = false;
    for frame in 0..2_000_000 {
        // A pause of fifty frames in the middle of the mission.
        if !paused_once && frame == 1500 {
            world.clock_mut().toggle_pause();
        }
        if !paused_once && frame == 1550 {
            world.clock_mut().toggle_pause();
            paused_once = true;
        }
        // Warp doubled, after the pause.
        if frame == 1600 {
            world.clock_mut().scale_warp(2.0);
        }

        world.step(0.016, 4);
        if world.clock().stall() == Some(Stall::MissionEnd) {
            break;
        }
    }

    assert!(
        paused_once,
        "the pause never fired -- the mission is shorter than the test thinks"
    );

    let snapshot = world.snapshot();
    let varied: Vec<Sample> = snapshot
        .vessels
        .iter()
        .flat_map(|v| v.legs.iter().flat_map(|leg| leg.samples.iter().copied()))
        .collect();

    assert_same(
        &plain,
        &varied,
        "steady warp against changed warp with a pause",
    );
}

/// The cursor never outruns what is computed.
///
/// Checked every frame at maximum warp and with one leg per frame, i.e. under
/// the worst conditions the game can create by itself.
#[test]
fn the_cursor_never_outruns_what_is_computed() {
    let mut world = mission::world(&mission::default_asset()).expect("the world builds");
    world.clock_mut().set_warp(game::clock::MAX_WARP);

    for _ in 0..400 {
        world.step(0.016, 1);
        let snapshot = world.snapshot();

        for vessel in &snapshot.vessels {
            assert!(
                snapshot.t <= vessel.computed_to,
                "the cursor at {} outran what is computed, {}",
                snapshot.t,
                vessel.computed_to
            );
        }

        if snapshot.stall == Some(Stall::MissionEnd) {
            return;
        }
    }

    panic!("at maximum warp the mission should have ended within 400 frames");
}

/// A world that computes nothing has no right to move its time.
///
/// This is the warp ceiling mechanism in its pure form: not a number in the
/// code, but the cursor having nowhere to go. A budget of zero is the extreme
/// case of "the integrator cannot keep up", and that is why it is here:
/// **on this mission warp never runs into throughput at all.** One leg is
/// about eleven days of trajectory, and a frame at maximum warp is 1.85 days;
/// the integrator is six times ahead of the clock even at the ceiling and
/// with one leg per frame. So provoking [`Stall::Horizon`] by honest work is
/// impossible here, and that is not a flaw of the test but a measured
/// property: in free flight the core's throughput (3e6 steps/s, ROADMAP I3)
/// is more than enough. It will bind under thrust and in the atmosphere --
/// there the step is fixed and small.
#[test]
fn a_world_that_computes_nothing_cannot_move_its_clock() {
    let mut world = mission::world(&mission::default_asset()).expect("the world builds");
    world.clock_mut().set_warp(game::clock::MAX_WARP);

    let before = world.clock().t();
    for _ in 0..10 {
        world.step(0.016, 0);
    }

    assert_eq!(
        world.clock().t().to_bits(),
        before.to_bits(),
        "time moved forward without a single computed leg"
    );
    assert_eq!(
        world.clock().stall(),
        Some(Stall::Horizon),
        "time stands still but does not say why"
    );
}

/// The interpolated state lies on the trajectory, not beside it.
///
/// The oracle is the trajectory itself: at sample instants the interpolation
/// must return exactly the sample (this catches an error in the Hermite
/// basis), and in between no further than the curve manages to bend away from
/// the chord.
#[test]
fn the_interpolated_state_lies_on_the_trajectory() {
    let mut world = mission::world(&mission::default_asset()).expect("the world builds");
    world.run_to_end(1.0, 8);

    let vessel = &world.vessels()[0];
    let trajectory = &vessel.trajectory;

    let mut worst_at_sample = 0.0f64;
    let mut worst_chord = 0.0f64;

    let all: Vec<Sample> = trajectory
        .legs()
        .iter()
        .flat_map(|leg| leg.samples.iter().copied())
        .collect();

    for pair in all.windows(2) {
        let (a, b) = (pair[0].state, pair[1].state);

        // At a node, exactly the node.
        let at = trajectory.state_at(a.t);
        worst_at_sample = worst_at_sample.max(distance(at.r, a.r));

        // In between, somewhere between the chord and the cubic. The chord is
        // the yardstick: an interpolation worse than it is no interpolation.
        let mid_t = 0.5 * (a.t + b.t);
        let mid = trajectory.state_at(mid_t);
        let chord = [
            0.5 * (a.r.x + b.r.x),
            0.5 * (a.r.y + b.r.y),
            0.5 * (a.r.z + b.r.z),
        ];
        worst_chord = worst_chord.max(distance(
            mid.r,
            core_rs::Vec3d {
                x: chord[0],
                y: chord[1],
                z: chord[2],
            },
        ));
    }

    println!("  at a node: {worst_at_sample:e} m, from the chord: {worst_chord:e} m");

    // At a node Hermite is exact by construction; only rounding is left.
    assert!(
        worst_at_sample < 1e-6,
        "at sample instants the interpolation should return the sample itself, \
         but missed by {worst_at_sample:e} m"
    );

    // The deviation from the chord must be noticeable, or the cubic has
    // degenerated into a straight line -- meaning velocities do not enter it,
    // and the error would not show.
    assert!(
        worst_chord > 1.0,
        "the cubic coincided with the chord ({worst_chord:e} m) -- velocities did \
         not reach the interpolation"
    );
}

fn distance(a: core_rs::Vec3d, b: core_rs::Vec3d) -> f64 {
    let d = [a.x - b.x, a.y - b.y, a.z - b.z];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}
