//! The hull is lit from where the planet beneath it is lit (stage V, step V5;
//! debt D16).
//!
//! The oracle is the **side**, not the brightness: the frame's centre of light
//! is computed separately for the ship and for the surface, and both must shift
//! to the same side of their geometric centres. The number here is not "how much
//! luminance" but where it went; brightness depends on the material, the side on
//! the light source.
//!
//! That is what catches what debt D16 was actually dangerous for: while the
//! direction was an engine constant, the hull and the sky could glow from
//! different sides, and no check asked about it.

use engine::camera::Camera;
use engine::gpu::Gpu;
use engine::scene::{Body, Scene, Ship, TileSet};
use engine::shot::Shot;
use engine::{frame, ship, shot, sphere};

const SIZE: u32 = 256;

/// The altitude at which the ship is seen against the surface, in metres.
const ALTITUDE_M: f64 = 400_000.0;

/// How many metres from the camera to the ship.
const RANGE_M: f64 = 15.0;

/// The scene: the ship in front of the camera, the planet beneath it, the light
/// source wherever it is told to be.
///
/// There is no air, deliberately. The sky would cover both hull and surface with
/// its own scattering, and the test would be measuring aerial perspective
/// instead of the diffuse term.
fn scene_lit_from(sun: [f64; 3]) -> Scene {
    scene_of(sun, true, true)
}

/// The same scene, but with a choice of what is in it.
///
/// Needed for the **masks**: the ship's silhouette is taken from a frame with no
/// planet in it at all, and vice versa. Classifying a pixel by colour is no
/// longer possible -- see [`lit_offset`].
fn scene_of(sun: [f64; 3], with_body: bool, with_ship: bool) -> Scene {
    let radius = sphere::EARTH_RADIUS_M + ALTITUDE_M;
    // The camera looks down at an angle: the frame then holds both ship and
    // surface.
    let centre = [radius, 0.0, 0.0];
    let eye = [radius + 0.6 * RANGE_M, -0.8 * RANGE_M, 0.0];
    let camera = Camera::look_at(eye, centre, [1.0, 0.0, 0.0]);

    let mut scene = Scene::new(camera);
    scene.sun = sun;
    if with_body {
        scene.bodies.push(Body {
            centre: [0.0, 0.0, 0.0],
            radius_m: sphere::EARTH_RADIUS_M,
            orientation: [1.0, 0.0, 0.0, 0.0],
            tiles: TileSet::Smooth,
            colour: frame::COLOUR,
            air: None,
        });
    }
    if !with_ship {
        return scene;
    }
    scene.ships.push(Ship {
        centre,
        orientation: [1.0, 0.0, 0.0, 0.0],
        height_m: ship::DEFAULT_HEIGHT_M,
        extent_m: 0.5 * ship::DEFAULT_HEIGHT_M,
        colour: [0.72, 0.74, 0.78, 1.0],
        // The material is the same as in the engine's other fixtures. The test
        // asks about the side, not the brightness, so a specular highlight does
        // not spoil it: at `dot(n, l) <= 0` the BRDF gives zero whatever the
        // material, and the dark side stays dark.
        roughness: ship::HULL_ROUGHNESS,
        metallic: ship::HULL_METALLIC,
    });
    scene
}

fn luminance(p: [u8; 4]) -> f64 {
    0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])
}

/// The silhouette: which pixels of the frame are not sky.
///
/// WARNING: **the mask is geometric, not colour-based, and that is a fix rather
/// than a complication.** The first edition separated ship from planet by the
/// ratio of channels -- the hull has `r ~= b`, the planet `r = 0.22*b`. That
/// worked exactly as long as the lighting had an ambient of 0.05 and no pixel
/// was black. With zero ambient (T5c, PROJECT.md section 7) the unlit half of
/// the hull became exactly `[0, 0, 0]`, and `0 > 0` is false -- that is, **the
/// ship's shadow was counted as the planet**, and the surface's centre of light
/// moved by eighty pixels. A mask from a separate frame has no such dependency
/// at all.
fn silhouette(shot: &Shot) -> Vec<bool> {
    let mut out = vec![false; (shot.width * shot.height) as usize];
    for y in 0..shot.height {
        for x in 0..shot.width {
            let p = shot.pixel(x, y);
            out[(y * shot.width + x) as usize] = [p[0], p[1], p[2]] != frame::CLEAR_BYTES;
        }
    }
    out
}

