//! Where to compute the rotating-frame transform -- on the GPU in `f32` or on
//! the CPU in `f64` (ROADMAP-UI.md, U6a1).
//!
//! PROJECT.md section 7 calls the transform in the vertex shader "the key
//! trick": the trajectory lives in inertial coordinates, switching frames is a
//! choice of pipeline, with no recomputation. F6 did exactly that and measured
//! that the GPU formula matches the C oracle.
//!
//! But the same section 7 has decision 1: **world coordinates never in a
//! `float`**. And the transform on the GPU demands exactly them -- in F6 a
//! vertex carries geocentric `vessel - earth` and `moon - earth`, up to 4e8 m,
//! in `f32`. In F6 that did not hurt, because the camera there is fixed and
//! distant; an interactive camera approaches the vessel, and the question
//! becomes quantitative.
//!
//! Hence two numbers here rather than an opinion:
//!
//! 1. **How many metres the `f32` path costs.** The same formula is run twice
//!    -- in `f64` from exact numbers and in `f32` from rounded ones, as the
//!    shader would see them -- and the difference is converted to metres and to
//!    pixels at several view widths.
//! 2. **How much the `f64` path costs on the CPU.** A camera-relative pass over
//!    the same points already happens every frame (`frame::Lines::upload`); the
//!    only question is how much the frame transform adds to it. Two numbers
//!    from one run, as always: either without the other means nothing.

use std::time::Instant;

use crate::camera::Camera;
use crate::trajectory::{self, Sample, MU};

/// View widths for which the error is converted to pixels.
///
/// 10 km is the vessel up close, 1e6 km is the whole Earth-Moon system in the
/// frame. Between them is the scale at which a lunar orbit is looked at.
const VIEW_WIDTHS_M: [f64; 4] = [1.0e4, 1.0e5, 1.0e6, 1.0e9];

/// Frame width in pixels, in which the error pixels are measured.
const WIDTH_PX: f64 = 1280.0;

/// The synodic position in `f64` -- the same as
/// [`trajectory::rotating_position`], but with explicit arguments so that an
/// `f32` copy can stand next to it.
fn rotating_f64(vessel: [f64; 3], moon: [f64; 3], z_axis: [f64; 3]) -> [f64; 3] {
    // Geocentric: Earth is already subtracted from both (as in F6 vertex
    // data).
    let d = moon;
    let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let x = [d[0] / length, d[1] / length, d[2] / length];
    let y = [
        z_axis[1] * x[2] - z_axis[2] * x[1],
        z_axis[2] * x[0] - z_axis[0] * x[2],
        z_axis[0] * x[1] - z_axis[1] * x[0],
    ];
    let origin = [MU * d[0], MU * d[1], MU * d[2]];
    let rel = [
        vessel[0] - origin[0],
        vessel[1] - origin[1],
        vessel[2] - origin[2],
    ];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    [
        dot(rel, x) / length,
        dot(rel, y) / length,
        dot(rel, z_axis) / length,
    ]
}

/// The same formula as the vertex shader sees it: inputs rounded to `f32`, all
/// arithmetic in `f32`.
///
/// Not a "model of the shader" but literally it: `trajectory.slang` computes
/// `synodic_basis` and the projection in the same operations and the same
/// order.
fn rotating_f32(vessel: [f64; 3], moon: [f64; 3], z_axis: [f64; 3]) -> [f64; 3] {
    let narrow = |v: [f64; 3]| [v[0] as f32, v[1] as f32, v[2] as f32];
    let (vessel, d, z_axis) = (narrow(vessel), narrow(moon), narrow(z_axis));

    let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let x = [d[0] / length, d[1] / length, d[2] / length];
    let y = [
        z_axis[1] * x[2] - z_axis[2] * x[1],
        z_axis[2] * x[0] - z_axis[0] * x[2],
        z_axis[0] * x[1] - z_axis[1] * x[0],
    ];
    let mu = MU as f32;
    let origin = [mu * d[0], mu * d[1], mu * d[2]];
    let rel = [
        vessel[0] - origin[0],
        vessel[1] - origin[1],
        vessel[2] - origin[2],
    ];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    [
        f64::from(dot(rel, x) / length),
        f64::from(dot(rel, y) / length),
        f64::from(dot(rel, z_axis) / length),
    ]
}

pub struct Precision {
    /// Worst divergence between the two paths, metres.
    pub worst_m: f64,
    /// Which sample it happened at, and how far the vessel was from Earth
    /// there.
    pub worst_sample: usize,
    pub worst_geocentric_m: f64,
    /// Mean divergence, metres -- so it is visible that the worst is not an
    /// outlier.
    pub mean_m: f64,
    /// The scale `L` at the worst sample, metres: synodic units are in it.
    pub length_m: f64,
}

