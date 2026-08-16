//! Raw FFI declarations for the numeric core (ROADMAP D2, PROJECT.md §5).
//!
//! Written **by hand**, not by bindgen. The boundary is small -- around 20
//! functions -- while bindgen pulls in a libclang dependency and generates
//! what nobody reads. Every line here must be read by eye, because this is the
//! one place where a mistake is not diagnosed: swapped struct fields or the
//! wrong integer type do not fail, they return plausible numbers.
//!
//! **There is no `unsafe` block here** -- only declarations. Our `unsafe`
//! lives in one place, `core-rs` (CLAUDE.md, invariant 1). Calling anything
//! from here directly out of `engine` or `game` is an architecture error, not
//! a shortcut.
//!
//! No safe wrapper, RAII or `Result` here either: that is D3.

#![no_std]

use core::ffi::{c_char, c_int, c_long};

/// Return code. In C this is `enum CoreResult`; here it is an integer.
///
/// Not a `#[repr(C)] enum`, and that is correctness rather than
/// simplification: if C ever returns a value outside the enumeration, a Rust
/// enum holding it is undefined behaviour, not a visible error. The raw layer
/// hands back exactly what C said; converting to a real `enum` with an
/// "anything else" arm is `core-rs`'s job, where there is room for it.
pub type CoreResult = c_int;

pub const CORE_OK: CoreResult = 0;
pub const CORE_ERR_BUFFER_TOO_SMALL: CoreResult = 1;
pub const CORE_ERR_TOLERANCE_NOT_MET: CoreResult = 2;
pub const CORE_ERR_INVALID_ARG: CoreResult = 3;

/// Synodic frame from `core/frame.h`.
///
/// Every field is declared, in the same order, though Rust reads few of them:
/// C writes into this struct, and a smaller struct would mean a write past its
/// end. A layout mistake here is not a strange number but corrupted memory.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SynodicFrame {
    /// Barycentre of the pair, inertial, metres.
    pub origin: Vec3d,
    pub origin_rate: Vec3d,
    /// Orthonormal basis in inertial components.
    pub x: Vec3d,
    pub y: Vec3d,
    pub z: Vec3d,
    /// Angular velocity of the basis, rad/s.
    pub omega: Vec3d,
    /// `L`, the distance between the bodies, metres.
    pub length: f64,
    /// `dL/dt`, m/s: zero in CR3BP, not here.
    pub length_rate: f64,
    /// `|omega|`, rad/s -- one dimensionless unit of time.
    pub rate: f64,
    /// `mu_S / (mu_P + mu_S)`.
    pub mu: f64,
    /// The instant the frame was built for.
    pub t: f64,
}

/// `Vec3d` from `core/vec3.h`. Metres, or metres per second.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3d {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// `Quat` from `core/quat.h`: unit quaternion, `w` first.
///
/// Field order is part of the contract, and the easiest part to get wrong:
/// half the world writes `(x, y, z, w)`. A misplaced `w` would give a
/// perfectly plausible rotation -- just not the right one -- and the only sign
/// would be a planet facing the wrong way. Hence the oracle prints all four
/// components separately (`core-sys/tests/ffi.rs`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    pub w: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Default for Quat {
    /// Identity, not zero: a zero quaternion is not a rotation at all, and a
    /// default value has no business being impossible.
    fn default() -> Self {
        Quat {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }
}

/// `State` from `core/core.h`: barycentric inertial frame, metres, m/s, `t` in
/// seconds from the loaded ephemeris epoch.
///
/// Field order is part of the boundary contract, not a detail. `core.h` says
/// outright that this must stay a plain struct of `double` with no alignment
/// surprises, precisely so it can be declared like this.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct State {
    pub r: Vec3d,
    pub v: Vec3d,
    pub t: f64,
}

