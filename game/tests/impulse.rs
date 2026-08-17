//! The physics of an impulse against external problems (ROADMAP L4, debt D1).
//!
//! `plan.rs` checks that the world flies a plan the way hand-stitched
//! `prop_run` calls do. That is a check of the **machinery**, and it says
//! nothing about whether the impulse makes physical sense: both sides there
//! do `v += dv` by the same formula and would both be wrong in the same way.
//!
//! Debt D1 named this hole as one claim. On closer look there are **two**,
//! and one test does not cover them.
//!
//! ## 1. The impulse takes the vessel where it promised
//!
//! The oracle is an external problem: Lambert (now at the boundary, L3) gives
//! the initial approximation, `prop_run_stm` corrects it in the full force
//! model, and the vessel must arrive at **the Moon's position from the
//! asset**. The target is neither invented nor computed by the machinery
//! under test: it is a number from the ephemeris.
//!
//! The manoeuvre is given in `Frame::Inertial` on purpose -- so that this
//! check knows nothing about the VNB basis. Its subject is applying the
//! impulse and the segment loop around it.
//!
//! ## 2. VNB means what it says
//!
//! Here the external problem of point 1 will not do, and that is the main
//! thing planning L4 turned up. If an inertial dv is expressed in VNB by the
//! same basis the game then unfolds it with, an error in the basis
//! **cancels against itself** -- swapped `normal` and `outward` would pass
//! such a check flawlessly.
//!
//! So the oracle is a different, textbook one, and none of its claims comes
//! from `dv_inertial`:
//!
//! - a pure **prograde** impulse is parallel to the velocity, so
//!   `|v+dv| = |v|+|dv|` and the orbital plane does not change;
//! - a pure **normal** one is perpendicular to it, so `|v+dv|^2 =
//!   |v|^2+|dv|^2`, and the plane does change;
//! - a pure **outward** one lies in the orbital plane and points **away**
//!   from the body.
//!
//! Prograde and normal are told apart by how the speed grows -- linearly
//! against quadratically -- and not by how they were computed.

use std::sync::Arc;

use core_rs::{lambert_solve, Ephemeris, PropConfig, Propagator, State, Vec3d};
use game::mission;
use game::plan::{Frame, Manoeuvre, Plan};
use game::world::{VesselId, World, EARTH};

const DAY: f64 = 86400.0;
const MOON: i32 = 4;

/// Earth's GM, `data/horizons/obj_earth.txt` -- the same number as in
/// `core/bench/bench_field.c` and the C tests. The asset does not hand it
/// across the boundary (there is no `eph_body_mu` there), so it is written
/// out here rather than read.
const MU_EARTH: f64 = 3.98600435436e14;

/// A low circular Earth orbit.
const R_LEO: f64 = 6.678e6;

/// When we burn and when we arrive. Three days is a typical transfer to the
/// Moon, and it is at this scale that two-body Lambert is wrong enough for
/// the correction to have something to correct. The start is ten minutes
/// before ignition: the manoeuvre must be in the future relative to where the
/// vessel began, or there is nowhere to apply it.
const T0: f64 = T_BURN - 600.0;
const T_BURN: f64 = 2.0 * DAY;
const T_ARRIVE: f64 = T_BURN + 3.0 * DAY;

