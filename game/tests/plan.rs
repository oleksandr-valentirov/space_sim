//! A plan turns into a sequence of `prop_run` calls (ROADMAP J3).
//!
//! Three claims, each about its own failure:
//!
//!   1. **the machinery adds nothing of its own** -- a plan flown by the world
//!      equals bitwise the same `prop_run` calls stitched together by hand;
//!   2. **an edit does not touch the past** -- not a bit before the manoeuvre
//!      moved;
//!   3. **a recomputation costs exactly the tail** -- and that is a number,
//!      not an intention.
//!
//! What is not here: the external problem. Both sides of this comparison do
//! `v += dv` by the same formula, so it says nothing about the **physical
//! meaning** of the impulse -- nor should it. That is the subject of
//! `game/tests/impulse.rs` (ROADMAP L4, debt D1): there Lambert from the
//! boundary gives an approximation, `prop_run_stm` corrects it, the vessel
//! arrives at the Moon's position from the asset, and the VNB basis is
//! checked against textbook two-body mechanics.

use core_rs::{Ephemeris, PropConfig, Propagator, State};
use game::leg::Sample;
use game::mission;
use game::plan::{Frame, Manoeuvre, Plan};
use game::world::{PlanRejected, VesselId, World, LEG};
use std::sync::Arc;

const DAY: f64 = 86400.0;

/// A plan of three burns: along the velocity, across the plane, outward.
///
/// Different frames on purpose: were all three inertial, the test would say
/// nothing about the basis transform, which is the only non-trivial thing
/// here.
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

/// A world with leg retirement off (N3a).
///
/// The tests in this file compare a **stream of samples** against an
/// independent run, and retirement discards samples -- i.e. it changes what
/// they compare without changing a bit of what was computed. Turning it off
/// is not a weaker oracle but the condition under which it asks anything at
/// all; retirement itself is checked by `tests/retire.rs`.
fn world_with(plan: Plan) -> World {
    let mut world = mission::world(&mission::default_asset()).expect("the world builds");
    world.set_history_trimming(None);
    world
        .commit_plan(VesselId(0), plan)
        .expect("a plan in the future");
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
                "{what}: sample {i}, {name}: {p:e} against {q:e}"
            );
        }
    }
}

/// The main check of J3: the world does exactly what a hand would do.
///
/// The oracle is a sequence of `prop_run` calls through `core-rs`, stitched
/// together in the test: propagate to the ignition instant, add dv, carry on.
/// The same physics, but **different machinery** -- no store, no snapshots, no
/// index of applied manoeuvres and no cascade. A divergence would mean a bug
/// in exactly those, and none of those bugs fails by itself: they all give a
/// plausible curve.
#[test]
fn the_world_flies_the_plan_a_hand_stitched_run_would_fly() {
    let start = mission::start();
    let plan = three_burns(start.t);
    let world = world_with(plan.clone());
    let mine = samples(&world);

    // --- the oracle, by hand ---
    let eph = Arc::new(Ephemeris::load(&mission::default_asset()).expect("asset"));
    let cfg = PropConfig {
        tol_m: mission::TOL_M,
        h_max_s: mission::H_MAX_S,
        ..PropConfig::default()
    };
    let mut prop = Propagator::new(eph.clone(), cfg).expect("propagator");

    let mission_end = start.t + mission::DAYS * DAY;
    let mut state = start;
    let mut step = 0.0;
    let mut buffer = vec![State::default(); LEG];
    let mut theirs: Vec<State> = Vec::new();

    // Segment boundaries: the manoeuvre instants, then the end of the mission.
    // That is what the world does too, only it takes them from the plan by
    // index.
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
                .expect("run");
            theirs.extend_from_slice(&buffer[..run.filled]);
            state = run.final_state;
            if run.stop == core_rs::Stop::ReachedEnd {
                break;
            }
        }

        // The impulse comes after the segment, with the same frame and the
        // same step.
        if let Some(m) = plan.get(index) {
            let body = m
                .frame_body()
                .map(|id| eph.body_state(id, state.t).expect("body"));
            let dv = m.dv_inertial(&state, body.as_ref());
            state.v.x += dv[0];
            state.v.y += dv[1];
            state.v.z += dv[2];
        }
    }

    assert_eq!(
        mine.len(),
        theirs.len(),
        "{} samples against {} in the hand-stitched run",
        mine.len(),
        theirs.len()
    );
    assert!(mine.len() > 1000, "too few samples to prove anything with");

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
                "sample {i}, {name}: {p:e} against {q:e}"
            );
        }
    }
}

