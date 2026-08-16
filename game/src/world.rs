//! The world: vessels, their trajectories and whoever computes them (J1).
//!
//! Single-threaded, deliberately. A thread added to an unsolved problem does
//! not solve it but doubles the cost of diagnosis; [`World::tick`] here is as
//! ordinary a function as any other, and J4 only moves its call into its own
//! thread (PROJECT.md §6).
//!
//! ## What a tick does
//!
//! Pulls the horizon forward -- exactly as many legs as it was allowed, round
//! robin between vessels so one does not starve the rest. How many legs fit
//! depends on the machine and the frame; **where** they end does not. That is
//! the whole point: `t_end` comes from the mission rather than a clock
//! (CLAUDE.md, invariant 9), and the sample buffer is fixed, so the sequence
//! of legs is the same on any machine.
//!
//! There is no time cursor here yet -- that is J2. For now the horizon simply
//! grows to the mission's end, and that suffices to check J1's main claim: the
//! game types change no bit against a direct run (`tests/trajectory.rs`).

use std::path::Path;
use std::sync::Arc;

use core_rs::{CoreError, Ephemeris, PropConfig, Propagator, State, VesselParams};

use crate::clock::{Clock, Stall};
use crate::leg::{Leg, Sample, Trajectory};
use crate::plan::Plan;
use crate::snapshot::{BodySnapshot, VesselSnapshot, WorldSnapshot};

/// Body indices in cooker order (`core/cook/cook_fixture.c`).
pub const SUN: i32 = 0;
pub const EARTH: i32 = 3;
pub const MOON: i32 = 4;

/// How many samples one `prop_run` call takes.
///
/// The number is not optimised and need not be: H1 proved it does not affect
/// the trajectory at all -- stitched legs equal one run bitwise. It affects
/// only the granularity at which work can be deferred, and the size of the
/// smallest chunk published in a snapshot.
///
/// Deliberately **not** 64 as in `engine::live`: the J1 test compares two
/// trajectories with different leg sizes, and the equality is bitwise.
/// Identical numbers here would make that check a tautology.
pub const LEG: usize = 256;

/// How many prediction legs to keep ahead of the cursor.
///
/// This is the whole horizon policy, and it is in **legs** rather than seconds
/// -- otherwise `t_end` would derive from time and the frame rate would flow
/// into the numbers (CLAUDE.md, invariant 9). How many days that is depends on
/// how densely the integrator places steps: on this orbit a 256-sample leg is
/// about eleven days, so four legs give a month of visible prediction.
///
/// Changing this number is safe: it decides how much prediction exists, never
/// what its numbers are.
pub const LEAD_LEGS: usize = 4;

/// How many legs behind the cursor keep all their raw samples (N3a).
///
/// In legs rather than days: the leg is the unit of everything (CLAUDE.md),
/// and in low orbit it covers a day and a half while on a lunar transfer it
/// covers eleven. A window in legs adapts to the regime itself; a window in
/// days does not.
///
/// Four -- as many as [`LEAD_LEGS`] ahead: the window around the cursor is
/// symmetric, and neither side has grounds to be wider.
pub const RAW_LEGS_BEHIND: usize = 4;

/// How many revolutions behind the cursor stay in history (N5a, decision Q4).
///
/// **Revolutions, not days and not megabytes.** A day is misleading -- 5100
/// samples in LEO against 720 on a lunar transfer -- while in revolutions the
/// window adapts to the regime itself. Megabytes are something the player
/// cannot relate to anything; they think in revolutions.
///
/// Twenty -- so the trail reads as a trail rather than a segment, and so it is
/// a day and a bit in low orbit. This is a **default value**, not a ceiling:
/// the window lives in a world field (`World::set_history_revolutions`),
/// because one day the player will turn it. How much memory that is is stated
/// by `Trajectory::history_bytes`, and that number should stand beside the
/// choice in the interface.
///
/// WARNING: **a one-way door** (invariant 5): a discarded leg does not come
/// back, so this is not a performance setting but a choice of how much past
/// the player wants to see.
pub const HISTORY_REVOLUTIONS: f64 = 20.0;

