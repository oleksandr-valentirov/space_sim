//! The D3 check: the wrapper changes nothing and leaks nothing.
//!
//! Two distinct claims, checked differently.
//!
//! **Numbers.** The wrapper may not adjust anything on the way, so what it
//! returns is compared bitwise against what the raw call gives. The D2 test
//! has already compared the raw layer against C, so the chain
//! C -> core-sys -> core-rs closes.
//!
//! **Memory.** That double free and use after free are impossible is shown by
//! the `compile_fail` doctests in `src/lib.rs`: a property of the types,
//! checked by the compiler rather than by a run. That freeing actually happens
//! (rather than merely not crashing) is caught by a tool -- see the "Valgrind"
//! step in CI.

use std::path::{Path, PathBuf};

use core_rs::{CoreError, Ephemeris};

const DAY: f64 = 86400.0;

fn repo_root() -> PathBuf {
    // cargo tests run from the crate root, while the asset lives in the
    // repository root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core-rs must live inside the repository")
        .to_path_buf()
}

fn fixture() -> PathBuf {
    repo_root().join("data/fixture/earth_moon.eph")
}

fn load() -> Ephemeris {
    Ephemeris::load(&fixture()).expect("the fixture must read from the repository root")
}

/// The wrapper returns exactly the same bits as the raw call.
///
/// Not "within tolerance": any difference here would mean something happened
/// on the way -- a type conversion, a copy by another route -- and there is
/// deliberately no such layer.
#[test]
fn wrapper_returns_the_same_bits_as_the_raw_call() {
    let eph = load();

    let mut raw_ctx: *mut core_sys::EphemerisCtx = std::ptr::null_mut();
    let path = fixture();
    let c_path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();

    // SAFETY: a second, independent context over the same file. A test of the
    // raw layer is the only place outside core-rs where we write unsafe, and
    // that is exactly why it is here: otherwise there would be nothing to
    // compare against.
    unsafe {
        assert_eq!(
            core_sys::eph_load(c_path.as_ptr(), &mut raw_ctx),
            core_sys::CORE_OK
        );
    }

    for body in [0, 3, 4] {
        for t in [0.0, 30.0 * DAY, 119.0 * DAY] {
            let safe = eph.body_state(body, t).expect("instant inside the span");

            let mut raw = core_sys::State::default();
            // SAFETY: raw_ctx was just loaded and is not yet freed.
            let code = unsafe { core_sys::eph_body_state(raw_ctx, body, t, &mut raw) };
            assert_eq!(code, core_sys::CORE_OK);

            for (i, (a, b)) in [
                (safe.r.x, raw.r.x),
                (safe.r.y, raw.r.y),
                (safe.r.z, raw.r.z),
                (safe.v.x, raw.v.x),
                (safe.v.y, raw.v.y),
                (safe.v.z, raw.v.z),
                (safe.t, raw.t),
            ]
            .iter()
            .enumerate()
            {
                assert_eq!(
                    a.to_bits(),
                    b.to_bits(),
                    "body {body}, time {t}, component {i}: the wrapper changed the number"
                );
            }
        }
    }

    // SAFETY: the context is still alive and is freed exactly once.
    unsafe { core_sys::eph_free(raw_ctx) };
}

/// C return codes become typed errors rather than disappearing.
#[test]
fn errors_arrive_as_errors() {
    let eph = load();

    for (label, body, t) in [
        ("time before the start", 0, -DAY),
        ("time after the end", 0, 200.0 * DAY),
        ("negative body", -1, 0.0),
        ("body past the list", 999, 0.0),
    ] {
        assert_eq!(
            eph.body_state(body, t),
            Err(CoreError::InvalidArg),
            "{label} should have given InvalidArg"
        );
    }

    // And the converse: inside the span, success. Without this the check above
    // would "pass" for a wrapper that always returns InvalidArg.
    assert!(eph.body_state(0, 0.0).is_ok());
}

#[test]
fn a_missing_file_is_an_error_not_a_panic() {
    let missing = repo_root().join("data/fixture/no-such-file.eph");
    assert!(matches!(
        Ephemeris::load(&missing),
        Err(CoreError::InvalidArg)
    ));
}

