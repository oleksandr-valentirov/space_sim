//! Safe wrapper over the numeric core (ROADMAP D3, PROJECT.md §5).
//!
//! **This is the only place in the project with our `unsafe`** (CLAUDE.md,
//! invariant 1). Third-party `-sys` crates are the exception; our code writes
//! it nowhere else. So this file must stay small and boring: anything doable
//! outside in safe Rust is done outside.
//!
//! The wrapper does not promise there are no bugs, but that two specific bugs
//! cannot be made even on purpose:
//!
//! - **double free** -- `eph_free` is not exported and the pointer field is
//!   private. Freeing happens exactly once, in `Drop`.
//! - **use after free** -- `Ephemeris` is neither `Copy` nor `Clone`, so after
//!   `drop` the value is moved and the compiler will not let it be touched.
//!
//! Both promises are checked by the `compile_fail` doctests below, not by a
//! comment.
//!
//! ## Style
//!
//! Deliberately simple (CLAUDE.md): concrete types instead of generics,
//! `&Path` instead of `impl AsRef<Path>`, no lifetimes in structs. `State` and
//! `Vec3d` are re-exported from `core-sys` as they are -- plain structs of
//! `double`, and a conversion layer would add work without a new guarantee.

use std::ffi::CString;
use std::fmt;
use std::path::Path;
use std::sync::Arc;

pub use core_sys::{State, Vec3d};

/// A core error.
///
/// `Unknown` is not there for completeness. `core-sys` hands the return code
/// back as a `c_int`, because a Rust enum holding an out-of-range value is
/// undefined behaviour; the conversion into this type is where an unknown
/// value becomes a visible error instead of silent corruption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreError {
    /// The supplied buffer is too small; C returns the needed count
    /// separately.
    BufferTooSmall,
    /// The iteration did not converge to the given tolerance.
    ToleranceNotMet,
    /// Invalid argument: unknown body, or a time outside the asset's span.
    InvalidArg,
    /// A code absent from `CoreResult`. Means C and the boundary have drifted
    /// apart.
    Unknown(i32),
    /// The path could not be passed to C: not UTF-8, or holding an interior
    /// `\0`. Our side's error; the core never saw it.
    BadPath,
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::BufferTooSmall => write!(f, "buffer too small"),
            CoreError::ToleranceNotMet => write!(f, "did not converge to tolerance"),
            CoreError::InvalidArg => write!(f, "invalid argument"),
            CoreError::Unknown(code) => {
                write!(f, "unknown core code {code} -- boundary drifted from C")
            }
            CoreError::BadPath => write!(f, "path is not UTF-8 or holds a \\0"),
        }
    }
}

impl std::error::Error for CoreError {}

pub type Result<T> = std::result::Result<T, CoreError>;

fn check(code: core_sys::CoreResult) -> Result<()> {
    match code {
        core_sys::CORE_OK => Ok(()),
        core_sys::CORE_ERR_BUFFER_TOO_SMALL => Err(CoreError::BufferTooSmall),
        core_sys::CORE_ERR_TOLERANCE_NOT_MET => Err(CoreError::ToleranceNotMet),
        core_sys::CORE_ERR_INVALID_ARG => Err(CoreError::InvalidArg),
        other => Err(CoreError::Unknown(other)),
    }
}

/// A loaded ephemeris. Frees itself.
///
/// ```no_run
/// use core_rs::Ephemeris;
/// use std::path::Path;
///
/// let eph = Ephemeris::load(Path::new("data/fixture/earth_moon.eph"))?;
/// let moon = eph.body_state(4, 0.0)?;
/// println!("{:?}", moon.r);
/// # Ok::<(), core_rs::CoreError>(())
/// ```
///
/// Use after free does not compile:
///
/// ```compile_fail
/// use core_rs::Ephemeris;
/// use std::path::Path;
///
/// let eph = Ephemeris::load(Path::new("data/fixture/earth_moon.eph")).unwrap();
/// drop(eph);
/// let _ = eph.body_state(4, 0.0);
/// ```
///
/// Freeing twice is impossible too -- there is nowhere to get `eph_free` from,
/// and the pointer field is private:
///
/// ```compile_fail
/// use core_rs::Ephemeris;
/// use std::path::Path;
///
/// let eph = Ephemeris::load(Path::new("data/fixture/earth_moon.eph")).unwrap();
/// let _ = eph.ctx;
/// ```
pub struct Ephemeris {
    ctx: *mut core_sys::EphemerisCtx,
}

// Reading the ephemeris involves no shared mutable state, and that is checked
// rather than assumed: `eph_body_state` takes a `const EphemerisCtx*`, touches
// only context fields and the heap the context owns, and has neither statics
// nor a cache (`core/ephemeris.c`, `core/cheb.c`). After `eph_load` the
// context does not change at all.
//
// So the pointer can move between threads (`Send`), and `&Ephemeris` can be
// read from several at once (`Sync`). That will be needed as soon as physics
// moves to its own thread (PROJECT.md §6), and the promise is better written
// by whoever has just read the C than by whoever eventually needs it.
unsafe impl Send for Ephemeris {}
unsafe impl Sync for Ephemeris {}

