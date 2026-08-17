//! A sphere at real scale: a flight from the surface to orbit with no breaks
//! (ROADMAP F5).
//!
//! Three claims:
//!
//!   1. close to the surface the sphere fills the whole frame -- it is convex
//!      and large, otherwise the camera-relative arithmetic got lost
//!      somewhere;
//!   2. at orbit (1e7 m) the measured silhouette matches the exact formula
//!      `asin(R/(R+altitude))` -- without this the first proves nothing;
//!   3. between them the frame coverage does NOT grow -- a jump upward would
//!      be exactly the break the F5 criterion forbids.

use engine::flight_probe::{expected_coverage, sweep};
use engine::gpu::Gpu;
use engine::sphere;

const SIZE: u32 = 256;
const STEPS: u32 = 15;

fn samples() -> Option<Vec<engine::flight_probe::Sample>> {
    let Ok(gpu) = Gpu::new(wgpu::Instance::default(), None) else {
        eprintln!("SKIPPED: no wgpu adapter");
        return None;
    };

    // A low-resolution mesh: the test checks scale and depth, not tessellation
    // quality -- and a headless run is faster that way.
    let mesh = sphere::generate(sphere::EARTH_RADIUS_M, 32, 64);
    Some(sweep(&gpu, SIZE, &mesh, STEPS).expect("the sweep should have run"))
}

#[test]
fn the_sphere_fills_the_frame_ten_metres_up() {
    let Some(samples) = samples() else {
        return;
    };
    let first = &samples[0];
    assert!(
        first.coverage > 0.99,
        "at altitude {} m the sphere should have filled the whole frame, it \
         took {}",
        first.altitude,
        first.coverage
    );
}

#[test]
fn the_silhouette_matches_the_analytic_disc_at_orbit() {
    let Some(samples) = samples() else {
        return;
    };
    let last = samples.last().unwrap();

    let expected = expected_coverage(last.expected_half_angle, 1.0)
        .expect("at 1e7 m the disc should have fitted in the frame");

    assert!(
        (last.coverage - expected).abs() < 0.02,
        "measured coverage {:.4} against the analytic {:.4} at altitude \
         {:.0e} m",
        last.coverage,
        expected,
        last.altitude
    );
}

/// The step's strongest claim: this is the one that checks "with no breaks".
#[test]
fn coverage_never_grows_as_altitude_increases() {
    let Some(samples) = samples() else {
        return;
    };

    for pair in samples.windows(2) {
        let [a, b] = pair else { unreachable!() };
        assert!(
            b.coverage <= a.coverage + 1e-9,
            "coverage grew with altitude: {:.4} m -> {:.4} m gave {:.4} -> \
             {:.4}",
            a.altitude,
            b.altitude,
            a.coverage,
            b.coverage
        );
    }
}
