//! A stitched planet has no holes on screen either (ROADMAP-PLANETS.md, R2c).
//!
//! The topology (`tests/lod.rs`) has already proved that no mesh edge is left
//! without a pair. That is not enough, and the reason is named in the step's
//! plan: a patch can be stitched flawlessly and still fail if the level was
//! chosen one way for the geometry and another for the index set. Then there
//! is no hole in the mesh, but there is one in the frame.
//!
//! So here is the second half of the check, and in exactly the order rule 7 of
//! stage R demands: a number on the CPU first, a shot second. The shot here is
//! a **detector**, not an oracle: if it finds a sky pixel, the cause will be in
//! R2a or R2b, and that is where to look for it.
//!
//! ## How sharp this detector is -- measured, not assumed
//!
//! Two mutations were run by hand, and **both left the shots green**:
//!
//! 1. **stitching switched off entirely** (`cubesphere::indices` ignores the
//!    mask). The topological test goes red at once, the shot does not, and the
//!    reason is a number: the widest gap of such a joint is **0.060 pixels**
//!    (the test below). Nor can it be wider: that is exactly the quantity the
//!    R2a criterion keeps under a one-pixel tolerance, otherwise it would
//!    divide the patch further;
//!
//! 2. **a visible patch not drawn at all** -- while there was no R3. Through
//!    the hole what showed was not sky but **the far side of the same planet**:
//!    back-face culling is off deliberately (`cull_mode: None`), and the far
//!    hemisphere was drawn. The pixels changed shade but never took on the
//!    colour of the sky.
//!
//!    With horizon culling (R3a) what lies past the limb is no longer drawn,
//!    and the same mutation gives **23310 sky pixels** out of 65536. Run again
//!    right there, as had been written down in advance.
//!
//! So the detector is sharp on exactly one thing: **a visible patch missing
//! from the frame**. A crack between levels it does not see and will not see --
//! that one is a sixteenth of a pixel. The step's oracle remains the equality
//! of vertices (rule 5).
//!
//! ## Why eight, and why from the corners
//!
//! A cube corner is the only place on the cubesphere where **three** patches
//! meet instead of four, and the only one where neighbourhood crosses two cube
//! edges at once. Eight corners are exactly as many as there are; fewer would
//! mean relying on the faces being symmetric, and in [`engine::cubesphere`]
//! only the fixed axis carries a sign -- i.e. three faces out of six are
//! mirrored.
//!
//! ## Why "zero sky pixels" means the whole frame
//!
//! From 100 km the Earth's disc has an angular radius of
//! `asin(R/(R+h)) = 79.9 deg`, while the half-diagonal of a square frame at a
//! 60 deg field of view is about 40 deg. The disc covers the frame entirely,
//! so "inside the silhouette" and "in the frame" are the same thing, and the
//! silhouette's boundary need be neither found nor approximated.

use engine::gpu::Gpu;
use engine::scene::{Body, Scene, TileSet};
use engine::shot::{self, Shot};
use engine::{camera::Camera, frame, sphere};

const SIZE: u32 = 256;
const ALTITUDE_M: f64 = 1.0e5;

fn gpu() -> Option<Gpu> {
    Gpu::for_tests()
}

/// How many pixels of the frame stayed the clear colour.
///
/// The equality is exact, with no tolerance: the sky colour is exactly the
/// bytes `LoadOp::Clear` wrote, while the darkest pixel of the planet (the
/// night side, `shade = 0.05`) gives `[3, 8, 11]` against the sky's
/// `[5, 8, 20]`. A tolerance here would only blur a boundary that is already
/// exact.
fn sky_pixels(shot: &Shot) -> usize {
    let mut sky = 0;
    for y in 0..shot.height {
        for x in 0..shot.width {
            let p = shot.pixel(x, y);
            if [p[0], p[1], p[2]] == frame::CLEAR_BYTES {
                sky += 1;
            }
        }
    }
    sky
}

/// The scene: one body of Earth's radius, the camera over a given direction.
fn looking_down(direction: [f64; 3], altitude: f64) -> Scene {
    let length =
        (direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2])
            .sqrt();
    let distance = sphere::EARTH_RADIUS_M + altitude;
    let eye = [
        direction[0] / length * distance,
        direction[1] / length * distance,
        direction[2] / length * distance,
    ];
    // The frame's vertical can be anything as long as it is not along the view.
    // A cube-corner direction is never parallel to the x axis, so this one
    // serves all eight.
    let camera = Camera::look_at(eye, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);

    let mut scene = Scene::new(camera);
    scene.bodies.push(Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: sphere::EARTH_RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: TileSet::Smooth,
        colour: frame::COLOUR,
        air: None,
    });
    scene
}