impl Ephemeris {
    /// Reads a cooked asset.
    pub fn load(path: &Path) -> Result<Ephemeris> {
        let text = path.to_str().ok_or(CoreError::BadPath)?;
        let c_path = CString::new(text).map_err(|_| CoreError::BadPath)?;

        let mut ctx: *mut core_sys::EphemerisCtx = std::ptr::null_mut();

        // SAFETY: c_path is a valid C string, alive to the end of the call.
        // `ctx` is a valid slot for one pointer. C writes it only on CORE_OK.
        let code = unsafe { core_sys::eph_load(c_path.as_ptr(), &mut ctx) };
        check(code)?;

        // Guard against a contract C does not break but could: CORE_OK and
        // NULL together. Without this check the bug would surface as a null
        // dereference somewhere later, with no trace of the cause.
        if ctx.is_null() {
            return Err(CoreError::Unknown(core_sys::CORE_OK));
        }

        Ok(Ephemeris { ctx })
    }

    /// Position and velocity of a body at time `t` (seconds from the asset
    /// epoch).
    ///
    /// A time outside the asset's span gives [`CoreError::InvalidArg`] rather
    /// than extrapolation: continuing a Chebyshev fit past its range returns
    /// confident nonsense.
    pub fn body_state(&self, body: i32, t: f64) -> Result<State> {
        let mut state = State::default();

        // SAFETY: self.ctx came from eph_load and is not yet freed -- only
        // Drop frees, and sole ownership guarantees Drop has not run. `state`
        // is a valid slot for a State.
        let code = unsafe { core_sys::eph_body_state(self.ctx, body, t, &mut state) };
        check(code)?;

        Ok(state)
    }

    /// Gravitational parameter of the body, m^3/s^2 (ROADMAP-UI.md, U5a).
    ///
    /// The same contract as [`Ephemeris::body_radius`]: a field read, zero for
    /// an unknown body. Needed by whoever computes a transfer: `mu` **must**
    /// come from the same asset as the trajectory, or the planner aims near a
    /// different Earth than the integrator flies.
    pub fn body_mu(&self, body: i32) -> f64 {
        // SAFETY: the same context as in body_state; C reads a field and
        // writes nothing.
        unsafe { core_sys::eph_body_mu(self.ctx, body) }
    }

    /// Mean body radius in metres; zero if the asset does not say (U2a).
    ///
    /// This is the same sphere the atmosphere (K7a) and the
    /// [`Event::Altitude`] event (K7c) measure altitude from -- not the
    /// harmonics reference radius, which for Earth is a different number. The
    /// first caller is the HUD: altitude above the surface cannot be computed
    /// without a radius, and taking it from the renderer would mean the game
    /// measured altitude from the drawn sphere.
    ///
    /// Not a `Result`, because in C this is a field read: an unknown body
    /// gives the same zero as a body with no size, and that is a decision, not
    /// an oversight -- both cases mean "no size available", and two distinct
    /// answers would have to be told apart by every caller.
    pub fn body_radius(&self, body: i32) -> f64 {
        // SAFETY: the same context as in body_state -- from eph_load, not yet
        // freed (only Drop frees). C reads a field and writes nothing, so no
        // output slot is needed at all.
        unsafe { core_sys::eph_body_radius(self.ctx, body) }
    }

    /// Body orientation at time `t` (ROADMAP-PLANETS.md, R1c).
    ///
    /// The quaternion takes vector components **from the body frame into the
    /// ephemeris frame**. The direction is named here rather than left to
    /// guesswork: the inverse quaternion is also a valid rotation, merely the
    /// other way, and the only sign would be a planet facing backwards.
    ///
    /// The first caller is the renderer: without orientation the Moon in frame
    /// is unrotated, so the mascons (K5) and the terrain face different ways.
    ///
    /// A body whose asset has no rotation channels returns the identity and
    /// `Ok` -- "not modelled" is an answer here, not an error; eight of the
    /// fixture's ten bodies are like that.
    ///
    /// # Errors
    ///
    /// [`CoreError::InvalidArg`] for an unknown body or a time outside the
    /// asset's span. Unlike radius and `mu`, there is time here, hence a way
    /// to fail.
    /// Synodic frame of the `primary`-`secondary` pair at time `t`
    /// (ROADMAP-UI.md, U6b1).
    ///
    /// A method on the ephemeris rather than a free function: the frame is
    /// built **from it** -- from both bodies' positions and velocities at that
    /// instant.
    pub fn synodic_frame(&self, primary: i32, secondary: i32, t: f64) -> Result<SynodicFrame> {
        let mut raw = core_sys::SynodicFrame::default();
        // SAFETY: `self.ctx` is valid while `self` lives; the output pointer
        // leads to a local, and C fills it entirely -- which is why the struct
        // layout is compared bitwise (`core-sys/tests/ffi.rs`).
        let code = unsafe { core_sys::frame_synodic(self.ctx, primary, secondary, t, &mut raw) };
        check(code)?;
        Ok(SynodicFrame { raw })
    }

