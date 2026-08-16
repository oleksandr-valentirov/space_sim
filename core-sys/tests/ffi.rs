//! The D2 check: the boundary declarations describe exactly what is in C.
//!
//! An FFI error does not fail. Swapped `State` fields, `int` instead of
//! `size_t`, `*mut` where C expects `*const` -- all of it compiles and returns
//! numbers that look like coordinates. So the check here is bitwise rather
//! than "within tolerance": the same function, the same asset, the same
//! instants, called from C and from Rust, must give **identical bits**. Any
//! layout divergence ruins them beyond recognition.
//!
//! The C oracle is `core-sys/oracle.c`, built by `build.rs`.
//!
//! There is `unsafe` here, and it does not violate the CLAUDE.md invariant:
//! the rule "our `unsafe` only in core-rs" is about the code we ship. A test
//! of the raw layer cannot be written any other way -- it is precisely about
//! the call across the boundary being correct.

use std::ffi::CString;
use std::path::Path;
use std::process::Command;

use core_sys::{
    eph_body_mu, eph_body_orientation, eph_body_radius, eph_body_state, eph_free, eph_load,
    porkchop_compute_eph, prop_create, prop_free, prop_run, prop_run_stm, CoreEvent, CoreResult,
    EphemerisCtx, PorkchopPoint, PropConfig, PropagatorCtx, Quat, State, CORE_ERR_BUFFER_TOO_SMALL,
    CORE_ERR_INVALID_ARG, CORE_EVENT_PERIAPSIS, CORE_INTEG_DOP853, CORE_OK, CORE_STOP_EVENT,
};

const ORACLE: &str = env!("CORE_ORACLE");
const ORACLE_PLANNING: &str = env!("CORE_ORACLE_PLANNING");
const REPO_ROOT: &str = env!("CORE_REPO_ROOT");

const ASSET: &str = "data/fixture/earth_moon.eph";
const DAY: f64 = 86400.0;

/// One line of oracle output: a tag and the numbers after it.
#[derive(Clone)]
struct Record {
    tag: String,
    values: Vec<f64>,
}

impl Record {
    fn state(&self, from: usize) -> State {
        State {
            t: self.values[from],
            r: core_sys::Vec3d {
                x: self.values[from + 1],
                y: self.values[from + 2],
                z: self.values[from + 3],
            },
            v: core_sys::Vec3d {
                x: self.values[from + 4],
                y: self.values[from + 5],
                z: self.values[from + 6],
            },
        }
    }
}

/// Bitwise comparison of two states with a legible message.
///
/// Bits are compared, not values: a difference here is struct layout or the
/// types in a declaration, and no tolerance says anything about that.
fn same_bits(from_c: &State, from_rust: &State, what: &str) {
    let c = [
        from_c.t, from_c.r.x, from_c.r.y, from_c.r.z, from_c.v.x, from_c.v.y, from_c.v.z,
    ];
    let rust = [
        from_rust.t,
        from_rust.r.x,
        from_rust.r.y,
        from_rust.r.z,
        from_rust.v.x,
        from_rust.v.y,
        from_rust.v.z,
    ];

    for (i, (&a, &b)) in c.iter().zip(rust.iter()).enumerate() {
        assert_eq!(
            a.to_bits(),
            b.to_bits(),
            "{what}, component {i}: C gave {a:.17e}, Rust {b:.17e}.\n\
             This is struct layout or declaration types, not physics."
        );
    }
}

/// Runs the oracle and parses its output.
///
/// `%.17g` recovers a double uniquely and Rust's parser rounds correctly, so
/// the text in between loses nothing -- the comparison can be bitwise.
fn oracle_records() -> Vec<Record> {
    records_from(ORACLE)
}

/// The same for any oracle. There are two, and the second is not a whim: the
/// planning oracle links with `-lm`, this one deliberately without (build.rs).
fn records_from(oracle: &str) -> Vec<Record> {
    let output = Command::new(oracle)
        .current_dir(REPO_ROOT)
        .output()
        .unwrap_or_else(|e| panic!("cannot run {oracle}: {e}"));

    assert!(
        output.status.success(),
        "the oracle exited with {}:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let text = String::from_utf8(output.stdout).expect("oracle output is not UTF-8");
    let mut records = Vec::new();

    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        assert!(fields.len() > 1, "unexpected oracle line: {line}");

        let values = fields[1..]
            .iter()
            .map(|f| {
                f.parse()
                    .unwrap_or_else(|e| panic!("not a number in '{line}': {e}"))
            })
            .collect();

        records.push(Record {
            tag: fields[0].to_string(),
            values,
        });
    }

    assert!(
        !records.is_empty(),
        "oracle {oracle} printed nothing -- an empty comparison silently \
         'passes', so this is a failure"
    );

    records
}

/// The lines of one tag, in output order. Clones rather than borrows -- the
/// limit here is readability, not performance (CLAUDE.md, Rust style).
fn tagged(records: &[Record], tag: &str) -> Vec<Record> {
    records.iter().filter(|r| r.tag == tag).cloned().collect()
}

