//! The leg is the unit of everything (ROADMAP J1, PROJECT.md §6).
//!
//! One `prop_run` call = one [`Leg`]. It is also the unit of sharing with a
//! snapshot, the unit of invalidation on a plan edit and the unit of uploading
//! vertices to the GPU. Not a coincidence but what the concept exists for: if
//! they differed, every boundary between them would need enumeration and
//! recomputation.
//!
//! ## Why a leg rather than "so many seconds"
//!
//! Work is measured in legs, because `t_end` may not come from a clock
//! (CLAUDE.md, invariant 9). `prop_run` lands its last step exactly on
//! `t_end`, so a `t_end` computed from a frame's `dt` would write the frame
//! rate into the step sequence. Stopping on a filled buffer has no such
//! property: ROADMAP H1 measured that stitching along it equals one run
//! bitwise, carried step included.
//!
//! ## The outgoing step is not diagnostics
//!
//! [`Leg::step_out`] exists so a run can continue from a leg boundary and get
//! the same trajectory. Without it restarting from the middle is impossible,
//! and it is exactly what makes cascade recomputation cheap: editing a
//! manoeuvre discards the legs after it and computes on from the last
//! survivor rather than from the epoch. Dropping the step costs seventyfold
//! work and a different trajectory -- H1 measured that too.

use std::sync::Arc;

use core_rs::{State, Stop};

/// One accepted integrator step, and where the bodies were then.
///
/// Earth's and the Moon's positions sit beside the state because they are what
/// is needed to draw this point: geocentric now, in the synodic frame once the
/// frame service arrives (PROJECT.md §7). The cost is 48 extra bytes per
/// sample and two ephemeris calls per step; both are cheaper than the
/// integration, and both disappear once a shader computes frames from a shared
/// body buffer.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub state: State,
    pub earth: [f64; 3],
    pub moon: [f64; 3],
}

/// What one `prop_run` call produced.
pub struct Leg {
    /// The state the leg started from.
    ///
    /// It is not among the samples -- `prop_run` does not sample the initial
    /// point, so legs stitch without repeated vertices. But it must be stored,
    /// and not for convenience: after a manoeuvre `entry` is the state
    /// **after** the impulse, while the previous leg's last sample is before
    /// it. Without this the interpolation would smooth the velocity
    /// discontinuity and draw an arc the vessel never flew.
    pub entry: State,
    /// The time the leg stopped at.
    pub t1: f64,
    /// The step the run left it with. Continue with that one.
    pub step_out: f64,
    pub samples: Vec<Sample>,
    pub stop: Stop,
}

/// The sequence of one vessel's legs.
///
/// An `Arc` per leg rather than per trajectory: a snapshot copies a vector of
/// pointers rather than megabytes of samples, and a leg someone is already
/// drawing survives the publication of the next.
pub struct Trajectory {
    /// The state everything started from.
    ///
    /// Kept separately because the legs do not hold it: `prop_run` does not
    /// sample the initial point. Without it the trajectory would have no first
    /// node of its own -- and that is needed by the interpolation on the very
    /// first segment, by the save (J6), and by cascade recomputation when an
    /// edit discards every leg (J3).
    start: State,
    legs: Vec<Arc<Leg>>,
    /// How many legs from the start have already been retired (N3a).
    ///
    /// An index rather than a flag in the leg: legs retire only from the front
    /// and only once, so one number suffices and it does not force touching a
    /// `Leg` that snapshots share.
    retired: usize,

    /// The angle the vessel's radius vector swept about the central body in
    /// each leg, radians (N5a).
    ///
    /// Beside the legs rather than inside them: snapshots share a leg, and
    /// adding a field for the sake of the window policy would broadcast this
    /// number to every reader that does not need it. The length always equals
    /// `legs`.
    swept: Vec<f64>,
}

impl Trajectory {
    pub fn new(start: State) -> Trajectory {
        Trajectory {
            start,
            legs: Vec::new(),
            retired: 0,
            swept: Vec::new(),
        }
    }

    pub fn start(&self) -> State {
        self.start
    }