    pub fn body_orientation(&self, body: i32, t: f64) -> Result<Quat> {
        let mut out = core_sys::Quat::default();
        // SAFETY: the context is alive (borrowed through &self, only Drop
        // frees); the output slot is a local alive to the end of the call. C
        // allocates and stores nothing: nowhere to write, nothing to free.
        let code = unsafe { core_sys::eph_body_orientation(self.ctx, body, t, &mut out) };
        check(code)?;
        Ok(Quat {
            w: out.w,
            x: out.x,
            y: out.y,
            z: out.z,
        })
    }
}

/// Rotation quaternion: `w` first, as in `core/quat.h`.
///
/// Its own type rather than a re-export from `core-sys`, for the same reason
/// as [`State`]: the raw layer stays raw, and no crate but this one sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    pub w: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Default for Quat {
    fn default() -> Self {
        Quat {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }
}

impl Quat {
    /// Rotates a vector: `q * v * q^-1`, i.e. from the body frame into the
    /// ephemeris frame.
    ///
    /// Rodrigues' formula rather than multiplying three quaternions: half the
    /// arithmetic and no intermediate quaternion tempting a renormalisation.
    /// `libm` does not appear at all -- only multiplies and adds -- so the
    /// question of invariant 3 never arises.
    pub fn rotate(&self, v: [f64; 3]) -> [f64; 3] {
        let u = [self.x, self.y, self.z];
        let cross = |a: [f64; 3], b: [f64; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let t = cross(u, v);
        let t = [2.0 * t[0], 2.0 * t[1], 2.0 * t[2]];
        let w_t = [self.w * t[0], self.w * t[1], self.w * t[2]];
        let u_t = cross(u, t);
        [
            v[0] + w_t[0] + u_t[0],
            v[1] + w_t[1] + u_t[1],
            v[2] + w_t[2] + u_t[2],
        ]
    }
}

impl Drop for Ephemeris {
    fn drop(&mut self) {
        // SAFETY: the pointer came from eph_load and is freed exactly here,
        // exactly once -- the field is private, the type is neither Copy nor
        // Clone, and there is no other route to eph_free.
        unsafe { core_sys::eph_free(self.ctx) };
    }
}

impl fmt::Debug for Ephemeris {
    /// No address inside: it explains nothing and adds noise to logs.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Ephemeris")
    }
}

// ---------------------------------------------------------------------------
// Propagator (ROADMAP H4)
// ---------------------------------------------------------------------------

/// Which integrator does the work.
///
/// `Rkn` is declared but the core does not have it yet: `Propagator::new` with
/// it returns [`CoreError::InvalidArg`]. Not a forgotten stub but what
/// PROJECT.md §4 asks for -- the integrator selection field exists from day
/// one so that adding RKN later means changing a call, not rewriting a
/// layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Integrator {
    Dop853,
    Rkn,
}

/// Propagator settings.
#[derive(Debug, Clone, Copy)]
pub struct PropConfig {
    pub integrator: Integrator,
    /// Position tolerance in metres -- absolute, not relative.
    pub tol_m: f64,
    /// Step ceiling in seconds. Set it: with zero the integrator picks the
    /// ceiling from the leg length, and a stitched run then leaves behind a
    /// different step than a continuous one (`core/prop.h`, measured).
    pub h_max_s: f64,
    /// Step limit for **one call** to `run`. 0 means the core's default.
    pub max_steps: i64,
    /// Air density multiplier for every vessel of this propagator (K7c).
    /// One means the profile the asset carries.
    ///
    /// Here rather than in [`VesselParams`], because it describes the air, not
    /// the ship: two vessels on one leg fly through the same atmosphere.
    /// Constant per leg rather than a function of time: the solar cycle is a
    /// sinusoid of eleven years, `libm` is forbidden in the integration loop,
    /// and nobody can say where the next maximum falls anyway. So the game
    /// computes the multiplier (a future "space weather" toggle) and the core
    /// only applies it.
    ///
    /// **Zero is inadmissible** -- `new` returns [`CoreError::InvalidArg`].
    /// The core deliberately does not read it as one: an unset field must fail
    /// loudly
    /// (`core/prop.h`).
    pub density_scale: f64,
}

impl Default for PropConfig {
    /// A metre of tolerance and an hour of ceiling -- the numbers the fixture
    /// was already computed with (`data/fixture/README.md`), not round values
    /// out of thin air.
    fn default() -> Self {
        PropConfig {
            integrator: Integrator::Dop853,
            tol_m: 1.0,
            h_max_s: 3600.0,
            max_steps: 0,
            // The asset profile as it is. A multiplier appears when the game
            // gives the player a solar activity switch.
            density_scale: 1.0,
        }
    }
}