/// `VesselParams` from `core/core.h` (ROADMAP K6b, K7b).
///
/// Gravity does not need it -- there the vessel is a massless test particle,
/// which is a separation the architecture rests on, not an approximation.
/// Radiation pressure does need it: its acceleration scales with `Cr·A/m`, a
/// property of the vessel itself. Drag likewise, with `Cd·A/m`.
///
/// `cd` waited for K7b and the atmosphere that reads it: before that it would
/// have been a field the caller fills and the core ignores, with nothing to
/// say so.
///
/// **Field order is a contract**, and this is where it breaks most quietly:
/// swapped `cr` and `cd` would give a perfectly plausible trajectory, just not
/// the right one. Hence `core-sys/oracle.c` runs a leg with both non-zero and
/// `tests/ffi.rs` compares bits.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VesselParams {
    pub mass_kg: f64,
    pub area_m2: f64,
    pub cr: f64,
    pub cd: f64,
}

/// Opaque ephemeris handle.
///
/// The empty private field is deliberate: it makes the type unconstructible
/// outside the crate, so the only way to obtain a `*mut EphemerisCtx` is
/// `eph_load`. The struct's layout lives in `core/ephemeris.c` and Rust
/// neither knows nor should know it (PROJECT.md §5, rule 2).
#[repr(C)]
pub struct EphemerisCtx {
    _opaque: [u8; 0],
}

/// Opaque propagator handle (`core/prop.h`, ROADMAP H3).
///
/// It borrows the ephemeris and does not own it: the ephemeris context must
/// outlive every propagator built on it. In `core-rs` that is a type rather
/// than a promise -- the wrapper holds an `Arc`.
#[repr(C)]
pub struct PropagatorCtx {
    _opaque: [u8; 0],
}

/// Integrator choice. `CoreIntegrator` is an `enum` in C too, i.e. an `int`.
pub type CoreIntegrator = c_int;

pub const CORE_INTEG_DOP853: CoreIntegrator = 0;
pub const CORE_INTEG_RKN: CoreIntegrator = 1;

/// Why the run ended. Values outside the enumeration are as inadmissible for
/// a Rust enum here as in `CoreResult`, so this is an integer.
pub type CoreStopReason = c_int;

pub const CORE_STOP_T_END: CoreStopReason = 0;
pub const CORE_STOP_BUFFER_FULL: CoreStopReason = 1;
pub const CORE_STOP_EVENT: CoreStopReason = 2;

pub type CoreEventKind = c_int;

pub const CORE_EVENT_PERIAPSIS: CoreEventKind = 0;
pub const CORE_EVENT_APOAPSIS: CoreEventKind = 1;
pub const CORE_EVENT_DISTANCE: CoreEventKind = 2;
/// Altitude above the body surface -- above the asset's mean radius (K7c).
pub const CORE_EVENT_ALTITUDE: CoreEventKind = 3;

/// How many events `prop_run` accepts at once (`PROP_MAX_EVENTS`).
pub const PROP_MAX_EVENTS: usize = 8;

/// `PropConfig` from `core/prop.h`.
///
/// `max_steps` is `c_long` because in C it is `long`: 64 bits on Linux and
/// macOS, 32 on Windows. `c_long` follows the platform the same way, so both
/// sides agree. It is a step-count limit, not arithmetic, so the differing
/// width does not touch determinism.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PropConfig {
    pub integrator: CoreIntegrator,
    pub tol_m: f64,
    pub h_max_s: f64,
    pub max_steps: c_long,
    /// Air density multiplier (ROADMAP K7c). **Must be positive**:
    /// `prop_create` rejects zero rather than reading it as one, because an
    /// unset field is the same bug that fell over on Windows in K7b.
    pub density_scale: f64,
}

/// `CoreEvent` from `core/prop.h`: an event described by data.
///
/// A struct like this -- `enum`, `int`, `double` in a row -- is exactly where
/// alignment diverges quietly, so `tests/ffi.rs` compares a run with an armed
/// event against C bitwise.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CoreEvent {
    pub kind: CoreEventKind,
    pub body_id: c_int,
    pub param: f64,
}

/// A cell of the porkchop grid (`core/planning/porkchop.h`).
///
/// Both `v_inf` are speeds **relative to the body** at their own end, i.e.
/// what departure and arrival cost. Fields in the same order as in C; swapped
/// `t1` and `tof` would give a perfectly plausible plot, which is why the
/// oracle compares them bitwise.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PorkchopPoint {
    pub t1: f64,
    pub tof: f64,
    pub v_inf_depart: f64,
    pub v_inf_arrive: f64,
}

