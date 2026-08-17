//! A flight from the surface to orbit with no jumps (ROADMAP F5).
//!
//! The camera looks at the centre of the sphere along a fixed radial
//! direction, from a distance of `R + altitude`. The sphere is convex, so the
//! angle its edge subtends from an outside point has an exact formula:
//!
//! ```text
//! half_angle = asin(R / (R + altitude))
//! ```
//!
//! Comparing the measured against the computed is the same instrument as in
//! `depth_probe` (F3, `resolvable_gap`) and `camera_probe` (F4): not "looks
//! about right" but a number against a number.

use crate::camera::Camera;
use crate::gpu::Gpu;
use crate::shot::Shot;
use crate::sphere::{self, Mesh};
use crate::sphere_render::{self, Params};

const FOV_Y: f64 = std::f64::consts::PI / 3.0;

/// Far smaller than the closest pass (10 m): the camera must not run into
/// `near` before it reaches the surface.
const NEAR: f64 = 1.0;

const LIGHT_DIR: [f32; 3] = [0.4, 0.4, 0.82];
const COLOUR: [f32; 4] = [0.2, 0.6, 0.9, 1.0];

pub struct Sample {
    pub altitude: f64,
    pub expected_half_angle: f64,
    /// The fraction of the frame's pixels the sphere occupies.
    pub coverage: f64,
    pub shot: Shot,
}

/// Draws the sphere from `altitude` above the surface, camera on the centre.
pub fn measure(
    gpu: &Gpu,
    width: u32,
    height: u32,
    mesh: &Mesh,
    altitude: f64,
) -> Result<Sample, String> {
    let radius = sphere::EARTH_RADIUS_M;
    let distance = radius + altitude;

    let camera = Camera::look_at([distance, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);

    let shot = sphere_render::render(
        gpu,
        width,
        height,
        &camera,
        mesh,
        &Params {
            near: NEAR,
            light_dir: LIGHT_DIR,
            colour: COLOUR,
        },
    )?;

    let coverage = coverage_fraction(&shot);

    Ok(Sample {
        altitude,
        expected_half_angle: (radius / distance).asin(),
        coverage,
        shot,
    })
}

fn coverage_fraction(shot: &Shot) -> f64 {
    let mut lit = 0u64;
    let total = u64::from(shot.width) * u64::from(shot.height);
    for y in 0..shot.height {
        for x in 0..shot.width {
            let p = shot.pixel(x, y);
            if p[0] > 3 || p[1] > 3 || p[2] > 3 {
                lit += 1;
            }
        }
    }
    lit as f64 / total as f64
}

/// The analytic frame fraction for a silhouette disc of radius `half_angle`.
///
/// Defined only in the two unambiguous cases: the disc entirely inside the
/// frame, or the frame entirely inside the disc (the sphere fills everything,
/// corners included). The range in between, where the disc is clipped by the
/// frame edge but does not cover the corners, has no simple formula without a
/// circular-segment integral and is deliberately not computed here.
pub fn expected_coverage(half_angle: f64, aspect: f64) -> Option<f64> {
    let radius_fraction = half_angle.tan() / (FOV_Y / 2.0).tan();
    let min_extent = aspect.min(1.0);
    let diagonal = (1.0 + aspect * aspect).sqrt();

    if radius_fraction <= min_extent {
        Some(std::f64::consts::PI * radius_fraction * radius_fraction / (4.0 * aspect))
    } else if radius_fraction >= diagonal {
        Some(1.0)
    } else {
        None
    }
}

/// Altitudes from 10 m to 1e7 m, `steps` points, uniform in the logarithm --
/// exactly the range of the F5 criterion.
pub fn altitudes(steps: u32) -> Vec<f64> {
    let lo = 10f64.log10();
    let hi = 7.0;
    (0..steps)
        .map(|i| {
            let t = f64::from(i) / f64::from(steps - 1);
            10f64.powf(lo + t * (hi - lo))
        })
        .collect()
}

/// Runs `altitudes(steps)` and returns the measured samples -- used both for
/// printing the table and for the continuity test.
pub fn sweep(gpu: &Gpu, size: u32, mesh: &Mesh, steps: u32) -> Result<Vec<Sample>, String> {
    altitudes(steps)
        .into_iter()
        .map(|altitude| measure(gpu, size, size, mesh, altitude))
        .collect()
}