/// An event the run stops on.
///
/// An enum rather than a struct with a `param` field meaning nothing for two
/// kinds out of three: here a periapsis with a distance cannot be expressed,
/// because no such variant exists.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// Closest point to the body.
    Periapsis { body: i32 },
    /// Furthest.
    Apoapsis { body: i32 },
    /// A given distance from the body **centre**, crossed either way. A
    /// sphere of influence or a rendezvous ring is a distance from the centre;
    /// it has nothing to do with the surface.
    Distance { body: i32, metres: f64 },
    /// A given altitude above the body **surface**, crossed either way (K7c).
    ///
    /// The surface is the asset's mean radius, the same sphere the atmosphere
    /// measures altitude from. A body whose size the asset does not name gives
    /// [`CoreError::InvalidArg`] on arming: a zero radius would silently turn
    /// this into [`Event::Distance`]. Zero altitude is allowed -- that is the
    /// surface.
    Altitude { body: i32, metres: f64 },
}

impl Event {
    fn raw(&self) -> core_sys::CoreEvent {
        match *self {
            Event::Periapsis { body } => core_sys::CoreEvent {
                kind: core_sys::CORE_EVENT_PERIAPSIS,
                body_id: body,
                param: 0.0,
            },
            Event::Apoapsis { body } => core_sys::CoreEvent {
                kind: core_sys::CORE_EVENT_APOAPSIS,
                body_id: body,
                param: 0.0,
            },
            Event::Distance { body, metres } => core_sys::CoreEvent {
                kind: core_sys::CORE_EVENT_DISTANCE,
                body_id: body,
                param: metres,
            },
            Event::Altitude { body, metres } => core_sys::CoreEvent {
                kind: core_sys::CORE_EVENT_ALTITUDE,
                body_id: body,
                param: metres,
            },
        }
    }
}

/// Why the run stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// Reached `t_end`.
    ReachedEnd,
    /// The sample buffer ran out. Continue from `final_state` with the same
    /// step -- it will be the same trajectory, bitwise.
    BufferFull,
    /// The event at this index in the supplied slice fired.
    Event(usize),
}

/// How many numbers in the transition matrix: 6x6 (`STM_SIZE` in
/// `core/stm.h`).
pub const STM_LEN: usize = 36;

/// State transition matrix, row-major 6x6 (ROADMAP K8).
///
/// A wrapper around the array rather than a bare `[f64; 36]`, for exactly one
/// reason: so `phi[(i, j)]` reads as "row i, column j" and there is no place
/// to write the index the other way round. A transposed transition matrix is a
/// perfectly plausible matrix, and the error would surface as a strange
/// correction rather than a crash.
#[derive(Debug, Clone, Copy)]
pub struct Stm(pub [f64; STM_LEN]);

impl Stm {
    /// dy_i(t_end) / dy_j(t0), state in the order `(x, y, z, vx, vy, vz)`.
    pub fn get(&self, row: usize, col: usize) -> f64 {
        assert!(row < 6 && col < 6, "STM is 6x6, but ({row}, {col}) was asked for");
        self.0[row * 6 + col]
    }

    /// The raw 36 numbers in the order C gave them.
    pub fn as_slice(&self) -> &[f64] {
        &self.0
    }
}

/// The vessel as the force model sees it (K6b, K7b, `core/core.h`).
///
/// Gravity does not need it: there the vessel is a massless test particle,
/// which is a separation the architecture rests on, not an approximation.
/// Solar radiation pressure does need it, because its acceleration scales with
/// `Cr·A/m`. Drag, with `Cd·A/m`.
///
/// Passed **per run** rather than in the propagator config: mass changes while
/// burning, and `/game` keeps one propagator for all vessels
/// (`game/src/world.rs`) -- a vessel in the config would make them one ship
/// with several trajectories.
///
/// **One area for two forces.** The cross-section presented to the Sun and the
/// one presented to the air are a single number here: separating them would
/// mean modelling orientation, and orientation is the local level
/// (PROJECT.md §4), not this one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VesselParams {
    pub mass_kg: f64,
    /// Cross-section area, shared by light and air, m^2.
    pub area_m2: f64,
    /// 1 is full absorption, 2 a mirror; real spacecraft sit near 1.3.
    pub cr: f64,
    /// Drag coefficient; near 2.2 for a blunt body in low orbit.
    pub cd: f64,
}

impl VesselParams {
    fn raw(&self) -> core_sys::VesselParams {
        core_sys::VesselParams {
            mass_kg: self.mass_kg,
            area_m2: self.area_m2,
            cr: self.cr,
            cd: self.cd,
        }
    }
}

/// What one call to [`Propagator::run`] produced.
#[derive(Debug, Clone, Copy)]
pub struct Run {
    /// How many samples were written to the front of the supplied slice.
    pub filled: usize,
    /// The state the run stopped in. Continue from exactly this.
    pub final_state: State,
    pub stop: Stop,
}

