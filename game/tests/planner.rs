//! The line you saw is the line you will fly (ROADMAP J5).
//!
//! The preview is computed by another thread, with another propagator, in its
//! own throwaway world. The promise is not "similar" but **bitwise the same**
//! as what `Sim` will compute later. Without it the manoeuvre planner is
//! pointless: the player would choose by one curve and fly another
//! (PROJECT.md §8, "флайт-планер").
//!
//! The easiest way to break the promise is to start the run in the wrong
//! place: not at a leg boundary, or with "pick the step yourself". H1
//! measured that this is a different trajectory, not merely a slower one.

use std::sync::Arc;
use std::time::{Duration, Instant};

use game::clock::Stall;
use game::leg::{restart_at, Leg};
use game::mission;
use game::plan::{Frame, Manoeuvre, Plan};
use game::planner::{Planner, Preview, PreviewRequest, Request};
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
        assert!(Instant::now() < deadline, "never arrived: {what}");
        std::thread::yield_now();
    }
}

/// The main check of J5: a preview from another thread is the future flight,
/// bitwise.
#[test]
fn a_preview_is_bit_identical_to_the_flight_that_follows() {
    let sim = Sim::spawn(mission::world(&mission::default_asset()).expect("world"))
        .expect("the simulation thread");

    // First let the cursor move away from the start, then stop it. The pause
    // is needed by the test, not by the design: otherwise the restart point
    // could slip into the past between the snapshot and the commit. And it has
    // to move away because the horizon trails the cursor -- near the start
    // there would be no legs left after it to compare.
    sim.send(Command::SetWarp(game::clock::MAX_WARP));
    wait_until("the cursor moves away from the start", || {
        sim.snapshot().t >= mission::start().t + 15.0 * DAY
    });
    sim.send(Command::TogglePause);
    wait_until("the pause arrives", || {
        sim.snapshot().stall == Some(Stall::Paused)
    });

    // The manoeuvre time is taken from where the cursor ACTUALLY stopped, not
    // from a round number. The command does not arrive instantly, and at
    // maximum warp each tick is almost two days; a fixed day 30 would mean
    // that on a slower machine the cursor drives past it and the plan is
    // rejected as "in the past". That is exactly how this test failed on macOS
    // while passing on Linux.
    let cursor = sim.snapshot().t;
    let burn_t = cursor + 5.0 * DAY;
    wait_until("the horizon reaches the manoeuvre", || {
        sim.snapshot().vessels[0].computed_to > burn_t
    });

    let snapshot = sim.snapshot();
    let vessel = &snapshot.vessels[0];

    // The restart point comes from the same function `Sim` will use when it
    // accepts the plan. That is the point: one function, not two identical
    // rules.
    let restart = restart_at(&vessel.legs, vessel.start, burn_t);
    assert!(
        restart.step > 0.0,
        "a restart with \"pick the step yourself\" -- the preview would start in \
         the wrong place"
    );

    let plan = burn_at(burn_t);
    let planner = Planner::spawn(sim.ephemeris(), mission::config()).expect("the planner thread");
    planner.request(Request::Preview(PreviewRequest {
        id: 1,
        vessel: VesselId(0),
        from: restart.state,
        step: restart.step,
        plan: plan.clone(),
        params: None,
        horizon_end: vessel.horizon_end,
    }));

    let mut preview: Option<Preview> = None;
    wait_until("the preview", || {
        preview = planner.latest();
        preview.is_some()
    });
    let preview = preview.expect("just checked");
    assert_eq!(preview.id, 1);
    assert!(
        preview.legs.len() >= 2,
        "a preview of {} legs -- too few to compare anything",
        preview.legs.len()
    );

    // And now the same plan for real.
    sim.send(Command::CommitPlan {
        vessel: VesselId(0),
        plan,
    });
    // A rejection here is not "not arrived yet" but a failure; waiting for it
    // until patience runs out would hide the cause behind a timeout.
    wait_until("an answer about the plan", || {
        for event in sim.events() {
            match event {
                Event::PlanCommitted { .. } => return true,
                Event::PlanRejected { why, .. } => panic!("the plan was rejected: {why:?}"),
                _ => {}
            }
        }
        false
    });

    // How many legs the flight computes after the restart point is decided by
    // the horizon, which trails the CURSOR, and the cursor is paused. The
    // preview computes its own four legs from the restart point itself, i.e.
    // it looks further. The overlap is compared -- that is enough and it is
    // honest: a divergence would show on the first leg already.
    let flown_after = |snapshot: &game::snapshot::WorldSnapshot| -> Vec<Arc<Leg>> {
        snapshot.vessels[0]
            .legs
            .iter()
            .filter(|leg| leg.entry.t >= restart.state.t)
            .cloned()
            .collect()
    };

    // Waiting for a leg count is not enough, and that is worth a word: there
    // were already enough of them before the commit, so the check would run on
    // the OLD trajectory. There is one sign the recomputation actually
    // happened -- a leg ending exactly at the manoeuvre; before the commit
    // there was none and could be none.
    wait_until("the flight recomputes its tail", || {
        let snapshot = sim.snapshot();
        snapshot.vessels[0].legs.iter().any(|leg| leg.t1 == burn_t)
            && flown_after(&snapshot).len() >= 2
    });

    let after = sim.snapshot();
    let flown = flown_after(&after);

    println!(
        "  restart on day {:.3}, manoeuvre on {:.3}",
        (restart.state.t - mission::start().t) / DAY,
        (burn_t - mission::start().t) / DAY
    );

    let overlap = preview.legs.len().min(flown.len());
    assert!(
        overlap >= 2,
        "nothing to compare: {} legs of preview, {} of flight",
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
            "leg {i}: {} samples in the preview against {} in the flight",
            shown.samples.len(),
            flew.samples.len()
        );
        assert_eq!(
            shown.step_out.to_bits(),
            flew.step_out.to_bits(),
            "leg {i}: different step on the way out"
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
                    "leg {i}, sample {j}, {name}: {p:e} against {q:e}"
                );
            }
        }
    }

    println!(
        "  compared {overlap} legs, {} samples",
        preview.legs[..overlap]
            .iter()
            .map(|l| l.samples.len())
            .sum::<usize>()
    );
}