/// A path with an interior `\0` cannot become a C string. That is our side's
/// error -- the core never sees it -- and it must be its own, not impersonate
/// a core error.
#[test]
fn a_path_with_a_nul_is_rejected_before_c_sees_it() {
    // matches!, not assert_eq!: `Ephemeris` is deliberately not PartialEq --
    // its equality would mean nothing, since it owns a handle rather than
    // being a value.
    let bad = Path::new("data/fixture/earth\0moon.eph");
    assert!(matches!(Ephemeris::load(bad), Err(CoreError::BadPath)));
}

/// Many loads and frees in a row.
///
/// The test proves nothing on its own -- it passes with a leak too. It exists
/// to give CI's Valgrind something to measure: a leak on one load is easy to
/// miss, a leak on fifty is not.
#[test]
fn loading_and_dropping_repeatedly_is_clean() {
    for _ in 0..50 {
        let eph = load();
        assert!(eph.body_state(4, 0.0).is_ok());
    }
}

/// `Send` and `Sync` are a promise justified by reading the C. Here is its
/// use.
#[test]
fn the_handle_can_be_shared_between_threads() {
    use std::sync::Arc;

    let eph = Arc::new(load());
    let mut handles = Vec::new();

    for _ in 0..4 {
        let shared = Arc::clone(&eph);
        handles.push(std::thread::spawn(move || {
            shared.body_state(4, 0.0).map(|s| s.r.x)
        }));
    }

    let first = eph.body_state(4, 0.0).unwrap().r.x;
    for handle in handles {
        let got = handle.join().expect("the thread should not have panicked").unwrap();
        assert_eq!(
            got.to_bits(),
            first.to_bits(),
            "concurrent reads diverged"
        );
    }
}

// ---------------------------------------------------------------------------
// Propagator (ROADMAP H4)
// ---------------------------------------------------------------------------

use std::sync::Arc;

use core_rs::{Event, Integrator, PropConfig, Propagator, State, Stm, Stop, STM_LEN};

const VESSEL_T0: f64 = DAY;
const VESSEL_DX: f64 = 42_164.0e3;
const VESSEL_VY: f64 = 1967.84;
const VESSEL_VZ: f64 = 1475.88;

const EARTH: i32 = 3;

/// The same vessel as in `core-sys/oracle.c`: an elongated Earth orbit given
/// as numbers, with a periapsis worth searching for.
fn vessel(eph: &Ephemeris) -> State {
    let earth = eph
        .body_state(EARTH, VESSEL_T0)
        .expect("Earth is within the asset");

    let mut s = State {
        r: earth.r,
        v: earth.v,
        t: VESSEL_T0,
    };
    s.r.x += VESSEL_DX;
    s.v.y += VESSEL_VY;
    s.v.z += VESSEL_VZ;
    s
}

fn config() -> PropConfig {
    PropConfig {
        integrator: Integrator::Dop853,
        tol_m: 1e-2,
        h_max_s: 1800.0,
        max_steps: 0,
        density_scale: 1.0,
    }
}

fn same_bits(a: &State, b: &State) -> bool {
    a.r.x.to_bits() == b.r.x.to_bits()
        && a.r.y.to_bits() == b.r.y.to_bits()
        && a.r.z.to_bits() == b.r.z.to_bits()
        && a.v.x.to_bits() == b.v.x.to_bits()
        && a.v.y.to_bits() == b.v.y.to_bits()
        && a.v.z.to_bits() == b.v.z.to_bits()
        && a.t.to_bits() == b.t.to_bits()
}