/// A vessel propagator in the field of every body in the asset.
///
/// Holds an [`Arc`] to the ephemeris rather than a borrowed reference: the C
/// context keeps a raw pointer to it, so it must outlive the propagator. A
/// lifetime in the struct would express the same thing and infect everything
/// the propagator is stored in (CLAUDE.md: no lifetimes in structs).
///
/// ```no_run
/// use core_rs::{Ephemeris, Event, PropConfig, Propagator, State};
/// use std::path::Path;
/// use std::sync::Arc;
///
/// let eph = Arc::new(Ephemeris::load(Path::new("data/fixture/earth_moon.eph"))?);
/// let mut prop = Propagator::new(eph.clone(), PropConfig::default())?;
///
/// let mut samples = vec![State::default(); 256];
/// let mut step = 0.0;
/// let vessel = State::default();
///
/// let run = prop.run(&vessel, None, 86_400.0, &[Event::Periapsis { body: 3 }],
///                    &mut samples, &mut step)?;
/// println!("{:?} after {} samples", run.stop, run.filled);
/// # Ok::<(), core_rs::CoreError>(())
/// ```
///
/// Use after free does not compile:
///
/// ```compile_fail
/// use core_rs::{Ephemeris, PropConfig, Propagator, State};
/// use std::path::Path;
/// use std::sync::Arc;
///
/// let eph = Arc::new(Ephemeris::load(Path::new("data/fixture/earth_moon.eph")).unwrap());
/// let mut prop = Propagator::new(eph, PropConfig::default()).unwrap();
/// drop(prop);
/// let mut step = 0.0;
/// let _ = prop.run(&State::default(), None, 1.0, &[], &mut [], &mut step);
/// ```
///
/// There is nothing to free twice with either -- `prop_free` is not
/// re-exported and the field is private:
///
/// ```compile_fail
/// use core_rs::{Ephemeris, PropConfig, Propagator};
/// use std::path::Path;
/// use std::sync::Arc;
///
/// let eph = Arc::new(Ephemeris::load(Path::new("data/fixture/earth_moon.eph")).unwrap());
/// let prop = Propagator::new(eph, PropConfig::default()).unwrap();
/// let _ = prop.ctx;
/// ```
pub struct Propagator {
    // Keeps the ephemeris alive. Read only by drop order -- the field must
    // exist rather than be used.
    _eph: Arc<Ephemeris>,
    ctx: *mut core_sys::PropagatorCtx,
}

// A propagator can be handed to another thread -- which is exactly what
// happens once physics moves to its own (PROJECT.md §6). It owns its context,
// and the ephemeris that context looks at is already `Sync` (reasoning above).
//
// `Sync` is NOT declared, and that is not a forgotten line: the context inside
// C carries a sticky error flag that `prop_run` clears at the start of every
// run. Two threads holding `&Propagator` could not even call `run` -- it takes
// `&mut self` -- but there is no point claiming safety nobody has checked. One
// thread, one propagator.
unsafe impl Send for Propagator {}

impl Propagator {
    pub fn new(eph: Arc<Ephemeris>, cfg: PropConfig) -> Result<Propagator> {
        let raw = core_sys::PropConfig {
            integrator: match cfg.integrator {
                Integrator::Dop853 => core_sys::CORE_INTEG_DOP853,
                Integrator::Rkn => core_sys::CORE_INTEG_RKN,
            },
            tol_m: cfg.tol_m,
            h_max_s: cfg.h_max_s,
            max_steps: cfg.max_steps as std::ffi::c_long,
            density_scale: cfg.density_scale,
        };

        let mut ctx: *mut core_sys::PropagatorCtx = std::ptr::null_mut();

        // SAFETY: eph.ctx came from eph_load and is alive -- the `Arc` below
        // holds it at least as long as this propagator. `raw` lives to the end
        // of the call and C only reads it. `ctx` is a valid slot for one
        // pointer, and C writes there only on CORE_OK.
        let code = unsafe { core_sys::prop_create(eph.ctx, &raw, &mut ctx) };
        check(code)?;

        if ctx.is_null() {
            return Err(CoreError::Unknown(core_sys::CORE_OK));
        }

        Ok(Propagator { _eph: eph, ctx })
    }

