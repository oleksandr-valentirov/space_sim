//! The planner thread: speculative runs (ROADMAP J5, PROJECT.md §6).
//!
//! PROJECT.md §6 calls this thread `Planner`, and its split is not "physics
//! against prediction" but **committed against speculative**. Prediction and
//! physics are one integration (§4, rule 5), so two threads integrating one
//! vessel would be the same vessel twice. But "show what happens if the burn
//! is here" is a different task: its result can be discarded, it can be
//! cancelled halfway, and it writes nothing into the world.
//!
//! ## The promise all of this exists for
//!
//! > The line you saw is the line you will fly.
//!
//! That is, a preview must be **bitwise** what `Sim` computes afterwards. It
//! does not follow on its own: starting a run from the wrong point, or with
//! "choose a step yourself", is enough for the preview to diverge from the
//! flight (H1 measured by how much).
//!
//! So there is no segment loop of its own here. The planner builds an ordinary
//! [`World`] on the same ephemeris, with the same `PropConfig`, and calls the
//! same `step`. Both take the restart point from one function
//! ([`crate::leg::restart_at`]). Shared code is not a saving but the promise
//! itself: two implementations would diverge, and would diverge
//! imperceptibly.
//!
//! ## Cancellation
//!
//! The player drags a manoeuvre node -- requests fly by the dozen per second,
//! and all but the last are of use to nobody. The thread abandons work
//! **between legs** as soon as a newer request appears in the channel: waiting
//! for the end of a run nobody is asking about would mean lagging the mouse by
//! exactly one run.

use std::sync::Arc;
use std::thread::JoinHandle;

use core_rs::{Ephemeris, PropConfig, State};
use crossbeam_channel::{Receiver, Sender, TryRecvError};

use crate::leg::Leg;
use crate::plan::Plan;
use crate::porkchop::{self, Grid, GridRequest};
use crate::world::{VesselId, World};

/// How many legs to compute between channel checks.
///
/// One: a leg is already the unit of work, and making cancellation coarser
/// would mean answering a whole leg late for no gain at all.
const LEGS_PER_CHECK: usize = 1;

/// What the thread is asked for.
///
/// Two kinds of work, **one channel**, which is exactly why the cancellation
/// rule stays single: a newer request cancels whatever is being computed,
/// whatever kind it is. The consequence is stated honestly -- asking for a
/// grid and immediately dragging a node means going without the grid. That is
/// the same "only the last one arrives" as for previews rather than a special
/// case; two channels would instead require deciding which kind of work
/// outranks the other, and we have no answer to that.
#[derive(Debug, Clone)]
pub enum Request {
    /// Show what happens if this plan is flown.
    Preview(PreviewRequest),
    /// Compute the transfer window grid (ROADMAP-UI.md, U5b).
    Grid(GridRequest),
}

/// What to compute.
///
/// `from` and `step` are the restart point computed by
/// [`crate::leg::restart_at`] from a snapshot. It is where `Sim` recomputes
/// the tail from once the plan is committed -- and exactly why the preview
/// must start from there rather than from "where the vessel is now".
#[derive(Debug, Clone)]
pub struct PreviewRequest {
    pub id: u64,
    pub vessel: VesselId,
    pub from: State,
    pub step: f64,
    pub plan: Plan,
    /// The vessel's mission end -- the same one the world holds.
    pub horizon_end: f64,
    /// The vessel as the force model sees it (K6b). Must be the same one as
    /// in the world: a preview with a different area is a line the vessel will
    /// not fly, i.e. exactly what this thread has no right to show.
    pub params: Option<core_rs::VesselParams>,
}

/// A computed prediction. Not world state: nobody put it anywhere.
pub struct Preview {
    pub id: u64,
    pub vessel: VesselId,
    pub plan: Plan,
    pub legs: Vec<Arc<Leg>>,
}

pub struct Planner {
    /// An `Option` for [`Drop`]'s sake: for the thread to exit, the sender
    /// must be **destroyed** rather than merely stop being used.
    requests: Option<Sender<Request>>,
    previews: Receiver<Preview>,
    /// Replies of the second kind. A separate channel although the requests
    /// share one: **order matters only for requests** -- it is what decides
    /// what to cancel. The replies are read by different code in different
    /// places of the frame, and a shared queue would force each reader to sift
    /// through the other's.
    grids: Receiver<Grid>,
    thread: Option<JoinHandle<()>>,
}

impl Planner {
    /// Starts the thread on **the same** ephemeris as the world.
    ///
    /// It can be shared because `Ephemeris` is `Sync`, proved by reading the C
    /// back in D3, long before it was needed. Each thread has its own
    /// propagator instead: it is `Send` but not `Sync`.
    pub fn spawn(eph: Arc<Ephemeris>, cfg: PropConfig) -> Result<Planner, String> {
        let (requests, request_rx) = crossbeam_channel::unbounded::<Request>();
        let (preview_tx, previews) = crossbeam_channel::unbounded();
        let (grid_tx, grids) = crossbeam_channel::unbounded();

        let thread = std::thread::Builder::new()
            .name("planner".to_string())
            .spawn(move || run(&eph, cfg, &request_rx, &preview_tx, &grid_tx))
            .map_err(|e| format!("the planner thread did not start: {e}"))?;

        Ok(Planner {
            requests: Some(requests),
            previews,
            grids,
            thread: Some(thread),
        })
    }

    pub fn request(&self, request: Request) {
        if let Some(requests) = &self.requests {
            let _ = requests.send(request);
        }
    }

    /// The freshest preview, if there is one. Older ones are discarded.
    ///
    /// Discarded **here** rather than in the thread: the thread does not know
    /// which request the caller considers current, while the caller does -- the
    /// last one it sent.
    pub fn latest(&self) -> Option<Preview> {
        self.previews.try_iter().last()
    }

