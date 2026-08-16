//! The simulation thread (ROADMAP J4, PROJECT.md §6).
//!
//! The world moves into its own thread, and the world's code does not change
//! by a single line: [`world::World::step`](crate::world::World::step) was an
//! ordinary function and stayed one. Only **who** calls it changes -- which is
//! exactly why J1-J3 were done single-threaded.
//!
//! ## Two primitives, and there is no third
//!
//! - **Channel** ([`Command`]) -- everything the main thread wants to do to
//!   the world.
//! - **Publication** ([`arc_swap`]) -- everything it wants to know about the
//!   world.
//!
//! There is no shared mutable state at all, so a race here is impossible not
//! because we are careful but because there is nothing to lock. A reader never
//! blocks a writer: `ArcSwap::load_full` is an atomic pointer exchange, not a
//! wait.
//!
//! ## Why events also travel by channel
//!
//! An [`Event`] is something that happened **once**: a plan accepted, a plan
//! rejected, a vessel hitting the asset's limit. A snapshot does not carry
//! such things: it is a sample, and a reader that missed a publication would
//! miss an event forever (CLAUDE.md, invariant 8).
//!
//! ## What the thread does NOT change
//!
//! The numbers. The thread measures its own `dt` and spins on its own tick,
//! i.e. does exactly what J2 already checked as safe: it changes cursor speed
//! and the amount of work per pass, never `t_end`. There is a check for that
//! (`tests/thread.rs`): the threaded trajectory equals the single-threaded one
//! bitwise.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use core_rs::CoreError;
use crossbeam_channel::{Receiver, Sender};

use crate::plan::Plan;
use crate::save;
use crate::snapshot::WorldSnapshot;
use crate::world::{PlanRejected, VesselId, World};

/// The simulation tick period.
///
/// Twice per frame at 60 Hz: a snapshot need not be fresher than a frame but
/// must not be staler. This is not a physics step -- there is no such thing
/// (`crate::clock`) -- only how often the thread wakes on its own. A command
/// wakes it immediately.
const TICK: Duration = Duration::from_millis(8);

/// How many legs may be computed in one tick.
///
/// A latency ceiling, not an optimisation: the larger the number, the longer
/// the thread goes without looking at the command channel. Does not affect the
/// numbers (invariant 9).
const LEGS_PER_TICK: usize = 4;

/// Ceiling on one tick's `dt`. The same reason as in `app`: a process the
/// system suspended for a minute should not wake holding a minute x warp.
const MAX_TICK_DT: f64 = 0.25;

/// What the main thread asks of the simulation.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    SetWarp(f64),
    ScaleWarp(f64),
    TogglePause,
    /// Move the cursor to an already computed instant (ROADMAP-UI.md, U3b).
    ///
    /// Not a `t_end` and not a request to compute: if nothing is computed
    /// there yet, the command is **rejected** (`Event::SeekRejected`).
    /// Otherwise a text field in the interface would decide how much to
    /// integrate -- directly against invariant 9.
    SeekTo(f64),
    CommitPlan {
        vessel: VesselId,
        plan: Plan,
    },
    /// Write a save. The **simulation thread** writes, because the world
    /// belongs to it; the main thread receives an [`Event::Saved`].
    Save(PathBuf),
    Shutdown,
}

/// What the simulation reports back. Discrete things -- what a snapshot does
/// not carry.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The plan was accepted; `from` is the instant recomputed from.
    PlanCommitted {
        vessel: VesselId,
        from: Option<f64>,
    },
    /// The seek was not accepted -- backwards, or into the uncomputed.
    SeekRejected {
        t: f64,
        why: crate::world::SeekRejected,
    },
    PlanRejected {
        vessel: VesselId,
        why: PlanRejected,
    },
    /// The vessel stopped being computed. The most likely cause is time
    /// leaving the asset's span.
    VesselFailed {
        vessel: VesselId,
        error: CoreError,
    },
    /// The save was written, or was not -- and then why is stated.
    Saved {
        error: Option<String>,
    },
}

/// A handle to the simulation thread.
///
/// Owns the thread: [`Drop`] asks it to stop and waits. Without that a process
/// whose window has closed would keep a live thread still computing.
pub struct Sim {
    commands: Sender<Command>,
    events: Receiver<Event>,
    published: Arc<ArcSwap<WorldSnapshot>>,
    /// The asset the world already loaded.
    ///
    /// Kept here so the planner can share it rather than read it a second time
    /// (`crate::planner`). `Ephemeris` is `Sync`, proved by reading the C in
    /// D3, when no second thread existed yet.
    eph: Arc<core_rs::Ephemeris>,
    thread: Option<JoinHandle<()>>,
}