/// The propagator wrapper changes no bit against the raw call.
///
/// The same requirement as for `Ephemeris` above, for the same reason:
/// `core-sys` is already compared against the C oracle, so the chain
/// C -> core-sys -> core-rs closes with this test.
#[test]
fn propagation_matches_the_raw_call_bit_for_bit() {
    const CAP: usize = 64;

    let eph = Arc::new(load());
    let start = vessel(&eph);

    let mut prop = Propagator::new(eph.clone(), config()).expect("the propagator must create");

    let mut samples = vec![State::default(); CAP];
    let mut step = 0.0;
    let run = prop
        .run(
            &start,
            None,
            VESSEL_T0 + 0.5 * DAY,
            &[],
            &mut samples,
            &mut step,
        )
        .expect("the run must succeed");

    // The raw path, the same buffer, the same numbers.
    let mut raw_samples = vec![State::default(); CAP];
    let mut raw_count: usize = 0;
    let mut raw_final = State::default();
    let mut raw_stop: core_sys::CoreStopReason = -1;
    let mut raw_event: i32 = -1;
    let mut raw_step = 0.0;

    unsafe {
        let raw_cfg = core_sys::PropConfig {
            integrator: core_sys::CORE_INTEG_DOP853,
            tol_m: 1e-2,
            h_max_s: 1800.0,
            max_steps: 0,
            density_scale: 1.0,
        };
        let mut raw_eph: *mut core_sys::EphemerisCtx = std::ptr::null_mut();
        let path = std::ffi::CString::new(fixture().to_str().unwrap()).unwrap();
        assert_eq!(
            core_sys::eph_load(path.as_ptr(), &mut raw_eph),
            core_sys::CORE_OK
        );

        let mut raw_prop: *mut core_sys::PropagatorCtx = std::ptr::null_mut();
        assert_eq!(
            core_sys::prop_create(raw_eph, &raw_cfg, &mut raw_prop),
            core_sys::CORE_OK
        );

        assert_eq!(
            core_sys::prop_run(
                raw_prop,
                &start,
                std::ptr::null(),
                VESSEL_T0 + 0.5 * DAY,
                std::ptr::null(),
                0,
                raw_samples.as_mut_ptr(),
                CAP,
                &mut raw_count,
                &mut raw_final,
                &mut raw_stop,
                &mut raw_event,
                &mut raw_step,
            ),
            core_sys::CORE_OK
        );

        core_sys::prop_free(raw_prop);
        core_sys::eph_free(raw_eph);
    }

    assert_eq!(run.filled, raw_count, "sample count");
    assert!(
        run.filled > 0,
        "a run with no samples proves nothing"
    );
    for (i, (safe, raw)) in samples[..run.filled]
        .iter()
        .zip(raw_samples[..raw_count].iter())
        .enumerate()
    {
        assert!(
            same_bits(safe, raw),
            "sample {i} diverged from the raw call"
        );
    }
    assert!(same_bits(&run.final_state, &raw_final), "final state");
    assert_eq!(step.to_bits(), raw_step.to_bits(), "carried step");
}

/// An empty slice means "no sampling", not "the buffer is already full".
///
/// The difference is not cosmetic: an empty slice in Rust is an aligned
/// dangling pointer, not null, and had it gone into C as a buffer the run
/// would stop at once without progress and the caller would spin forever.
#[test]
fn an_empty_slice_means_no_sampling() {
    let eph = Arc::new(load());
    let start = vessel(&eph);
    let mut prop = Propagator::new(eph, config()).unwrap();

    let mut step = 0.0;
    let run = prop
        .run(&start, None, VESSEL_T0 + 0.5 * DAY, &[], &mut [], &mut step)
        .expect("a run without samples must succeed");

    assert_eq!(run.filled, 0);
    assert_eq!(run.stop, Stop::ReachedEnd);
    assert!(run.final_state.t > start.t, "time must have advanced");
}

/// An event arrives as an event, with an index into the supplied slice.
#[test]
fn events_come_back_as_events() {
    let eph = Arc::new(load());
    let start = vessel(&eph);
    let mut prop = Propagator::new(eph.clone(), config()).unwrap();

    let events = [
        Event::Apoapsis { body: EARTH },
        Event::Periapsis { body: EARTH },
    ];

    let mut step = 0.0;
    let run = prop
        .run(
            &start,
            None,
            VESSEL_T0 + 4.0 * DAY,
            &events,
            &mut [],
            &mut step,
        )
        .expect("the run must succeed");

    // The vessel starts exactly at apoapsis, so periapsis must come first --
    // that is index 1, and it is what proves the index is neither invented nor
    // zero by default.
    assert_eq!(run.stop, Stop::Event(1));

    let earth = eph.body_state(EARTH, run.final_state.t).unwrap();
    let dx = run.final_state.r.x - earth.r.x;
    let dy = run.final_state.r.y - earth.r.y;
    let dz = run.final_state.r.z - earth.r.z;
    let r = (dx * dx + dy * dy + dz * dz).sqrt();
    assert!(r < VESSEL_DX, "periapsis must be closer than the start: {r} m");
}