    /// Integrates from `initial` to `t_end`, to the first event, or until
    /// `samples` runs out.
    ///
    /// `samples` may be empty -- then the run proceeds without sampling and
    /// stops only at `t_end` or on an event. It is the same integration, step
    /// for step, which is why physics and the prediction line can share one
    /// path (CLAUDE.md, invariant 5).
    ///
    /// `step` carries the integrator step between calls: 0 on the first, then
    /// whatever the previous one left. It goes into the save (PROJECT.md §4),
    /// and that is no formality: dropping it costs seventyfold work and a
    /// different trajectory (`core/test/test_prop.c`).
    ///
    /// `vessel` is `None` for a massless test particle, i.e. exactly what this
    /// call did before K6b. With `Some`, solar radiation pressure with a
    /// shadow model joins the forces.
    pub fn run(
        &mut self,
        initial: &State,
        vessel: Option<&VesselParams>,
        t_end: f64,
        events: &[Event],
        samples: &mut [State],
        step: &mut f64,
    ) -> Result<Run> {
        let raw_events: Vec<core_sys::CoreEvent> = events.iter().map(|e| e.raw()).collect();
        let raw_vessel = vessel.map(|v| v.raw());

        let mut count: usize = 0;
        let mut final_state = State::default();
        let mut stop: core_sys::CoreStopReason = -1;
        let mut event: std::ffi::c_int = -1;

        // An empty slice in Rust is NOT a null pointer but an aligned
        // dangling one, and C distinguishes the cases: it treats a buffer with
        // no room as a caller error, since the caller would spin without
        // progress. So emptiness is translated explicitly.
        let (out_ptr, out_cap) = if samples.is_empty() {
            (std::ptr::null_mut(), 0)
        } else {
            (samples.as_mut_ptr(), samples.len())
        };

        let events_ptr = if raw_events.is_empty() {
            std::ptr::null()
        } else {
            raw_events.as_ptr()
        };

        // `None` translates to a null pointer, which C reads as "massless
        // test particle" -- the same run as before K6b, bit for bit.
        let vessel_ptr = raw_vessel
            .as_ref()
            .map_or(std::ptr::null(), |v| v as *const _);

        // SAFETY: self.ctx came from prop_create and is not yet freed (only
        // Drop frees, and `&mut self` proves it has not run). `initial` and
        // `raw_events` live to the end of the call and are only read. The
        // buffer holds exactly `out_cap` `State` elements and C promises not
        // to write past them -- which is exactly why the capacity travels
        // beside it. `raw_vessel` lives on the stack to the end of the call
        // and is only read, and null there is a legal value, not an error. The
        // remaining pointers are stack slots for one value each.
        let code = unsafe {
            core_sys::prop_run(
                self.ctx,
                initial,
                vessel_ptr,
                t_end,
                events_ptr,
                raw_events.len(),
                out_ptr,
                out_cap,
                &mut count,
                &mut final_state,
                &mut stop,
                &mut event,
                step,
            )
        };
        check(code)?;

        let stop = match stop {
            core_sys::CORE_STOP_T_END => Stop::ReachedEnd,
            core_sys::CORE_STOP_BUFFER_FULL => Stop::BufferFull,
            core_sys::CORE_STOP_EVENT => {
                // The index comes from C and points into the slice the caller
                // supplied. Checked rather than trusted: it is about to be
                // used for slicing.
                if event < 0 || (event as usize) >= events.len() {
                    return Err(CoreError::Unknown(event));
                }
                Stop::Event(event as usize)
            }
            other => return Err(CoreError::Unknown(other)),
        };

        Ok(Run {
            filled: count,
            final_state,
            stop,
        })
    }

    /// The same integration as `run`, but carrying the state transition
    /// matrix
    /// (ROADMAP K8).
    ///
    /// Returns the final state and the row-major 6x6
    /// Phi = dy(t_end)/dy(initial), state in the order
    /// `(x, y, z, vx, vy, vz)`. This is what M3's differential correction asks
    /// for and what moves the covariance in M6.
    ///
    /// **The trajectory is bit-identical to what `run` would give** with the
    /// same arguments and the same `step` -- not "within tolerance". The step
    /// controller in C reads only the reference block, so the six variational
    /// blocks ride the same step sequence without voting on it
    /// (`core/test/test_prop.c` measures this). A planner correcting a
    /// manoeuvre with a matrix from a slightly different trajectory would aim
    /// where the vessel is not.
    ///
    /// No events here, deliberately: the question concerns one leg with two
    /// ends, and an event would cut it where the caller did not ask. Whoever
    /// wants both runs `run` first to find the event, then this on the leg
    /// found.
    pub fn run_stm(
        &mut self,
        initial: &State,
        vessel: Option<&VesselParams>,
        t_end: f64,
        step: &mut f64,
    ) -> Result<(State, Stm)> {
        let mut final_state = State::default();
        let mut phi = [0.0f64; STM_LEN];
        let raw_vessel = vessel.map(|v| v.raw());
        let vessel_ptr = raw_vessel
            .as_ref()
            .map_or(std::ptr::null(), |v| v as *const _);

        // SAFETY: self.ctx came from prop_create and is not yet freed (only
        // Drop frees, and `&mut self` proves it has not run). `initial` lives
        // to the end of the call and is only read. `phi` is exactly STM_LEN
        // contiguous values, as many as C declared in `out_stm[36]`; the
        // remaining pointers are stack slots for one value each.
        let code = unsafe {
            core_sys::prop_run_stm(
                self.ctx,
                initial,
                vessel_ptr,
                t_end,
                &mut final_state,
                phi.as_mut_ptr(),
                step,
            )
        };
        check(code)?;

        Ok((final_state, Stm(phi)))
    }
}