/// The same plan twice -- bitwise the same.
#[test]
fn a_plan_replayed_gives_the_same_bits() {
    let start = mission::start();
    let first = samples(&world_with(three_burns(start.t)));
    let second = samples(&world_with(three_burns(start.t)));

    assert_eq!(first.len(), second.len());
    assert_same(&first, &second, first.len(), "a replayed plan");
}

/// The burn really changes something, and exactly where it says.
///
/// Without this check every bitwise comparison above would be just as true
/// for a plan nobody flew.
#[test]
fn the_burn_changes_the_trajectory_and_only_after_itself() {
    let start = mission::start();
    let burn_t = start.t + 10.0 * DAY;

    let plain = samples(&world_with(Plan::new()));
    let burned = samples(&world_with(three_burns(start.t)));

    let before: Vec<usize> = (0..plain.len().min(burned.len()))
        .filter(|&i| plain[i].state.t < burn_t)
        .collect();
    assert!(
        !before.is_empty(),
        "there should have been samples before the manoeuvre"
    );

    assert_same(
        &plain,
        &burned,
        *before.last().unwrap() + 1,
        "before the first manoeuvre",
    );

    // And after it they diverge, noticeably: 12 m/s on an unstable orbit is a
    // lot.
    let last = plain.len().min(burned.len()) - 1;
    let miss = distance(plain[last].state, burned[last].state);
    println!("  divergence at the end of the mission: {miss:e} m");
    assert!(
        miss > 1.0e6,
        "a 12 m/s manoeuvre should have parted the trajectories by more than {miss:e} m"
    );
}

/// Editing a manoeuvre moves not a bit before it -- and costs only the tail.
///
/// This is the cascading recomputation of PROJECT.md §6, in two claims at
/// once: what exactly is preserved and what it costs.
///
/// The cursor deliberately stays on day 20: only the future can be edited,
/// and that is not a limitation of the test but the design itself -- which is
/// why the history is untouchable.
#[test]
fn editing_a_manoeuvre_costs_only_the_tail() {
    let start = mission::start();
    let plan = three_burns(start.t);

    let mut world = mission::world(&mission::default_asset()).expect("the world builds");
    // Retirement off, for the same reason as in `world_with`.
    world.set_history_trimming(None);
    world
        .commit_plan(VesselId(0), plan.clone())
        .expect("a plan in the future");
    world.run_to_day(start.t + 20.0 * DAY, 1.0, 8);

    let before_edit = samples(&world);
    let legs_before = world.vessels()[0].trajectory.legs().len();
    let horizon_before = world.vessels()[0].trajectory.computed_to();

    // The second manoeuvre, on day 25, is already inside the computed span,
    // and there is still something to recompute after it. The third will not
    // do: the horizon stops exactly on it (it is the segment boundary), so an
    // edit would discard nothing and the measurement would come out empty.
    let second = *plan.get(1).expect("three manoeuvres");
    assert!(
        horizon_before > second.t,
        "the horizon {horizon_before} did not pass the manoeuvre at {}",
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
        .expect("an edit in the future")
        .expect("the plan changed");
    assert_eq!(
        from.to_bits(),
        second.t.to_bits(),
        "the recomputation did not start at the manoeuvre"
    );

    let legs_kept = world.vessels()[0].trajectory.legs().len();
    let cost_before = world.legs_computed();

    // dt = 0: the cursor stands still and only the horizon works. That
    // measures exactly the cost of the recomputation, with no new work ahead
    // mixed in.
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
        "there should have been many samples before the second manoeuvre, but there are {kept_samples}"
    );
    assert_same(
        &before_edit,
        &after_edit,
        kept_samples,
        "before the edited manoeuvre",
    );

    let horizon_after = world.vessels()[0].trajectory.computed_to();
    println!(
        "  legs were {legs_before}, {legs_kept} left, {recomputed} recomputed; \
         {kept_samples} samples kept; horizon {:.1} -> {:.1} days",
        (horizon_before - start.t) / DAY,
        (horizon_after - start.t) / DAY
    );

    // The main number of the step: the recomputation touched only the tail.
    // Equality cannot be demanded here -- with a different dv the
    // "buffer full" boundaries fall elsewhere, so the leg count may come out
    // one higher or lower.
    assert!(
        recomputed as usize <= legs_before - legs_kept + 1,
        "{recomputed} legs recomputed while only {} were discarded",
        legs_before - legs_kept
    );
    assert!(
        legs_kept > 0 && (recomputed as usize) < legs_before,
        "the recomputation ate the whole forecast: {recomputed} against {legs_before} legs"
    );

    // The horizon recovered -- though not necessarily to the same second, and
    // that is no oversight: it is measured in LEGS, and after a different dv
    // the "buffer full" boundaries land elsewhere. Demanding the same second
    // would demand that a different trajectory have the same density of steps.
    assert!(
        horizon_after >= horizon_before,
        "the forecast after the recomputation is shorter than it was: \
         {horizon_after} against {horizon_before}"
    );
}

