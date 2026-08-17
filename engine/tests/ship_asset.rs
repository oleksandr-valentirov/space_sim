//! The frame draws a cooked mesh, not a stub (stage T, step T5d3).
//!
//! The oracle is the same as in V2, and that is exactly why it means something
//! here: the silhouette in the frame against the projection of **the model's own
//! vertices** through `Camera::to_screen` -- two independent implementations of
//! one transform. The only difference is where the vertices come from: from the
//! asset rather than from `ship::generate`.
//!
//! ## The model in the test is synthetic, and that is deliberate
//!
//! The real asset lives in `assets/`, which is not in git (`.gitignore`), so a
//! test that read it from disk would in CI be checking that the file is missing.
//! Here the model is built by code -- and built **asymmetric about all three
//! axes**, because a symmetric one hides swapped axes, rotation and scale alike
//! (D13 and D14 lived exactly that way).

use engine::camera::Camera;
use engine::gpu::Gpu;
use engine::mesh::Model;
use engine::scene::{Scene, Ship};
use engine::shot::{self, Shot};
use engine::sphere::Mesh;
use engine::{frame, ship};

const SIZE: u32 = 256;
const FOV_Y: f64 = std::f64::consts::PI / 3.0;
const DISTANCE: f64 = 15.0;

/// The model's length in metres -- not the one the ship stands at in the scene.
///
/// The numbers differ deliberately: the cooker normalises the mesh to unit
/// height, and the scene multiplies it by its own. If they agreed, a forgotten
/// division would look right.
const MODEL_M: f64 = 3.0;
const SHIP_M: f64 = 8.0;

fn gpu() -> Option<Gpu> {
    Gpu::for_tests()
}

/// A wedge: long along `+Z`, wider on the left, with one corner cut off.
///
/// A closed shell of six vertices; no plane of symmetry maps it onto itself, so
/// both rotation and a swap of axes are visible in the frame.
fn wedge(length_m: f64) -> Mesh {
    let h = 0.5 * length_m;
    let w = 0.28 * length_m;
    let positions = vec![
        [-w, -0.6 * w, -h],
        [1.7 * w, -0.4 * w, -h],
        [0.2 * w, 1.3 * w, -h],
        [-0.5 * w, -0.2 * w, h],
        [0.9 * w, -0.5 * w, h],
        [0.1 * w, 0.4 * w, h],
    ];
    // The normals are not an oracle here (the silhouette does not ask for them),
    // but lying with them is not allowed either: we take the direction from the
    // centre, as in V1.
    let normals = positions
        .iter()
        .map(|p: &[f64; 3]| {
            let n = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
            [(p[0] / n) as f32, (p[1] / n) as f32, (p[2] / n) as f32]
        })
        .collect();
    let indices = vec![
        0, 2, 1, // tail
        3, 4, 5, // nose
        0, 1, 4, 0, 4, 3, // side
        1, 2, 5, 1, 5, 4, // side
        2, 0, 3, 2, 3, 5, // side
    ];
    Mesh {
        positions,
        normals,
        indices,
    }
}

fn model() -> Model {
    Model::from_metres(wedge(MODEL_M), Vec::new()).expect("a wedge is a model")
}

fn scene_with(orientation: [f64; 4], extent: f64) -> Scene {
    let eye = [DISTANCE, 0.0, 0.0];
    let camera = Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    let mut scene = Scene::new(camera);
    scene.ships.push(Ship {
        centre: [0.0, 0.0, 0.0],
        orientation,
        height_m: SHIP_M,
        extent_m: extent * SHIP_M,
        colour: [0.72, 0.74, 0.78, 1.0],
        roughness: ship::HULL_ROUGHNESS,
        metallic: ship::HULL_METALLIC,
    });
    scene
}

/// A frame with a mesh of choice: `None` is V1's stub, `Some` the cooked
/// model.
fn take(gpu: &Gpu, scene: &Scene, model: Option<&Model>) -> Shot {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ship asset shot"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shot::FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut frame = frame::Frame::new(gpu, shot::FORMAT);
    if let Some(model) = model {
        frame.load_ship(gpu, model);
    }
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ship asset"),
        });
    frame.draw(gpu, &mut encoder, &view, SIZE, SIZE, scene);
    shot::read_back(gpu, encoder, &texture, SIZE, SIZE).expect("the frame should have come out")
}