/// Loads the fixture. The caller must pass the result to `eph_free` -- exactly
/// the obligation D3 makes impossible to break.
///
/// # Safety
///
/// Returns a raw pointer, valid until `eph_free`.
unsafe fn load_fixture() -> *mut EphemerisCtx {
    let path = Path::new(REPO_ROOT).join(ASSET);
    let c_path = CString::new(path.to_str().expect("path is not UTF-8")).expect("path holds a \\0");

    let mut ctx: *mut EphemerisCtx = std::ptr::null_mut();
    let result: CoreResult = eph_load(c_path.as_ptr(), &mut ctx);

    assert_eq!(result, CORE_OK, "eph_load did not read {}", path.display());
    assert!(!ctx.is_null(), "eph_load returned CORE_OK and NULL");

    ctx
}

#[test]
fn states_match_the_c_oracle_bit_for_bit() {
    let records = oracle_records();
    let samples = tagged(&records, "eph");
    assert!(!samples.is_empty(), "the oracle gave no eph line");

    unsafe {
        let ctx = load_fixture();

        for sample in &samples {
            let body = sample.values[0] as i32;
            let t = sample.values[1];

            let mut state = State::default();
            let result = eph_body_state(ctx, body, t, &mut state);
            assert_eq!(result, CORE_OK, "body {body} at time {t}");

            let mut expected = sample.state(1);
            // The time in the line is the one requested; the rest is from C.
            expected.t = t;
            same_bits(&expected, &state, &format!("body {body}, time {t}"));
        }

        eph_free(ctx);
    }
}

/// Orientation compared bitwise, all four components (R1c).
///
/// The easiest mistake here is a convention, not arithmetic: half the world
/// writes a quaternion as `(x, y, z, w)`, and a misplaced `w` is still a
/// perfectly valid rotation, merely the wrong one. Neither the return code nor
/// the length (unit under any permutation) shows it -- only a per-component
/// comparison against C.
///
/// The second thing pinned here: a body with no rotation model returns the
/// **identity** and `CORE_OK`. "Not modelled" must not drift into "failed":
/// eight of the fixture's ten bodies are like that.
#[test]
fn orientations_match_the_c_oracle_bit_for_bit() {
    let records = oracle_records();
    let quats = tagged(&records, "quat");
    assert!(!quats.is_empty(), "the oracle gave no quat line");

    unsafe {
        let ctx = load_fixture();

        let mut turning = 0;
        for record in &quats {
            let body = record.values[0] as i32;
            let t = record.values[1];

            let mut got = Quat::default();
            let result = eph_body_orientation(ctx, body, t, &mut got);
            assert_eq!(result, CORE_OK, "body {body} at time {t}");

            for (k, (name, expected)) in [
                ("w", record.values[2]),
                ("x", record.values[3]),
                ("y", record.values[4]),
                ("z", record.values[5]),
            ]
            .iter()
            .enumerate()
            {
                let component = [got.w, got.x, got.y, got.z][k];
                assert_eq!(
                    component.to_bits(),
                    expected.to_bits(),
                    "body {body}, time {t}, component {name}: C gave \
                     {expected}, the boundary {component}"
                );
            }

            // The identity quaternion means "does not rotate"; comparing only
            // identities would pass even for a function that reads nothing.
            if got != Quat::default() {
                turning += 1;
            }
        }

        assert!(
            turning > 0,
            "every quaternion is the identity -- the comparison checked nothing"
        );

        eph_free(ctx);
    }
}

/// Radii are compared bitwise too (ROADMAP U2a).
///
/// The function returns a `double` rather than a code, so the only way to
/// notice a declaration diverging from C is to compare the number itself. And
/// **every** oracle body must be compared, together with a non-existent one:
/// Earth's and the Moon's radii differ by a factor of three, so an off-by-one
/// body in the context array would give a perfectly plausible radius -- and an
/// invisible bug.
#[test]
fn radii_match_the_c_oracle_bit_for_bit() {
    let records = oracle_records();
    let radii = tagged(&records, "rad");
    assert!(!radii.is_empty(), "the oracle gave no rad line");

    unsafe {
        let ctx = load_fixture();

        let mut nonzero = 0;
        for record in &radii {
            let body = record.values[0] as i32;
            let expected = record.values[1];

            let got = eph_body_radius(ctx, body);
            assert_eq!(
                got.to_bits(),
                expected.to_bits(),
                "body {body}: C gave {expected}, the boundary {got}"
            );

            if got != 0.0 {
                nonzero += 1;
            }
        }

        // Comparing zeros against zeros would pass for a function that always
        // returns zero. The fixture does hold sized bodies; at least one must
        // appear here.
        assert!(
            nonzero > 0,
            "every radius is zero -- the comparison checked nothing"
        );

        eph_free(ctx);
    }
}

