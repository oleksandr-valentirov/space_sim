//! The halo trajectory from stage C, for checking the frame transform
//! (ROADMAP F6).
//!
//! The data is the fixture `data/fixture/halo_inertial.csv`, exported by
//! `core/export/ex_trajectory` (ROADMAP C6): the same catalogue-1151 orbit,
//! carried into a real ephemeris and stitched by multiple shooting (C4). It is
//! committed like `data/fixture/earth_moon.eph` -- the engine does not link
//! `core-rs` (a separate, larger decision, not for a step about rendering), so
//! the data arrives as a ready asset rather than through FFI.
//!
//! The `vx,vy,vz` columns are the vessel velocity. The line does not need it
//! and so it was not exported at first; now it is needed, because the live
//! prediction starts from it (`engine::live`, ROADMAP H5): the propagator
//! needs a state, not a position.
//!
//! The `sx,sy,sz` columns are synodic coordinates from `frame_from_inertial`
//! (C, `core/frame.h`), in dimensionless CR3BP units. That is an oracle, not
//! renderer input: PROJECT.md section 7 requires computing the same transform
//! in the vertex shader from the positions of Earth and the Moon rather than
//! trusting a ready number from a CSV. `engine/tests/trajectory.rs` compares
//! [`rotating_position`] against this oracle.

const CSV: &str = include_str!("../../data/fixture/halo_inertial.csv");

/// mu_Moon / (mu_Earth + mu_Moon). Printed by `make csv`
/// (`ex_cr3bp: ... mu = 0.012150585609624041`) -- a mass constant of the
/// system, not a recomputation of physics, so it is hard-coded here exactly
/// like [`crate::sphere::EARTH_RADIUS_M`].
pub const MU: f64 = 0.012_150_585_609_624_04;

pub struct Sample {
    pub t: f64,
    pub vessel: [f64; 3],
    /// The vessel velocity. The line does not need it -- whoever continues
    /// this trajectory does (`engine::live`).
    pub velocity: [f64; 3],
    pub earth: [f64; 3],
    pub moon: [f64; 3],
    /// Normal of the instantaneous Earth-Moon orbital plane, `d x d_dot`
    /// (`core/frame.h`, `z = h/|h|`), by a central difference over neighbouring
    /// samples. It depends neither on the camera nor on the ship vertex, so it
    /// is computed once at load rather than in the shader.
    pub z_axis: [f64; 3],
    /// `sx,sy,sz` from the fixture -- an oracle for the test; the engine does
    /// not use it.
    pub synodic_reference: [f64; 3],
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(v: [f64; 3]) -> [f64; 3] {
    let len = dot(v, v).sqrt();
    [v[0] / len, v[1] / len, v[2] / len]
}

/// The same computation the vertex shader (`trajectory.slang`) does on the GPU
/// every frame: `origin`, an orthonormal basis from `d = moon - earth`, the
/// projection of the ship onto it, in dimensionless CR3BP units (the scale `L`,
/// as in `core/frame.h`).
pub fn rotating_position(
    vessel: [f64; 3],
    earth: [f64; 3],
    moon: [f64; 3],
    z_axis: [f64; 3],
) -> [f64; 3] {
    let d = sub(moon, earth);
    let length = dot(d, d).sqrt();
    let x_axis = [d[0] / length, d[1] / length, d[2] / length];
    let y_axis = cross(z_axis, x_axis);

    let origin = [
        earth[0] + MU * d[0],
        earth[1] + MU * d[1],
        earth[2] + MU * d[2],
    ];
    let rel = sub(vessel, origin);

    [
        dot(rel, x_axis) / length,
        dot(rel, y_axis) / length,
        dot(rel, z_axis) / length,
    ]
}

/// Reads the fixture and derives `z_axis` by a central difference.
///
/// The end samples take a one-sided difference -- half is borrowed, not lost:
/// the first and last sample still need a normal, and a one-sided difference on
/// a dense grid (~2.7 h between samples against a 27-day lunar month) brings an
/// error far too small to see at this scale.
pub fn load() -> Vec<Sample> {
    let mut lines = CSV.lines();
    lines.next(); // header

    let rows: Vec<[f64; 16]> = lines
        .map(|line| {
            let mut values = [0.0; 16];
            for (slot, field) in values.iter_mut().zip(line.split(',')) {
                *slot = field.parse().expect("the fixture holds valid numbers");
            }
            values
        })
        .collect();

    let mut samples: Vec<Sample> = rows
        .iter()
        .map(|row| Sample {
            t: row[0],
            vessel: [row[1], row[2], row[3]],
            velocity: [row[4], row[5], row[6]],
            earth: [row[7], row[8], row[9]],
            moon: [row[10], row[11], row[12]],
            z_axis: [0.0, 0.0, 0.0],
            synodic_reference: [row[13], row[14], row[15]],
        })
        .collect();

    fill_axes(&mut samples);
    samples
}

/// Derives `z_axis` by a central difference over neighbouring samples.
///
/// Separate from [`load`], because the live prediction needs the same
/// (`engine::live`): the plane normal is a property of a series of samples, not
/// of where they came from.
pub fn fill_axes(samples: &mut [Sample]) {
    let d_of = |s: &Sample| -> [f64; 3] { sub(s.moon, s.earth) };

    let d: Vec<[f64; 3]> = samples.iter().map(d_of).collect();

    for i in 0..samples.len() {
        let prev = if i == 0 { i } else { i - 1 };
        let next = if i + 1 == samples.len() { i } else { i + 1 };
        let d_dot = sub(d[next], d[prev]);
        samples[i].z_axis = normalize(cross(d[i], d_dot));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fixture_is_not_empty() {
        let samples = load();
        assert!(samples.len() > 1000, "{} samples left", samples.len());
    }

    /// The main check of the algorithm, apart from the GPU: does
    /// [`rotating_position`] reproduce the same synodic frame that
    /// `core/frame.h` put into the fixture. The tolerance here is easy to
    /// tighten if a normal more precise than a central difference is ever
    /// needed.
    #[test]
    fn rotating_position_matches_the_c_oracle() {
        let samples = load();
        let mut max_error = 0.0f64;

        for s in &samples {
            let computed = rotating_position(s.vessel, s.earth, s.moon, s.z_axis);
            for (c, r) in computed.iter().zip(s.synodic_reference) {
                max_error = max_error.max((c - r).abs());
            }
        }

        // Measured: 3.48e-7, at sample 0 -- where the central difference
        // degenerates into a one-sided one (an end point of the series). Twice
        // the margin, not an order: a tight tolerance catches a regression in
        // the algorithm itself, not only "something broke completely".
        assert!(
            max_error < 7e-7,
            "worst divergence from the oracle: {max_error:e}"
        );
    }
}
