//! Time: who owns it and why it does not enter the arithmetic (ROADMAP J2).
//!
//! The cursor belongs to the world -- since J4, to the simulation thread --
//! and is written nowhere else (PROJECT.md §6). What matters about it is not
//! who owns it but what it does **not** do:
//!
//! > The clock does not enter the integrator.
//!
//! The future is computed before the cursor crawls to it; the cursor only
//! picks a point on an already finished polyline. So:
//!
//! - no fixed simulation step is needed -- there is nothing to fix;
//! - the frame rate cannot change one bit of a trajectory;
//! - pause, warp and a frame drop are cursor speed, not different physics.
//!
//! ## Accumulated drift is harmless here, and that can be computed
//!
//! The cursor is `f64` seconds from the asset epoch, moved by adding `dt`. At
//! `t ~ 6e9` s (200 years) an `f64` step is 1.4 us, so thousands of additions
//! give a random walk of microsecond scale. A comparison that explains why
//! that does not matter: H2 measured that stopping at periapsis is stable to
//! 8.3 us. But the real reason is different -- the cursor does not feed the
//! arithmetic at all, so its error propagates nowhere.
//!
//! ## The cursor may not outrun the horizon
//!
//! If the integrator cannot keep up, **time** stops, not the physics. That is
//! a derived warp limit rather than a number baked into the code, and it is
//! visible ([`Stall`]) -- otherwise the game would simply "ease off" with no
//! explanation.

/// Why the cursor is standing still.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stall {
    /// The player pressed pause.
    Paused,
    /// The prediction is not computed that far yet. This is the warp ceiling.
    Horizon,
    /// The mission is over: there is nothing further to compute.
    MissionEnd,
}

pub struct Clock {
    t: f64,
    warp: f64,
    paused: bool,
    stall: Option<Stall>,
}

/// Slowest and fastest warp.
///
/// The ceiling is not physical but a guard against mistakes: one click with a
/// stuttering mouse should not throw the mission years forward. The real
/// ceiling is set by the horizon ([`Stall::Horizon`]) -- which is exactly why
/// this one need not be precise.
pub const MIN_WARP: f64 = 1.0;
pub const MAX_WARP: f64 = 1.0e7;

impl Clock {
    pub fn new(t: f64, warp: f64) -> Clock {
        Clock {
            t,
            warp: warp.clamp(MIN_WARP, MAX_WARP),
            paused: false,
            stall: None,
        }
    }

    pub fn t(&self) -> f64 {
        self.t
    }

    pub fn warp(&self) -> f64 {
        self.warp
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn stall(&self) -> Option<Stall> {
        self.stall
    }

    pub fn set_warp(&mut self, warp: f64) {
        self.warp = warp.clamp(MIN_WARP, MAX_WARP);
    }

    /// Multiply warp by `factor` -- multiply, specifically.
    ///
    /// A range from 1 to 1e7 is seven decades; a constant step in seconds
    /// would be either immobile at one end or unusable at the other. The same
    /// reason as the camera wheel (`engine::orbit`).
    pub fn scale_warp(&mut self, factor: f64) {
        self.set_warp(self.warp * factor);
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    /// Place the cursor at a specific instant (ROADMAP-UI.md, U3b).
    ///
    /// Checking that the instant is computed and not in the past is the
    /// world's job (`World::seek_to`): the clock does not know how far the
    /// prediction reached. Here there is only the jump itself, and with it
    /// `stall` clears: the reason time was standing concerned the previous
    /// moment.
    pub fn seek_to(&mut self, t: f64) {
        self.t = t;
        self.stall = if self.paused {
            Some(Stall::Paused)
        } else {
            None
        };
    }

    /// Advance the cursor by `dt_wall` seconds of real time.
    ///
    /// `limit` is how far has been computed; the cursor goes no further.
    /// `mission_end` is how far there is anything to compute at all, and is
    /// needed only to tell "did not keep up" from "the mission is over": the
    /// first is grounds to show that warp is hitting a wall, the second is
    /// not.
    ///
    /// `dt_wall` arrives as an argument rather than from `Instant::now()`: the
    /// operating system's clock never appears in this struct, which is exactly
    /// why its behaviour can be checked with reproducible numbers.
    pub fn advance(&mut self, dt_wall: f64, limit: f64, mission_end: f64) {
        if self.paused {
            self.stall = Some(Stall::Paused);
            return;
        }

        let wanted = self.t + dt_wall * self.warp;

        if wanted <= limit {
            self.t = wanted;
            self.stall = None;
            return;
        }

        // The cursor never goes backwards. Not cosmetic: after loading a save
        // the horizon is not computed yet, and without this rule the clock
        // would roll back to the restart point, i.e. loading would make the
        // game jump into the past (`crate::save`).
        self.t = self.t.max(limit);
        self.stall = Some(if limit >= mission_end {
            Stall::MissionEnd
        } else {
            Stall::Horizon
        });
    }
}