/// A preview starts at a leg boundary, not "where the vessel is now".
///
/// This check exists separately because that is where the promise breaks most
/// quietly: a run from an arbitrary point gives a plausible curve that is
/// simply not the right one.
#[test]
fn starting_a_preview_from_the_wrong_step_gives_a_different_line() {
    let sim = Sim::spawn(mission::world(&mission::default_asset()).expect("world"))
        .expect("the simulation thread");
    sim.send(Command::TogglePause);

    let burn_t = mission::start().t + 30.0 * DAY;
    wait_until("the horizon", || {
        sim.snapshot().vessels[0].computed_to > burn_t
    });

    let snapshot = sim.snapshot();
    let vessel = &snapshot.vessels[0];
    let restart = restart_at(&vessel.legs, vessel.start, burn_t);
    let plan = burn_at(burn_t);

    let planner = Planner::spawn(sim.ephemeris(), mission::config()).expect("the planner");

    let ask = |id: u64, step: f64| -> Preview {
        planner.request(Request::Preview(PreviewRequest {
            id,
            vessel: VesselId(0),
            from: restart.state,
            step,
            plan: plan.clone(),
            params: None,
            horizon_end: vessel.horizon_end,
        }));
        let mut got = None;
        wait_until("the preview", || {
            got = planner.latest();
            got.as_ref().is_some_and(|p| p.id == id)
        });
        got.expect("just checked")
    };

    let right = ask(1, restart.step);
    let wrong = ask(2, 0.0);

    let count = |p: &Preview| p.legs.iter().map(|l| l.samples.len()).sum::<usize>();
    println!(
        "  with the carried step: {} samples; with \"pick it yourself\": {}",
        count(&right),
        count(&wrong)
    );

    assert_ne!(
        count(&right),
        count(&wrong),
        "the run with \"pick the step yourself\" gave exactly the same -- then \
         carrying the step means nothing, and H1 measured something else"
    );
}