/// Altitude arrives as altitude, not as distance (ROADMAP K7c).
///
/// The oracle here is the pair of events itself, with no number about Earth in
/// the test. The same figure, given as an altitude and as a distance, stops
/// the vessel at two different radii, and **the difference between them is the
/// body radius**. The test only has to say that this really is Earth's radius
/// -- to within a hundred kilometres, i.e. at the level of a fact about the
/// world rather than a number from the asset.
///
/// Had the variant collapsed into `Event::Distance`, the difference would be
/// zero.
#[test]
fn altitude_is_measured_from_the_surface() {
    let eph = Arc::new(load());
    let start = vessel(&eph);
    let mut prop = Propagator::new(eph.clone(), config()).unwrap();

    const METRES: f64 = 30_000.0e3;

    let radius_at = |run: &core_rs::Run| {
        let earth = eph.body_state(EARTH, run.final_state.t).unwrap();
        let dx = run.final_state.r.x - earth.r.x;
        let dy = run.final_state.r.y - earth.r.y;
        let dz = run.final_state.r.z - earth.r.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    };

    let mut step = 0.0;
    let high = prop
        .run(
            &start,
            None,
            VESSEL_T0 + 4.0 * DAY,
            &[Event::Altitude {
                body: EARTH,
                metres: METRES,
            }],
            &mut [],
            &mut step,
        )
        .expect("the run must succeed");
    assert_eq!(high.stop, Stop::Event(0));

    let mut step = 0.0;
    let low = prop
        .run(
            &start,
            None,
            VESSEL_T0 + 4.0 * DAY,
            &[Event::Distance {
                body: EARTH,
                metres: METRES,
            }],
            &mut [],
            &mut step,
        )
        .expect("the run must succeed");
    assert_eq!(low.stop, Stop::Event(0));

    // The altitude is crossed earlier: it lies further from the centre.
    assert!(high.final_state.t < low.final_state.t);

    // The difference of the two events is the radius, and it need no longer be
    // guessed with a "somewhere between 6.3 and 6.4 thousand kilometres" fork
    // -- the asset can be asked (ROADMAP U2a). No number about Earth is left
    // in the test: both sides of the equality come from data.
    //
    // The tolerance is in metres and is about the integrator, not the radius:
    // the event is found by a root search within a step, so the equality here
    // is as exact as the last step's landing.
    let measured = radius_at(&high) - radius_at(&low);
    let stated = eph.body_radius(EARTH);
    assert!(
        stated > 0.0,
        "the fixture must name Earth\'s size, or the check is empty"
    );
    assert!(
        (measured - stated).abs() < 1.0,
        "the event difference must be the asset radius: {measured} against {stated} m"
    );
}

/// A body whose size the asset does not name gives zero -- and so does an
/// unknown body
/// (ROADMAP U2a).
///
/// Both sides are required. A zero-only test would pass for a wrapper that
/// always returns zero; an Earth-only test, for one that ignores its
/// argument.
#[test]
fn a_body_without_a_size_answers_zero() {
    let eph = load();

    assert!(eph.body_radius(EARTH) > 0.0, "Earth has a radius in the fixture");
    assert_eq!(eph.body_radius(-1), 0.0, "negative body index");
    assert_eq!(eph.body_radius(999), 0.0, "index past the list");
}

/// A negative altitude is a sign error in the caller, and the boundary says
/// so.
#[test]
fn a_negative_altitude_is_refused() {
    let eph = Arc::new(load());
    let start = vessel(&eph);
    let mut prop = Propagator::new(eph, config()).unwrap();

    let mut step = 0.0;
    let err = prop
        .run(
            &start,
            None,
            VESSEL_T0 + DAY,
            &[Event::Altitude {
                body: EARTH,
                metres: -1.0,
            }],
            &mut [],
            &mut step,
        )
        .expect_err("a negative altitude must be rejected");
    assert!(matches!(err, CoreError::InvalidArg), "{err:?}");
}

/// A zero density multiplier is not read as one (ROADMAP K7c).
#[test]
fn a_zero_density_scale_is_refused() {
    let eph = Arc::new(load());
    let mut cfg = config();
    cfg.density_scale = 0.0;

    let err = Propagator::new(eph, cfg).expect_err("zero must be rejected");
    assert!(matches!(err, CoreError::InvalidArg), "{err:?}");
}