/// The rectangle all non-empty pixels are inscribed in.
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

/// The same silhouette on the CPU: the model's vertices multiplied by the ship's
/// height.
fn projected_bounds(camera: &Camera, model: &Model) -> (f64, f64, f64, f64) {
    let mut bounds = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in &model.mesh.positions {
        let world = [p[0] * SHIP_M, p[1] * SHIP_M, p[2] * SHIP_M];
        let screen = camera
            .to_screen(FOV_Y, SIZE, SIZE, world)
            .expect("a vertex behind the camera -- wrong scene");
        bounds.0 = bounds.0.min(f64::from(screen[0]));
        bounds.1 = bounds.1.min(f64::from(screen[1]));
        bounds.2 = bounds.2.max(f64::from(screen[0]));
        bounds.3 = bounds.3.max(f64::from(screen[1]));
    }
    bounds
}

/// The asset's silhouette in the frame is the projection of its own vertices.
///
/// Catches everything the step exists for: vertices that were not read, a
/// forgotten division by the model's length (the model is 3 m, the ship 8 m --
/// the numbers differ deliberately), swapped axes, and a mesh that stayed a
/// stub.
#[test]
fn the_asset_fills_exactly_the_pixels_its_own_projection_says() {
    let Some(gpu) = gpu() else { return };
    let model = model();
    let scene = scene_with([1.0, 0.0, 0.0, 0.0], model.extent);
    let shot = take(&gpu, &scene, Some(&model));

    let (x0, y0, x1, y1) = lit_bounds(&shot).expect("the frame is empty -- there is no ship");
    let expected = projected_bounds(&scene.camera, &model);
    println!("  frame {x0},{y0} ... {x1},{y1}");
    println!("  projection {expected:?}");

    // The tolerance is the same as in V2, and for the same reason: a pixel is
    // painted by its centre, so near a point the last pixel does not get filled,
    // while outwards past the outermost vertex the silhouette cannot go at
    // all.
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

/// The asset really did change the frame rather than arriving and being ignored.
///
/// V1's stub and the wedge occupy different pixels, and the difference must be
/// large: "the frames are not bitwise identical" would pass on a change of one
/// pixel.
#[test]
fn loading_a_model_changes_what_is_drawn() {
    let Some(gpu) = gpu() else { return };
    let model = model();
    let scene = scene_with([1.0, 0.0, 0.0, 0.0], model.extent);

    let stub = take(&gpu, &scene, None);
    let asset = take(&gpu, &scene, Some(&model));

    let mut differ = 0;
    let mut drawn = 0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let a = stub.pixel(x, y);
            let b = asset.pixel(x, y);
            if [a[0], a[1], a[2]] != frame::CLEAR_BYTES || [b[0], b[1], b[2]] != frame::CLEAR_BYTES
            {
                drawn += 1;
                if a != b {
                    differ += 1;
                }
            }
        }
    }
    let share = f64::from(differ) / f64::from(drawn.max(1));
    println!("  differing pixels: {differ} of {drawn} ({share:.3})");
    assert!(
        share > 0.3,
        "the asset barely changed the frame: {share:.3}"
    );
}

/// The model turns together with the ship.
///
/// The wedge is asymmetric about every axis, so no rotation other than the
/// identity leaves the silhouette in place -- the same statement V1/V4 check for
/// the stub. On a sphere it would have to fail, and that is its point.
#[test]
fn turning_the_ship_turns_the_asset() {
    let Some(gpu) = gpu() else { return };
    let model = model();

    let upright = take(
        &gpu,
        &scene_with([1.0, 0.0, 0.0, 0.0], model.extent),
        Some(&model),
    );
    let half = std::f64::consts::FRAC_PI_4;
    let turned = take(
        &gpu,
        &scene_with([half.cos(), half.sin(), 0.0, 0.0], model.extent),
        Some(&model),
    );

    let a = lit_bounds(&upright).expect("a silhouette");
    let b = lit_bounds(&turned).expect("a silhouette");
    println!("  upright {a:?}, turned {b:?}");
    assert_ne!(
        a, b,
        "the rotation did not change the silhouette -- the orientation is not visible"
    );

    let mut moved = 0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            if upright.pixel(x, y) != turned.pixel(x, y) {
                moved += 1;
            }
        }
    }
    println!("  moved pixels: {moved}");
    assert!(moved > 500, "the rotation moved only {moved} pixels");
}
