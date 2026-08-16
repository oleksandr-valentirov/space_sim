//! The zero-velocity curves say what they should about the system (UI,
//! U6b3).
//!
//! The oracle here is topology, and **in both directions**: a curve drawn the
//! same way every time looks correct. So it is checked both that at a high `C`
//! the gate is shut (two closed lobes, one around each body) and that at a low
//! one it is open (Earth's lobe reaches past L1).
//!
//! Plus a claim about the vessel itself: it never ends up in the forbidden
//! region built from its own `C` at the start. That is no tautology -- `C`
//! drifts (U6b1 measured 0.007% per day), and it is exactly that drift this
//! bounds with a number.

use core_rs::{cr3bp_jacobi, cr3bp_lagrange, cr3bp_mu, Vec3d};
use game::mission;
use game::world::{EARTH, MOON};
use game::zvc;

/// The fixture's `mu` -- the same pair as in the game.
fn mass_ratio() -> f64 {
    cr3bp_mu(398_600_435_436_000.0, 4_902_800_066_000.0)
}

/// `2*Omega` at a point: the `C` of a body standing still.
///
/// There is no separate potential function on the boundary and none is needed
/// -- zero velocity is the curve's definition.
fn two_omega(r: Vec3d, mu: f64) -> f64 {
    cr3bp_jacobi(r, Vec3d::default(), mu)
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// At a `C` above `C(L1)` the gate is shut: two closed lobes with nothing
/// between them.
#[test]
fn a_high_jacobi_constant_shuts_the_gate_around_each_body() {
    let mu = mass_ratio();
    let l1 = cr3bp_lagrange(mu, 1).expect("L1 exists");
    let c1 = two_omega(l1, mu);

    // Unit scale: what is checked here is geometry rather than conversion to
    // metres.
    let curves = zvc::curves(mu, c1 + 0.05, 1.0);
    assert_eq!(
        curves.len(),
        2,
        "with the gate shut there are exactly two lobes -- one per body"
    );

    for (name, piece) in [("Earth", &curves[0]), ("Moon", &curves[1])] {
        let first = piece.points[0];
        let last = piece.points[piece.points.len() - 1];
        assert!(
            distance(first, last) < 1e-6,
            "the {name} lobe did not close: {first:?} against {last:?}"
        );
    }

    // And the main number: the Moon's lobe does not reach L1, i.e. the gate
    // really is shut rather than "nearly".
    let moon_lobe_min_x = curves[1]
        .points
        .iter()
        .map(|p| p[0])
        .fold(f64::INFINITY, f64::min);
    println!(
        "  the Moon lobe starts at x = {moon_lobe_min_x:.4}, L1 at {:.4}",
        l1.x
    );
    assert!(
        moon_lobe_min_x > l1.x,
        "the Moon lobe reached {moon_lobe_min_x:.4}, while L1 is at {:.4}",
        l1.x
    );
}

/// At a `C` below `C(L1)` the gate is open: Earth's lobe reaches past L1, and
/// the curve breaks where the regions merged.
#[test]
fn a_low_jacobi_constant_opens_the_gate_at_l1() {
    let mu = mass_ratio();
    let l1 = cr3bp_lagrange(mu, 1).expect("L1 exists");
    let c1 = two_omega(l1, mu);

    let curves = zvc::curves(mu, c1 - 0.05, 1.0);
    assert!(
        curves.len() > 2,
        "with the gate open the curve breaks, but here there are {} pieces",
        curves.len()
    );

    let reach = curves
        .iter()
        .flat_map(|p| p.points.iter())
        .map(|p| p[0])
        .fold(f64::NEG_INFINITY, f64::max);
    println!("  the curve reaches x = {reach:.4}, L1 at {:.4}", l1.x);
    assert!(
        reach > l1.x,
        "the curve did not pass L1: {reach:.4} against {:.4}",
        l1.x
    );

    // And the floor: at a small enough `C` there is no forbidden region at
    // all, and an empty result is an answer rather than a failure.
    assert!(
        zvc::curves(mu, 2.5, 1.0).is_empty(),
        "at C = 2.5 there can be no forbidden region"
    );
}

/// The vessel never flies into its own forbidden region.
///
/// `C` is taken at the start -- which is exactly why this is no tautology:
/// over 42 days of prediction it drifts, and the claim holds exactly as far as
/// that drift is small. Had the curve been drawn from someone else's `C` (say,
/// from the barycentre's `2*Omega`, or with the sign of `v^2` confused), the
/// vessel would be inside a wall at the very first sample.
#[test]
fn the_vessel_never_enters_the_region_its_own_constant_forbids() {
    let eph = core_rs::Ephemeris::load(&mission::default_asset()).expect("the asset");
    let mut world = mission::world(&mission::default_asset()).expect("the world");
    world.tick(16);
    let snapshot = world.snapshot();

    let c0 = snapshot.vessels[0].jacobi.expect("C is computed");
    let mut worst = f64::INFINITY;
    let mut samples = 0;

    for leg in &snapshot.vessels[0].legs {
        for sample in &leg.samples {
            let frame = eph
                .synodic_frame(EARTH, MOON, sample.state.t)
                .expect("the frame exists");
            let synodic = frame.from_inertial(&sample.state);
            // The margin: how far `2*Omega` here exceeds the starting `C`. A
            // negative value would mean the vessel inside a wall.
            worst = worst.min(two_omega(synodic.r, frame.mass_ratio()) - c0);
            samples += 1;
        }
    }

    println!("  {samples} samples, smallest margin 2*Omega - C0 = {worst:.6}");
    assert!(
        worst > -0.01,
        "the vessel entered the forbidden region by {:.6} -- that is no longer C drift",
        -worst
    );
}