/// Where the centre of light shifted to from the geometric centre of the marked
/// pixels, in screen pixels.
///
/// The difference of the two centres specifically, not the centre of light
/// itself: the ship's silhouette is asymmetric, and its centroid is already
/// offset without any lighting at all.
fn lit_offset(shot: &Shot, mask: &[bool]) -> [f64; 2] {
    let mut area = (0.0, 0.0, 0.0);
    let mut light = (0.0, 0.0, 0.0);
    for y in 0..shot.height {
        for x in 0..shot.width {
            let p = shot.pixel(x, y);
            if !mask[(y * shot.width + x) as usize] {
                continue;
            }
            let (fx, fy) = (f64::from(x), f64::from(y));
            area = (area.0 + 1.0, area.1 + fx, area.2 + fy);
            let l = luminance(p);
            light = (light.0 + l, light.1 + l * fx, light.2 + l * fy);
        }
    }
    assert!(area.0 > 100.0, "only {} pixels are marked", area.0);
    assert!(light.0 > 0.0, "not a single marked pixel is lit");
    [
        light.1 / light.0 - area.1 / area.0,
        light.2 / light.0 - area.2 / area.0,
    ]
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

/// Hull and surface glow from the same side, and both obey the light source.
///
/// Two light sources, opposite each other across the frame. For each of them the
/// hull's side must agree with the surface's side (a positive dot product), and
/// between the sources both sides must **flip**. One of these conditions alone
/// would be too little: agreement without flipping would hold even when the
/// light reached nowhere, and flipping without agreement when hull and planet
/// read different directions -- that is, exactly under debt D16.
#[test]
fn the_hull_and_the_surface_are_lit_from_the_same_side() {
    let Some(gpu) = Gpu::for_tests() else {
        return;
    };

    let mut hull = Vec::new();
    let mut surface = Vec::new();
    for sign in [1.0, -1.0] {
        // Across the view and slightly towards it: purely lateral light would
        // leave half the frame entirely black, and there would be nothing to
        // compute a centre of light from.
        //
        // WARNING: what flips is `z`, and that is the component which in this
        // frame lies **horizontally** (the camera looks along world `x`, and that
        // one goes into the screen's vertical). An attempt to make "the lateral
        // component the main one" by taking `[+-0.8, 0, 0.6]` flips the vertical
        // component and leaves the horizontal shift constant -- and the flip
        // check fails even though the physics is right.
        let sun = [0.4, 0.0, sign * 0.92];
        let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene_lit_from(sun)).expect("a frame");

        // The masks come from frames holding one thing each. The ship is nearer
        // than the planet, so its silhouette covers it: it is subtracted from the
        // surface mask.
        let only_ship = shot::take_scene(&gpu, SIZE, SIZE, &scene_of(sun, false, true))
            .expect("a frame of the ship alone");
        let only_body = shot::take_scene(&gpu, SIZE, SIZE, &scene_of(sun, true, false))
            .expect("a frame of the planet alone");
        let hull_mask = silhouette(&only_ship);
        let body_mask: Vec<bool> = silhouette(&only_body)
            .iter()
            .zip(&hull_mask)
            .map(|(body, ship)| *body && !*ship)
            .collect();

        hull.push(lit_offset(&shot, &hull_mask));
        surface.push(lit_offset(&shot, &body_mask));
    }

    for k in 0..2 {
        assert!(
            dot(hull[k], surface[k]) > 0.0,
            "light source {k}: the hull shifted to {:?}, the surface to {:?}",
            hull[k],
            surface[k]
        );
    }
    assert!(
        dot(hull[0], hull[1]) < 0.0,
        "the hull did not notice the light source moving to the other side: {hull:?}"
    );
    assert!(
        dot(surface[0], surface[1]) < 0.0,
        "the surface did not notice the light source moving to the other side: {surface:?}"
    );
}