/// Eight cube corners, and through none of them is sky visible through the
/// planet.
#[test]
fn no_sky_shows_through_the_planet_from_any_cube_corner() {
    let Some(gpu) = gpu() else { return };

    let out = std::path::Path::new("build/r2c");
    for &x in &[-1.0, 1.0] {
        for &y in &[-1.0, 1.0] {
            for &z in &[-1.0, 1.0] {
                let scene = looking_down([x, y, z], ALTITUDE_M);
                let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene)
                    .expect("the frame should have drawn");
                let sky = sky_pixels(&shot);

                let name = format!(
                    "corner_{}{}{}.png",
                    if x > 0.0 { 'p' } else { 'm' },
                    if y > 0.0 { 'p' } else { 'm' },
                    if z > 0.0 { 'p' } else { 'm' }
                );
                // The shot goes to disk regardless of the result: when it
                // eventually turns red, there will be something to look at.
                let _ = shot.write_png(&out.join(&name));

                println!("  {name}: {sky} sky pixels");
                assert_eq!(
                    sky, 0,
                    "{name}: sky shows through the planet in {sky} pixels -- \
                     that is a crack, and its cause is in R2a or R2b"
                );
            }
        }
    }
}

/// A control: the sky detector does see the sky.
///
/// Without it the previous test would be green both on a frame painted
/// uniformly with anything and on a broken colour comparison. Here the camera
/// pulls back to an altitude from which the disc deliberately does not cover
/// the frame, and sky has to appear -- roughly as much of the frame as
/// `asin(R/(R+h))` leaves.
#[test]
fn the_sky_detector_does_see_the_sky() {
    let Some(gpu) = gpu() else { return };

    let scene = looking_down([1.0, 1.0, 1.0], frame::DEFAULT_ALTITUDE_M);
    let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene).expect("the frame should have drawn");
    let sky = sky_pixels(&shot);
    let all = (SIZE * SIZE) as usize;

    println!(
        "  from {:.1e} m: {sky} sky pixels out of {all} ({:.3} of the frame)",
        frame::DEFAULT_ALTITUDE_M,
        sky as f64 / all as f64
    );
    assert!(
        sky > all / 4 && sky < all,
        "from {:.1e} m the sky took {sky} pixels out of {all} -- the detector \
         measures the wrong thing",
        frame::DEFAULT_ALTITUDE_M
    );
}

/// **Exactly how many pixels an unstitched joint costs -- and why the shot
/// says nothing about it.**
///
/// The number the first of the two mutations in the module header rests on.
///
/// An unstitched edge leaves a T-joint: the odd node of the finer patch sticks
/// out of the coarser one's chord by exactly the sagitta of its own cell. But
/// that is precisely the quantity the R2a criterion keeps under a one-pixel
/// tolerance -- otherwise it would divide the patch further. So the gap cannot
/// be wider than a pixel by construction, and a gap of a fraction of a pixel
/// covers no fragment centre: no sky shows through it, crack or no crack.
///
/// That is why the step's oracle remains the equality of vertices (rule 5),
/// while the shot catches the crude cases -- the wrong face, the wrong index
/// range, a lost patch. Here is the number that explains why, and a guard in
/// case the criterion ever loosens the tolerance.
#[test]
fn an_unstitched_joint_would_be_thinner_than_a_pixel() {
    use engine::cubesphere::{Edge, EDGES, SIDE};
    use engine::lod::{self, Body as LodBody};

    let focal = lod::focal_px(frame::FOV_Y, f64::from(SIZE));
    let scene = looking_down([1.0, 1.0, 1.0], ALTITUDE_M);
    let eye = scene.camera.position();
    let radius = sphere::EARTH_RADIUS_M;
    let selection = lod::select(
        &LodBody::still([0.0, 0.0, 0.0], radius),
        &scene.camera,
        focal,
        None,
    );

    let node = |patch: &engine::cubesphere::Patch, edge: Edge, k: usize| match edge {
        Edge::AMin => patch.vertex(0, k, radius),
        Edge::AMax => patch.vertex(SIDE, k, radius),
        Edge::BMin => patch.vertex(k, 0, radius),
        Edge::BMax => patch.vertex(k, SIDE, radius),
    };

    let mut worst_px: f64 = 0.0;
    let mut joints = 0;
    for (patch, &mask) in selection.patches.iter().zip(&selection.masks) {
        for edge in EDGES {
            if mask & edge.bit() == 0 {
                continue;
            }
            for k in (1..SIDE).step_by(2) {
                let here = node(patch, edge, k);
                let before = node(patch, edge, k - 1);
                let after = node(patch, edge, k + 1);
                // The coarser neighbour's chord runs through the even nodes;
                // the odd one's overhang is measured from it.
                let gap = (0..3)
                    .map(|c| (here[c] - (before[c] + after[c]) / 2.0).powi(2))
                    .sum::<f64>()
                    .sqrt();
                let range = (0..3)
                    .map(|c| (here[c] - eye[c]).powi(2))
                    .sum::<f64>()
                    .sqrt();
                worst_px = worst_px.max(gap / range * focal);
                joints += 1;
            }
        }
    }

    println!(
        "  {} patches, {joints} joints; widest gap without stitching \
         {worst_px:.3} pixels at a tolerance of {:.1}",
        selection.patches.len(),
        lod::TOLERANCE_PX
    );

    assert!(joints > 0, "the set has no level joint at all");
    assert!(
        worst_px > 0.0,
        "the gap is zero even before stitching -- the joint is measured in the wrong place"
    );
    assert!(
        worst_px <= lod::TOLERANCE_PX,
        "a gap of {worst_px:.3} pixels is wider than the tolerance of {:.1} -- \
         the shot ought to see it, and it does not",
        lod::TOLERANCE_PX
    );
}