/// Stale previews never reach the caller.
///
/// The player drags a node and requests fly out dozens per second. The
/// current one is always the last, and it is the one that must arrive; nobody
/// needs the rest.
#[test]
fn only_the_latest_request_is_answered() {
    let sim = Sim::spawn(mission::world(&mission::default_asset()).expect("world"))
        .expect("the simulation thread");
    sim.send(Command::TogglePause);

    let burn_t = mission::start().t + 30.0 * DAY;
    wait_until("the horizon", || {
        sim.snapshot().vessels[0].computed_to > burn_t
    });

    let snapshot = sim.snapshot();
    let vessel = &snapshot.vessels[0];
    let restart = restart_at(&vessel.legs, vessel.start, burn_t);

    let planner = Planner::spawn(sim.ephemeris(), mission::config()).expect("the planner");

    // Twenty requests in a row, as from a mouse drag: each with its own dv.
    for id in 1..=20u64 {
        let mut plan = Plan::new();
        plan.insert(Manoeuvre {
            t: burn_t,
            dv: [-(id as f64), 0.0, 0.0],
            frame: Frame::Inertial,
        });
        planner.request(Request::Preview(PreviewRequest {
            id,
            vessel: VesselId(0),
            from: restart.state,
            step: restart.step,
            plan,
            params: None,
            horizon_end: vessel.horizon_end,
        }));
    }

    let mut last = None;
    wait_until("the last preview", || {
        if let Some(preview) = planner.latest() {
            last = Some(preview);
        }
        last.as_ref().is_some_and(|p| p.id == 20)
    });

    let last = last.expect("just checked");
    assert_eq!(last.id, 20, "it was not the last preview that arrived");
    assert!(!last.legs.is_empty(), "the last preview is empty");
}

/// A request that **interrupted** a run is answered too.
///
/// The check above sends a batch in one go and therefore proves nothing about
/// this case: all twenty are already in the channel when the thread takes the
/// first, and ordinary queue draining picks them up. Here it is different --
/// the second request arrives **in the middle** of the first run, so it is
/// the cancellation check that sees it. That check takes the message out of
/// the channel, and the question is exactly where it goes next.
///
/// This is what the end of a node drag looks like: the player released the
/// mouse, the last request flew out, and there will be no more. If
/// cancellation eats it, the thread falls asleep on `recv()` and the line of
/// the second-to-last position stays on screen forever.
///
/// The sleep is needed by the test: a preview takes about 50 ms (measured
/// this session), and the channel is checked between legs, so five
/// milliseconds are enough for the second request to catch the first at work
/// and too few for it to finish.
#[test]
fn the_request_that_cancelled_the_work_is_answered_too() {
    let sim = Sim::spawn(mission::world(&mission::default_asset()).expect("world"))
        .expect("the simulation thread");
    sim.send(Command::TogglePause);

    let burn_t = mission::start().t + 30.0 * DAY;
    wait_until("the horizon", || {
        sim.snapshot().vessels[0].computed_to > burn_t
    });

    let snapshot = sim.snapshot();
    let vessel = &snapshot.vessels[0];
    let restart = restart_at(&vessel.legs, vessel.start, burn_t);

    let planner = Planner::spawn(sim.ephemeris(), mission::config()).expect("the planner");

    let ask = |id: u64| {
        let mut plan = Plan::new();
        plan.insert(Manoeuvre {
            t: burn_t,
            dv: [-(id as f64), 0.0, 0.0],
            frame: Frame::Inertial,
        });
        planner.request(Request::Preview(PreviewRequest {
            id,
            vessel: VesselId(0),
            from: restart.state,
            step: restart.step,
            plan,
            params: None,
            horizon_end: vessel.horizon_end,
        }));
    };

    ask(1);
    std::thread::sleep(Duration::from_millis(5));
    ask(2);

    let mut last = None;
    wait_until("a preview for the second request", || {
        if let Some(preview) = planner.latest() {
            last = Some(preview);
        }
        last.as_ref().is_some_and(|p| p.id == 2)
    });

    assert_eq!(last.expect("just checked").id, 2);
}