/// Errors must arrive as errors, not as zeros.
///
/// The other half of the contract: if the return code is read wrongly, a call
/// outside the asset's span looks like success with a state full of garbage --
/// the worst possible outcome, because the trajectory comes out plausible.
#[test]
fn out_of_range_is_reported_not_extrapolated() {
    unsafe {
        let ctx = load_fixture();
        let mut state = State::default();

        // The fixture covers 120 days from J2000 (data/fixture/README.md).
        for (label, body, t) in [
            ("time before the start", 0, -DAY),
            ("time after the end", 0, 200.0 * DAY),
            ("negative body index", -1, 0.0),
            ("index past the list", 999, 0.0),
        ] {
            let result = eph_body_state(ctx, body, t, &mut state);
            assert_eq!(
                result, CORE_ERR_INVALID_ARG,
                "{label}: expected CORE_ERR_INVALID_ARG, got {result}"
            );
        }

        // And what is inside the span must pass -- otherwise the check above
        // would "pass" even with a broken return code.
        assert_eq!(
            eph_body_state(ctx, 0, 0.0, &mut state),
            CORE_OK,
            "the start of the span should have read"
        );

        eph_free(ctx);
    }
}

/// `eph_free(NULL)` is allowed -- `core/ephemeris.h` says so.
///
/// A small thing, but D3 will rest on it: the RAII wrapper frees in `Drop`
/// unconditionally, and if that promise is false it will crash not here but
/// somewhere in the game while unloading a scene.
#[test]
fn freeing_null_is_allowed() {
    unsafe {
        eph_free(std::ptr::null_mut());
    }
}