    pub fn push(&mut self, leg: Leg) {
        self.swept.push(swept_angle(&leg.samples));
        self.legs.push(Arc::new(leg));
    }

    pub fn legs(&self) -> &[Arc<Leg>] {
        &self.legs
    }

    /// A copy of the leg list for a snapshot -- a vector of pointers, not
    /// samples.
    pub fn share(&self) -> Vec<Arc<Leg>> {
        self.legs.clone()
    }

    pub fn is_empty(&self) -> bool {
        self.legs.is_empty()
    }

    pub fn sample_count(&self) -> usize {
        self.legs.iter().map(|leg| leg.samples.len()).sum()
    }

    /// Retires the legs left behind the window around the cursor
    /// (ROADMAP.md, N3a).
    ///
    /// **Retirement is thinning the samples once and for all**, not
    /// discarding the leg: the trail must stay on the map (assumption Q4).
    /// Three things stay untouched, the ones the save rests on
    /// (`restart_at`): `entry`, the **last sample** and `step_out`.
    /// Douglas-Peucker always keeps both ends, so the last sample survives
    /// retirement by construction rather than by exception.
    ///
    /// WARNING: **a one-way door.** Invariant 5 forbids integrating the past a
    /// second time, so discarded samples never come back. So the tolerance
    /// here is a decision rather than a setting: `tol_m` must stay sub-pixel at
    /// whatever scale the player will one day look at the old past from.
    ///
    /// Returns how many samples disappeared -- that is the step's number.
    pub fn retire_before(&mut self, keep_raw_legs: usize, tol_m: f64) -> usize {
        if self.legs.len() <= keep_raw_legs {
            return 0;
        }

        let last_to_retire = self.legs.len() - keep_raw_legs;
        let mut dropped = 0;
        for index in self.retired..last_to_retire {
            let leg = &self.legs[index];
            let points: Vec<[f64; 3]> = leg
                .samples
                .iter()
                .map(|s| [s.state.r.x, s.state.r.y, s.state.r.z])
                .collect();
            let keep = crate::thin::simplify3(&points, tol_m);
            if keep.len() == leg.samples.len() {
                continue;
            }

            dropped += leg.samples.len() - keep.len();
            let samples = keep.into_iter().map(|i| leg.samples[i]).collect();
            self.legs[index] = Arc::new(Leg {
                entry: leg.entry,
                t1: leg.t1,
                step_out: leg.step_out,
                samples,
                stop: leg.stop,
            });
        }
        self.retired = last_to_retire;
        dropped
    }

    /// Discards legs older than `revolutions` revolutions behind the cursor
    /// (ROADMAP.md, N5a; decision Q4).
    ///
    /// **A revolution is measured by angle, not by periapsis.** The sum of
    /// angles between adjacent geocentric radius vectors works on a closed
    /// orbit, on a transfer and around L2 alike -- there the question "where is
    /// periapsis" has no good answer.
    ///
    /// WARNING: **a one-way door.** Invariant 5 forbids integrating the past a
    /// second time, so a discarded leg does not come back. Hence `start` moves
    /// to the `entry` of the oldest survivor: without that, `state_at` and the
    /// cascade recomputation would base themselves on a state the trajectory no
    /// longer holds.
    ///
    /// Returns how many samples disappeared.
    pub fn keep_revolutions(&mut self, cursor_t: f64, revolutions: f64) -> usize {
        if self.legs.is_empty() || revolutions <= 0.0 {
            return 0;
        }

        // From the leg under the cursor rather than from the last: the
        // prediction runs `LEAD_LEGS` ahead, and measuring the window from it
        // would mean discarding what the player just saw.
        let cursor_leg = self
            .legs
            .partition_point(|leg| leg.t1 < cursor_t)
            .min(self.legs.len() - 1);

        let budget = revolutions * std::f64::consts::TAU;
        let mut total = 0.0;
        let mut oldest = cursor_leg;
        while oldest > 0 {
            total += self.swept[oldest];
            if total >= budget {
                break;
            }
            oldest -= 1;
        }

        if oldest == 0 {
            return 0;
        }

        let dropped: usize = self.legs[..oldest]
            .iter()
            .map(|leg| leg.samples.len())
            .sum();

        self.start = self.legs[oldest].entry;
        self.legs.drain(..oldest);
        self.swept.drain(..oldest);
        self.retired = self.retired.saturating_sub(oldest);
        dropped
    }

