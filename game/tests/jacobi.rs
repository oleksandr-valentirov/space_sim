//! How far the Jacobi constant drifts along a real mission (ROADMAP-UI.md,
//! U6b1).
//!
//! This is a measurement **before** any drawing, and the widget's shape
//! depends on it. `C` is conserved in CR3BP -- there two bodies at a fixed
//! distance rotate uniformly. The game flies in the full ephemeris: the
//! Earth-Moon distance wanders by a tenth over a month, and the model holds
//! the Sun, harmonics and radiation pressure. So a zero-velocity curve built
//! from an instantaneous `C` is **reference, not a boundary**, and the only
//! question is how much it breathes before the eyes.
//!
//! The number that comes out of here decides: a live curve chasing the vessel,
//! or a slice at a chosen instant.

use core_rs::{cr3bp_jacobi, State, Vec3d};
use game::mission;
use game::world::{EARTH, MOON};

/// The vessel's Jacobi constant at every sample of the mission.
///
/// Every sample takes the frame of **its own instant** -- otherwise what would
/// be measured is the pair's own rotation rather than `C`'s drift.
fn jacobi_along_the_mission() -> Vec<(f64, f64)> {
    let eph = std::sync::Arc::new(
        core_rs::Ephemeris::load(&mission::default_asset()).expect("the asset reads"),
    );
    let mut world = mission::world(&mission::default_asset()).expect("world");
    world.run_to_end(1.0, 8);
    let snapshot = world.snapshot();

    let mut out = Vec::new();
    for leg in &snapshot.vessels[0].legs {
        for sample in &leg.samples {
            let t = sample.state.t;
            let frame = eph
                .synodic_frame(EARTH, MOON, t)
                .expect("the frame builds at every instant of the mission");

            let inertial = State {
                t,
                r: Vec3d {
                    x: sample.state.r.x,
                    y: sample.state.r.y,
                    z: sample.state.r.z,
                },
                v: Vec3d {
                    x: sample.state.v.x,
                    y: sample.state.v.y,
                    z: sample.state.v.z,
                },
            };
            let synodic = frame.from_inertial(&inertial);
            out.push((t, cr3bp_jacobi(synodic.r, synodic.v, frame.mass_ratio())));
        }
    }
    out
}

/// The spread of `C` along the mission -- the one number that decides the
/// widget's shape.
///
/// A table of windows is printed rather than one number: a game day at warp
/// 1e5 is ten seconds of watching, a month is five minutes, and "breathes
/// before the eyes" means different things at those two scales. The last row
/// (the whole mission) is no longer drift but decay: the vessel leaves the
/// halo orbit and goes where the pair's synodic frame describes nothing.
#[test]
fn how_far_the_jacobi_constant_drifts_along_the_mission() {
    let series = jacobi_along_the_mission();
    assert!(
        series.len() > 1000,
        "the mission gave only {} points",
        series.len()
    );

    let (t0, c0) = series[0];
    let spread = |from: usize, to: usize| {
        let mut low = f64::INFINITY;
        let mut high = f64::NEG_INFINITY;
        for &(_, c) in &series[from..to] {
            low = low.min(c);
            high = high.max(c);
        }
        high - low
    };

    let whole = spread(0, series.len());

    println!("  C at the start: {c0:.6}");
    println!("  {:>10} {:>12} {:>12}", "window", "C spread", "% of C");
    for days in [1.0, 7.0, 14.0, 30.0] {
        let end = series
            .iter()
            .position(|&(t, _)| t > t0 + days * 86400.0)
            .unwrap_or(series.len());
        let range = spread(0, end);
        println!(
            "  {:>8.0} days {:>12.6} {:>11.4}%",
            days,
            range,
            range / c0 * 100.0
        );
    }
    println!(
        "  {:>8.0} days {:>12.6} {:>11.4}%   (whole mission)",
        (series[series.len() - 1].0 - t0) / 86400.0,
        whole,
        whole / c0 * 100.0
    );

    // The assertions are deliberately loose: what is measured here is physics
    // rather than our code, and too tight a tolerance would turn a measurement
    // into pinning a number. What is caught is different -- that `C` is
    // computed at all (not NaN, not zero) and that it is not constant.
    assert!(c0.is_finite() && (2.0..4.0).contains(&c0), "C = {c0}");
    assert!(
        whole > 0.0,
        "the Jacobi constant did not change at all -- that would mean the \
         measurement measures the wrong thing"
    );
    assert!(
        whole < c0,
        "a spread of {whole:.6}, the size of the constant {c0:.6} itself -- that is no longer drift"
    );
}