    /// The freshest grid, if there is one. The same reasoning as
    /// [`Self::latest`].
    pub fn latest_grid(&self) -> Option<Grid> {
        self.grids.try_iter().last()
    }
}

impl Drop for Planner {
    fn drop(&mut self) {
        // A closed request channel is the exit signal: the thread has nobody
        // to expect work from. No separate `Shutdown` command is needed here,
        // because unlike `Sim` the planner does nothing without a request.
        self.requests = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// How one unit of work ended.
///
/// Not an `Option`, precisely because "cancelled" must **carry the request
/// that cancelled**: otherwise it is lost, and the newest one is exactly what
/// is lost -- see
/// [`Outcome::Cancelled`].
enum Outcome {
    /// A prediction was computed.
    Ready(Preview),
    /// A grid was computed.
    Sheet(Grid),
    /// The run did not start (the world did not build). There will be no
    /// reply, but there is nothing to wait for either -- this is not a
    /// cancellation.
    Nothing,
    /// Cancelled, and here is the request that cancelled it.
    ///
    /// It is returned upwards rather than discarded, and that is no small
    /// thing: checking the channel **removes** the message, so discarding it
    /// would lose exactly the last mouse movement -- the one after which the
    /// player looks at the screen. The thread would then fall asleep on
    /// `recv()` with a stale preview on screen, waiting for a request nobody
    /// will send.
    Cancelled(Request),
    /// The request channel is closed -- exit.
    Gone,
}

fn run(
    eph: &Arc<Ephemeris>,
    cfg: PropConfig,
    requests: &Receiver<Request>,
    previews: &Sender<Preview>,
    grids: &Sender<Grid>,
) {
    let Ok(first) = requests.recv() else {
        return;
    };
    let mut pending = Some(first);

    while let Some(request) = pending.take() {
        // While we computed (or slept) several more may have arrived. The
        // current one is the last.
        let mut request = request;
        while let Ok(newer) = requests.try_recv() {
            request = newer;
        }

        let outcome = match &request {
            Request::Preview(ask) => compute(eph, cfg, ask, requests),
            Request::Grid(ask) => sweep(eph, ask, requests),
        };

        match outcome {
            Outcome::Ready(preview) => {
                if previews.send(preview).is_err() {
                    return;
                }
                pending = requests.recv().ok();
            }
            Outcome::Sheet(grid) => {
                if grids.send(grid).is_err() {
                    return;
                }
                pending = requests.recv().ok();
            }
            Outcome::Nothing => pending = requests.recv().ok(),
            // That is what we compute next, rather than waiting for
            // another.
            Outcome::Cancelled(newer) => pending = Some(newer),
            Outcome::Gone => return,
        }
    }
}

/// Computes the window grid, abandoning work between rows.
///
/// A row is the unit of work here, as a leg is for a prediction, and for the
/// same reason: 0.24 ms against 22 ms for the whole grid (measured in
/// `crate::porkchop`). Cutting finer is pointless, coarser would mean waiting
/// for a grid nobody is asking about any more.
fn sweep(eph: &Arc<Ephemeris>, request: &GridRequest, requests: &Receiver<Request>) -> Outcome {
    if request.depart.is_empty() || request.tof.is_empty() {
        // An empty axis is not a grid of zero cells but a request about
        // nothing. There is nothing to draw, and silence is more honest than
        // an empty plot.
        return Outcome::Nothing;
    }

    let mut cells = Vec::with_capacity(request.depart.len() * request.tof.len());
    for i in 0..request.depart.len() {
        match requests.try_recv() {
            Ok(newer) => return Outcome::Cancelled(newer),
            Err(TryRecvError::Disconnected) => return Outcome::Gone,
            Err(TryRecvError::Empty) => {}
        }
        cells.extend(porkchop::row(eph, request, i));
    }

    Outcome::Sheet(Grid {
        id: request.id,
        t1: request.t1(),
        tof: request.tof.clone(),
        cells,
    })
}

/// Computes a prediction, abandoning work as soon as a newer request
/// arrives.
fn compute(
    eph: &Arc<Ephemeris>,
    cfg: PropConfig,
    request: &PreviewRequest,
    requests: &Receiver<Request>,
) -> Outcome {
    // A perfectly ordinary world. The same code, the same `step`, the same
    // `PropConfig` -- which is exactly why the result matches bitwise what
    // `Sim` will compute.
    let Ok(mut world) = World::with_ephemeris(eph.clone(), cfg, request.from.t, 1.0) else {
        return Outcome::Nothing;
    };
    // A speculative world lives a few legs and disappears -- retirement here
    // would only manage to pay for the pass (N3a).
    world.set_history_trimming(None);
    let vessel = world.add_planned_vessel(
        "preview",
        request.from,
        request.step,
        request.horizon_end,
        request.plan.clone(),
        request.params,
    );

    loop {
        match requests.try_recv() {
            // A newer request or a vanished channel -- this result is no
            // longer needed. The newer one goes upwards, not into the bin.
            Ok(newer) => return Outcome::Cancelled(newer),
            Err(TryRecvError::Disconnected) => return Outcome::Gone,
            Err(TryRecvError::Empty) => {}
        }

        // The cursor is not moved (`dt = 0`): a preview is a prediction, not
        // a flight.
        let done = world.step(0.0, LEGS_PER_CHECK);
        if done.legs == 0 {
            break;
        }
    }

    let vessel = &world.vessels()[vessel.0 as usize];
    Outcome::Ready(Preview {
        id: request.id,
        vessel: request.vessel,
        plan: request.plan.clone(),
        legs: vessel.trajectory.share(),
    })
}