/// A run sliced by the buffer is the same trajectory (CLAUDE.md, invariant
/// 5), through the wrapper too.
#[test]
fn stitched_legs_are_the_same_trajectory() {
    let eph = Arc::new(load());
    let start = vessel(&eph);
    let t_end = VESSEL_T0 + 0.5 * DAY;

    let mut whole = Propagator::new(eph.clone(), config()).unwrap();
    let mut step = 0.0;
    let single = whole
        .run(&start, None, t_end, &[], &mut [], &mut step)
        .expect("one run");

    let mut legs = Propagator::new(eph, config()).unwrap();
    let mut piece = [State::default(); 4];
    let mut leg_step = 0.0;
    let mut state = start;
    let mut n_legs = 0;

    loop {
        let run = legs
            .run(&state, None, t_end, &[], &mut piece, &mut leg_step)
            .expect("leg");
        state = run.final_state;
        n_legs += 1;

        if run.stop == Stop::ReachedEnd {
            break;
        }
        assert_eq!(run.stop, Stop::BufferFull);
        assert!(n_legs < 1000, "the legs never end -- no progress");
    }

    assert!(n_legs > 1, "a four-sample buffer should have sliced the run");
    assert!(
        same_bits(&state, &single.final_state),
        "the trajectory diverged"
    );
    assert_eq!(leg_step.to_bits(), step.to_bits(), "carried step");
}

/// An integrator that does not exist yet is an error, not a silent
/// substitution of the one that does.
#[test]
fn asking_for_an_integrator_that_does_not_exist_is_an_error() {
    let eph = Arc::new(load());

    let cfg = PropConfig {
        integrator: Integrator::Rkn,
        ..config()
    };
    assert_eq!(Propagator::new(eph, cfg).err(), Some(CoreError::InvalidArg));
}

/// Leaving the asset's span arrives as an error, not as a plausible
/// trajectory of a vessel nothing pulls on.
#[test]
fn running_past_the_asset_is_an_error() {
    let eph = Arc::new(load());
    let start = vessel(&eph);
    let mut prop = Propagator::new(eph, config()).unwrap();

    let mut step = 0.0;
    assert_eq!(
        prop.run(&start, None, 200.0 * DAY, &[], &mut [], &mut step)
            .err(),
        Some(CoreError::InvalidArg)
    );

    // And the context is not poisoned: the next run within the asset
    // succeeds.
    let mut step = 0.0;
    assert!(prop
        .run(&start, None, VESSEL_T0 + 3600.0, &[], &mut [], &mut step)
        .is_ok());
}

/// Propagators are created and freed, which is what valgrind measures in CI:
/// the types prove that freeing twice is impossible but do not prove that
/// freeing happens at all -- a leak is not a type error.
#[test]
fn creating_and_dropping_repeatedly_is_clean() {
    let eph = Arc::new(load());

    for _ in 0..50 {
        let mut prop = Propagator::new(eph.clone(), config()).unwrap();
        let start = vessel(&eph);
        let mut step = 0.0;
        let mut samples = [State::default(); 8];
        let _ = prop.run(
            &start,
            None,
            VESSEL_T0 + 600.0,
            &[],
            &mut samples,
            &mut step,
        );
    }
}

/// `run_stm` (ROADMAP K8): the matrix arrives, and the trajectory stays the
/// same.
///
/// The second claim is the important one. The matrix is worth something only
/// if it belongs to the trajectory the vessel actually flies; the comparison
/// is bitwise, because "roughly the same" would mean nothing here.
#[test]
fn the_stm_run_is_the_same_trajectory() {
    let eph = Arc::new(load());
    let start = vessel(&eph);
    let mut prop = Propagator::new(eph, config()).unwrap();

    let t_end = VESSEL_T0 + 0.25 * DAY;

    let mut plain_step = 0.0;
    let plain = prop
        .run(&start, None, t_end, &[], &mut [], &mut plain_step)
        .unwrap();

    let mut stm_step = 0.0;
    let (final_state, phi) = prop.run_stm(&start, None, t_end, &mut stm_step).unwrap();

    for (a, b) in [
        (final_state.r.x, plain.final_state.r.x),
        (final_state.r.y, plain.final_state.r.y),
        (final_state.r.z, plain.final_state.r.z),
        (final_state.v.x, plain.final_state.v.x),
        (final_state.v.y, plain.final_state.v.y),
        (final_state.v.z, plain.final_state.v.z),
    ] {
        assert_eq!(a.to_bits(), b.to_bits(), "the trajectory must be the same");
    }
    assert_eq!(
        stm_step.to_bits(),
        plain_step.to_bits(),
        "and so must the step left for the next leg"
    );

    // The matrix is meaningful: not the identity, not empty, and indexed as
    // promised. Without this everything above would compare zeros.
    assert_eq!(phi.as_slice().len(), STM_LEN);
    let off_diagonal: f64 = (0..6)
        .flat_map(|i| (0..6).map(move |j| (i, j)))
        .filter(|(i, j)| i != j)
        .map(|(i, j)| phi.get(i, j).abs())
        .sum();
    assert!(off_diagonal > 1.0, "the STM looks like the identity");

    // get(row, col) reads the same element as the raw slice -- otherwise a
    // transposition would go unnoticed.
    for i in 0..6 {
        for j in 0..6 {
            assert_eq!(phi.get(i, j).to_bits(), phi.as_slice()[i * 6 + j].to_bits());
        }
    }
}