extern "C" {
    /// Loads a cooked asset. `path` must be a C string with a `\0`.
    ///
    /// One of only two allocating functions in the whole API. Paired with
    /// [`eph_free`]; breaking that pair is the boundary's only leak risk, and
    /// the construction of the types in `core-rs` makes it impossible (D3).
    pub fn eph_load(path: *const c_char, out: *mut *mut EphemerisCtx) -> CoreResult;

    /// Frees the context. `NULL` is allowed.
    pub fn eph_free(ctx: *mut EphemerisCtx);

    /// Mean body radius in metres, or zero (ROADMAP U2a).
    ///
    /// Zero is an answer, not an error, and means exactly one thing: **the
    /// asset does not say** how large the body is. An unknown body gives the
    /// same zero deliberately -- two distinct values for the one case "no size
    /// available" would have to be told apart by every caller, and this is
    /// already settled the same way for the SRP shadow (K6a).
    ///
    /// Returns no [`CoreResult`], because in C this is a plain field read --
    /// no time, no Chebyshev evaluation and no way to fail.
    pub fn eph_body_radius(ctx: *const EphemerisCtx, body: c_int) -> f64;

    /// Gravitational parameter of the body, m^3/s^2 (ROADMAP-UI.md, U5a).
    ///
    /// Same contract as [`eph_body_radius`]: a field read, zero for an unknown
    /// body, no return code. It arrived with the porkchop grid, which needs
    /// `mu` as an argument -- and taking that from a constant in the game
    /// would mean flying near a different Earth than the integrator.
    pub fn eph_body_mu(ctx: *const EphemerisCtx, body: c_int) -> f64;

    /// Body orientation at time `t` (ROADMAP-PLANETS.md, R1c).
    ///
    /// A quaternion taking vector components from the body's own frame into
    /// the ephemeris frame (`core/quat.h` convention). A body with no rotation
    /// model returns the identity and [`CORE_OK`]: "rotation not modelled" is
    /// an answer, just like a zero degree from the harmonics.
    ///
    /// Unlike radius and `mu` this has a return code, and that is not
    /// inconsistency: there is **time** here, hence a way to fail -- a
    /// Chebyshev fit outside the asset's span returns confident nonsense.
    ///
    /// It deliberately does not return the derivative (angular velocity):
    /// rendering does not need it, and whoever does gets both from one pass
    /// inside the core (`eph_body_angular_velocity`, debt D8).
    pub fn eph_body_orientation(
        ctx: *const EphemerisCtx,
        body: c_int,
        t: f64,
        out: *mut Quat,
    ) -> CoreResult;

    /// Position and velocity of the body at time `t`.
    ///
    /// Returns [`CORE_ERR_INVALID_ARG`] for an unknown body or a time outside
    /// the asset's span: extrapolating a Chebyshev fit gives confident
    /// nonsense, so going out of range is an event the caller must hear
    /// about.
    pub fn eph_body_state(
        ctx: *const EphemerisCtx,
        body: c_int,
        t: f64,
        out: *mut State,
    ) -> CoreResult;

    /// Creates a propagator over the ephemeris. The second (and last)
    /// allocating pair of the boundary; `prop_free(NULL)` is allowed.
    pub fn prop_create(
        eph: *const EphemerisCtx,
        cfg: *const PropConfig,
        out: *mut *mut PropagatorCtx,
    ) -> CoreResult;

    /// Frees the context. `NULL` is allowed.
    pub fn prop_free(p: *mut PropagatorCtx);

    /// Integrates the vessel from `initial` to `t_end`, to the first armed
    /// event, or until `out_states` fills up.
    ///
    /// **Rust** supplies the buffer and C only fills it and returns the actual
    /// count (PROJECT.md §5, rule 1) -- so the question of who frees it never
    /// arises.
    ///
    /// `in_out_step` carries the integrator step between calls. Zero on the
    /// first call means "choose one yourself"; afterwards, pass back whatever
    /// the function left there, or the trajectory differs -- measured in
    /// `core/test/test_prop.c`.
    #[allow(clippy::too_many_arguments)]
    pub fn prop_run(
        p: *mut PropagatorCtx,
        initial: *const State,
        vessel: *const VesselParams,
        t_end: f64,
        events: *const CoreEvent,
        n_events: usize,
        out_states: *mut State,
        out_cap: usize,
        out_count: *mut usize,
        out_final: *mut State,
        out_stop: *mut CoreStopReason,
        out_event: *mut c_int,
        in_out_step: *mut f64,
    ) -> CoreResult;

    /// The same integration, but carrying the state transition matrix (K8).
    ///
    /// `out_stm` is row-major 6x6, i.e. exactly `STM_SIZE` = 36 `f64`, with
    /// state order `(x, y, z, vx, vy, vz)`. Rust supplies the buffer, as
    /// everywhere on this boundary.
    ///
    /// **The trajectory is bit-identical to `prop_run`** -- not "within
    /// tolerance": the step controller in `core/dop853.c` reads only block 0,
    /// so the six variational blocks ride the same step sequence without
    /// influencing it. Measured in `core/test/test_prop.c`. This is CLAUDE.md
    /// invariant 5 in the place it is easiest to lose.
    ///
    /// No events and no sample buffer here, deliberately: the question "where
    /// does a change in the initial state reach by `t_end`" concerns one leg
    /// with two ends, and an event would cut it where the caller did not ask.
    ///
    /// `vessel` as in `prop_run`: null means a massless test particle.
    pub fn prop_run_stm(
        p: *mut PropagatorCtx,
        initial: *const State,
        vessel: *const VesselParams,
        t_end: f64,
        out_final: *mut State,
        out_stm: *mut f64,
        in_out_step: *mut f64,
    ) -> CoreResult;

    /// Porkchop grid computed **from the ephemeris** (ROADMAP-UI.md, U5a).
    ///
    /// The tenth boundary function and the second outside the determinism
    /// zone. The existing `porkchop_compute` takes two callbacks, which is
    /// exactly why it does not cross the boundary (invariant 7): a callback
    /// would mean C calling Rust inside its own loop. The resolution is in C,
    /// not Rust: the wrapper feeds `eph_body_state` itself and the boundary
    /// stays batched -- Rust gave a buffer, C filled it and returned a count.
    ///
    /// `out_count` is filled even when [`CORE_ERR_BUFFER_TOO_SMALL`] is
    /// returned: the same convention as `prop_run`.
    #[allow(clippy::too_many_arguments)]
    pub fn porkchop_compute_eph(
        eph: *const EphemerisCtx,
        depart_body: c_int,
        arrive_body: c_int,
        mu: f64,
        prograde: c_int,
        t1_grid: *const f64,
        n_t1: usize,
        tof_grid: *const f64,
        n_tof: usize,
        out: *mut PorkchopPoint,
        out_cap: usize,
        out_count: *mut usize,
    ) -> CoreResult;

    /// Synodic frame of a real pair of bodies at time `t` (`core/frame.h`,
    /// ROADMAP C4).
    ///
    /// Needed wherever synodic **velocity** is needed: the game computes the
    /// position itself from what is in the sample, but velocity in a rotating
    /// frame is `-omega x r` on top of the rotation, i.e. physics rather than
    /// shuffling axes (ROADMAP-UI.md, U6b1).
    pub fn frame_synodic(
        eph: *const EphemerisCtx,
        primary: c_int,
        secondary: c_int,
        t: f64,
        out: *mut SynodicFrame,
    ) -> CoreResult;

    /// Inertial state in metres -> dimensionless CR3BP state of this frame.
    ///
    /// Returns no code: either the frame was built or it does not exist, and
    /// the change of coordinates itself cannot fail.
    pub fn frame_from_inertial(f: *const SynodicFrame, input: *const State, out: *mut State);

    /// `mu` of a two-body system: `m2 / (m1 + m2)`, dimensionless
    /// (`core/cr3bp.h`, ROADMAP C1).
    ///
    /// One division -- and that is exactly why it lives here rather than in
    /// Rust: it defines the system in which both the Jacobi constant and the
    /// zero-velocity curve are computed. Two different `mu` in one frame would
    /// mean a curve from one system drawn over a trajectory from another.
    pub fn cr3bp_mu(gm_primary: f64, gm_secondary: f64) -> f64;

    /// Jacobi constant `C = 2*Omega - v^2` (`core/cr3bp.h`, ROADMAP C1).
    ///
    /// **Dimensionless, in the CR3BP normalisation**: bodies at unit distance,
    /// total mass 1, frame rotating at unit angular velocity. A caller passing
    /// metres and metres per second gets a number that looks like a Jacobi
    /// constant and means nothing -- the convention is checked not by the
    /// compiler but by a test against an external value (U6b2).
    ///
    /// Takes both vectors **by value**, like `lambert_solve`.
    pub fn cr3bp_jacobi(r: Vec3d, v: Vec3d, mu: f64) -> f64;

    /// Lagrange points 1..5 in dimensionless coordinates (`core/cr3bp.h`).
    ///
    /// Needed by the game's checks rather than the game: "the curve closes
    /// before L1" without L1 is a claim without a number. L4 and L5 are exact;
    /// L1-L3 are found by bisection on the x axis.
    pub fn cr3bp_lagrange(mu: f64, point: c_int, out: *mut Vec3d) -> CoreResult;

    /// Where the ray `from + r*dir_unit` crosses the zero-velocity curve
    /// (`core/cr3bp.h`, ROADMAP G4).
    ///
    /// The curve is built from rays because that is how C hands it over: the
    /// root search lives in C, and Rust only turns an angle into a unit vector
    /// -- the very `cos`/`sin` that cannot exist in C (invariant 3).
    ///
    /// [`CORE_ERR_TOLERANCE_NOT_MET`] is **an answer, not an error**: along
    /// this ray the region is either wholly forbidden or wholly open, and
    /// there is no crossing. A caller treating it as a failure will draw the
    /// curve where there is none.
    pub fn cr3bp_zvc_radius(
        mu: f64,
        c: f64,
        from: Vec3d,
        dir_unit: Vec3d,
        r_max: f64,
        r_out: *mut f64,
    ) -> CoreResult;

    /// Lambert's problem: velocities of the orbit flying from `r1` to `r2` in
    /// `dt` (`core/planning/lambert.h`, ROADMAP L3).
    ///
    /// **The first boundary function outside the determinism zone, and
    /// deliberately so.** PROJECT.md §4: the determinism boundary runs along
    /// propagation, not planning -- the result here is **data** (`time, dv`),
    /// and what comes out of it must match bitwise, while how the player
    /// arrived at it need not. So it lives in `libcore_planning.a`, calls
    /// `libm` freely, and for the same reason has its **own** oracle
    /// (`core-sys/oracle_planning.c`): the existing one links without libm as
    /// the runtime-zone check.
    ///
    /// **The first boundary function taking a struct by value.** `Vec3d` is
    /// three `double`, i.e. 24 bytes: under none of our ABIs does it fit in
    /// registers, so it travels through memory. A divergence here would not
    /// fail but return plausible velocities, so the oracle comparison is
    /// bitwise.
    ///
    /// `prograde` is the sign of the z component of angular momentum, **not**
    /// "short or long arc". `n_revs` must be 0: the multi-revolution case
    /// brackets more than one root of the same equation and needs its own
    /// scheme, which C does not have yet.
    ///
    /// Returns [`CORE_ERR_INVALID_ARG`] for `dt <= 0`, `mu <= 0`,
    /// `n_revs != 0` and for `r1`, `r2` collinear through the origin;
    /// [`CORE_ERR_TOLERANCE_NOT_MET`] if Newton did not converge.
    pub fn lambert_solve(
        r1: Vec3d,
        r2: Vec3d,
        dt: f64,
        mu: f64,
        prograde: c_int,
        n_revs: c_int,
        v1_out: *mut Vec3d,
        v2_out: *mut Vec3d,
    ) -> CoreResult;
}