/// Propagation across the boundary gives the same bits as a direct C call (H3).
///
/// More than one function is checked here. `prop_run` takes eleven arguments,
/// among them two structs (`PropConfig` with an `enum`, two `double` and a
/// `long`; `CoreEvent` with an `enum`, an `int` and a `double`), three output
/// pointers and a buffer supplied by Rust. Each is its own way to diverge
/// silently: a shifted field, the wrong integer type, swapped
/// `out_cap`/`out_count`. None of them fails -- all return numbers resembling
/// a trajectory.
///
/// The oracle performs two runs: one to a given time with samples, one to
/// periapsis. The second goes through `CoreEvent` and through the stop code.
#[test]
fn propagation_matches_the_c_oracle_bit_for_bit() {
    // The same literals as in core-sys/oracle.c. The vessel is given as
    // numbers rather than computed: the oracle links without libm, so there is
    // no sqrt there.
    const VESSEL_T0: f64 = DAY;
    const VESSEL_DX: f64 = 42_164.0e3;
    const VESSEL_VY: f64 = 1967.84;
    const VESSEL_VZ: f64 = 1475.88;
    // A low orbit for the drag check (ROADMAP K7b); mirrors core-sys/oracle.c,
    // which explains why a second one is needed.
    const LEO_DX: f64 = 6_698_137.0;
    const LEO_VY: f64 = 6680.0;
    const LEO_VZ: f64 = 3860.0;
    const CAP: usize = 64;

    let records = oracle_records();
    let oracle_samples = tagged(&records, "samp");
    let oracle_runs = tagged(&records, "run");
    let oracle_ends = tagged(&records, "end");

    assert!(!oracle_samples.is_empty(), "the oracle gave no sample");
    assert_eq!(oracle_runs.len(), 2, "the oracle should have given two runs");
    assert_eq!(oracle_ends.len(), 2);

    unsafe {
        let ctx = load_fixture();

        let mut earth = State::default();
        assert_eq!(eph_body_state(ctx, 3, VESSEL_T0, &mut earth), CORE_OK);

        let mut vessel = State {
            r: earth.r,
            v: earth.v,
            t: VESSEL_T0,
        };
        vessel.r.x += VESSEL_DX;
        vessel.v.y += VESSEL_VY;
        vessel.v.z += VESSEL_VZ;

        // density_scale = 1 mirrors the oracle: it also builds its config with
        // one, which is why these two runs can be compared bitwise.
        let cfg = PropConfig {
            integrator: CORE_INTEG_DOP853,
            tol_m: 1e-2,
            h_max_s: 1800.0,
            max_steps: 0,
            density_scale: 1.0,
        };

        let mut p: *mut PropagatorCtx = std::ptr::null_mut();
        assert_eq!(prop_create(ctx, &cfg, &mut p), CORE_OK);
        assert!(!p.is_null(), "prop_create returned CORE_OK and NULL");

        // ---- First run: samples up to a given time.
        let mut samples = vec![State::default(); CAP];
        let mut count: usize = 0;
        let mut final_state = State::default();
        let mut stop: core_sys::CoreStopReason = -1;
        let mut event: i32 = -2;
        let mut step = 0.0f64;

        let result = prop_run(
            p,
            &vessel,
            std::ptr::null(),
            VESSEL_T0 + 0.5 * DAY,
            std::ptr::null(),
            0,
            samples.as_mut_ptr(),
            CAP,
            &mut count,
            &mut final_state,
            &mut stop,
            &mut event,
            &mut step,
        );
        assert_eq!(result, CORE_OK);

        assert_eq!(
            count,
            oracle_samples.len(),
            "sample count diverged: the Rust-supplied buffer fills differently \
             than in C"
        );
        for (k, from_c) in oracle_samples.iter().enumerate() {
            assert_eq!(from_c.values[0] as usize, k, "oracle sample order");
            same_bits(&from_c.state(1), &samples[k], &format!("sample {k}"));
        }

        let run = &oracle_runs[0];
        assert_eq!(run.values[0] as usize, count, "out_count");
        assert_eq!(run.values[1] as i32, stop, "stop code");
        assert_eq!(run.values[2] as i32, event, "event index");
        assert_eq!(
            run.values[3].to_bits(),
            step.to_bits(),
            "carried step: {} against {}",
            run.values[3],
            step
        );
        same_bits(&oracle_ends[0].state(0), &final_state, "final state");

        // ---- Second run: stopping on an event.
        let ev = CoreEvent {
            kind: CORE_EVENT_PERIAPSIS,
            body_id: 3,
            param: 0.0,
        };

        step = 0.0;
        let result = prop_run(
            p,
            &vessel,
            std::ptr::null(),
            VESSEL_T0 + 4.0 * DAY,
            &ev,
            1,
            std::ptr::null_mut(),
            0,
            &mut count,
            &mut final_state,
            &mut stop,
            &mut event,
            &mut step,
        );
        assert_eq!(result, CORE_OK);

        // Not only "matched the oracle" but "the thing asked for happened":
        // had the event not fired, both sides would equally have reached t_end
        // and the comparison would have passed silently.
        assert_eq!(stop, CORE_STOP_EVENT, "the event should have stopped the run");
        assert_eq!(event, 0);

        let run = &oracle_runs[1];
        assert_eq!(run.values[1] as i32, stop);
        assert_eq!(run.values[2] as i32, event);
        assert_eq!(run.values[3].to_bits(), step.to_bits(), "step after the event");
        same_bits(&oracle_ends[1].state(0), &final_state, "state at the event");

        // --- prop_run_stm (ROADMAP K8) --------------------------------
        //
        // Two distinct claims, and the second does not follow from the first:
        // that the boundary carries the matrix's 36 numbers without
        // permutation, and that the trajectory is meanwhile bit-identical to
        // what prop_run would give.
        let oracle_stm_run = tagged(&records, "stmrun");
        let oracle_stm_end = tagged(&records, "stmend");
        let oracle_stm = tagged(&records, "stm");

        assert_eq!(oracle_stm_run.len(), 1);
        assert_eq!(oracle_stm_end.len(), 1);
        assert_eq!(oracle_stm.len(), 36, "the matrix must be 6x6");

        let mut stm_final = State::default();
        let mut phi = [0.0f64; 36];
        let mut stm_step = 0.0f64;

        let result = prop_run_stm(
            p,
            &vessel,
            std::ptr::null(),
            VESSEL_T0 + 0.5 * DAY,
            &mut stm_final,
            phi.as_mut_ptr(),
            &mut stm_step,
        );
        assert_eq!(result, CORE_OK);

        assert_eq!(
            oracle_stm_run[0].values[0].to_bits(),
            stm_step.to_bits(),
            "step after the run with the matrix"
        );
        same_bits(&oracle_stm_end[0].state(0), &stm_final, "final STM state");

        // Element order is row-major, and the oracle carries the index on each
        // line, so transposing rows and columns fails here rather than
        // surfacing as a strange correction six months later.
        for (k, record) in oracle_stm.iter().enumerate() {
            assert_eq!(record.values[0] as usize, k, "STM element order");
            assert_eq!(
                record.values[1].to_bits(),
                phi[k].to_bits(),
                "STM element {k}"
            );
        }

        // --- A vessel feeling radiation pressure (ROADMAP K6b) ----------
        //
        // What is checked here is not the physics -- core/test/test_srp.c
        // measures that -- but the `VesselParams` declaration: swapping
        // `area_m2` and `cr` would give a perfectly plausible trajectory,
        // merely the wrong one. The oracle computes the same leg in C and
        // prints the result.
        let oracle_srp_run = tagged(&records, "srprun");
        let oracle_srp_end = tagged(&records, "srpend");
        assert_eq!(oracle_srp_run.len(), 1);
        assert_eq!(oracle_srp_end.len(), 1);

        let sail = core_sys::VesselParams {
            mass_kg: 1000.0,
            area_m2: 20.0,
            cr: 1.3,
            cd: 0.0,
        };

        let mut srp_final = State::default();
        step = 0.0;
        let result = prop_run(
            p,
            &vessel,
            &sail,
            VESSEL_T0 + 0.5 * DAY,
            std::ptr::null(),
            0,
            samples.as_mut_ptr(),
            CAP,
            &mut count,
            &mut srp_final,
            &mut stop,
            &mut event,
            &mut step,
        );
        assert_eq!(result, CORE_OK);

        let run = &oracle_srp_run[0];
        assert_eq!(run.values[0] as usize, count, "sample count under SRP");
        assert_eq!(run.values[3].to_bits(), step.to_bits(), "step under SRP");
        same_bits(
            &oracle_srp_end[0].state(0),
            &srp_final,
            "final state under SRP",
        );

        // And it really is different: if the vessel pointer went nowhere,
        // everything above would match an oracle that felt nothing either.
        let moved = ((srp_final.r.x - final_state.r.x).powi(2)
            + (srp_final.r.y - final_state.r.y).powi(2)
            + (srp_final.r.z - final_state.r.z).powi(2))
        .sqrt();
        assert!(
            moved > 1.0,
            "a vessel with area should have flown differently, but moved {moved} m"
        );

        // The matrix is neither identity nor empty -- otherwise everything
        // above would compare zeros and pass on any error.
        let off_diagonal: f64 = (0..36)
            .filter(|k| k / 6 != k % 6)
            .map(|k| phi[k].abs())
            .sum();
        assert!(off_diagonal > 1.0, "the STM looks like the identity: {phi:?}");

        // --- A vessel feeling air (ROADMAP K7b) -------------------------
        //
        // The same reason as SRP above, sharper by one field: `cr` and `cd`
        // sit next to each other, share a type and hold plausible values for
        // one another, so swapping them would give a trajectory that looks
        // flawless. The leg is low on purpose -- at geostationary altitude,
        // where the vessel above flies, there is no air at all, and a run with
        // `cd` would print what it prints without it.
        let oracle_drag_run = tagged(&records, "dragrun");
        let oracle_drag_end = tagged(&records, "dragend");
        assert_eq!(oracle_drag_run.len(), 1);
        assert_eq!(oracle_drag_end.len(), 1);

        let mut low = State::default();
        let result = eph_body_state(ctx, 3, VESSEL_T0, &mut low);
        assert_eq!(result, CORE_OK);
        low.r.x += LEO_DX;
        low.v.y += LEO_VY;
        low.v.z += LEO_VZ;
        low.t = VESSEL_T0;

        let blunt = core_sys::VesselParams {
            mass_kg: 1000.0,
            area_m2: 20.0,
            cr: 1.3,
            cd: 2.2,
        };

        let mut drag_final = State::default();
        step = 0.0;
        let result = prop_run(
            p,
            &low,
            &blunt,
            VESSEL_T0 + 600.0,
            std::ptr::null(),
            0,
            samples.as_mut_ptr(),
            CAP,
            &mut count,
            &mut drag_final,
            &mut stop,
            &mut event,
            &mut step,
        );
        assert_eq!(result, CORE_OK);

        let run = &oracle_drag_run[0];
        assert_eq!(
            run.values[0] as usize, count,
            "sample count under drag"
        );
        assert_eq!(run.values[3].to_bits(), step.to_bits(), "step under drag");
        same_bits(
            &oracle_drag_end[0].state(0),
            &drag_final,
            "final state under drag",
        );

        // And drag really did something: the same leg without `cd` must land
        // elsewhere. Without this, everything above would compare against an
        // oracle that also flew through vacuum.
        let dry = core_sys::VesselParams { cd: 0.0, ..blunt };
        let mut dry_final = State::default();
        step = 0.0;
        let result = prop_run(
            p,
            &low,
            &dry,
            VESSEL_T0 + 600.0,
            std::ptr::null(),
            0,
            samples.as_mut_ptr(),
            CAP,
            &mut count,
            &mut dry_final,
            &mut stop,
            &mut event,
            &mut step,
        );
        assert_eq!(result, CORE_OK);

        let moved = ((drag_final.r.x - dry_final.r.x).powi(2)
            + (drag_final.r.y - dry_final.r.y).powi(2)
            + (drag_final.r.z - dry_final.r.z).powi(2))
        .sqrt();
        assert!(
            moved > 1e-3,
            "a vessel with cd should have slowed, but diverged by {moved} m"
        );

        prop_free(p);
        eph_free(ctx);
    }
}