/// The same rejection as in `run`: outside the asset this is an error, not a
/// matrix for a vessel that felt no gravity.
#[test]
fn an_stm_run_past_the_asset_is_an_error() {
    let eph = Arc::new(load());
    let start = vessel(&eph);
    let mut prop = Propagator::new(eph, config()).unwrap();

    let mut step = 0.0;
    assert_eq!(
        prop.run_stm(&start, None, 200.0 * DAY, &mut step).err(),
        Some(CoreError::InvalidArg)
    );
}

/// `Stm::get` outside 6x6 is a programmer error, and it must be loud.
#[test]
#[should_panic(expected = "STM is 6x6")]
fn indexing_the_stm_out_of_range_panics() {
    let phi = Stm([0.0; STM_LEN]);
    let _ = phi.get(6, 0);
}

/// Solar radiation pressure through the wrapper (ROADMAP K6b).
///
/// `core/test/test_srp.c` measures the physics; what is checked here is the
/// translation of `Option<&VesselParams>` into a pointer, plus three claims,
/// each of which breaks separately:
///
/// - `None` and a vessel with no area are the same thing, **bitwise**:
///   everything that flew before K6b flies identically;
/// - a vessel with area flies differently, and by how much is visible;
/// - `run_stm` carries the same vessel, so the matrix belongs to the
///   trajectory rather than a neighbouring one (that is K8c, checked again
///   where it is easiest to lose).
#[test]
fn a_vessel_with_area_feels_the_sun() {
    let eph = Arc::new(load());
    let start = vessel(&eph);
    let mut prop = Propagator::new(eph, config()).unwrap();

    let t_end = VESSEL_T0 + 0.5 * DAY;

    let bare = core_rs::VesselParams {
        mass_kg: 1000.0,
        area_m2: 0.0,
        cr: 1.3,
        cd: 0.0,
    };
    let sail = core_rs::VesselParams {
        mass_kg: 1000.0,
        area_m2: 20.0,
        cr: 1.3,
        cd: 0.0,
    };

    let mut step = 0.0;
    let none = prop
        .run(&start, None, t_end, &[], &mut [], &mut step)
        .unwrap();

    let mut step_bare = 0.0;
    let zero_area = prop
        .run(&start, Some(&bare), t_end, &[], &mut [], &mut step_bare)
        .unwrap();
    assert!(
        same_bits(&none.final_state, &zero_area.final_state),
        "a vessel with no area is the same test particle"
    );
    assert_eq!(step.to_bits(), step_bare.to_bits(), "and the same step");

    let mut step_sail = 0.0;
    let lit = prop
        .run(&start, Some(&sail), t_end, &[], &mut [], &mut step_sail)
        .unwrap();

    let moved = ((lit.final_state.r.x - none.final_state.r.x).powi(2)
        + (lit.final_state.r.y - none.final_state.r.y).powi(2)
        + (lit.final_state.r.z - none.final_state.r.z).powi(2))
    .sqrt();
    println!("  half a day under SRP moved the vessel by {moved:.4} m");
    assert!(
        moved > 1.0,
        "area should have changed the trajectory, but moved it {moved} m"
    );

    let mut stm_step = 0.0;
    let (stm_final, _) = prop
        .run_stm(&start, Some(&sail), t_end, &mut stm_step)
        .unwrap();
    assert!(
        same_bits(&stm_final, &lit.final_state),
        "the matrix must belong to the trajectory the vessel actually flies"
    );
}

