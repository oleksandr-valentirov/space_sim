//! Camera-relative does not depend on the distance to the origin (ROADMAP
//! F4).
//!
//! Three claims, each failing on its own:
//!
//!   1. at 1 AU the correct path moves the object smoothly;
//!   2. the naive path loses the object there entirely -- without this the
//!      first would be a claim with nothing to compare against;
//!   3. the correct path gives **the same** motion at 1e3 and at 1e11 m. That
//!      is the strongest formulation: not "the error is small" but "distance
//!      is not in the equation".

use engine::camera::Camera;
use engine::camera_probe::{sweep_at, ASTRONOMICAL_UNIT};
use engine::gpu::Gpu;

const SIZE: u32 = 256;
const STEPS: u32 = 12;
/// A step visible in pixels. At a millimetre both paths would give zero and
/// the test would tell nothing apart -- 1 mm at 10 m is 0.004 pixels.
const STEP_M: f64 = 0.1;

fn shifts(relative: bool, distance: f64) -> Option<Vec<f64>> {
    let gpu = Gpu::for_tests()?;

    let steps = sweep_at(&gpu, SIZE, relative, STEPS, STEP_M, distance)
        .expect("the measurement should have run");

    if steps.iter().any(|s| s.visible == 0) {
        return Some(Vec::new());
    }

    Some(steps.iter().skip(1).map(|s| s.shift).collect())
}

#[test]
fn camera_relative_moves_smoothly_at_one_astronomical_unit() {
    let Some(shifts) = shifts(true, ASTRONOMICAL_UNIT) else {
        return;
    };
    assert!(
        !shifts.is_empty(),
        "the object should have been visible in every frame"
    );

    let mean = shifts.iter().sum::<f64>() / shifts.len() as f64;
    assert!(
        mean > 1.0,
        "the camera moved and the image did not: {mean:.3} px"
    );

    for shift in &shifts {
        assert!(
            (shift - mean).abs() < mean * 0.5,
            "the motion is uneven: a step of {shift:.3} px against a mean of {mean:.3}"
        );
    }
}

#[test]
fn the_naive_path_loses_the_object_entirely_there() {
    let Some(shifts) = shifts(false, ASTRONOMICAL_UNIT) else {
        return;
    };
    assert!(
        shifts.is_empty(),
        "the naive path suddenly coped at 1 AU -- then the comparison in F4 \
         proves nothing, and the cause has to be found"
    );
}

/// The step's strongest claim.
#[test]
fn camera_relative_behaves_the_same_eight_orders_apart() {
    let Some(near) = shifts(true, 1e3) else {
        return;
    };
    let Some(far) = shifts(true, ASTRONOMICAL_UNIT) else {
        return;
    };

    assert!(!near.is_empty() && !far.is_empty());
    assert_eq!(near.len(), far.len());

    for (a, b) in near.iter().zip(far.iter()) {
        assert!(
            (a - b).abs() < 1e-9,
            "the motion at 1e3 m ({a:.6}) and at 1 AU ({b:.6}) diverged -- the \
             distance to the origin must not enter the result"
        );
    }
}

/// Projection to the screen: the screen centre, the sides, and what is behind
/// (U4b).
///
/// Three claims, and the third matters most: a point behind the camera **has
/// no** screen coordinate. Without it picking would catch nodes on the far
/// side of the planet -- there the formula gives an entirely plausible
/// pixel.
#[test]
fn projecting_to_the_screen_puts_things_where_they_are_seen() {
    let camera = Camera::look_at([0.0, 0.0, 1000.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let fov = std::f64::consts::PI / 3.0;
    let (w, h) = (800, 600);

    // The point the camera looks at is the centre of the frame.
    let centre = camera
        .to_screen(fov, w, h, [0.0, 0.0, 0.0])
        .expect("in front");
    assert!((centre[0] - 400.0).abs() < 0.5, "x = {}", centre[0]);
    assert!((centre[1] - 300.0).abs() < 0.5, "y = {}", centre[1]);

    // A shift up in the world is a smaller `y` on screen: the screen axis
    // points down.
    let higher = camera
        .to_screen(fov, w, h, [0.0, 100.0, 0.0])
        .expect("in front");
    assert!(
        higher[1] < centre[1],
        "a point higher up should have given a smaller y: {} against {}",
        higher[1],
        centre[1]
    );

    // And what is behind has no screen coordinate at all.
    assert!(camera.to_screen(fov, w, h, [0.0, 0.0, 2000.0]).is_none());
}