    /// How much the history weighs in memory, bytes.
    ///
    /// 104 bytes per sample -- the number debt D7 speaks in, and the one the
    /// player should see beside the window choice (N5a).
    pub fn history_bytes(&self) -> usize {
        self.sample_count() * 104
    }

    /// The last sample's time, if there is one.
    pub fn end(&self) -> Option<f64> {
        self.legs.last().map(|leg| leg.t1)
    }

    /// Discards the legs ending later than `t` and says where to continue
    /// from.
    ///
    /// This is the whole cascade recomputation (PROJECT.md §6): not "recompute
    /// from the epoch" but "cut the tail". What makes it legitimate is the H1
    /// measurement -- stitched legs equal one run bitwise, carried step
    /// included -- so continuing from a leg boundary gives the same trajectory
    /// a continuous run would.
    pub fn truncate_after(&mut self, t: f64) -> Restart {
        let keep = self.legs.partition_point(|leg| leg.t1 <= t);
        self.legs.truncate(keep);
        self.swept.truncate(keep);
        // The cascade cut the tail, so the retired count cannot shrink -- but
        // the boundary must stay inside the vector.
        self.retired = self.retired.min(self.legs.len());
        restart_at(&self.legs, self.start, t)
    }

    /// How far it is computed. For an empty trajectory, the start instant.
    pub fn computed_to(&self) -> f64 {
        self.end().unwrap_or(self.start.t)
    }

    /// How many legs end later than `t`.
    ///
    /// This is the measure of "how far the prediction is ahead of the cursor",
    /// and it is measured in legs rather than seconds: seconds per leg depend
    /// on how densely the integrator places steps, i.e. on the trajectory
    /// itself.
    pub fn legs_after(&self, t: f64) -> usize {
        // The legs are ordered in time, so the first with `t1 > t` cuts the
        // tail.
        self.legs.len() - self.legs.partition_point(|leg| leg.t1 <= t)
    }

    /// The state at time `t`, by Hermite interpolation between adjacent
    /// samples.
    ///
    /// Cubic Hermite rather than linear interpolation, and not for beauty:
    /// every node carries **both** position **and** velocity, so the cubic is
    /// determined exactly, with no assumption. Linear interpolation between
    /// steps hours long would cut orbit corners by kilometres.
    ///
    /// Past what is computed it returns the endpoint rather than `None`: the
    /// cursor is not allowed there (`clock::Clock::advance`), and returning
    /// "no state" at a moment when the vessel obviously is somewhere would
    /// force every caller to invent what to do about it.
    pub fn state_at(&self, t: f64) -> State {
        state_at(&self.legs, self.start, t)
    }
}

/// The state at time `t` over already computed legs -- the same interpolation,
/// but available to whoever holds only a snapshot.
///
/// A free function for the same reason as [`restart_at`]: the rule must be
/// **one**. The world reads the trajectory through [`Trajectory::state_at`],
/// while the panel and the planner read a snapshot, which holds only `legs`
/// and `start`; two implementations of one interpolation would diverge
/// quietly, in the third digit.
///
/// Past what is computed it returns the endpoint rather than `None` -- which is
/// exactly why a caller needing a state **in the future** must first check
/// against `computed_to`: an endpoint looks like an ordinary state.
pub fn state_at(legs: &[Arc<Leg>], start: State, t: f64) -> State {
    if t <= start.t || legs.is_empty() {
        return start;
    }

    let leg_index = legs.partition_point(|leg| leg.t1 < t);
    let Some(leg) = legs.get(leg_index) else {
        // Later than everything computed.
        return legs
            .last()
            .and_then(|leg| leg.samples.last())
            .map_or(start, |s| s.state);
    };

    let index = leg.samples.partition_point(|s| s.state.t < t);
    let Some(after) = leg.samples.get(index) else {
        // A leg ends earlier than t only if it is empty, and we do not store
        // those (`world::World::extend`).
        return start;
    };

    // At a leg's start the left node is its `entry`, NOT the previous leg's
    // last sample: after a manoeuvre those are different states, differing by
    // exactly dv.
    let before = if index > 0 {
        leg.samples[index - 1].state
    } else {
        leg.entry
    };

    hermite(&before, &after.state, t)
}