/// `prop_free(NULL)` is allowed -- `core/prop.h` says so, and H4 relies on it.
#[test]
fn freeing_a_null_propagator_is_allowed() {
    unsafe {
        prop_free(std::ptr::null_mut());
    }
}

/// Lambert across the boundary gives the same bits as C (L3, debt D1).
///
/// **The subtlest point is the struct by value.** `lambert_solve` is the first
/// on the boundary to take `Vec3d` other than by pointer: 24 bytes, fitting in
/// no register set under any of our ABIs, so it travels through memory. Had
/// Rust and C disagreed on how, the test would not fail with an error -- it
/// would return plausible velocities for a different geometry. Hence the
/// bitwise comparison, and hence the oracle's `r2` with a non-zero z: an error
/// that confuses field order has a chance to hide in the xy plane.
#[test]
fn lambert_matches_the_c_oracle_bit_for_bit() {
    let records = records_from(ORACLE_PLANNING);
    let solved = tagged(&records, "lam");
    assert_eq!(
        solved.len(),
        2,
        "the planning oracle should have given two solved problems (prograde \
         and retrograde)"
    );

    // The same numbers as in core-sys/oracle_planning.c. The duplication is
    // deliberate: a test taking its arguments from the oracle's output would
    // compare the oracle against itself and pass even when Rust passed C
    // something else entirely.
    let r1 = core_sys::Vec3d {
        x: 1.4959787e11,
        y: 0.0,
        z: 0.0,
    };
    let r2 = core_sys::Vec3d {
        x: -1.9e11,
        y: 1.1e11,
        z: 8.0e9,
    };
    let mu = 1.32712440018e20;
    let dt = 2.5e7;

    for (i, prograde) in [1, 0].into_iter().enumerate() {
        let mut v1 = core_sys::Vec3d::default();
        let mut v2 = core_sys::Vec3d::default();

        let result =
            unsafe { core_sys::lambert_solve(r1, r2, dt, mu, prograde, 0, &mut v1, &mut v2) };
        assert_eq!(result, CORE_OK, "prograde = {prograde}");

        let expected = &solved[i].values;
        let got = [v1.x, v1.y, v1.z, v2.x, v2.y, v2.z];

        for (k, (&c, &rust)) in expected.iter().zip(got.iter()).enumerate() {
            assert_eq!(
                c.to_bits(),
                rust.to_bits(),
                "prograde = {prograde}, component {k}: C gave {c:.17e}, \
                 Rust {rust:.17e}.\n\
                 This is struct-by-value passing or argument order, not \
                 physics."
            );
        }
    }
}

