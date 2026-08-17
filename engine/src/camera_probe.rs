//! Camera-relative against the naive path (ROADMAP F4).
//!
//! PROJECT.md section 7, decision 1: world coordinates NEVER reach a `float`.
//! Transforms are computed in `double` on the CPU relative to the camera, and
//! what goes to the shader is already `float` in camera space.
//!
//! This module measures what that buys. Both paths end the same way -- a quad
//! in camera space, the same shader, the same pipeline. The difference is only
//! in the CPU arithmetic:
//!
//!   camera-relative   `(object_world - camera_world) as f32`
//!   naive             `object_world as f32 - camera_world as f32`
//!
//! The second loses everything subtracting close large numbers. At 1 AU
//! (1.496e11 m) one ULP in `f32` is about **16 km**, so both coordinates first
//! land on a 16-kilometre lattice and only then get subtracted. An object ten
//! metres from the camera ends up either exactly on it or kilometres away.
//!
//! What is measured is the shift of the object's centroid in pixels between
//! frames while the camera moves in millimetre steps. The correct path gives
//! smooth subpixel motion; the naive one gives stillness and then a jump --
//! exactly the jitter.

use crate::depth;
use crate::depth_probe::{render_quads, Params};
use crate::gpu::Gpu;

/// Distance from the world origin -- the same as in the F4 criterion.
pub const ASTRONOMICAL_UNIT: f64 = 1.495_978_707e11;

/// Metres between the camera and the object. Ten metres is the scale of the
/// ship's local scene from PROJECT.md section 7.
const RANGE: f64 = 10.0;

/// Half width of the object. A metre at ten metres is noticeable but not
/// screen-filling, otherwise the centroid would show nothing.
const HALF_SIZE: f64 = 1.0;

const FOV_Y: f64 = std::f64::consts::PI / 3.0;
const NEAR: f64 = 0.1;

pub struct Step {
    /// Centroid shift against the previous frame, pixels.
    pub shift: f64,
    /// How many pixels the object took. Zero means it is not visible.
    pub visible: u64,
}

/// Runs the camera through `steps` steps of `step_m` metres and returns how
/// the object moved in the frame.
pub fn sweep(
    gpu: &Gpu,
    size: u32,
    relative: bool,
    steps: u32,
    step_m: f64,
) -> Result<Vec<Step>, String> {
    sweep_at(gpu, size, relative, steps, step_m, ASTRONOMICAL_UNIT)
}

/// The same, but with an arbitrary distance to the world origin.
///
/// Exists because at 1 AU the naive path does not jitter, it **vanishes**: a
/// 16-kilometre ULP against a ten-metre scene leaves nothing of it. Jitter is
/// visible where the ULP is comparable to the size of the object, and finding
/// that distance is a separate question the sweep answers.
pub fn sweep_at(
    gpu: &Gpu,
    size: u32,
    relative: bool,
    steps: u32,
    step_m: f64,
    origin_distance: f64,
) -> Result<Vec<Step>, String> {
    let projection = depth::reversed_infinite(FOV_Y, 1.0, NEAR);

    // Both are far from the origin. That is what breaks the naive path: not
    // a large distance between them, but a large distance to zero.
    let object_world = [origin_distance, 0.0, 0.0];

    let mut out = Vec::new();
    let mut previous: Option<(f64, f64)> = None;

    for i in 0..steps {
        let camera_world = [
            origin_distance + RANGE,
            f64::from(i) * step_m,
            f64::from(i) * step_m,
        ];

        let view = if relative {
            // Subtract in double, narrow only afterwards. The difference is
            // small, so f32 represents it exactly.
            [
                (object_world[0] - camera_world[0]) as f32,
                (object_world[1] - camera_world[1]) as f32,
                (object_world[2] - camera_world[2]) as f32,
            ]
        } else {
            // Narrow first, subtract afterwards. Both terms land on a lattice
            // of ~16 km, and what remains is the noise of that lattice.
            [
                object_world[0] as f32 - camera_world[0] as f32,
                object_world[1] as f32 - camera_world[1] as f32,
                object_world[2] as f32 - camera_world[2] as f32,
            ]
        };

        // The camera looks along -z, while the difference above lies along
        // +x. Translate: distance from the camera becomes -z.
        let centre = [view[1], view[2], -view[0].abs()];

        let params = Params {
            projection,
            colour: [0.2, 0.9, 0.3, 1.0],
            placement: [centre[0], centre[1], centre[2], HALF_SIZE as f32],
        };

        let measured = render_quads(gpu, size, size, true, &[params])?;
        let centroid = centroid(&measured.shot);

        let shift = match (previous, centroid) {
            (Some((px, py)), Some((cx, cy))) => ((cx - px).powi(2) + (cy - py).powi(2)).sqrt(),
            _ => 0.0,
        };

        if centroid.is_some() {
            previous = centroid;
        }

        out.push(Step {
            shift,
            visible: visible_pixels(&measured.shot),
        });
    }

    Ok(out)
}

fn visible_pixels(shot: &crate::shot::Shot) -> u64 {
    let mut count = 0;
    for y in 0..shot.height {
        for x in 0..shot.width {
            if shot.pixel(x, y)[1] > 40 {
                count += 1;
            }
        }
    }
    count
}

/// Centroid of the visible pixels. `None` if the object is not in the frame.
fn centroid(shot: &crate::shot::Shot) -> Option<(f64, f64)> {
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut count = 0u64;

    for y in 0..shot.height {
        for x in 0..shot.width {
            if shot.pixel(x, y)[1] > 40 {
                sum_x += f64::from(x);
                sum_y += f64::from(y);
                count += 1;
            }
        }
    }

    if count == 0 {
        None
    } else {
        Some((sum_x / count as f64, sum_y / count as f64))
    }
}
