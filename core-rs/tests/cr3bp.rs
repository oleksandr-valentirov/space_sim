//! CR3BP on the boundary means what it should mean (ROADMAP-UI.md, U6b2).
//!
//! `core-sys/tests/ffi.rs` has already compared bits -- but comparing the
//! boundary against itself does not check **meaning**, and this project has
//! stepped on that twice: a conjugated quaternion (R1c) and a porkchop grid in
//! the wrong coordinates (U5b) both passed the bitwise comparison flawlessly.
//!
//! So the numbers here come from **outside**: published Earth-Moon system
//! parameters and geometry checkable without our own code. If a declaration
//! swapped argument order or units, these numbers will not agree even while
//! the bits still match.

use core_rs::{cr3bp_jacobi, cr3bp_lagrange, cr3bp_mu, CoreError, Vec3d};

/// Gravitational parameters from the asset (`data/fixture/earth_moon.eph`,
/// DE440), m^3/s^2. The same numbers the oracle prints.
const GM_EARTH: f64 = 398_600_435_436_000.0;
const GM_MOON: f64 = 4_902_800_066_000.0;

/// Mean Earth-Moon distance in metres -- the one the game scales the map by.
const L_M: f64 = 3.844e8;

/// The pair's mass fraction matches the published one -- and the discrepancy
/// is worth recording.
///
/// 0.0121505856 is the classical Earth-Moon constant (lunar mass as 1/81.30056
/// of Earth's), written into `engine::trajectory::MU`. The asset's `mu`
/// (DE440) gives **0.0121505843**: a difference of 1.3e-9, i.e. 1.1e-7
/// relative.
///
/// Neither number is wrong -- they are two different sources, and this
/// difference is exactly what explains the 0.54 m by which the game's synodic
/// coordinates diverge from the engine's formula (`game/tests/scene.rs`,
/// U6a2). So the tolerance here is named rather than tuned: anything above
/// 5e-9 is a different system.
#[test]
fn the_mass_ratio_matches_the_published_one() {
    let mu = cr3bp_mu(GM_EARTH, GM_MOON);
    let classic = 0.012_150_585_609_624_04;
    println!(
        "  mu from the asset {mu:.12}, classical {classic:.12}, difference {:.2e}",
        mu - classic
    );
    assert!(
        (mu - classic).abs() < 5e-9,
        "mu = {mu:.12}, but the classical one is {classic:.12}"
    );
}

/// The Lagrange points sit where kilometres measure them.
///
/// The oracle is deliberately twofold: L4 has an **exact** formula
/// (`1/2 - mu`, `sqrt(3)/2`), while L1 and L2 have published distances from
/// Earth, 3.26e8 and 4.49e8 m. The first catches an error in the axes, the
/// second in the units: dimensionless coordinates multiplied by the wrong
/// thing would give the right shape at the wrong scale.
#[test]
fn the_lagrange_points_land_where_they_are_measured() {
    let mu = cr3bp_mu(GM_EARTH, GM_MOON);

    let l4 = cr3bp_lagrange(mu, 4).expect("L4 always computes -- it is exact");
    assert!((l4.x - (0.5 - mu)).abs() < 1e-12, "L4.x = {}", l4.x);
    assert!(
        (l4.y - 3.0f64.sqrt() / 2.0).abs() < 1e-12,
        "L4.y = {}",
        l4.y
    );
    assert!(l4.z.abs() < 1e-15, "L4 left the plane: z = {}", l4.z);

    // Distance from **Earth**, not from the barycentre: Earth sits at -mu.
    for (point, published_m) in [(1, 3.26e8), (2, 4.49e8)] {
        let l = cr3bp_lagrange(mu, point).expect("L1 and L2 are found by bisection");
        let from_earth_m = (l.x + mu) * L_M;
        println!("  L{point} at {from_earth_m:.4e} m from Earth");
        assert!(
            (from_earth_m - published_m).abs() / published_m < 0.02,
            "L{point} at {from_earth_m:.4e} m, but measured as {published_m:.3e} m"
        );
    }

    // There is no point outside 1..5, and that is a rejection, not a silent
    // zero.
    assert!(matches!(cr3bp_lagrange(mu, 6), Err(CoreError::InvalidArg)));
}

/// The Jacobi constant at a Lagrange point is `2*Omega` there, and it rises
/// from L1 to L3.
///
/// The order `C(L1) > C(L2) > C(L3)` is a textbook property of the system, not
/// a number of ours: the gate opens first to L1, then to L2, then to L3. An
/// error in the `(r, v, mu)` argument order would destroy exactly that while
/// leaving plausible magnitudes.
#[test]
fn the_jacobi_constant_orders_the_gates_the_way_the_textbook_does() {
    let mu = cr3bp_mu(GM_EARTH, GM_MOON);

    let c = |point: i32| {
        let l = cr3bp_lagrange(mu, point).expect("the point exists");
        cr3bp_jacobi(l, Vec3d::default(), mu)
    };
    let (c1, c2, c3) = (c(1), c(2), c(3));
    println!("  C(L1) = {c1:.6}, C(L2) = {c2:.6}, C(L3) = {c3:.6}");

    assert!(
        c1 > c2,
        "C(L1) = {c1:.6} should be greater than C(L2) = {c2:.6}"
    );
    assert!(
        c2 > c3,
        "C(L2) = {c2:.6} should be greater than C(L3) = {c3:.6}"
    );

    // And the magnitude: for Earth-Moon all three sit near three.
    for (name, value) in [("C1", c1), ("C2", c2), ("C3", c3)] {
        assert!(
            (2.9..3.3).contains(&value),
            "{name} = {value:.6} -- that is no longer the Earth-Moon system"
        );
    }

    // Velocity is subtracted, not added: a moving point has a smaller `C`.
    let l1 = cr3bp_lagrange(mu, 1).expect("L1 exists");
    let moving = cr3bp_jacobi(
        l1,
        Vec3d {
            x: 0.0,
            y: 0.1,
            z: 0.0,
        },
        mu,
    );
    assert!(
        moving < c1,
        "motion should have reduced C: {moving:.6} against {c1:.6}"
    );
    assert!(
        (c1 - moving - 0.01).abs() < 1e-12,
        "C should have dropped by exactly v^2 = 0.01, but dropped by {:.12}",
        c1 - moving
    );
}