/// The tolerance a leg retires with, metres (N3a).
///
/// **Derived rather than chosen**, and derived from the scale the map opens
/// at: `mission::CAMERA_ALTITUDE_M` = 1e9 m, `focal_px` at 720p is 623 pixels
/// per radian, so half a pixel is `1e9 * 0.5 / 623 ~ 8e5` m.
///
/// WARNING: **a one-way door, and here is where it shows.** Approaching the
/// old past closer than 1e9 m, the player will see chords instead of arcs --
/// the samples between them are gone and will not return (invariant 5). That
/// is the price of assumption Q4, and exactly why the number lives here with
/// its derivation rather than in a function body.
pub const RETIRE_TOL_M: f64 = 8.0e5;

/// A vessel's index in `Vec<Vessel>` (CLAUDE.md: indices instead of
/// references).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VesselId(pub u32);

pub struct Vessel {
    pub id: VesselId,
    pub name: String,

    /// The state to continue from. The end of what is computed, not "now".
    pub tip: State,
    /// The integrator step to continue with. Goes into the save
    /// (PROJECT.md §4).
    pub tip_step: f64,
    /// The mission's end: nothing is computed beyond it.
    pub horizon_end: f64,

    /// The manoeuvre plan. Empty means free flight.
    pub plan: Plan,
    /// How many of the plan's manoeuvres are already baked into the
    /// trajectory.
    ///
    /// An index rather than a time: comparing times would need a precision
    /// floating point does not have, while a counter says it unambiguously.
    /// After a cascade recomputation it is recounted from zero
    /// ([`World::commit_plan`]).
    pub applied: usize,

    pub trajectory: Trajectory,

    /// Area, mass and reflectivity -- everything radiation pressure needs
    /// (ROADMAP K6b). `None` is a massless test particle, as before K6b.
    ///
    /// Set at creation and unchanged afterwards. Not forgetfulness: changing
    /// the force model on the fly would turn the already computed part of the
    /// prediction into a trajectory the vessel will not fly, i.e. would demand
    /// the cascade recomputation a plan edit performs. A vessel's area does
    /// not change, and its mass changes while burning, which the impulsive
    /// manoeuvre model does not have.
    pub params: Option<VesselParams>,

    /// Why the horizon stopped growing. The core returns errors as codes, and
    /// the worst thing to do with them is panic: the world stays valid, this
    /// vessel simply stops being computed.
    pub failed: Option<CoreError>,
}

impl Vessel {
    /// Whether there is anything left to compute with the cursor at `cursor`.
    ///
    /// Three conditions, each disabling work for a different reason: the
    /// vessel broke, the mission ended, or the prediction already reaches far
    /// enough ahead.
    fn wants_work(&self, cursor: f64) -> bool {
        self.failed.is_none()
            && self.tip.t < self.horizon_end
            && self.trajectory.legs_after(cursor) < LEAD_LEGS
    }

    /// How far the next leg integrates to.
    ///
    /// The next unapplied manoeuvre or the mission's end -- and never the
    /// cursor (CLAUDE.md, invariant 9). This is where a plan becomes a
    /// sequence of `prop_run` calls: each segment between manoeuvres is walked
    /// in legs, and the segment boundary becomes `t_end`.
    fn next_boundary(&self) -> f64 {
        match self.plan.get(self.applied) {
            Some(m) if m.t < self.horizon_end => m.t,
            _ => self.horizon_end,
        }
    }

    /// How far it is computed.
    fn computed_to(&self) -> f64 {
        self.trajectory.computed_to()
    }
}

pub struct World {
    eph: Arc<Ephemeris>,
    /// One propagator per configuration rather than per vessel: the C context
    /// holds settings while the vessel's state is ours (`core/prop.h`). It is
    /// `Send` but not `Sync`, so it belongs to exactly one thread -- the one
    /// calling `tick`.
    prop: Propagator,
    vessels: Vec<Vessel>,
    /// The time cursor. Written only here (PROJECT.md §6).
    clock: Clock,
    /// How many times the world changed. A snapshot's reader sees from it that
    /// the picture is new without comparing contents.
    version: u64,
    /// How many legs were computed in total. Not statistics: this measures the
    /// cost of cascade recomputation (`tests/plan.rs`).
    legs_computed: u64,
    /// How many legs behind the cursor stay raw (`set_history_trimming`).
    retire_behind: Option<usize>,
    /// How many revolutions of history to keep (`set_history_revolutions`).
    history_revolutions: f64,
}

