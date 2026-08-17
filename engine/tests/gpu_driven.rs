//! The vertex stage lays the triangle list out exactly as the index buffer did
//! (ROADMAP-PLANETS.md, R6a).
//!
//! ## Why this test exists
//!
//! Before R6a the stitching of levels lived in `cubesphere::indices`: sixteen
//! index sets, one draw call per patch. After R6a the same substitution is done
//! by arithmetic in the vertex shader, and the frame is drawn with one call per
//! body.
//!
//! So one rule is now written down **twice** -- in Rust and in Slang. That is
//! exactly the situation in which two copies diverge at the fourth edit, and the
//! only thing that saves you from it is a guard that compares them directly.
//!
//! The shader's arithmetic is reproduced here **verbatim**, in the same
//! integers, and checked against `cubesphere::indices` for all sixteen masks.
//! This is not "the same thing written twice": the left-hand side is a
//! line-by-line translation of Slang, the right-hand side an independent
//! implementation via a node table. They can only agree if both are correct.
//!
//! A screenshot does not catch this, and that is measured: `--shot` after R6a
//! is bitwise the same as before it -- but the engine's probe scene has five
//! patches and **not a single stitched edge**. That is, bitwise equality of the
//! frame proves the triangle layout and says nothing about node substitution.

use engine::cubesphere::{self, SIDE};

/// A line-by-line translation of `node_of` from `shaders/patch.slang`.
///
/// Deliberately clumsy: `u32`, division with remainder, the same names. If one
/// day it feels tempting to write this "more nicely" -- that is exactly when it
/// stops being a comparison against the shader.
fn node_of(vertex: u32, mask: u32) -> u32 {
    const SIDE_U: u32 = SIDE as u32;
    const NODES: u32 = SIDE_U + 1;

    let triangle = vertex / 3;
    let corner = vertex % 3;
    let cell = triangle / 2;
    let half = triangle % 2;

    let mut a = cell / SIDE_U;
    let mut b = cell % SIDE_U;

    let first = [(0u32, 0u32), (1, 0), (0, 1)];
    let second = [(0u32, 1u32), (1, 0), (1, 1)];
    let step = if half == 0 {
        first[corner as usize]
    } else {
        second[corner as usize]
    };
    a += step.0;
    b += step.1;

    let odd_on_b = a % 2 == 1 && ((b == 0 && mask & 4 != 0) || (b == SIDE_U && mask & 8 != 0));
    let odd_on_a = b % 2 == 1 && ((a == 0 && mask & 1 != 0) || (a == SIDE_U && mask & 2 != 0));
    if odd_on_b {
        a -= 1;
    }
    if odd_on_a {
        b -= 1;
    }

    a * NODES + b
}

/// For all sixteen masks the shader's arithmetic gives the same node list as
/// the index buffer -- vertex by vertex, in the same order.
#[test]
fn the_shader_walks_the_same_triangles_as_the_index_buffer() {
    let count = SIDE * SIDE * 6;
    for mask in 0..16u8 {
        let expected = cubesphere::indices(mask);
        assert_eq!(expected.len(), count);

        for (vertex, &wanted) in expected.iter().enumerate() {
            let by_shader = node_of(vertex as u32, u32::from(mask));
            assert_eq!(
                by_shader, wanted,
                "mask {mask:04b}, vertex {vertex}: the shader gives node \
                 {by_shader}, the index buffer {wanted}"
            );
        }
    }
    println!("  {count} vertices x 16 masks agreed down to a single node");
}

/// The grid in the shader and the grid in the code are the same number.
///
/// `SIDE` is written both in `cubesphere` and in `shaders/patch.slang` as
/// `static const uint SIDE = 32`. There is no constant shared between Rust and
/// Slang, so a guard is what is left -- and that is exactly why it looks into
/// the **shader file** rather than repeating the number.
#[test]
fn the_shader_and_the_code_agree_on_the_patch_size() {
    let source = include_str!("../shaders/patch.slang");
    let wanted = format!("static const uint SIDE = {SIDE};");
    assert!(
        source.contains(&wanted),
        "shaders/patch.slang has no line \"{wanted}\" -- the grid has diverged \
         from cubesphere::SIDE, and the frame will draw different triangles"
    );
}