// ---------------------------------------------------------------------------
// Porkchop across the boundary (ROADMAP-UI.md, U5a)

/// A plot cell equals a direct Lambert call with the same arguments.
///
/// The step's oracle verbatim, and it is this way because it is **not the same
/// code**: the grid goes through `porkchop_compute_eph` while the comparison
/// goes through `lambert_solve`, both across the boundary but by different
/// routes. That is how swapped grid axes are caught, and they do get swapped:
/// `t1` and `tof` are both positive, both in seconds, and a transposed grid
/// looks perfectly plausible.
#[test]
fn a_porkchop_cell_equals_a_direct_lambert_solve() {
    let eph = load();
    const EARTH: i32 = 3;
    const MOON: i32 = 4;

    let mu = eph.body_mu(EARTH);
    assert!(mu > 0.0, "the fixture must know Earth\'s mass");

    let t1_grid = [0.0, 3.0 * DAY, 6.0 * DAY];
    let tof_grid = [4.0 * DAY, 5.0 * DAY];

    let grid = core_rs::porkchop(&eph, EARTH, MOON, mu, true, &t1_grid, &tof_grid)
        .expect("the grid should have computed");
    assert!(!grid.is_empty(), "no cells -- nothing to compare");

    for cell in &grid {
        // A cell must lie on the grid rather than somewhere between: that is
        // the first thing to break when the axes are swapped.
        assert!(
            t1_grid.contains(&cell.t1),
            "t1 = {} is not in the departure grid",
            cell.t1
        );
        assert!(
            tof_grid.contains(&cell.tof),
            "tof = {} is not in the flight-time grid",
            cell.tof
        );

        let from = eph.body_state(EARTH, cell.t1).unwrap();
        let to = eph.body_state(MOON, cell.t1 + cell.tof).unwrap();

        let (v1, v2) = core_rs::lambert_solve(from.r, to.r, cell.tof, mu, true, 0)
            .expect("the same transfer should converge directly too");

        let speed = |a: core_rs::Vec3d, b: core_rs::Vec3d| {
            let (dx, dy, dz) = (a.x - b.x, a.y - b.y, a.z - b.z);
            (dx * dx + dy * dy + dz * dz).sqrt()
        };

        let depart = speed(v1, from.v);
        let arrive = speed(v2, to.v);

        // Not bitwise, and that is stated honestly: both paths compute the
        // same thing with the same functions, but the compiler is free to keep
        // an intermediate in a register on one path and not on the other. A
        // part in 1e12 is nine orders below anything a swapped axis would
        // survive.
        assert!(
            (cell.v_inf_depart - depart).abs() <= 1e-12 * depart,
            "departure: grid {} against Lambert {depart}",
            cell.v_inf_depart
        );
        assert!(
            (cell.v_inf_arrive - arrive).abs() <= 1e-12 * arrive,
            "arrival: grid {} against Lambert {arrive}",
            cell.v_inf_arrive
        );
    }
}

/// An empty grid is an argument error, not an empty result.
///
/// The difference is not formal: an empty `Vec` would look like "no cell
/// converged", i.e. a forbidden zone covering the whole plot.
#[test]
fn an_empty_grid_is_refused() {
    let eph = load();
    let mu = eph.body_mu(3);

    assert_eq!(
        core_rs::porkchop(&eph, 3, 4, mu, true, &[], &[86400.0]),
        Err(CoreError::InvalidArg)
    );
    assert_eq!(
        core_rs::porkchop(&eph, 3, 4, mu, true, &[0.0], &[]),
        Err(CoreError::InvalidArg)
    );
}

/// An unknown body gives a zero `mu` -- and a grid with nothing to compute
/// from.
#[test]
fn an_unknown_body_has_no_mass() {
    let eph = load();
    assert_eq!(eph.body_mu(999), 0.0);
    assert!(eph.body_mu(3) > 3.9e14, "Earth\'s GM is about 3.986e14");
}