fn sub(a: Vec3d, b: Vec3d) -> Vec3d {
    Vec3d {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

fn add(a: Vec3d, b: Vec3d) -> Vec3d {
    Vec3d {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

fn scale(a: Vec3d, k: f64) -> Vec3d {
    Vec3d {
        x: a.x * k,
        y: a.y * k,
        z: a.z * k,
    }
}

fn unit(a: Vec3d) -> Vec3d {
    scale(a, 1.0 / norm(a))
}

fn norm(a: Vec3d) -> f64 {
    (a.x * a.x + a.y * a.y + a.z * a.z).sqrt()
}

fn dot(a: Vec3d, b: Vec3d) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn cross(a: Vec3d, b: Vec3d) -> Vec3d {
    Vec3d {
        x: a.y * b.z - a.z * b.y,
        y: a.z * b.x - a.x * b.z,
        z: a.x * b.y - a.y * b.x,
    }
}

fn config() -> PropConfig {
    PropConfig {
        tol_m: mission::TOL_M,
        h_max_s: mission::H_MAX_S,
        ..PropConfig::default()
    }
}

/// A circular departure orbit **in the transfer plane**, and that is not
/// cosmetics.
///
/// The first version of the test took ready numbers from `core-sys/oracle.c`
/// -- and Lambert demanded 11.7 km/s instead of a realistic four and a half,
/// because that orbit's plane did not contain the target, so half the dv went
/// into a plane change. Newton did not converge on such a problem at all.
/// Real missions are built the same way: the plane first, then the window.
///
/// The starting point is roughly opposite the target (~170 degrees), so that
/// the transfer is close to Hohmann rather than a quarter-revolution arc.
fn leo_start(eph: &Ephemeris, target_dir: Vec3d) -> State {
    let earth = eph
        .body_state(EARTH, T0)
        .expect("Earth within the asset's span");

    let sideways = unit(cross(
        target_dir,
        Vec3d {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        },
    ));
    let r_dir = unit(add(scale(target_dir, -1.0), scale(sideways, 0.18)));
    let plane_normal = unit(cross(r_dir, target_dir));
    let v_dir = unit(cross(plane_normal, r_dir));

    State {
        t: T0,
        r: add(earth.r, scale(r_dir, R_LEO)),
        v: add(earth.v, scale(v_dir, (MU_EARTH / R_LEO).sqrt())),
    }
}

/// Solves a 3x3 system by Cramer's rule. Appropriate here precisely because
/// the system is small and fixed: no pivoting, no library, and what is being
/// computed is visible.
fn solve3(a: [[f64; 3]; 3], b: [f64; 3]) -> [f64; 3] {
    let det = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };

    let d = det(a);
    assert!(
        d.abs() > 0.0,
        "the sensitivity matrix is singular -- that is not a converged correction \
         but the absence of one"
    );

    let mut out = [0.0; 3];
    for (col, value) in out.iter_mut().enumerate() {
        let mut m = a;
        for row in 0..3 {
            m[row][col] = b[row];
        }
        *value = det(m) / d;
    }
    out
}

/// Oracle 1: an impulse found by Lambert and corrected by the state
/// transition matrix takes the vessel to the Moon's position from the asset.
///
/// Why this is an external problem and not another run of the same machinery:
/// the target is a number from the ephemeris, the initial approximation comes
/// from `lambert_solve`, i.e. from code that knows nothing about the plan or
/// the world. Were the impulse applied differently from what is assumed here,
/// the vessel simply would not arrive.
#[test]
fn a_lambert_burn_corrected_by_the_stm_arrives_where_the_moon_is() {
    let eph = Arc::new(Ephemeris::load(&mission::default_asset()).expect("asset"));
    let mut prop = Propagator::new(eph.clone(), config()).expect("propagator");

    let earth_arrive = eph.body_state(EARTH, T_ARRIVE).expect("Earth");
    let moon_arrive = eph.body_state(MOON, T_ARRIVE).expect("Moon");
    let target = sub(moon_arrive.r, earth_arrive.r);

    let start = leo_start(&eph, unit(target));

    // The state at ignition comes by propagating from [`T0`], because that is
    // where the game will take it from.
    let mut step = 0.0;
    let (at_burn, _) = prop
        .run_stm(&start, None, T_BURN, &mut step)
        .expect("the run to ignition");

    let earth_burn = eph.body_state(EARTH, T_BURN).expect("Earth");
    let r1 = sub(at_burn.r, earth_burn.r);

    // `prograde` is the sign of the z component of the angular momentum, and
    // that is how it is computed here rather than guessed with a flag. Getting
    // it wrong would mean solving a different problem that also converges.
    let prograde = cross(r1, target).z > 0.0;

    let (v1, _v2) = lambert_solve(r1, target, T_ARRIVE - T_BURN, MU_EARTH, prograde, 0)
        .expect("a two-body transfer to the Moon exists");

    let mut dv = sub(add(earth_burn.v, v1), at_burn.v);
    assert!(
        norm(dv) < 6.0e3,
        "Lambert demanded {:.4e} m/s -- that is the price of a plane change, not \
         of a transfer. The departure orbit is not in the target's plane.",
        norm(dv)
    );

    // Newton on dv: the residual is the position miss at arrival, the
    // derivative is the d r_final / d v_initial block of the state transition
    // matrix.
    //
    // **The step is halved, and that is measured rather than caution.** At
    // full step the sequence of misses jumps (1.2e7 -> 4.4e6 -> 2.2e6 ->
    // 6.2e6): near the Moon the problem is noticeably non-linear and Newton
    // overshoots. Halved, it is monotone and falls by three orders. This is
    // the same non-linearity for which real missions make corrections instead
    // of one exact impulse.
    const DAMPING: f64 = 0.5;
    let mut miss = Vec::new();
    for _ in 0..8 {
        let burned = State {
            t: T_BURN,
            r: at_burn.r,
            v: add(at_burn.v, dv),
        };

        let mut step = 0.0;
        let (arrived, phi) = prop
            .run_stm(&burned, None, T_ARRIVE, &mut step)
            .expect("the run to arrival");

        let residual = sub(arrived.r, moon_arrive.r);
        miss.push(norm(residual));

        let mut jacobian = [[0.0; 3]; 3];
        for (i, row) in jacobian.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = phi.get(i, 3 + j);
            }
        }

        let delta = solve3(jacobian, [-residual.x, -residual.y, -residual.z]);
        dv = add(
            dv,
            scale(
                Vec3d {
                    x: delta[0],
                    y: delta[1],
                    z: delta[2],
                },
                DAMPING,
            ),
        );
    }

    // The two-body approximation misses by enough that there is something to
    // correct; after the correction the miss falls by orders. Both claims are
    // needed: without the first the test would pass on a problem where nothing
    // happens. The last correction is not checked yet: the loop computes the
    // residual and then fixes dv. Hence a final run of its own -- it gives
    // both the miss and the prediction the game is compared against below.
    let mut step = 0.0;
    let (predicted, _) = prop
        .run_stm(
            &State {
                t: T_BURN,
                r: at_burn.r,
                v: add(at_burn.v, dv),
            },
            None,
            T_ARRIVE,
            &mut step,
        )
        .expect("the final run");
    let final_miss = norm(sub(predicted.r, moon_arrive.r));
    assert!(
        miss[0] > 1.0e6,
        "two-body Lambert missed by only {:.3e} m -- then the correction proves \
         nothing",
        miss[0]
    );
    assert!(
        final_miss < 5.0e4,
        "after seven corrections the miss is {final_miss:.3e} m. The sequence: {miss:?}"
    );

    // --- and the same through the game ---
    //
    // The same dv, given as a plan. If the world applies the impulse
    // differently from the run just made, the vessel arrives elsewhere -- no
    // matter that both use the same integrator.
    let mut world = World::with_ephemeris(eph.clone(), config(), T0, mission::DEFAULT_WARP)
        .expect("the world builds");
    let id = world.add_vessel("lambert", start, T_ARRIVE, None);
    assert_eq!(id, VesselId(0));

    let mut plan = Plan::new();
    plan.insert(Manoeuvre {
        t: T_BURN,
        dv: [dv.x, dv.y, dv.z],
        frame: Frame::Inertial,
    });
    world
        .commit_plan(id, plan)
        .expect("a manoeuvre in the future");
    world.run_to_end(1.0, 64);

    let flown = world.vessels()[0].trajectory.state_at(T_ARRIVE);

    // Compared against **the run's prediction** rather than against the Moon,
    // and that is no weakening. The question of this test is whether the world
    // applies the impulse the way the run did; how closely either hit the Moon
    // was said above. Comparing with the Moon would mix two different errors.
    let drift = norm(sub(flown.r, predicted.r));
    let game_miss = norm(sub(flown.r, moon_arrive.r));

    // The threshold comes from measurement, not from caution. The intrinsic
    // drift is 25 m: the world cuts the run into legs by buffer fill, while
    // the run above went in one call, so the step sequences differ and near
    // the Moon the difference is amplified. An error of 1e-6 of dv already
    // gives 1194 m, one of 1e-5 gives 1.1e4. Five hundred metres leaves a
    // twentyfold margin over the intrinsic drift and still catches the
    // smallest of the three measured mutations.
    assert!(
        drift < 5.0e2,
        "the game arrived {drift:.3e} m from what the run with the same dv \
         predicted (Moon miss {game_miss:.3e} m against {final_miss:.3e} m). The \
         difference here is the segment loop or the moment the impulse is \
         applied, not the physics."
    );
}

/// Oracle 2: the VNB basis means what it says.
///
/// Three claims of textbook two-body mechanics, none of them derived from
/// `dv_inertial`. The main one is that prograde and normal impulses change
/// the speed **differently**: linearly against quadratically. Swapping them
/// and passing this test is impossible.
#[test]
fn the_vnb_basis_means_what_it_says() {
    let eph = Ephemeris::load(&mission::default_asset()).expect("asset");
    let moon = eph.body_state(MOON, T_ARRIVE).expect("Moon");
    let earth_arrive = eph.body_state(EARTH, T_ARRIVE).expect("Earth");
    let vessel = leo_start(&eph, unit(sub(moon.r, earth_arrive.r)));
    let earth = eph.body_state(EARTH, T0).expect("Earth");

    let rel_r = sub(vessel.r, earth.r);
    let rel_v = sub(vessel.v, earth.v);
    let h = cross(rel_r, rel_v);
    let speed = norm(rel_v);

    let burn = 100.0;
    let inertial = |dv: [f64; 3]| {
        Manoeuvre {
            t: T0,
            dv,
            frame: Frame::Vnb { body: EARTH },
        }
        .dv_inertial(&vessel, Some(&earth))
    };

    // --- prograde: parallel to the velocity ---
    let prograde = inertial([burn, 0.0, 0.0]);
    let after = Vec3d {
        x: rel_v.x + prograde[0],
        y: rel_v.y + prograde[1],
        z: rel_v.z + prograde[2],
    };

    assert!(
        (norm(after) - (speed + burn)).abs() < 1e-6,
        "the prograde impulse should have given |v|+dv = {:.9e}, gave {:.9e}. That \
         is not speed along the velocity vector.",
        speed + burn,
        norm(after)
    );

    let h_after = cross(rel_r, after);
    let tilt = norm(cross(h, h_after)) / (norm(h) * norm(h_after));
    assert!(
        tilt < 1e-12,
        "the prograde impulse turned the orbital plane by {tilt:.3e} -- it must not \
         leave the plane at all"
    );

    // --- normal: perpendicular, hence Pythagoras ---
    let normal = inertial([0.0, burn, 0.0]);
    let after = Vec3d {
        x: rel_v.x + normal[0],
        y: rel_v.y + normal[1],
        z: rel_v.z + normal[2],
    };

    let pythagoras = (speed * speed + burn * burn).sqrt();
    assert!(
        (norm(after) - pythagoras).abs() < 1e-6,
        "the normal impulse should have given sqrt(|v|^2+dv^2) = {pythagoras:.9e}, \
         gave {:.9e}. If it came out as |v|+dv, normal and prograde are swapped.",
        norm(after)
    );

    let h_after = cross(rel_r, after);
    let tilt = norm(cross(h, h_after)) / (norm(h) * norm(h_after));
    assert!(
        tilt > 1e-3,
        "the normal impulse did not turn the orbital plane (tilt {tilt:.3e}) -- then \
         it is not normal"
    );

    // --- outward: in the orbital plane and AWAY from the body ---
    let outward = inertial([0.0, 0.0, burn]);
    let outward = Vec3d {
        x: outward[0],
        y: outward[1],
        z: outward[2],
    };

    assert!(
        dot(outward, rel_r) > 0.0,
        "\"outward\" points towards the body rather than away from it: r.dv = \
         {:.3e}. That is the sign in cross(prograde, normal), i.e. the \
         orientation of the triple.",
        dot(outward, rel_r)
    );
    assert!(
        dot(outward, h).abs() / (burn * norm(h)) < 1e-12,
        "\"outward\" left the orbital plane"
    );
    assert!(
        dot(outward, rel_v).abs() / (burn * speed) < 1e-12,
        "\"outward\" has a component along the velocity"
    );
}