impl Sim {
    /// Takes a finished world and gives it a thread.
    ///
    /// The world is built **outside**, in the calling thread: an error loading
    /// the asset or the save must reach whoever can display it rather than
    /// kill a thread nobody knows about yet.
    pub fn spawn(mut world: World) -> Result<Sim, String> {
        let eph = world.ephemeris();
        let published = Arc::new(ArcSwap::from_pointee(world.snapshot()));
        let (commands, command_rx) = crossbeam_channel::unbounded();
        let (event_tx, events) = crossbeam_channel::unbounded();

        let publish = published.clone();
        let thread = std::thread::Builder::new()
            .name("sim".to_string())
            .spawn(move || run(&mut world, &command_rx, &event_tx, &publish))
            .map_err(|e| format!("the simulation thread did not start: {e}"))?;

        Ok(Sim {
            commands,
            events,
            published,
            eph,
            thread: Some(thread),
        })
    }

    /// The world's current slice.
    ///
    /// **One call per frame, and hold the result for the whole frame.** Two
    /// calls in a row can give two different "now"s, and then the camera looks
    /// at one instant while the trajectory is drawn from another.
    pub fn snapshot(&self) -> Arc<WorldSnapshot> {
        self.published.load_full()
    }

    pub fn ephemeris(&self) -> Arc<core_rs::Ephemeris> {
        self.eph.clone()
    }

    pub fn send(&self, command: Command) {
        // The channel closes only with the thread, i.e. in `Drop`. An error
        // here would mean the thread died; the world is not corrupted by that,
        // and there is nobody to tell -- there is no UI yet.
        let _ = self.commands.send(command);
    }

    /// Takes every event that has accumulated. Does not block.
    pub fn events(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
}

impl Drop for Sim {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The thread's loop.
///
/// Wakes on either a command or a tick -- `select!` exists for the first:
/// pausing on a space bar press should not wait for the period to end.
fn run(
    world: &mut World,
    commands: &Receiver<Command>,
    events: &Sender<Event>,
    published: &ArcSwap<WorldSnapshot>,
) {
    let ticker = crossbeam_channel::tick(TICK);
    let mut last = Instant::now();
    let mut reported_failure = vec![false; world.vessels().len()];

    loop {
        crossbeam_channel::select! {
            recv(commands) -> command => {
                match command {
                    // The sender disappeared with `Sim` -- exit exactly as on
                    // Shutdown.
                    Err(_) => return,
                    Ok(Command::Shutdown) => return,
                    Ok(command) => apply(world, command, events),
                }
            }
            recv(ticker) -> _ => {}
        }

        // The rest of the commands that accumulated, so a burst of key presses
        // does not stretch across a burst of ticks.
        while let Ok(command) = commands.try_recv() {
            if command == Command::Shutdown {
                return;
            }
            apply(world, command, events);
        }

        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f64();
        last = now;

        world.step(dt.min(MAX_TICK_DT), LEGS_PER_TICK);

        // A breakage is reported once per vessel: the channel must not become
        // a stream of the same message every tick.
        reported_failure.resize(world.vessels().len(), false);
        for (index, vessel) in world.vessels().iter().enumerate() {
            if let Some(error) = vessel.failed {
                if !reported_failure[index] {
                    reported_failure[index] = true;
                    let _ = events.send(Event::VesselFailed {
                        vessel: vessel.id,
                        error,
                    });
                }
            } else {
                reported_failure[index] = false;
            }
        }

        published.store(Arc::new(world.snapshot()));
    }
}

fn apply(world: &mut World, command: Command, events: &Sender<Event>) {
    match command {
        Command::SetWarp(warp) => world.clock_mut().set_warp(warp),
        Command::ScaleWarp(factor) => world.clock_mut().scale_warp(factor),
        Command::TogglePause => world.clock_mut().toggle_pause(),
        Command::SeekTo(t) => {
            if let Err(why) = world.seek_to(t) {
                let _ = events.send(Event::SeekRejected { t, why });
            }
        }
        Command::Save(path) => {
            let error = save::write_world(world, &path).err();
            let _ = events.send(Event::Saved { error });
        }
        Command::CommitPlan { vessel, plan } => {
            let event = match world.commit_plan(vessel, plan) {
                Ok(from) => Event::PlanCommitted { vessel, from },
                Err(why) => Event::PlanRejected { vessel, why },
            };
            let _ = events.send(event);
        }
        // Handled above: it does not reach here.
        Command::Shutdown => {}
    }
}