impl Drop for Propagator {
    fn drop(&mut self) {
        // SAFETY: the pointer came from prop_create and is freed exactly here,
        // exactly once -- the field is private, the type neither Copy nor
        // Clone.
        unsafe { core_sys::prop_free(self.ctx) };
    }
}

impl fmt::Debug for Propagator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Propagator")
    }
}

/// Lambert's problem: transfer velocities from `r1` to `r2` in `dt` seconds
/// about a body of gravitational parameter `mu` (ROADMAP L3, debt D1).
///
/// Returns `(v1, v2)`, the departure and arrival velocities in m/s.
///
/// **Outside the determinism boundary, and the only such function here.**
/// PROJECT.md §4: simulating a given plan must match bit for bit, while how
/// the player arrived at that plan need not. Lambert's result is **data** -- a
/// manoeuvre `(time, dv)` comes out of it, and executing that is what
/// reproduces exactly. So this function may use `libm` (it lives in a separate
/// `libcore_planning.a`), and its numbers are not part of the hash comparison.
///
/// `prograde` is the sign of the z component of angular momentum, **not**
/// "short or long arc". A caller not working in the ephemeris plane rotates
/// `r1` and `r2` into a plane where that holds before calling.
///
/// # Errors
///
/// [`CoreError::InvalidArg`] for `dt <= 0`, `mu <= 0`, `n_revs != 0`, or `r1`
/// and `r2` on one line through the origin (the transfer plane, and with it
/// the direction convention, are undefined there).
/// [`CoreError::ToleranceNotMet`] if Newton did not converge;
/// `core/test/test_lambert.c` records which geometries really do that.
///
/// ```no_run
/// use core_rs::{lambert_solve, Vec3d};
///
/// let r1 = Vec3d { x: 1.4959787e11, y: 0.0, z: 0.0 };
/// let r2 = Vec3d { x: -1.9e11, y: 1.1e11, z: 8.0e9 };
/// let (v1, _v2) = lambert_solve(r1, r2, 2.5e7, 1.32712440018e20, true, 0)?;
/// println!("{v1:?}");
/// # Ok::<(), core_rs::CoreError>(())
/// ```
/// A porkchop grid cell: when to leave, how long to fly and what it costs.
///
/// `v_inf` is the speed relative to the body at its own end: the first says
/// what is needed to break away, the second what you arrive with.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PorkchopPoint {
    pub t1: f64,
    pub tof: f64,
    pub v_inf_depart: f64,
    pub v_inf_arrive: f64,
}

/// Porkchop grid computed from the ephemeris (ROADMAP-UI.md, U5a).
///
/// Rust supplies the buffer, C fills it and returns a count (PROJECT.md §5,
/// rule 1), so the question of who frees never arises. The buffer is sized for
/// the whole grid: there cannot be more cells than pairs, and fewer is easy,
/// because where Lambert did not converge the cell is **skipped** -- an
/// expected part of the plot, not an error (ROADMAP, stage G).
///
/// A wrapper over `porkchop_compute_eph`, not over `porkchop_compute`: that
/// one takes callbacks, which is exactly why it does not cross the boundary
/// (invariant 7). The resolution lives in C -- see
/// `core/planning/porkchop.h`.
pub fn porkchop(
    eph: &Ephemeris,
    depart_body: i32,
    arrive_body: i32,
    mu: f64,
    prograde: bool,
    t1_grid: &[f64],
    tof_grid: &[f64],
) -> Result<Vec<PorkchopPoint>> {
    if t1_grid.is_empty() || tof_grid.is_empty() {
        return Err(CoreError::InvalidArg);
    }

    let mut out = vec![core_sys::PorkchopPoint::default(); t1_grid.len() * tof_grid.len()];
    let mut count: usize = 0;

    // SAFETY: the context is alive (borrowed), both grids are Rust slices
    // with their own lengths, and the buffer is ours with exactly the capacity
    // we declare. C owns none of it after returning.
    let code = unsafe {
        core_sys::porkchop_compute_eph(
            eph.ctx,
            depart_body,
            arrive_body,
            mu,
            i32::from(prograde),
            t1_grid.as_ptr(),
            t1_grid.len(),
            tof_grid.as_ptr(),
            tof_grid.len(),
            out.as_mut_ptr(),
            out.len(),
            &mut count,
        )
    };
    check(code)?;

    out.truncate(count);
    Ok(out
        .into_iter()
        .map(|p| PorkchopPoint {
            t1: p.t1,
            tof: p.tof,
            v_inf_depart: p.v_inf_depart,
            v_inf_arrive: p.v_inf_arrive,
        })
        .collect())
}

