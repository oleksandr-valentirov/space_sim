//! The thread changed nothing but who does the computing (ROADMAP J4).
//!
//! What matters most here is what did **not** have to be checked. There is no
//! data race, not because we went looking for one, but because there is no
//! shared mutable state at all: the world belongs to the thread, immutable
//! snapshots go out, commands come in over a channel (PROJECT.md §6). That
//! leaves two things: the numbers are the same, and the reader never blocks.
//!
//! The numbers must match the single-threaded run of J3 **bitwise**, and that
//! is not self-evident: the thread measures its own `dt`, spins on its own
//! tick and does a different amount of work per pass. J2 already declared all
//! of that safe; here it is checked in the form it will actually run in.

use std::time::{Duration, Instant};

use game::clock::{Stall, MAX_WARP};
use game::mission;
use game::plan::{Frame, Manoeuvre, Plan};
use game::sim::{Command, Event, Sim};
use game::snapshot::WorldSnapshot;
use game::world::{PlanRejected, VesselId};

const DAY: f64 = 86400.0;

/// How long to wait on the thread before calling the test broken.
///
/// The mission at maximum warp takes about a second; ten means "something
/// stopped", not "the machine is slow".
const PATIENCE: Duration = Duration::from_secs(10);

fn spawn(demo_plan: bool) -> Sim {
    let build = if demo_plan {
        mission::world_with_demo_plan
    } else {
        mission::world
    };
    Sim::spawn(build(&mission::default_asset()).expect("world")).expect("the thread starts")
}

/// Spins the thread at maximum warp until the mission ends.
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
            "the thread did not finish the mission in {PATIENCE:?}: day {:.2}, stall {:?}",
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

/// The main check of J4: the same trajectory, down to the last bit.
#[test]
fn the_thread_computes_what_one_thread_computes() {
    let threaded = samples_of(&run_to_end(&spawn(true)));

    // The oracle is the same world computed on this thread, with no channels
    // and no publications.
    let mut world = mission::world_with_demo_plan(&mission::default_asset()).expect("world");
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
        "{} samples from the thread against {} single-threaded",
        threaded.len(),
        plain.len()
    );
    assert!(
        threaded.len() > 1000,
        "too few samples to prove anything with"
    );

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
                "sample {i}, {name}: {p:e} against {q:e}"
            );
        }
    }
}

/// The reader never waits for the writer.
///
/// This is the whole reason snapshots are `arc-swap` and not a `Mutex`: under
/// a mutex the frame would wait for the simulation tick, and 60 fps would
/// hold exactly until the thread took on a long leg.
///
/// The measurement runs with the thread saturated (maximum warp), i.e. under
/// the conditions in which a mutex would start blocking.
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
        // The snapshot really is read, not optimised away.
        assert!(snapshot.t.is_finite());
    }

    println!("  {reads} reads, worst {worst:?}");

    // The threshold is deliberately generous: on a loaded CI runner the
    // scheduler can take the thread away at any moment, and the test should
    // catch blocking, not the scheduler. In practice this is tens of
    // nanoseconds.
    assert!(
        worst < Duration::from_millis(50),
        "the longest snapshot read took {worst:?} -- the reader waits for someone"
    );
    assert!(
        reads > 1000,
        "too few reads for the measurement to mean much"
    );
}

/// A command is answered by an event, and by the right one.
///
/// The channel back exists for the discrete: a snapshot could not say that a
/// plan was rejected -- it would simply show that nothing changed.
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

    // A manoeuvre at the start instant is in the past: the cursor stands
    // exactly there.
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
            "the thread did not answer in {PATIENCE:?}: {seen:?}"
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
        "the first answer should have been about the accepted plan: {:?}",
        seen[0]
    );
    assert_eq!(
        seen[1],
        Event::PlanRejected {
            vessel: VesselId(0),
            why: PlanRejected::InThePast
        },
        "a manoeuvre at the cursor instant should have been rejected"
    );
}

/// Pause sent over the channel arrives and stops the cursor.
#[test]
fn pause_reaches_the_thread_and_stops_the_cursor() {
    let sim = spawn(false);
    sim.send(Command::SetWarp(MAX_WARP));

    // Give it time to move, so that "standing" does not mean "not started".
    let deadline = Instant::now() + PATIENCE;
    while sim.snapshot().t <= mission::start().t {
        assert!(Instant::now() < deadline, "the cursor never moved");
        std::thread::yield_now();
    }

    sim.send(Command::TogglePause);

    // Wait for the pause to arrive.
    let deadline = Instant::now() + PATIENCE;
    while sim.snapshot().stall != Some(Stall::Paused) {
        assert!(Instant::now() < deadline, "the pause did not arrive");
        std::thread::yield_now();
    }

    let stopped = sim.snapshot().t;
    std::thread::sleep(Duration::from_millis(50));
    let still = sim.snapshot();

    assert_eq!(
        still.t.to_bits(),
        stopped.to_bits(),
        "the cursor moved while paused: {} -> {}",
        stopped,
        still.t
    );
    assert_eq!(still.stall, Some(Stall::Paused));
}

/// The thread stops together with the handle.
///
/// Without this a process with a closed window would keep a live thread
/// computing away; it would only show up in the task manager.
#[test]
fn dropping_the_handle_stops_the_thread() {
    let sim = spawn(false);
    sim.send(Command::SetWarp(MAX_WARP));

    let deadline = Instant::now() + PATIENCE;
    while sim.snapshot().t <= mission::start().t {
        assert!(Instant::now() < deadline, "the cursor never moved");
        std::thread::yield_now();
    }

    // `Drop` sends Shutdown and joins the thread. If it never exited the test
    // would not fail but hang -- which is why the whole test has a patience
    // ceiling.
    let at = Instant::now();
    drop(sim);
    let took = at.elapsed();

    println!("  the thread stopped in {took:?}");
    assert!(
        took < Duration::from_secs(1),
        "the thread took {took:?} to exit -- Shutdown does not wake it"
    );
}