/// Why the plan was not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanRejected {
    NoSuchVessel,
    /// The change touches an instant the cursor has already passed.
    ///
    /// Not a convenience restriction but what the inviolability of history
    /// rests on: only the future may be edited, so only prediction legs are
    /// rewritten (PROJECT.md §6).
    InThePast,
}

/// Why a seek was not accepted (ROADMAP-UI.md, U3b).
///
/// A refusal is a refusal rather than silent ignoring: rule 8 of stage U
/// requires the panel to show the answer rather than its own assumption of
/// success.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SeekRejected {
    /// The cursor never goes backwards (stage J).
    Backwards,
    /// Nothing is computed that far yet; `computed_to` says how far is
    /// possible.
    NotComputedYet { computed_to: f64 },
}

/// What one tick did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tick {
    /// How many legs were computed.
    pub legs: usize,
    /// Whether work remains: some vessel's horizon has not reached the end.
    pub pending: bool,
    /// How many samples retired on this tick (N3a).
    pub retired: usize,
    /// How many samples left the window and disappeared on this tick (N5a).
    pub dropped: usize,
}

impl World {
    pub fn new(asset: &Path, cfg: PropConfig, epoch: f64, warp: f64) -> Result<World, CoreError> {
        World::with_ephemeris(Arc::new(Ephemeris::load(asset)?), cfg, epoch, warp)
    }

    /// The same, but on an already loaded ephemeris.
    ///
    /// The planner needs it (J5): its speculative world shares the asset with
    /// the real one. Sharing is possible because `Ephemeris` is `Sync`, and
    /// that is read C rather than an assumption (`core-rs`, D3): after
    /// `eph_load` the context does not change at all.
    pub fn with_ephemeris(
        eph: Arc<Ephemeris>,
        cfg: PropConfig,
        epoch: f64,
        warp: f64,
    ) -> Result<World, CoreError> {
        let prop = Propagator::new(eph.clone(), cfg)?;

        Ok(World {
            eph,
            prop,
            vessels: Vec::new(),
            clock: Clock::new(epoch, warp),
            version: 0,
            legs_computed: 0,
            retire_behind: Some(RAW_LEGS_BEHIND),
            history_revolutions: HISTORY_REVOLUTIONS,
        })
    }

    /// Whether to touch the past at all, and how many legs behind the cursor
    /// stay raw (N3a, N5a).
    ///
    /// One knob for two actions, because they are one policy: the
    /// [`HISTORY_REVOLUTIONS`] window **discards** legs older than itself,
    /// while retirement **thins** what remains further than `behind_legs` legs
    /// from the cursor. `None` disables both.
    ///
    /// A policy rather than a setting, and it has two legitimate values in the
    /// project itself. The game trims: otherwise memory grows the way D7 says.
    /// Two callers do not -- the planner's speculative world, which lives a few
    /// legs and would only manage to pay for the pass, and any check comparing
    /// the **stream of samples** against an independent run: trimming changes
    /// what such a check compares without changing one bit of what was
    /// computed.
    pub fn set_history_trimming(&mut self, behind_legs: Option<usize>) {
        self.retire_behind = behind_legs;
    }

    /// How many revolutions of past stay in history (N5a, decision Q4).
    ///
    /// WARNING: decreasing is a one-way door: discarded legs do not come back
    /// (invariant 5). Increasing affects only the future.
    pub fn set_history_revolutions(&mut self, revolutions: f64) {
        self.history_revolutions = revolutions;
    }

    pub fn ephemeris(&self) -> Arc<Ephemeris> {
        self.eph.clone()
    }

    pub fn add_vessel(
        &mut self,
        name: &str,
        start: State,
        horizon_end: f64,
        params: Option<VesselParams>,
    ) -> VesselId {
        // Zero means "choose one yourself" only on the first call; afterwards
        // what the previous one left is carried over (`core/prop.h`).
        self.add_planned_vessel(name, start, 0.0, horizon_end, Plan::new(), params)
    }

    /// A vessel continuing someone else's flight: with a given step and an
    /// already given plan.
    ///
    /// This is the planner's path (J5). `step` here is not cosmetic: a
    /// prediction starting from "choose one yourself" is a different
    /// trajectory than a continuation with the carried step (H1) -- exactly
    /// what a preview has no right to show.
    pub fn add_planned_vessel(
        &mut self,
        name: &str,
        start: State,
        step: f64,
        horizon_end: f64,
        plan: Plan,
        params: Option<VesselParams>,
    ) -> VesselId {
        let id = VesselId(self.vessels.len() as u32);
        self.vessels.push(Vessel {
            id,
            name: name.to_string(),
            tip: start,
            tip_step: step,
            horizon_end,
            plan,
            applied: 0,
            trajectory: Trajectory::new(start),
            params,
            failed: None,
        });

        let index = self.vessels.len() - 1;
        bake_applied(&self.eph, &mut self.vessels[index]);

        self.version += 1;
        id
    }