/// Lambert's rejections cross the boundary as rejections too.
///
/// The mirror of the previous test, with its own point: `CoreResult` is
/// declared as a `c_int` with constants precisely because a Rust enum holding
/// an out-of-range value would be UB. That is worth something only if the
/// values are actually compared.
#[test]
fn lambert_refusals_cross_the_boundary() {
    let records = records_from(ORACLE_PLANNING);
    let refused = tagged(&records, "lerr");
    assert_eq!(refused.len(), 2, "the oracle should have given two rejections");

    let r1 = core_sys::Vec3d {
        x: 1.4959787e11,
        y: 0.0,
        z: 0.0,
    };
    let r2 = core_sys::Vec3d {
        x: -1.9e11,
        y: 1.1e11,
        z: 8.0e9,
    };
    let opposite = core_sys::Vec3d {
        x: -r1.x,
        y: -r1.y,
        z: -r1.z,
    };
    let mu = 1.32712440018e20;
    let dt = 2.5e7;

    let mut v1 = core_sys::Vec3d::default();
    let mut v2 = core_sys::Vec3d::default();

    // The multi-revolution case: lambert.h says n_revs must be 0.
    let many_revs = unsafe { core_sys::lambert_solve(r1, r2, dt, mu, 1, 1, &mut v1, &mut v2) };
    // Degenerate geometry: r1 and r2 on one line through the origin.
    let collinear =
        unsafe { core_sys::lambert_solve(r1, opposite, dt, mu, 1, 0, &mut v1, &mut v2) };

    for (label, got, expected) in [
        ("n_revs = 1", many_revs, refused[0].values[0] as i32),
        ("collinear r1 and r2", collinear, refused[1].values[0] as i32),
    ] {
        assert_eq!(
            got, expected,
            "{label}: C returned {expected}, Rust saw {got}"
        );
        assert_eq!(
            got, CORE_ERR_INVALID_ARG,
            "{label}: and it should have been CORE_ERR_INVALID_ARG specifically"
        );
    }
}

/// `mu` is compared bitwise too -- over the same bodies as the radii.
///
/// Its own test rather than a line in the previous one: these are different
/// context fields, and an off-by-one body in the `mu` array would look like
/// perfectly reasonable gravity.
#[test]
fn gravitational_parameters_match_the_c_oracle_bit_for_bit() {
    let records = oracle_records();
    let mus = tagged(&records, "mu");
    assert!(!mus.is_empty(), "the oracle gave no mu line");

    unsafe {
        let ctx = load_fixture();

        for record in &mus {
            let body = record.values[0] as i32;
            let expected = record.values[1];
            let got = eph_body_mu(ctx, body);

            assert_eq!(
                got.to_bits(),
                expected.to_bits(),
                "body {body}: C gave {expected:e}, the boundary {got:e}"
            );
            assert!(got > 0.0, "body {body} in the fixture must have mass");
        }

        assert_eq!(
            eph_body_mu(ctx, 999),
            0.0,
            "an unknown body gives zero, like the radius"
        );

        eph_free(ctx);
    }
}