/// The past cannot be edited, and that is a refusal rather than silent
/// agreement.
#[test]
fn a_manoeuvre_in_the_past_is_refused() {
    let start = mission::start();
    let mut world = mission::world(&mission::default_asset()).expect("the world builds");

    // Bring the cursor to day thirty.
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
        "a manoeuvre on day 20 with the cursor on day 30 should have been refused"
    );

    // In the future it is accepted.
    let mut future = Plan::new();
    future.insert(Manoeuvre {
        t: start.t + 60.0 * DAY,
        dv: [1.0, 0.0, 0.0],
        frame: Frame::Inertial,
    });
    assert!(world.commit_plan(VesselId(0), future).is_ok());
}

/// What it costs to reset the integrator step at a burn instead of carrying
/// it.
///
/// ROADMAP J3 required both numbers to be measured rather than guessed. The
/// comparison goes straight through `core-rs`, with no world: the question
/// here is not about machinery but about how the step controller behaves
/// after a velocity discontinuity.
#[test]
fn carrying_the_step_through_a_burn_against_resetting_it() {
    let start = mission::start();
    let eph = Arc::new(Ephemeris::load(&mission::default_asset()).expect("asset"));
    let cfg = PropConfig {
        tol_m: mission::TOL_M,
        h_max_s: mission::H_MAX_S,
        ..PropConfig::default()
    };

    let burn_t = start.t + 10.0 * DAY;
    let end_t = burn_t + 5.0 * DAY;

    let fly = |reset: bool| -> (usize, State) {
        let mut prop = Propagator::new(eph.clone(), cfg).expect("propagator");
        let mut buffer = vec![State::default(); LEG];
        let mut step = 0.0;
        let mut state = start;
        let mut count = 0;

        for boundary in [burn_t, end_t] {
            loop {
                let run = prop
                    .run(&state, None, boundary, &[], &mut buffer, &mut step)
                    .expect("run");
                count += run.filled;
                state = run.final_state;
                if run.stop == core_rs::Stop::ReachedEnd {
                    break;
                }
            }

            if boundary == burn_t {
                // The same impulse in both cases; only the step differs.
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
        "  carried step: {carried_steps} steps; reset: {reset_steps}; \
         divergence at the end: {miss:e} m"
    );

    // Both trajectories are correct within tolerance -- the question is only
    // the price.
    assert!(
        miss < 1.0e3,
        "the two ways of carrying the step gave different trajectories ({miss:e} m) \
         -- that is no longer about cost"
    );
    assert!(
        carried_steps <= reset_steps,
        "carrying the step turned out dearer: {carried_steps} against {reset_steps}"
    );
}

/// The dv is applied exactly at the manoeuvre instant, and exactly once.
///
/// The leg must end bitwise at the manoeuvre time, and the velocity jump
/// between its last sample and the next leg's `entry` must equal |dv| to
/// within rounding. A double application (the likeliest bug in index
/// bookkeeping) would give exactly twice that.
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
        .expect("no leg ended exactly at the manoeuvre");

    let before = legs[at].samples.last().expect("the leg is not empty").state;
    let after = legs[at + 1].entry;

    assert_eq!(before.t.to_bits(), after.t.to_bits(), "a gap in time");
    assert_eq!(
        before.r.x.to_bits(),
        after.r.x.to_bits(),
        "the position jumped"
    );

    let jump = (
        after.v.x - before.v.x,
        after.v.y - before.v.y,
        after.v.z - before.v.z,
    );
    let magnitude = (jump.0 * jump.0 + jump.1 * jump.1 + jump.2 * jump.2).sqrt();

    assert!(
        (magnitude - 12.0).abs() < 1e-9,
        "a velocity jump of {magnitude} m/s instead of 12 -- the manoeuvre was not \
         applied once"
    );
    assert!(
        (jump.0 - 12.0).abs() < 1e-9,
        "the impulse went the wrong way: {jump:?}"
    );
}

fn distance(a: State, b: State) -> f64 {
    let d = [a.r.x - b.r.x, a.r.y - b.r.y, a.r.z - b.r.z];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}
