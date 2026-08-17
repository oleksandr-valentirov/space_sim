//! The ship in the frame: it is there, it is the right size and it turns
//! (stage V, step V2).
//!
//! The oracle is not "something got drawn" but **a number against a number**,
//! as in F5: in the projection the height of an object in pixels is expressed
//! exactly, without approximation. The nose and the tail lie at equal distance
//! from the camera by construction of the scene, so `y_view / (-z_view)` for
//! them is exactly `+-(h/2)/d`.

use engine::camera::Camera;
use engine::gpu::Gpu;
use engine::scene::{Scene, Ship};
use engine::shot::Shot;
use engine::{frame, ship, shot};

const SIZE: u32 = 256;
const FOV_Y: f64 = std::f64::consts::PI / 3.0;
const DISTANCE: f64 = 15.0;

fn gpu() -> Option<Gpu> {
    Gpu::for_tests()
}

/// The scene: an empty sky and one ship [`DISTANCE`] metres in front of the
/// camera.
///
/// Empty deliberately -- no body, no polyline. What is visible in the frame can
/// only be the ship, and no other drawer can accidentally give the same
/// pixels.
fn scene_with(orientation: [f64; 4]) -> Scene {
    let eye = [DISTANCE, 0.0, 0.0];
    // Up is world `+Z`, i.e. the ship's axis lies vertically in the frame.
    let camera = Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);

    let mut scene = Scene::new(camera);
    scene.ships.push(Ship {
        centre: [0.0, 0.0, 0.0],
        orientation,
        height_m: ship::DEFAULT_HEIGHT_M,
        extent_m: 0.5 * ship::DEFAULT_HEIGHT_M,
        colour: [0.72, 0.74, 0.78, 1.0],
        roughness: engine::ship::HULL_ROUGHNESS,
        metallic: engine::ship::HULL_METALLIC,
    });
    scene
}

/// The rectangle all non-empty pixels are inscribed in: `(x0, y0, x1, y1)`.
fn lit_bounds(shot: &Shot) -> Option<(u32, u32, u32, u32)> {
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in 0..shot.height {
        for x in 0..shot.width {
            let p = shot.pixel(x, y);
            if [p[0], p[1], p[2]] == frame::CLEAR_BYTES {
                continue;
            }
            bounds = Some(match bounds {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        }
    }
    bounds
}

/// The same silhouette computed on the CPU: every vertex of the mesh goes
/// through [`Camera::to_screen`] -- the same function picking uses to catch
/// manoeuvre nodes (U4b).
///
/// This is not "an estimate by angle". An angular estimate here would be
/// **wrong**, and that is a measurement, not a theory: a fin turned towards the
/// camera sticks out 2.28 m forward, i.e. projects larger than the nose, which
/// stands further away. The nose gives 88.7 pixels while the frame gives 96,
/// and the extra seven and a half come from exactly there.
///
/// So the oracle here is the same as in `cull` against `cull.slang`: two
/// independent implementations of one transform must give one number.
fn projected_bounds(camera: &Camera, height_m: f64) -> (f64, f64, f64, f64) {
    let mesh = ship::generate(height_m);
    let mut bounds = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in &mesh.positions {
        let screen = camera
            .to_screen(FOV_Y, SIZE, SIZE, *p)
            .expect("a vertex behind the camera -- wrong scene");
        bounds.0 = bounds.0.min(f64::from(screen[0]));
        bounds.1 = bounds.1.min(f64::from(screen[1]));
        bounds.2 = bounds.2.max(f64::from(screen[0]));
        bounds.3 = bounds.3.max(f64::from(screen[1]));
    }
    bounds
}

#[test]
fn the_ship_fills_exactly_the_pixels_the_projection_says() {
    let Some(gpu) = gpu() else {
        return;
    };

    let scene = scene_with([1.0, 0.0, 0.0, 0.0]);
    let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene).expect("a frame with a ship");
    let (x0, y0, x1, y1) = lit_bounds(&shot).expect("the frame is empty -- there is no ship");
    let expected = projected_bounds(&scene.camera, ship::DEFAULT_HEIGHT_M);

    // The tolerance is asymmetric, and that is not an indulgence but what the
    // rasteriser actually does. A pixel is painted when its **centre** is
    // covered, so near a point -- the nose cone and the tip of a fin -- the last
    // pixel or two do not get filled. Outwards, past the outermost vertex, the
    // silhouette cannot go at all: there is no geometry there.
    //
    // So the statement is stronger than "roughly agrees": nowhere larger than
    // the projection, and nowhere smaller by more than two and a half pixels.
    let inside = |what: &str, drawn: f64, want: f64, sign: f64| {
        let over = sign * (drawn - want);
        assert!(
            over <= 1.0,
            "{what}: the frame overshot the projection by {over} px ({drawn} against {want})"
        );
        assert!(
            over >= -2.5,
            "{what}: the frame fell short of the projection by {} px ({drawn} against {want})",
            -over
        );
    };
    inside("left", f64::from(x0), expected.0, -1.0);
    inside("top", f64::from(y0), expected.1, -1.0);
    inside("right", f64::from(x1), expected.2, 1.0);
    inside("bottom", f64::from(y1), expected.3, 1.0);
}

#[test]
fn turning_the_ship_turns_it_in_the_frame() {
    let Some(gpu) = gpu() else {
        return;
    };

    let upright = scene_with([1.0, 0.0, 0.0, 0.0]);
    // A quarter turn about world `+X`, i.e. about the view axis: the ship's
    // axis lies horizontally.
    let half = std::f64::consts::FRAC_PI_4;
    let sideways = scene_with([half.cos(), half.sin(), 0.0, 0.0]);

    let a = shot::take_scene(&gpu, SIZE, SIZE, &upright).expect("a frame");
    let b = shot::take_scene(&gpu, SIZE, SIZE, &sideways).expect("a frame");

    let (ax0, ay0, ax1, ay1) = lit_bounds(&a).expect("there is no ship");
    let (bx0, by0, bx1, by1) = lit_bounds(&b).expect("there is no ship");

    let tall = f64::from(ay1 - ay0 + 1) / f64::from(ax1 - ax0 + 1);
    let wide = f64::from(by1 - by0 + 1) / f64::from(bx1 - bx0 + 1);

    // An upright ship is taller than it is wide; a laid-down one the other way
    // round. One comparison against unity would be too little: the oracle must
    // also fail when the rotation did not reach the GPU at all, i.e. when both
    // numbers are the same.
    assert!(tall > 1.2, "an upright ship should be tall: {tall}");
    assert!(wide < 0.8, "a laid-down ship should be wide: {wide}");
}

/// A scene without ships is the frame from before step V2, and not "almost".
///
/// The cheapest guard against the new pipeline always drawing something: an
/// empty ship list must give not a single pixel, and `--shot` of the engine's
/// probes must stay `30812bf2...`.
#[test]
fn a_scene_without_ships_draws_nothing_new() {
    let Some(gpu) = gpu() else {
        return;
    };

    let eye = [DISTANCE, 0.0, 0.0];
    let camera = Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    let scene = Scene::new(camera);

    let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene).expect("a frame");
    assert!(
        lit_bounds(&shot).is_none(),
        "an empty scene drew something that is not in it"
    );
}