/// The porkchop grid crosses the boundary bitwise (ROADMAP-UI.md, U5a).
///
/// The function returns an **array of structs**, which is new on the boundary:
/// until now only scalars or `State` travelled across. Swapped `t1` and `tof`
/// would give a perfectly plausible plot -- both positive, both in seconds --
/// so all four fields of every cell are compared.
#[test]
fn the_porkchop_grid_matches_the_c_oracle_bit_for_bit() {
    let records = records_from(ORACLE_PLANNING);
    let cells = tagged(&records, "pork");
    assert!(!cells.is_empty(), "the oracle gave no pork line");

    unsafe {
        let ctx = load_fixture();

        const EARTH: i32 = 3;
        const MOON: i32 = 4;
        let day = 86400.0;
        let t1s = [0.0, 3.0 * day, 6.0 * day];
        let tofs = [4.0 * day, 5.0 * day];

        let mut grid = [PorkchopPoint::default(); 6];
        let mut count: usize = 0;
        let result = porkchop_compute_eph(
            ctx,
            EARTH,
            MOON,
            eph_body_mu(ctx, EARTH),
            1,
            t1s.as_ptr(),
            t1s.len(),
            tofs.as_ptr(),
            tofs.len(),
            grid.as_mut_ptr(),
            grid.len(),
            &mut count,
        );

        assert_eq!(result, CORE_OK);
        assert_eq!(count, cells.len(), "cell count diverged");

        for (k, cell) in cells.iter().enumerate() {
            let got = grid[k];
            for (name, from_c, from_rust) in [
                ("t1", cell.values[1], got.t1),
                ("tof", cell.values[2], got.tof),
                ("v_inf_depart", cell.values[3], got.v_inf_depart),
                ("v_inf_arrive", cell.values[4], got.v_inf_arrive),
            ] {
                assert_eq!(
                    from_c.to_bits(),
                    from_rust.to_bits(),
                    "cell {k}, {name}: C gave {from_c:e}, boundary {from_rust:e}"
                );
            }
        }

        // Too small a buffer is a rejection carrying a count, not silence:
        // the same convention as `prop_run`, and it must be checked here,
        // because that is how the caller learns how much room to ask for.
        let mut one = [PorkchopPoint::default(); 1];
        let mut written: usize = 0;
        let squeezed = porkchop_compute_eph(
            ctx,
            EARTH,
            MOON,
            eph_body_mu(ctx, EARTH),
            1,
            t1s.as_ptr(),
            t1s.len(),
            tofs.as_ptr(),
            tofs.len(),
            one.as_mut_ptr(),
            one.len(),
            &mut written,
        );
        assert_eq!(squeezed, CORE_ERR_BUFFER_TOO_SMALL);
        assert_eq!(written, 1, "the written count must arrive on rejection too");

        eph_free(ctx);
    }
}