// ---------------------------------------------------------------------------
// Culling in compute (R6b)

use engine::camera::Camera;
use engine::cull;
use engine::frame::{Frame, FOV_Y};
use engine::gpu::Gpu;
use engine::lod;
use engine::scene::{Body, Scene, TileSet};
use engine::shot;

const SIZE: u32 = 256;
const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// **The oracle R3 was done on the CPU for.**
///
/// The number of patches the GPU drew must equal the number the CPU kept -- on
/// the same eight cameras as in R2c. Two independent paths, one number. Without
/// it an error in GPU culling looks like "something somewhere did not get
/// drawn" and is hunted by eye.
///
/// The agreement demanded is **exact**, and that is not overconfidence: both
/// paths compute the same formula, and the difference in arithmetic (`f64` on
/// the CPU against `f32` on the GPU) can only play out on a patch standing
/// exactly on the culling boundary. The step's plan named that fork in advance:
/// if agreement is not reached, it is the statement that should be narrowed,
/// not the tolerance. Measured -- narrowing was not needed.
#[test]
fn the_gpu_draws_exactly_as_many_patches_as_the_cpu_kept() {
    let Some(gpu) = Gpu::for_tests() else { return };

    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("cull shot"),
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
    let mut frame = Frame::new(&gpu, shot::FORMAT);

    let focal = lod::focal_px(FOV_Y, f64::from(SIZE));
    let aspect = 1.0;
    let mut checked = 0;

    for &x in &[-1.0f64, 1.0] {
        for &y in &[-1.0f64, 1.0] {
            for &z in &[-1.0f64, 1.0] {
                for altitude in [1.0e5, 3.0e5, 4.0e6] {
                    let length = (x * x + y * y + z * z).sqrt();
                    let distance = EARTH_RADIUS_M + altitude;
                    let eye = [
                        x / length * distance,
                        y / length * distance,
                        z / length * distance,
                    ];
                    let camera = Camera::look_at(eye, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);

                    // The CPU path: the same level selection, the same culling.
                    let body = lod::Body::still([0.0, 0.0, 0.0], EARTH_RADIUS_M);
                    let selection = lod::select(&body, &camera, focal, None);
                    let occluder =
                        cull::Body::smooth([0.0, 0.0, 0.0], EARTH_RADIUS_M, body.rotation);
                    let mut visibility = cull::horizon(&selection, &occluder, &camera);
                    cull::frustum(
                        &mut visibility,
                        &selection,
                        &occluder,
                        &camera,
                        FOV_Y,
                        aspect,
                    );

                    // The GPU path: draw the frame and ask the indirect counter.
                    let mut scene =
                        Scene::new(Camera::look_at(eye, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
                    scene.bodies.push(Body {
                        centre: [0.0, 0.0, 0.0],
                        radius_m: EARTH_RADIUS_M,
                        orientation: [1.0, 0.0, 0.0, 0.0],
                        tiles: TileSet::Smooth,
                        colour: engine::frame::COLOUR,
                        air: None,
                    });
                    let mut encoder =
                        gpu.device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("cull"),
                            });
                    frame.draw(&gpu, &mut encoder, &view, SIZE, SIZE, &scene);
                    gpu.queue.submit([encoder.finish()]);

                    let drawn = frame
                        .drawn_patches(&gpu)
                        .expect("the counter should have been read");

                    assert_eq!(
                        drawn[0] as usize,
                        visibility.drawn(),
                        "direction ({x}, {y}, {z}), altitude {altitude:.1e} m: the \
                         GPU drew {} patches, the CPU kept {} of {}",
                        drawn[0],
                        visibility.drawn(),
                        selection.patches.len()
                    );
                    checked += 1;
                }
            }
        }
    }

    println!("  {checked} cameras: GPU and CPU culled equally on every one");
}