/// Where to continue from after the trajectory's tail was discarded.
#[derive(Debug, Clone, Copy)]
pub struct Restart {
    pub state: State,
    pub step: f64,
}

/// The restart point for time `t`: the last leg boundary not later than it.
///
/// A free function because two callers need it and it must be **one**: the
/// world, to recompute the tail after a plan edit, and the planner, to compute
/// a preview from the same point. Two implementations of this rule would
/// diverge, and would diverge exactly where the price is highest: the preview
/// would show a line the vessel will not then fly (ROADMAP J5).
///
/// `legs` must already be trimmed by `t` or ordered -- the last with
/// `t1 <= t` is taken.
pub fn restart_at(legs: &[Arc<Leg>], start: State, t: f64) -> Restart {
    let keep = legs.partition_point(|leg| leg.t1 <= t);

    match keep.checked_sub(1).and_then(|i| legs.get(i)) {
        Some(leg) => Restart {
            state: leg.samples.last().map_or(leg.entry, |s| s.state),
            step: leg.step_out,
        },
        // Zero means "choose one yourself" -- exactly what is wanted when no
        // legs remain at all (`core/prop.h`).
        None => Restart {
            state: start,
            step: 0.0,
        },
    }
}

/// Cubic Hermite over position and velocity.
///
/// The velocity is the derivative of that same cubic rather than interpolated
/// separately: otherwise the drawn position and the displayed velocity would
/// describe different motions.
fn hermite(a: &State, b: &State, t: f64) -> State {
    let h = b.t - a.t;
    if h <= 0.0 {
        return *a;
    }

    let s = (t - a.t) / h;
    let s2 = s * s;
    let s3 = s2 * s;

    let p0 = 2.0 * s3 - 3.0 * s2 + 1.0;
    let m0 = s3 - 2.0 * s2 + s;
    let p1 = -2.0 * s3 + 3.0 * s2;
    let m1 = s3 - s2;

    let dp0 = 6.0 * s2 - 6.0 * s;
    let dm0 = 3.0 * s2 - 4.0 * s + 1.0;
    let dp1 = -6.0 * s2 + 6.0 * s;
    let dm1 = 3.0 * s2 - 2.0 * s;

    let axis = |r0: f64, v0: f64, r1: f64, v1: f64| -> (f64, f64) {
        (
            p0 * r0 + m0 * h * v0 + p1 * r1 + m1 * h * v1,
            (dp0 * r0 + dm0 * h * v0 + dp1 * r1 + dm1 * h * v1) / h,
        )
    };

    let (x, vx) = axis(a.r.x, a.v.x, b.r.x, b.v.x);
    let (y, vy) = axis(a.r.y, a.v.y, b.r.y, b.v.y);
    let (z, vz) = axis(a.r.z, a.v.z, b.r.z, b.v.z);

    State {
        r: core_rs::Vec3d { x, y, z },
        v: core_rs::Vec3d {
            x: vx,
            y: vy,
            z: vz,
        },
        t,
    }
}

/// The angle the radius vector swept about the central body over these
/// samples.
///
/// The sum of angles between adjacent vectors, via `atan2` of the cross
/// product's length and the dot product: near zero that is exact, whereas
/// `acos` there loses half its significant digits. This is storage policy, not
/// the integrator, so `libm` is allowed here (invariant 3 speaks of the
/// integration loop).
fn swept_angle(samples: &[Sample]) -> f64 {
    let geocentric = |s: &Sample| {
        [
            s.state.r.x - s.earth[0],
            s.state.r.y - s.earth[1],
            s.state.r.z - s.earth[2],
        ]
    };

    let mut total = 0.0;
    for pair in samples.windows(2) {
        let a = geocentric(&pair[0]);
        let b = geocentric(&pair[1]);
        let cross = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        let sin = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
        let cos = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        total += sin.atan2(cos);
    }
    total
}