/// CR3BP across the boundary is bit-for-bit what C gives (U6b2).
///
/// These four functions differ from the rest of the boundary in working in the
/// **dimensionless** normalisation rather than in metres. An error here
/// neither fails nor even looks odd: a Jacobi constant of 3.11 and one of 3.14
/// are equally plausible. Hence the bitwise comparison, while the meaning is
/// checked separately against external numbers (`core-rs/tests/cr3bp.rs`).
///
/// Two of them take `Vec3d` **by value** -- the same point `lambert_solve`
/// insists on: 24 bytes travel through memory under all our ABIs, and a
/// swapped `(r, v, mu)` argument order would give a plausible number.
#[test]
fn cr3bp_crosses_the_boundary_unchanged() {
    let records = oracle_records();

    let cmu = tagged(&records, "cmu");
    assert_eq!(
        cmu.len(),
        1,
        "the oracle should have printed exactly one mass fraction"
    );
    let (gm1, gm2, mu_c) = (cmu[0].values[0], cmu[0].values[1], cmu[0].values[2]);

    let mu = unsafe { core_sys::cr3bp_mu(gm1, gm2) };
    assert_eq!(
        mu.to_bits(),
        mu_c.to_bits(),
        "mu: C gave {mu_c:e}, boundary {mu:e}"
    );

    let jac = tagged(&records, "jac");
    assert_eq!(jac.len(), 1);
    let (x, z, vy, mu_from_c, c_from_c) = (
        jac[0].values[0],
        jac[0].values[1],
        jac[0].values[2],
        jac[0].values[3],
        jac[0].values[4],
    );
    let c = unsafe {
        core_sys::cr3bp_jacobi(
            core_sys::Vec3d { x, y: 0.0, z },
            core_sys::Vec3d {
                x: 0.0,
                y: vy,
                z: 0.0,
            },
            mu_from_c,
        )
    };
    assert_eq!(
        c.to_bits(),
        c_from_c.to_bits(),
        "Jacobi constant: C gave {c_from_c:e}, boundary {c:e}"
    );

    for row in tagged(&records, "lag") {
        let point = row.values[0] as i32;
        let mut out = core_sys::Vec3d {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let result = unsafe { core_sys::cr3bp_lagrange(mu, point, &mut out) };
        assert_eq!(result, CORE_OK, "L{point} did not compute");
        for (name, from_c, from_rust) in [
            ("x", row.values[1], out.x),
            ("y", row.values[2], out.y),
            ("z", row.values[3], out.z),
        ] {
            assert_eq!(
                from_c.to_bits(),
                from_rust.to_bits(),
                "L{point}.{name}: C gave {from_c:e}, boundary {from_rust:e}"
            );
        }
    }

    // Both sides of the gate near L1: a crossing, and no crossing. The second
    // line is precisely the case easily mistaken for a failure, which is why
    // it is in the oracle and here.
    let zvc = tagged(&records, "zvc");
    assert_eq!(zvc.len(), 2, "the oracle should have given both sides of the gate");
    for row in zvc {
        let (c, result_c, r_c) = (row.values[0], row.values[1] as i32, row.values[2]);
        let mut r = 0.0;
        let result = unsafe {
            core_sys::cr3bp_zvc_radius(
                mu,
                c,
                core_sys::Vec3d {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                core_sys::Vec3d {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
                0.95,
                &mut r,
            )
        };
        assert_eq!(result, result_c, "answer code at C = {c}");
        if result == CORE_OK {
            assert_eq!(
                r.to_bits(),
                r_c.to_bits(),
                "radius at C = {c}: C gave {r_c:e}, boundary {r:e}"
            );
        }
    }
}

/// The synodic frame crosses the boundary whole (ROADMAP-UI.md, U6b1).
///
/// `SynodicFrame` is the boundary's largest struct: six `Vec3d` and five
/// `double`, which **C fills itself**. So a layout error here does not give a
/// strange number, it gives a write past the end of the struct. Hence the
/// comparison is not only bitwise: alongside it stands a claim that catches
/// field shift specifically -- the Moon in its own frame must sit at
/// `(1 - mu, 0, 0)`.
#[test]
fn the_synodic_frame_crosses_the_boundary_whole() {
    const EARTH: i32 = 3;
    const MOON: i32 = 4;

    let records = oracle_records();
    let syn = tagged(&records, "syn");
    let fri = tagged(&records, "fri");
    assert_eq!(syn.len(), fri.len(), "the oracle should have given a pair per instant");
    assert!(!syn.is_empty());

    unsafe {
        let ctx = load_fixture();

        for (frame_row, moon_row) in syn.iter().zip(fri.iter()) {
            let t = frame_row.values[0];

            let mut frame = core_sys::SynodicFrame::default();
            let code = core_sys::frame_synodic(ctx, EARTH, MOON, t, &mut frame);
            assert_eq!(code, CORE_OK, "the frame at t = {t} did not build");

            for (name, from_c, from_rust) in [
                ("length", frame_row.values[1], frame.length),
                ("length_rate", frame_row.values[2], frame.length_rate),
                ("rate", frame_row.values[3], frame.rate),
                ("mu", frame_row.values[4], frame.mu),
            ] {
                assert_eq!(
                    from_c.to_bits(),
                    from_rust.to_bits(),
                    "{name} at t = {t}: C gave {from_c:e}, boundary {from_rust:e}"
                );
            }

            let mut moon = State::default();
            assert_eq!(eph_body_state(ctx, MOON, t, &mut moon), CORE_OK);
            let mut moon_syn = State::default();
            core_sys::frame_from_inertial(&frame, &moon, &mut moon_syn);

            // Six numbers are compared rather than the whole `State`: the
            // first field of the oracle line is the requested instant, not the
            // result's `t` (`frame_from_inertial` puts the frame's
            // dimensionless time there).
            for (k, (name, from_rust)) in [
                ("x", moon_syn.r.x),
                ("y", moon_syn.r.y),
                ("z", moon_syn.r.z),
                ("vx", moon_syn.v.x),
                ("vy", moon_syn.v.y),
                ("vz", moon_syn.v.z),
            ]
            .iter()
            .enumerate()
            {
                let from_c = moon_row.values[k + 1];
                assert_eq!(
                    from_c.to_bits(),
                    from_rust.to_bits(),
                    "Moon in its own frame, {name} at t = {t}: \
                     C gave {from_c:e}, boundary {from_rust:e}"
                );
            }

            // And the meaning, not only the bits: by the frame's construction
            // the Moon sits exactly here. A shifted struct field would spoil
            // the basis, and this inequality is what sees that, not the
            // bitwise comparison above.
            assert!(
                (moon_syn.r.x - (1.0 - frame.mu)).abs() < 1e-12,
                "the Moon in its own frame ended up at x = {}",
                moon_syn.r.x
            );
            assert!(
                moon_syn.r.y.abs() < 1e-12 && moon_syn.r.z.abs() < 1e-12,
                "the Moon left the axis of its own frame: {:?}",
                (moon_syn.r.y, moon_syn.r.z)
            );
        }

        eph_free(ctx);
    }
}
