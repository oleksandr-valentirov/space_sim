//! Zero-velocity curves (ROADMAP-UI.md, U6b3).
//!
//! `v^2 = 2*Omega - C`, so where `2*Omega < C` the velocity would be imaginary
//! -- the region is unreachable. That region's boundary is the curve; all the
//! physics here is that inequality, not some contour tracing.
//!
//! ## Why by rays, and why a ray without an answer is also an answer
//!
//! `cr3bp_zvc_radius` (C) finds the first crossing along one ray. The curve is
//! a fan of such rays: Rust turns an angle into a unit vector, because
//! `cos`/`sin` are forbidden in `/core` (CLAUDE.md, invariant 3).
//!
//! A ray with no crossing returns `ToleranceNotMet` -- **topology, not a
//! failure**: in that direction the region is either wholly forbidden or
//! wholly open. Such a ray **breaks** the polyline rather than closing it by a
//! shortcut. It is in that break that the gate near L1 becomes visible: when
//! `C` falls below `C(L1)`, Earth's lobe and the Moon's merge, and the curve
//! in that direction simply stops existing.
//!
//! ## Why two fans rather than one
//!
//! A ray finds the **first** crossing from its own start point. From the
//! barycentre a ray towards the Moon hits the boundary of Earth's lobe and
//! never reaches the Moon's. So there are two fans -- from Earth and from the
//! Moon -- each tracing its own lobe.
//!
//! There is no outer branch (the one bounding the region from outside): its
//! rays would run from outside inwards, which C cannot do. That is named
//! rather than forgotten -- drawing it from the other side would mean
//! inventing a second root search beside the one already checked in
//! `/core`.

use engine::scene::Polyline;

/// How many rays per lobe. 180 is a two-degree step: at 1280 pixels that is a
/// chord shorter than a pixel at any scale where the curve is visible at
/// all.
const RAYS: usize = 180;

/// How far to search for a crossing along a ray, in dimensionless units.
///
/// One and a half times the distance between the bodies: beyond that the outer
/// branch begins, which this pass does not draw.
const R_MAX: f64 = 1.5;

/// The curve's colour. A translucent layer rather than a foreground line: this
/// is reference information (PROJECT.md §7), and it should look like it.
pub const COLOUR: [f32; 4] = [0.55, 0.75, 0.95, 0.35];

/// Zero-velocity curve polylines for the constant `c`.
///
/// Coordinates are metres of the synodic frame, the same scale the bodies and
/// the trajectory sit in (`crate::frame_view::SYNODIC_SCALE_M`).
///
/// An empty result is a legitimate answer: at small `C` there are no forbidden
/// regions around the bodies at all.
pub fn curves(mu: f64, c: f64, scale_m: f64) -> Vec<Polyline> {
    // The bodies sit on the x axis by the synodic frame's construction.
    let earth = core_rs::Vec3d {
        x: -mu,
        y: 0.0,
        z: 0.0,
    };
    let moon = core_rs::Vec3d {
        x: 1.0 - mu,
        y: 0.0,
        z: 0.0,
    };

    let mut out = Vec::new();
    for centre in [earth, moon] {
        out.extend(lobe(mu, c, centre, scale_m));
    }
    out
}

/// Tracing one lobe: rays around a circle, broken where there is no
/// crossing.
fn lobe(mu: f64, c: f64, from: core_rs::Vec3d, scale_m: f64) -> Vec<Polyline> {
    let mut pieces = Vec::new();
    let mut run: Vec<[f64; 3]> = Vec::new();

    // One extra ray at the end closes the circle: 0 and 360 degrees are the
    // same direction, and without it a closed curve would keep a one-step
    // gap.
    for k in 0..=RAYS {
        let angle = std::f64::consts::TAU * k as f64 / RAYS as f64;
        let dir = core_rs::Vec3d {
            x: angle.cos(),
            y: angle.sin(),
            z: 0.0,
        };

        match core_rs::cr3bp_zvc_radius(mu, c, from, dir, R_MAX) {
            Ok(r) => run.push([
                (from.x + r * dir.x) * scale_m,
                (from.y + r * dir.y) * scale_m,
                0.0,
            ]),
            // The curve ends here -- and this is where the gate shows.
            Err(_) => {
                flush(&mut pieces, &mut run);
            }
        }
    }
    flush(&mut pieces, &mut run);
    pieces
}

fn flush(pieces: &mut Vec<Polyline>, run: &mut Vec<[f64; 3]>) {
    if run.len() >= 2 {
        pieces.push(Polyline {
            points: std::mem::take(run),
            colour: COLOUR,
        });
    } else {
        run.clear();
    }
}
