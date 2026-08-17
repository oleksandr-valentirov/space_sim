//! The third-person camera really does show the ship turning (stage V, step
//! V4).
//!
//! The oracle is the **share of changed pixels**, not "it looks different". The
//! camera stands still while it does: only the ship moves, so everything that
//! changed in the frame was changed by the rotation itself.
//!
//! That is what checks the main decision of `engine::chase`: the camera takes
//! position from the ship, not orientation. Tied to the ship's axes, it would
//! give zero here on all three rows at once.

use engine::chase::Chase;
use engine::gpu::Gpu;
use engine::scene::{Scene, Ship};
use engine::shot::Shot;
use engine::{frame, ship, shot};

const SIZE: u32 = 256;

/// The ship stands far from the origin -- where `f32` no longer holds anything
/// and camera-relative does (F4).
const CENTRE: [f64; 3] = [4.1e6, -2.7e6, 3.3e6];

/// The "up" reference is oblique, so that no axis of the ship coincides with an
/// axis of the screen: a symmetric fixture has already hidden two bugs in a row
/// (D13, D14).
fn up() -> [f64; 3] {
    let v = [0.37, -0.51, 0.77_f64];
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / n, v[1] / n, v[2] / n]
}

fn scene_with(orientation: [f64; 4]) -> Scene {
    let ship = Ship {
        centre: CENTRE,
        orientation,
        height_m: ship::DEFAULT_HEIGHT_M,
        extent_m: 0.5 * ship::DEFAULT_HEIGHT_M,
        colour: [0.72, 0.74, 0.78, 1.0],
        roughness: engine::ship::HULL_ROUGHNESS,
        metallic: engine::ship::HULL_METALLIC,
    };
    // An empty sky deliberately: no body, no polyline. Whatever changed in the
    // frame could only have been changed by the ship turning.
    let mut scene = Scene::new(Chase::default().camera(&ship, up()));
    scene.ships.push(ship);
    scene
}

/// The quaternion of a rotation by `angle` about the axis `axis`.
fn turn(axis: [f64; 3], angle: f64) -> [f64; 4] {
    let half = 0.5 * angle;
    let (s, c) = half.sin_cos();
    [c, s * axis[0], s * axis[1], s * axis[2]]
}

fn drawn(shot: &Shot, x: u32, y: u32) -> bool {
    let p = shot.pixel(x, y);
    [p[0], p[1], p[2]] != frame::CLEAR_BYTES
}

/// How many silhouette pixels changed, as a share of the silhouette itself.
///
/// The denominator is the union of the two silhouettes, not the whole frame:
/// the ship takes a few percent of the frame, and a share of the frame would
/// speak about the field of view rather than about the rotation.
fn silhouette_change(a: &Shot, b: &Shot) -> f64 {
    let mut union = 0usize;
    let mut differing = 0usize;
    for y in 0..a.height {
        for x in 0..a.width {
            let (left, right) = (drawn(a, x, y), drawn(b, x, y));
            if left || right {
                union += 1;
            }
            if left != right {
                differing += 1;
            }
        }
    }
    assert!(union > 0, "there is no ship in the frame at all");
    differing as f64 / union as f64
}

/// The rectangle the silhouette is inscribed in.
fn bounds(shot: &Shot) -> (u32, u32, u32, u32) {
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in 0..shot.height {
        for x in 0..shot.width {
            if !drawn(shot, x, y) {
                continue;
            }
            bounds = Some(match bounds {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        }
    }
    bounds.expect("there is no ship in the frame")
}

/// A rotation about each of the three axes is visible in the frame.
///
/// The angle is 40 deg, not 90, and that is not taste: **there are four fins**,
/// so a quarter turn about the nose maps the silhouette onto itself, and roll
/// would look motionless under any correct camera. Forty degrees coincide with
/// no symmetry of the mesh.
///
/// Measured, as shares of the silhouette: **0.578 about x, 0.305 about y and
/// 0.107 about z**. The third number is smaller not because of the camera but
/// because of the shape: the hull is a solid of revolution, so roll is visible
/// only through the fins and the porthole. That is exactly what V1 measured on
/// the mesh itself, when removing the porthole collapsed the roll mismatch to
/// 8e-16.
///
/// The threshold is 0.05, twice below the weakest of the three: it catches "the
/// camera turned along with the ship" (zero) and "the rotation did not reach
/// the GPU" (zero too), it does not measure exact numbers that have nothing to
/// rest on.
#[test]
fn every_axis_of_rotation_changes_the_silhouette() {
    let Some(gpu) = Gpu::for_tests() else {
        return;
    };

    let angle = 40.0_f64.to_radians();
    let upright = shot::take_scene(&gpu, SIZE, SIZE, &scene_with([1.0, 0.0, 0.0, 0.0]))
        .expect("a frame with a ship");

    for (name, axis) in [
        ("x", [1.0, 0.0, 0.0]),
        ("y", [0.0, 1.0, 0.0]),
        ("z", [0.0, 0.0, 1.0]),
    ] {
        let turned = shot::take_scene(&gpu, SIZE, SIZE, &scene_with(turn(axis, angle)))
            .expect("a frame with a ship");
        let change = silhouette_change(&upright, &turned);
        assert!(
            change > 0.05,
            "rotation about {name}: the silhouette changed by only {change}"
        );
    }
}

/// The camera keeps the ship in frame however it turns.
///
/// The other half of the same statement: a rotation changes the silhouette but
/// does not drag it out of the frame -- otherwise "everything changed" would
/// mean the ship had simply left over the edge. What is checked is the centre
/// of the bounding rectangle: it must stay in the centre of the frame to within
/// the size of the ship itself.
#[test]
fn the_ship_stays_in_the_middle_however_it_turns() {
    let Some(gpu) = Gpu::for_tests() else {
        return;
    };

    let angle = 40.0_f64.to_radians();
    for orientation in [
        [1.0, 0.0, 0.0, 0.0],
        turn([1.0, 0.0, 0.0], angle),
        turn([0.0, 1.0, 0.0], angle),
        turn([0.0, 0.0, 1.0], angle),
    ] {
        let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene_with(orientation))
            .expect("a frame with a ship");
        let (x0, y0, x1, y1) = bounds(&shot);
        let centre = [
            0.5 * f64::from(x0 + x1) - 0.5 * f64::from(SIZE),
            0.5 * f64::from(y0 + y1) - 0.5 * f64::from(SIZE),
        ];
        // The tolerance is half the height of the silhouette: the centre of the
        // bounding rectangle does not coincide with the centre of the ship (the
        // nose is longer than the tail), and demanding more would mean checking
        // the shape of the mesh rather than the camera.
        let tolerance = 0.5 * f64::from(y1 - y0 + 1);
        assert!(
            centre[0].abs() < tolerance && centre[1].abs() < tolerance,
            "the ship shifted by {centre:?} px against a tolerance of {tolerance}"
        );
    }
}
