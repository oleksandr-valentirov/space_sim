//! Colour is a property of a body, not of the frame (stage T, step T1).
//!
//! The oracle deliberately demands **two** bodies. With one, the very
//! implementation this step gets rid of would pass too: colour rides in a
//! uniform with a dynamic offset per pass, and "the last caller won" -- exactly
//! the bug that once made polyline colour a vertex attribute (J1). A single
//! body does not show it at all.

use engine::camera::Camera;
use engine::gpu::Gpu;
use engine::scene::{Body, Scene, TileSet};
use engine::shot::Shot;
use engine::{frame, shot, sphere};

const SIZE: u32 = 256;

/// Two planets on either side of the view axis, each with its own colour.
///
/// Earth radius, four radii apart, camera at twenty. The numbers are chosen so
/// that both discs fit into the frame whole and do not touch: discs that
/// overlapped would make "whose pixel is this" a question of depth rather than
/// of colour.
fn two_bodies(left: [f32; 4], right: [f32; 4]) -> Scene {
    let radius = sphere::EARTH_RADIUS_M;
    let body = |centre: [f64; 3], colour: [f32; 4]| Body {
        centre,
        radius_m: radius,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: TileSet::Smooth,
        colour,
        air: None,
    };

    let camera = Camera::look_at([20.0 * radius, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    let mut scene = Scene::new(camera);
    scene.bodies.push(body([0.0, 2.0 * radius, 0.0], left));
    scene.bodies.push(body([0.0, -2.0 * radius, 0.0], right));
    scene
}

/// The column in which the camera sees the centre of a body.
///
/// Asked of the camera itself rather than derived from the axes: world `+y` in
/// this frame lands **to the right** on screen, and the first attempt at this
/// test assumed the opposite. A guess about the direction of an axis is exactly
/// what an oracle must take from the code, not from one's head.
fn screen_x(scene: &Scene, index: usize) -> f64 {
    let centre = scene.bodies[index].centre;
    let screen = scene
        .camera
        .to_screen(frame::FOV_Y, SIZE, SIZE, centre)
        .expect("the body is behind the camera -- wrong scene");
    f64::from(screen[0])
}

/// The mean column of the pixels in which the first channel dominates the third
/// (or the other way round), and how many there are.
fn centroid(shot: &Shot, red: bool) -> (f64, usize) {
    let mut count = 0usize;
    let mut sum = 0.0;
    for y in 0..shot.height {
        for x in 0..shot.width {
            let p = shot.pixel(x, y);
            if [p[0], p[1], p[2]] == frame::CLEAR_BYTES {
                continue;
            }
            let dominant = if red { p[0] > p[2] } else { p[2] > p[0] };
            if dominant {
                count += 1;
                sum += f64::from(x);
            }
        }
    }
    (sum / count.max(1) as f64, count)
}

/// Two bodies of different colours give two colours in the frame, each on its
/// own side.
///
/// One of the two statements alone would be too little: "both colours are
/// there" would pass even when they had swapped places, and "the left one is on
/// the left" when both bodies are grey and the classification is catching
/// noise. Together they name both the colour and its owner.
#[test]
fn two_bodies_keep_their_own_colours() {
    let Some(gpu) = Gpu::for_tests() else {
        return;
    };

    let red = [0.9, 0.1, 0.1, 1.0];
    let blue = [0.1, 0.1, 0.9, 1.0];
    let scene = two_bodies(red, blue);
    let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene).expect("a frame with two bodies");

    let (red_x, red_n) = centroid(&shot, true);
    let (blue_x, blue_n) = centroid(&shot, false);

    // An Earth-radius disc from twenty radii -- about 850 pixels in a 256x256
    // frame. The threshold is an order of magnitude lower: it tells "the body
    // is there" from "a few noise pixels on the terminator", it does not
    // measure area.
    assert!(red_n > 100, "only {red_n} red pixels");
    assert!(blue_n > 100, "only {blue_n} blue pixels");
    // Where each body is, the camera knows. Red is the one first in the list.
    let (want_red, want_blue) = (screen_x(&scene, 0), screen_x(&scene, 1));
    assert!(
        (red_x - want_red).abs() < 20.0,
        "the red centre is in column {red_x}, but the body is at {want_red}"
    );
    assert!(
        (blue_x - want_blue).abs() < 20.0,
        "the blue centre is in column {blue_x}, but the body is at {want_blue}"
    );

    // And the other way round: swapping the colours must swap the frame too.
    // Without this the check above would pass on a frame where the colour is
    // not read from the body at all but taken from the draw order.
    let swapped = two_bodies(blue, red);
    let shot = shot::take_scene(&gpu, SIZE, SIZE, &swapped).expect("a frame with two bodies");
    let (red_x, _) = centroid(&shot, true);
    let (blue_x, _) = centroid(&shot, false);
    assert!(
        (red_x - want_blue).abs() < 20.0 && (blue_x - want_red).abs() < 20.0,
        "the colour did not follow the body: red at {red_x}, blue at {blue_x}"
    );
}