/// Orientation through the wrapper -- and what bits will not show (R1c).
///
/// The bitwise comparison against C already exists in `core-sys` and catches
/// swapped components. What it does not catch is **meaning**: a conjugated
/// quaternion agrees with the original in everything but the sign of three
/// components, stays unit, and rotates exactly as much, merely the other way.
/// So what is checked here is not bits but three astronomical numbers, none of
/// which comes from our own code.
///
/// Measured on the fixture, and all three matched the published values:
///
/// | | |
/// |---|---|
/// | pole at J2000 | `(0, 0, 1)` to within 1e-16 |
/// | RA of the prime meridian at J2000 | 280.194 deg |
/// | rate | 15.041 deg per hour, towards increasing RA |
#[test]
fn the_earth_turns_the_way_the_quaternion_says() {
    const EARTH: i32 = 3;
    const HOUR: f64 = 3600.0;
    const DAY: f64 = 86400.0;

    let eph = load();
    let turn = |t: f64, v: [f64; 3]| -> [f64; 3] {
        eph.body_orientation(EARTH, t)
            .expect("Earth is within the asset")
            .rotate(v)
    };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

    // 1. The pole. The asset frame is equatorial (ICRF), and Earth's pole per
    //    IAU/NAIF pck00011 has exactly RA = 0, Dec = 90 deg at J2000, i.e. the
    //    z axis. This catches swapped axes: a pole on x or y is obvious.
    let pole = turn(0.0, [0.0, 0.0, 1.0]);
    assert!(
        (dot(pole, pole) - 1.0).abs() < 1e-12,
        "the rotation does not preserve length -- this is not a quaternion"
    );
    println!("  pole at J2000: {pole:?}");
    assert!(
        pole[2] > 1.0 - 1e-12 && pole[0].abs() < 1e-9 && pole[1].abs() < 1e-9,
        "the pole is not on the z axis: {pole:?} -- either the frame is not \
         equatorial or the axes are swapped"
    );

    // 2. The phase. Earth's rotation angle (ERA) at J2000 is 280.4606 deg,
    //    and that is how far the prime meridian sits from the x axis. The
    //    asset counts from the TT scale while ERA is defined from UT1, so the
    //    63.83 s difference gives -0.267 deg: 280.194 deg. This number is what
    //    catches **conjugation**, which nothing else catches: the inverse
    //    quaternion would give 79.8 deg.
    let ra = |t: f64| -> f64 {
        let e = turn(t, [1.0, 0.0, 0.0]);
        e[1].atan2(e[0]).to_degrees().rem_euclid(360.0)
    };
    let at_epoch = ra(0.0);
    println!("  prime meridian at J2000: RA = {at_epoch:.4} deg");
    assert!(
        (at_epoch - 280.194).abs() < 0.01,
        "prime meridian RA {at_epoch:.4} deg instead of 280.194 -- the \
         quaternion is conjugated or the phase is wrong"
    );

    // 3. Rate and direction. A sidereal day is 15.041 deg per hour towards
    //    increasing RA. The solar 15.000 deg is distinguishable here too: over
    //    a day that is nearly a degree of difference.
    let rate = (ra(HOUR) - at_epoch).rem_euclid(360.0);
    println!("  per hour: {rate:.4} deg");
    assert!(
        (rate - 15.0411).abs() < 0.001,
        "{rate:.4} deg per hour is not a sidereal day (15.0411)"
    );

    // The axis stays put meanwhile: precession over a day is arcseconds, and
    // cannot be confused with rotation.
    let drift = dot(pole, turn(DAY, [0.0, 0.0, 1.0])).clamp(-1.0, 1.0);
    assert!(
        drift > 0.999_999_999,
        "the rotation axis moved by {} over a day",
        1.0 - drift
    );
}

/// A body with no rotation model gives the identity quaternion and `Ok`.
#[test]
fn a_body_without_a_rotation_model_is_not_an_error() {
    let eph = load();

    // The Sun (0) carries no rotation channels in the fixture -- eight of the
    // ten bodies do not.
    let quiet = eph.body_orientation(0, 0.0).expect("this is not an error");
    assert_eq!(quiet, core_rs::Quat::default());

    // An unknown body, though, is an error: unlike the radius there is time
    // here, and a silent identity would hide a swapped index.
    assert!(eph.body_orientation(999, 0.0).is_err());
    // As is a time outside the asset's span.
    assert!(eph.body_orientation(3, 1.0e9).is_err());
}
