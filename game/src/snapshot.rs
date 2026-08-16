//! An immutable slice of the world (ROADMAP J1, PROJECT.md §6).
//!
//! This is what `arc-swap` publishes from J4 onwards, which is why it exists
//! already now, while there is one thread: a boundary drawn after the thread
//! appears runs where the thread finds it convenient rather than where it is
//! right. Here it runs along "continuous state".
//!
//! **What is not here and will not be: events.** A snapshot is a sample; a
//! reader that missed a publication would miss an event forever. Discrete
//! things travel by channel (CLAUDE.md, invariant 8), and in J4 that becomes
//! its own type.

use std::sync::Arc;

use core_rs::{CoreError, State};

use crate::clock::Stall;
use crate::leg::Leg;
use crate::plan::Plan;
use crate::world::VesselId;

pub struct VesselSnapshot {
    pub id: VesselId,
    pub name: String,

    /// The legs as they are. Cloning this vector clones pointers: a leg is
    /// immutable from the moment it was computed, so sharing it is safe and
    /// free.
    ///
    /// **History and prediction are not separated here, and that is not an
    /// oversight.** They are the same legs; the cursor makes them history, not
    /// a rewrite (PROJECT.md §4, rule 5). Whoever draws splits them by
    /// comparing `sample.t` with `t`.
    pub legs: Vec<Arc<Leg>>,

    /// Where the vessel is **now** -- interpolated at `WorldSnapshot::t`.
    pub state: State,

    /// The vessel's Jacobi constant in the Earth-Moon synodic frame,
    /// dimensionless
    /// (ROADMAP-UI.md, U6b3).
    ///
    /// Computed here, in the world thread, for the same reason as `state`:
    /// the frame is built from the ephemeris, and neither the panel nor the
    /// renderer calls it (rule 5 of stage U). `None` means there is no frame
    /// -- the asset has no pair -- rather than "zero".
    ///
    /// **This is an instantaneous value, not an invariant of the motion.**
    /// `C` is conserved in CR3BP while the game flies in the full ephemeris:
    /// measured (U6b1) 0.007% drift per day and 0.076% per month, while the
    /// vessel stays near the pair.
    pub jacobi: Option<f64>,

    /// The plan this trajectory was computed from.
    ///
    /// Cloned whole rather than shared by `Arc`: a plan is a few dozen bytes
    /// per manoeuvre, not megabytes of samples (CLAUDE.md: clone freely).
    pub plan: Plan,

    /// The state the trajectory started from. Needed by whoever computes the
    /// restart point (`leg::restart_at`) on a still-empty trajectory.
    pub start: State,

    /// The end of what is computed: the state the prediction continues from.
    pub tip: State,
    /// The time of that end. The cursor may not outrun it.
    pub computed_to: f64,

    /// The vessel's mission end -- the same one the world sees.
    pub horizon_end: f64,

    /// The vessel as the force model sees it (K6b). Travels in the snapshot
    /// so the planner can ask for a preview of **the same** vessel: a
    /// prediction with a different area is a line this ship will not fly.
    pub params: Option<core_rs::VesselParams>,

    pub failed: Option<CoreError>,
}

impl VesselSnapshot {
    pub fn sample_count(&self) -> usize {
        self.legs.iter().map(|leg| leg.samples.len()).sum()
    }
}

/// A body at the snapshot's instant (ROADMAP-PLANETS.md, R1c).
///
/// The renderer reads this (`crate::view` -> `engine::scene::Body`), which is
/// exactly why it lives in the snapshot rather than in the frame: the
/// ephemeris is called once, in the world thread, and everyone looking at this
/// frame sees one "now".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodySnapshot {
    /// Index in the ephemeris -- needed by whoever chooses what to draw with
    /// what (`view` takes Earth as the origin).
    pub body: i32,
    /// Barycentric position, metres.
    pub position: [f64; 3],
    /// Barycentric velocity, m/s.
    ///
    /// The rotating frame reads it (U6a2) and so far nobody else: the normal
    /// of the instantaneous Earth-Moon plane is `d x d_dot`, and `d_dot` comes
    /// from nowhere else. The ephemeris returns it in the same call as the
    /// position, so the field costs nothing -- and appears together with its
    /// reader rather than in advance.
    pub velocity: [f64; 3],
    /// Mean radius from the asset, metres; zero means "the asset does not
    /// say".
    pub radius_m: f64,
    /// Gravitational parameter from the asset, m^3/s^2. The rotating frame
    /// needs it to find the pair's barycentre: `mu = mu_Moon / (sum)`.
    pub mu: f64,
    /// Rotation from the body frame into the ephemeris frame, `[w, x, y, z]`.
    pub orientation: [f64; 4],
}

pub struct WorldSnapshot {
    pub version: u64,

    /// The time cursor, seconds from the asset epoch. One "now" per frame.
    pub t: f64,
    pub warp: f64,
    /// Why time is standing still, if it is. [`Stall::Horizon`] means warp is
    /// hitting the throughput limit, and that is what the UI should show
    /// instead of silently easing off.
    pub stall: Option<Stall>,

    /// The bodies in frame -- centre, size, rotation. An empty list means the
    /// asset says nothing about them, not "draw Earth by default".
    pub bodies: Vec<BodySnapshot>,

    /// The Sun's barycentric position, metres -- the same "now" as the bodies.
    ///
    /// Deliberately not in [`WorldSnapshot::bodies`]: that holds the bodies
    /// the frame **draws**, and the Sun among them would stretch the scene to
    /// an astronomical unit (`Frame::far_for` measures the span over the
    /// bodies). Here it is only a light source, i.e. a direction, not a
    /// sphere.
    ///
    /// `None` means the asset says nothing about the Sun -- then the frame
    /// keeps its default lighting rather than a zero that would black
    /// everything out.
    pub sun: Option<[f64; 3]>,

    pub vessels: Vec<VesselSnapshot>,
}