pub fn lambert_solve(
    r1: Vec3d,
    r2: Vec3d,
    dt: f64,
    mu: f64,
    prograde: bool,
    n_revs: i32,
) -> Result<(Vec3d, Vec3d)> {
    let mut v1 = Vec3d::default();
    let mut v2 = Vec3d::default();

    // SAFETY: both output pointers lead to locals alive to the end of the
    // call; the input structs are passed by value and copied on the C side.
    // The function allocates and stores nothing -- there is nothing to free,
    // hence no create/free pair here.
    let code = unsafe {
        core_sys::lambert_solve(
            r1,
            r2,
            dt,
            mu,
            i32::from(prograde),
            n_revs,
            &mut v1,
            &mut v2,
        )
    };
    check(code)?;

    Ok((v1, v2))
}

/// Mass fraction of a body pair, `mu = m2/(m1+m2)` -- the definition of the
/// system in which both the Jacobi constant and the zero-velocity curve are
/// computed (ROADMAP-UI.md,
/// U6b2).
///
/// One division, and it lives here anyway: two different `mu` in one frame
/// would mean a curve from one system over a trajectory from another.
pub fn cr3bp_mu(gm_primary: f64, gm_secondary: f64) -> f64 {
    // SAFETY: two `double` by value, no pointers and no state -- neither
    // lifetime nor freeing can go wrong here.
    unsafe { core_sys::cr3bp_mu(gm_primary, gm_secondary) }
}

/// Jacobi constant `C = 2*Omega - v^2`, **in dimensionless CR3BP units**.
///
/// The units are a convention, not a detail: bodies at unit distance, total
/// mass 1, frame rotating at unit angular velocity. Metres can be passed in,
/// and the result will resemble a Jacobi constant just closely enough to go
/// unnoticed.
pub fn cr3bp_jacobi(r: Vec3d, v: Vec3d, mu: f64) -> f64 {
    // SAFETY: both structs are passed by value and copied on the C side;
    // nothing is allocated or stored.
    unsafe { core_sys::cr3bp_jacobi(r, v, mu) }
}

/// Lagrange point 1..5 in dimensionless coordinates.
pub fn cr3bp_lagrange(mu: f64, point: i32) -> Result<Vec3d> {
    let mut out = Vec3d::default();
    // SAFETY: the output pointer leads to a local alive to the end of the
    // call; the remaining arguments are by value.
    let code = unsafe { core_sys::cr3bp_lagrange(mu, point, &mut out) };
    check(code)?;
    Ok(out)
}

/// Where the ray `from + r*dir_unit` crosses the zero-velocity curve, or
/// [`CoreError::ToleranceNotMet`] if it does not cross at all.
///
/// **That error is an answer, not a failure**, and the caller must tell them
/// apart: along such a ray the region is either wholly forbidden or wholly
/// open. Whoever treats it as a failure draws the curve where there is none.
///
/// `dir_unit` must be a unit vector -- the caller computes it, because
/// `cos`/`sin` are forbidden in `/core` (CLAUDE.md, invariant 3).
pub fn cr3bp_zvc_radius(mu: f64, c: f64, from: Vec3d, dir_unit: Vec3d, r_max: f64) -> Result<f64> {
    let mut r = 0.0;
    // SAFETY: the output pointer leads to a local; the input structs go by
    // value and are copied on the C side.
    let code = unsafe { core_sys::cr3bp_zvc_radius(mu, c, from, dir_unit, r_max, &mut r) };
    check(code)?;
    Ok(r)
}

/// Synodic frame of a real body pair at a specific instant (`core/frame.h`,
/// ROADMAP C4; on the boundary, U6b1).
///
/// Keeps the C struct whole and does not expose what nobody asked for: three
/// quantities defining the scale, plus the transform itself, go out.
#[derive(Clone, Copy, Debug)]
pub struct SynodicFrame {
    raw: core_sys::SynodicFrame,
}

impl SynodicFrame {
    /// `L`, the distance between the bodies at that instant, metres.
    pub fn length(&self) -> f64 {
        self.raw.length
    }

    /// `|omega|`, rad/s: one dimensionless unit of time.
    pub fn rate(&self) -> f64 {
        self.raw.rate
    }

    /// `mu` of the pair.
    pub fn mass_ratio(&self) -> f64 {
        self.raw.mu
    }

    /// Inertial state in metres -> dimensionless CR3BP state.
    ///
    /// **This is not merely rotating axes.** The velocity transforms along
    /// with the coordinates: in a rotating frame `-omega x r` is added to it,
    /// which is why the transform lives in C rather than beside the camera.
    pub fn from_inertial(&self, state: &State) -> State {
        let mut out = State::default();
        // SAFETY: both pointers lead to locals alive to the end of the call;
        // the function allocates nothing and returns no code -- a change of
        // coordinates cannot fail.
        unsafe { core_sys::frame_from_inertial(&self.raw, state, &mut out) };
        out
    }
}