/// How many metres the `f32` path costs over the whole fixture orbit.
pub fn precision(samples: &[Sample]) -> Precision {
    let mut worst_m = 0.0;
    let mut worst_sample = 0;
    let mut worst_geocentric_m = 0.0;
    let mut length_m = 0.0;
    let mut total = 0.0;

    for (index, s) in samples.iter().enumerate() {
        let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let vessel = sub(s.vessel, s.earth);
        let moon = sub(s.moon, s.earth);
        let length = (moon[0] * moon[0] + moon[1] * moon[1] + moon[2] * moon[2]).sqrt();

        let exact = rotating_f64(vessel, moon, s.z_axis);
        let narrow = rotating_f32(vessel, moon, s.z_axis);

        // Synodic units are dimensionless -- the same scale L they were
        // divided by converts them back to metres.
        let error_m = length
            * ((exact[0] - narrow[0]).powi(2)
                + (exact[1] - narrow[1]).powi(2)
                + (exact[2] - narrow[2]).powi(2))
            .sqrt();

        total += error_m;
        if error_m > worst_m {
            worst_m = error_m;
            worst_sample = index;
            worst_geocentric_m =
                (vessel[0] * vessel[0] + vessel[1] * vessel[1] + vessel[2] * vessel[2]).sqrt();
            length_m = length;
        }
    }

    Precision {
        worst_m,
        worst_sample,
        worst_geocentric_m,
        mean_m: total / samples.len() as f64,
        length_m,
    }
}

/// How many pixels `error_m` amounts to in a frame `view_m` wide.
pub fn error_px(error_m: f64, view_m: f64) -> f64 {
    error_m / view_m * WIDTH_PX
}

pub struct Cost {
    pub points: usize,
    /// The pass the frame already does today: camera-relative in `f64`.
    pub camera_ns: f64,
    /// The same plus the frame transform -- what this step asks of the CPU.
    pub camera_and_frame_ns: f64,
}

impl Cost {
    /// By what percentage the pass gets more expensive.
    pub fn overhead(&self) -> f64 {
        (self.camera_and_frame_ns - self.camera_ns) / self.camera_ns * 100.0
    }
}

/// The cost of both passes over the same points, nanoseconds per point.
///
/// Measured **together and in one run**: the difference between runs on one
/// machine is larger than what the transform costs.
pub fn cost(samples: &[Sample], passes: u32) -> Cost {
    let camera = Camera::look_at([4.0e8, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);

    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let points: Vec<[f64; 3]> = samples.iter().map(|s| sub(s.vessel, s.earth)).collect();
    let frames: Vec<([f64; 3], [f64; 3])> = samples
        .iter()
        .map(|s| (sub(s.moon, s.earth), s.z_axis))
        .collect();

    let mut bytes: Vec<u8> = Vec::with_capacity(points.len() * 12);

    let mut plain = || {
        bytes.clear();
        for &p in &points {
            for value in camera.relative(p) {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    };
    for _ in 0..2 {
        plain();
    }
    let start = Instant::now();
    for _ in 0..passes {
        plain();
    }
    let camera_ns =
        start.elapsed().as_secs_f64() * 1.0e9 / (f64::from(passes) * points.len() as f64);
    assert_eq!(bytes.len(), points.len() * 12);

    let mut with_frame = || {
        bytes.clear();
        for (&p, &(moon, z_axis)) in points.iter().zip(frames.iter()) {
            let turned = rotating_f64(p, moon, z_axis);
            for value in camera.relative(turned) {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    };
    for _ in 0..2 {
        with_frame();
    }
    let start = Instant::now();
    for _ in 0..passes {
        with_frame();
    }
    let camera_and_frame_ns =
        start.elapsed().as_secs_f64() * 1.0e9 / (f64::from(passes) * points.len() as f64);
    assert_eq!(bytes.len(), points.len() * 12);

    Cost {
        points: points.len(),
        camera_ns,
        camera_and_frame_ns,
    }
}

/// Both numbers from one run -- what `--rotating-probe` prints.
pub fn report() {
    let samples = trajectory::load();

    let p = precision(&samples);
    println!(
        "Precision. The same formula in f64 and in f32 (as the vertex shader\n\
         sees it), {} halo-orbit samples from the fixture.\n",
        samples.len()
    );
    println!(
        "  worst divergence: {:.2} m (sample {}, vessel {:.3e} m from Earth, L = {:.3e} m)",
        p.worst_m, p.worst_sample, p.worst_geocentric_m, p.length_m
    );
    println!("  mean divergence:  {:.2} m\n", p.mean_m);

    println!(
        "  {:>14} {:>12} {:>12}",
        "frame width", "m per pixel", "error, px"
    );
    for view in VIEW_WIDTHS_M {
        println!(
            "  {:>14.0e} {:>12.1} {:>12.2}",
            view,
            view / WIDTH_PX,
            error_px(p.worst_m, view)
        );
    }

    let c = cost(&samples, 200);
    println!(
        "\nCPU cost, {} points, nanoseconds per point (both numbers, one run):\n",
        c.points
    );
    println!("  camera-relative, as today:        {:.2} ns", c.camera_ns);
    println!(
        "  plus the frame transform:         {:.2} ns  ({:+.0}%)",
        c.camera_and_frame_ns,
        c.overhead()
    );
}