    /// A vessel from a save: everything given explicitly, nothing derived.
    ///
    /// The main difference from [`World::add_planned_vessel`] is that
    /// `applied` is **taken** rather than computed. A manoeuvre exactly at
    /// `tip`'s instant is already applied (its dv is in `tip`), but the
    /// numbers do not show that: the states before and after an impulse share
    /// a time. Deriving it here would mean executing the manoeuvre a second
    /// time on every load (`crate::save`).
    ///
    /// Takes a [`crate::save::SavedVessel`] whole rather than seven arguments.
    /// That struct already describes exactly what must be restored, and two
    /// field lists side by side would diverge silently -- which is exactly how
    /// `params` (K6b) would have been lost on load.
    pub fn add_saved_vessel(&mut self, saved: crate::save::SavedVessel) -> VesselId {
        let id = VesselId(self.vessels.len() as u32);
        self.vessels.push(Vessel {
            id,
            name: saved.name,
            tip: saved.tip,
            tip_step: saved.step,
            horizon_end: saved.horizon_end,
            plan: saved.plan,
            applied: saved.applied,
            trajectory: Trajectory::new(saved.tip),
            params: saved.params,
            failed: None,
        });
        self.version += 1;
        id
    }

    pub fn vessels(&self) -> &[Vessel] {
        &self.vessels
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// How many legs were computed over the world's whole life.
    pub fn legs_computed(&self) -> u64 {
        self.legs_computed
    }

    /// Accepts a new plan for a vessel and recomputes only what follows the
    /// change.
    ///
    /// Returns the instant the recomputation started from, or `None` if the
    /// plan did not change.
    ///
    /// **History is not rewritten here -- and not because we are careful.**
    /// Edits in the past are rejected, so everything the cursor has already
    /// passed lies, by construction, in legs the change does not touch.
    pub fn commit_plan(&mut self, id: VesselId, plan: Plan) -> Result<Option<f64>, PlanRejected> {
        let cursor = self.clock.t();
        let vessel = self
            .vessels
            .get_mut(id.0 as usize)
            .ok_or(PlanRejected::NoSuchVessel)?;

        let Some(from) = vessel.plan.diverges_from(&plan) else {
            return Ok(None);
        };
        if from <= cursor {
            return Err(PlanRejected::InThePast);
        }

        let restart = vessel.trajectory.truncate_after(from);
        vessel.tip = restart.state;
        vessel.tip_step = restart.step;
        vessel.plan = plan;
        // A new plan is a new attempt: the previous one may have hit the
        // asset's limit with exactly the manoeuvre just removed.
        vessel.failed = None;

        // Manoeuvres earlier than the restart point are already baked into the
        // stored samples. One falling exactly on it is not: a leg ends with the
        // state BEFORE the impulse, and the impulse itself lived in `tip`,
        // which we have just overwritten.
        bake_applied(&self.eph, vessel);

        self.version += 1;
        Ok(Some(from))
    }

    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    pub fn clock_mut(&mut self) -> &mut Clock {
        &mut self.clock
    }

    /// One world step: compute first, then move time.
    ///
    /// The order is exactly this, and it is not cosmetic. Cursor first would
    /// mean that at high warp time hits a horizon that should have grown in
    /// this very frame -- the game would "stutter" every other frame at
    /// perfectly sufficient throughput.
    ///
    /// Move the cursor to an already computed instant (ROADMAP-UI.md, U3b).
    ///
    /// **Integrates nothing.** This is cursor movement over what is already
    /// computed, which is exactly why seeking to an event changes no bit of
    /// the trajectory: the step's check is that `legs_computed()` did not grow
    /// afterwards.
    ///
    /// Refuses twice, and both refusals are named:
    ///
    /// - **the cursor never goes backwards** (the same reason as in
    ///   `Clock::advance`: otherwise a save would jump into the past). The
    ///   player must see that the game cannot do this rather than think they
    ///   missed with the mouse;
    /// - **forwards, no further than computed.** Otherwise "seek" would mean
    ///   "compute", i.e. a `t_end` from the interface -- directly against
    ///   invariant 9.
    pub fn seek_to(&mut self, t: f64) -> Result<(), SeekRejected> {
        if t < self.clock.t() {
            return Err(SeekRejected::Backwards);
        }

        let limit = self
            .vessels
            .iter()
            .filter(|v| v.failed.is_none())
            .map(Vessel::computed_to)
            .fold(f64::INFINITY, f64::min);

        if !limit.is_finite() || t > limit {
            return Err(SeekRejected::NotComputedYet { computed_to: limit });
        }

        self.clock.seek_to(t);
        Ok(())
    }

    /// `dt_wall` is seconds of real time, as an argument. The world does not
    /// read a clock itself, which is exactly why this function can be run with
    /// any sequence of frames and compared bitwise (`tests/time.rs`).
    pub fn step(&mut self, dt_wall: f64, budget: usize) -> Tick {
        let mut done = self.tick(budget);

        // Retirement after the work rather than before: a leg just computed
        // leaves the window no earlier than the fourth after it appears.
        if let Some(window) = self.retire_behind {
            let cursor = self.clock.t();
            for vessel in &mut self.vessels {
                // Window first, then retirement: discarding is cheaper than
                // thinning what you are about to discard.
                done.dropped += vessel
                    .trajectory
                    .keep_revolutions(cursor, self.history_revolutions);
                done.retired += vessel.trajectory.retire_before(window, RETIRE_TOL_M);
            }
        }

        let cursor_limit = self
            .vessels
            .iter()
            .filter(|v| v.failed.is_none())
            .map(Vessel::computed_to)
            .fold(f64::INFINITY, f64::min);

        let mission_end = self
            .vessels
            .iter()
            .filter(|v| v.failed.is_none())
            .map(|v| v.horizon_end)
            .fold(f64::NEG_INFINITY, f64::max);

        // A world with no live vessels has nothing to bound the cursor with --
        // and nowhere to lead it. Let it stand still.
        if cursor_limit.is_finite() {
            self.clock.advance(dt_wall, cursor_limit, mission_end);
        }

        done
    }

    /// Pulls the horizon forward, by no more than `budget` legs.
    ///
    /// Round robin between vessels: otherwise the first in the list would eat
    /// the whole budget and the player's ninth vessel would never be
    /// computed.
    pub fn tick(&mut self, budget: usize) -> Tick {
        let mut done = Tick::default();
        let cursor = self.clock.t();

        while done.legs < budget {
            let mut worked = false;

            for index in 0..self.vessels.len() {
                if done.legs >= budget {
                    break;
                }
                if !self.vessels[index].wants_work(cursor) {
                    continue;
                }

                // Progress is counted, not attempts. Without that distinction
                // a vessel that somehow does not move would spin the loop
                // forever -- and would not crash but hang, which is far worse.
                // Correct code never does that (`extend` always either adds a
                // leg or executes a manoeuvre), so this is a guard rather than
                // a mechanism. Found by a teeth check: a disabled manoeuvre
                // turned the run into an infinite loop.
                if self.extend(index) {
                    done.legs += 1;
                    worked = true;
                }
            }

            // Nobody had anything to compute -- the budget is not spent on
            // empty loop turns.
            if !worked {
                break;
            }
        }

        if done.legs > 0 {
            self.version += 1;
        }
        done.pending = self.vessels.iter().any(|v| v.wants_work(cursor));
        done
    }

    /// The same, but to a given instant rather than the mission's end.
    ///
    /// The cursor is led by the same `step` rather than set by assignment: a
    /// state the game cannot reach by playing is not worth being able to
    /// show.
    pub fn run_to_day(&mut self, until: f64, dt_wall: f64, budget: usize) -> usize {
        let mut steps = 0;
        while self.clock.t() < until {
            let before = self.clock.t();
            let done = self.step(dt_wall, budget);
            steps += 1;

            if self.clock.stall() == Some(Stall::MissionEnd) {
                break;
            }
            if done.legs == 0 && self.clock.t() == before {
                break;
            }
        }
        steps
    }

    /// Runs the mission to its end: computes and leads the cursor until it
    /// stops.
    ///
    /// Not a game mode but a convenience for whoever needs the whole mission
    /// at once: windowless captures and tests. `dt_wall` here is deliberately
    /// large -- time hits the horizon anyway, and that is exactly how it is
    /// checked that it hits it correctly.
    pub fn run_to_end(&mut self, dt_wall: f64, budget: usize) -> usize {
        let mut steps = 0;
        loop {
            let before = self.clock.t();
            let done = self.step(dt_wall, budget);
            steps += 1;

            if self.clock.stall() == Some(Stall::MissionEnd) {
                return steps;
            }
            // A guard against an infinite loop: nobody computed anything and
            // time did not move -- it will not move next time either.
            if done.legs == 0 && self.clock.t() == before {
                return steps;
            }
        }
    }

    /// One leg of one vessel. Returns whether there was progress.
    fn extend(&mut self, index: usize) -> bool {
        let vessel = &mut self.vessels[index];

        let mut buffer = vec![State::default(); LEG];
        let entry = vessel.tip;
        let boundary = vessel.next_boundary();

        // t_end from the plan or the mission, not from a clock. The leg ends
        // either here or on a filled buffer -- both bounds are reproducible.
        let run = match self.prop.run(
            &vessel.tip,
            vessel.params.as_ref(),
            boundary,
            &[],
            &mut buffer,
            &mut vessel.tip_step,
        ) {
            Ok(run) => run,
            Err(e) => {
                vessel.failed = Some(e);
                return false;
            }
        };

        buffer.truncate(run.filled);

        let mut samples = Vec::with_capacity(buffer.len());
        for state in buffer {
            let (earth, moon) = match (
                position(&self.eph, EARTH, state.t),
                position(&self.eph, MOON, state.t),
            ) {
                (Ok(earth), Ok(moon)) => (earth, moon),
                (Err(e), _) | (_, Err(e)) => {
                    vessel.failed = Some(e);
                    return false;
                }
            };
            samples.push(Sample { state, earth, moon });
        }

        vessel.tip = run.final_state;

        // A leg with no samples occurs in exactly one case: two manoeuvres at
        // one instant, when `prop_run` has nowhere to integrate. Storing it is
        // pointless, and it would break plenty -- from the interpolation to
        // boundary comparisons.
        let mut progressed = false;
        if !samples.is_empty() {
            self.legs_computed += 1;
            progressed = true;
            vessel.trajectory.push(Leg {
                entry,
                t1: run.final_state.t,
                step_out: vessel.tip_step,
                samples,
                stop: run.stop,
            });
        }

        // Reached the manoeuvre exactly -- execute it. The comparison is exact,
        // and that is not carelessness: `prop.c` writes `t_end` into the final
        // state verbatim and distinguishes `CORE_STOP_T_END` by exactly that
        // equality.
        if run.stop == core_rs::Stop::ReachedEnd {
            if let Some(m) = vessel.plan.get(vessel.applied) {
                if m.t == vessel.tip.t {
                    apply_manoeuvre(&self.eph, vessel);
                    progressed = true;
                }
            }
        }

        progressed
    }

    /// An immutable slice of the world for readers.
    ///
    /// In J1 it is consumed immediately on the same thread; its type is
    /// already the one `arc-swap` will publish in J4, which is exactly why it
    /// is built here rather than in the renderer -- so the boundary exists
    /// before the thread does.
    pub fn snapshot(&self) -> WorldSnapshot {
        let t = self.clock.t();

        WorldSnapshot {
            version: self.version,
            t,
            warp: self.clock.warp(),
            stall: self.clock.stall(),
            bodies: self.bodies_at(t),
            // The Sun as its own field rather than among the bodies: see
            // `WorldSnapshot::sun`.
            sun: self
                .eph
                .body_state(SUN, t)
                .ok()
                .map(|state| [state.r.x, state.r.y, state.r.z]),
            vessels: self
                .vessels
                .iter()
                .map(|v| {
                    // The interpolation happens here rather than in the
                    // renderer, and that is not a saving: two consumers, each
                    // with its own `state_at`, would see two different "now"s
                    // in one frame. For the same reason `C` is computed from
                    // **this** state rather than from a second
                    // interpolation.
                    let state = v.trajectory.state_at(t);
                    VesselSnapshot {
                        id: v.id,
                        name: v.name.clone(),
                        state,
                        jacobi: self.jacobi_at(t, &state),
                        legs: v.trajectory.share(),
                        plan: v.plan.clone(),
                        start: v.trajectory.start(),
                        tip: v.tip,
                        computed_to: v.computed_to(),
                        horizon_end: v.horizon_end,
                        params: v.params,
                        failed: v.failed,
                    }
                })
                .collect(),
        }
    }

    /// The vessel's Jacobi constant in the pair's synodic frame (U6b3).
    ///
    /// One ephemeris call per snapshot, in the world thread -- exactly where
    /// the bodies are already computed. An error means "there is no frame"
    /// rather than zero: zero is also a value of `C`, and it would draw the
    /// curve in the wrong place.
    fn jacobi_at(&self, t: f64, state: &State) -> Option<f64> {
        let frame = self.eph.synodic_frame(EARTH, MOON, t).ok()?;
        let synodic = frame.from_inertial(state);
        Some(core_rs::cr3bp_jacobi(
            synodic.r,
            synodic.v,
            frame.mass_ratio(),
        ))
    }

    /// The bodies visible in frame at time `t` (ROADMAP-PLANETS.md, R1c).
    ///
    /// **Computed here, in the world thread, not in the frame** -- the same
    /// decision already made for `state_at`: two consumers, each with its own
    /// ephemeris call, would see two different "now"s in one frame. Plus rule
    /// 5 of stage U: the panel and the renderer do not call the ephemeris.
    ///
    /// Radius and orientation come from the asset -- Earth's size and rotation
    /// are not properties of the engine. There is nowhere and no reason to
    /// swallow an ephemeris error here: a body with no rotation model returns
    /// the identity quaternion anyway, and a time outside the asset's span is
    /// impossible for the cursor -- the horizon does not allow it. So a failure
    /// reads as "the body is not in frame", and that is visible.
    fn bodies_at(&self, t: f64) -> Vec<BodySnapshot> {
        [EARTH, MOON]
            .iter()
            .filter_map(|&body| {
                let state = self.eph.body_state(body, t).ok()?;
                let q = self.eph.body_orientation(body, t).ok()?;
                Some(BodySnapshot {
                    body,
                    position: [state.r.x, state.r.y, state.r.z],
                    velocity: [state.v.x, state.v.y, state.v.z],
                    radius_m: self.eph.body_radius(body),
                    mu: self.eph.body_mu(body),
                    orientation: [q.w, q.x, q.y, q.z],
                })
            })
            .collect()
    }
}

/// Counts how many of the plan's manoeuvres are already baked into
/// `vessel.tip`.
///
/// Those earlier than `tip.t` are baked into the stored samples. One falling
/// exactly on it is not: a leg ends with the state BEFORE the impulse, and the
/// impulse itself lived in `tip`, which was just overwritten (or did not exist
/// at all yet).
fn bake_applied(eph: &Ephemeris, vessel: &mut Vessel) {
    vessel.applied = 0;
    while let Some(m) = vessel.plan.get(vessel.applied).copied() {
        if m.t < vessel.tip.t {
            vessel.applied += 1;
        } else if m.t == vessel.tip.t {
            // `apply_manoeuvre` does the increment itself.
            apply_manoeuvre(eph, vessel);
        } else {
            break;
        }
    }
}

/// Executes the next unapplied manoeuvre on `vessel.tip`.
///
/// A free function rather than a method: it needs the world's ephemeris and a
/// mutable vessel at once, and those are two fields of one struct.
///
/// An ephemeris error is not lost here but stops the vessel: a manoeuvre
/// executed with a "zero" frame would quietly be a different manoeuvre.
fn apply_manoeuvre(eph: &Ephemeris, vessel: &mut Vessel) {
    let Some(m) = vessel.plan.get(vessel.applied).copied() else {
        return;
    };

    let body = match m.frame_body() {
        Some(id) => match eph.body_state(id, vessel.tip.t) {
            Ok(state) => Some(state),
            Err(e) => {
                vessel.failed = Some(e);
                return;
            }
        },
        None => None,
    };

    let dv = m.dv_inertial(&vessel.tip, body.as_ref());
    vessel.tip.v.x += dv[0];
    vessel.tip.v.y += dv[1];
    vessel.tip.v.z += dv[2];
    vessel.applied += 1;
}

fn position(eph: &Ephemeris, body: i32, t: f64) -> Result<[f64; 3], CoreError> {
    let s = eph.body_state(body, t)?;
    Ok([s.r.x, s.r.y, s.r.z])
}
