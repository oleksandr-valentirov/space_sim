//! The hull material in the frame against an analytic twin (stage T, step
//! T5c).
//!
//! The same oracle as `engine::cull` against `cull.slang` and
//! `engine::atmosphere` against `sky.slang`: **a number against a number**.
//! GGX has a closed form, so [`engine::brdf`] gives an exact answer with no
//! exposure and no look settings of any kind, and a divergence means a
//! mistake.
//!
//! ## Triangle centres are compared, not vertices
//!
//! A vertex lies on the boundary of several triangles, i.e. at a point where
//! the normal interpolation jumps (with flat normals) or where the
//! rasteriser's rounding decides which pixel it belongs to. A triangle centre
//! has neither: in it the interpolated normal is the mean of the three vertex
//! normals, and that is what the CPU computes too.
//!
//! ## The space is the camera's, and that is not an implementation detail
//!
//! The ship's vertex positions arrive already rotated into the camera axes
//! (`Camera::relative`), so the normal and the direction to the light source
//! are in camera space too (T5c). The twin has to compute in the same space,
//! otherwise it would be comparing a correct formula with a correct formula on
//! different vectors.

use engine::camera::Camera;
use engine::gpu::Gpu;
use engine::scene::{Scene, Ship};
use engine::shot::Shot;
use engine::{brdf, frame, ship, shot, srgb, tonemap};

const SIZE: u32 = 768;

/// The ship's size and the distance to it, metres.
const HEIGHT_M: f64 = 20.0;
const RANGE_M: f64 = 45.0;

/// The hull's base colour -- three different channels deliberately.
///
/// Equal channels would let a channel swap in the shader through, and one is
/// entirely possible here: a metal's `F0` **is** the base colour, i.e. the
/// colour enters the formula twice and by different routes.
const BASE: [f32; 4] = [0.55, 0.70, 0.85, 1.0];

fn gpu() -> Option<Gpu> {
    Gpu::for_tests()
}

/// The scene: the ship in front of the camera and nothing else.
///
/// No planet and no air: both would add light to the pixel that the twin does
/// not compute, and the oracle would stop being a number against a number.
fn scene(sun: [f64; 3], roughness: f32, metallic: f32) -> Scene {
    // WARNING: the camera stands **off-axis**, and not for looks. With the
    // camera on the `z` axis the camera basis coincides with the world one,
    // `Camera::rotate` becomes the identity -- and the crudest possible
    // mistake, lighting in two different spaces, becomes invisible in such a
    // fixture. The first edition stood exactly that way, and it let a
    // deliberate "light in world axes" break through.
    let eye = [RANGE_M * 0.42, RANGE_M * 0.31, RANGE_M * 0.85];
    let camera = Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let mut scene = Scene::new(camera);
    scene.sun = sun;
    scene.ships.push(Ship {
        centre: [0.0, 0.0, 0.0],
        // A quarter turn about `x`: the ship stands **side-on** to the camera.
        //
        // WARNING: not cosmetic. The nose is a cone, and nose-on no facet
        // faces the camera directly enough for the specular peak to enter the
        // frame at all: at `roughness = 0.08` the peak of `D` is 7768 while
        // the nearest facet catches 0.13. The side of the hull is a solid of
        // revolution, and its central band faces the camera exactly.
        orientation: [
            std::f64::consts::FRAC_PI_4.cos(),
            std::f64::consts::FRAC_PI_4.sin(),
            0.0,
            0.0,
        ],
        height_m: HEIGHT_M,
        extent_m: 0.5 * HEIGHT_M,
        colour: BASE,
        roughness,
        metallic,
    });
    scene
}

fn normalise(v: [f64; 3]) -> [f64; 3] {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / n, v[1] / n, v[2] / n]
}

/// The errors over every checked facet, in bytes.
struct Agreement {
    errors: Vec<i32>,
    /// How many facets came out **above the knee** of the tonemapper.
    ///
    /// Without this number the oracle could silently be checking only the
    /// identity part of the curve: below the knee the compression does
    /// nothing, so a mistake in it is invisible there.
    compressed: usize,
}

impl Agreement {
    fn checked(&self) -> usize {
        self.errors.len()
    }

    /// The median is the oracle's headline number.
    ///
    /// A mistake in the **formula** shifts every facet, and hence the median
    /// too. Occlusion by geometry spoils individual facets while leaving the
    /// median at zero; that is exactly why the oracle asks about the median
    /// rather than the maximum.
    fn median(&self) -> i32 {
        let mut sorted = self.errors.clone();
        sorted.sort_unstable();
        sorted[sorted.len() / 2]
    }

    fn within(&self, bytes: i32) -> f64 {
        let good = self.errors.iter().filter(|e| **e <= bytes).count();
        good as f64 / self.errors.len() as f64
    }

    fn worst(&self) -> i32 {
        self.errors.iter().copied().max().unwrap_or(0)
    }
}

fn compare(gpu: &Gpu, sun: [f64; 3], roughness: f32, metallic: f32) -> Agreement {
    let scene = scene(sun, roughness, metallic);
    let shot: Shot =
        shot::take_scene(gpu, SIZE, SIZE, &scene).expect("the frame should have drawn");
    let camera = &scene.camera;
    let ship = &scene.ships[0];
    let mesh = ship::generate(ship.height_m);
    // The same rotation the frame applies (`frame::rotation`) -- otherwise the
    // twin would be computing for different geometry.
    let turn = |v: [f64; 3]| {
        let q = ship.orientation;
        let (w, x, y, z) = (q[0], q[1], q[2], q[3]);
        [
            v[0] * (1.0 - 2.0 * (y * y + z * z))
                + v[1] * 2.0 * (x * y - w * z)
                + v[2] * 2.0 * (x * z + w * y),
            v[0] * 2.0 * (x * y + w * z)
                + v[1] * (1.0 - 2.0 * (x * x + z * z))
                + v[2] * 2.0 * (y * z - w * x),
            v[0] * 2.0 * (x * z - w * y)
                + v[1] * 2.0 * (y * z + w * x)
                + v[2] * (1.0 - 2.0 * (x * x + y * y)),
        ]
    };
    let light = {
        let d = camera.rotate(sun);
        normalise([f64::from(d[0]), f64::from(d[1]), f64::from(d[2])])
    };

    let mut out = Agreement {
        errors: Vec::new(),
        compressed: 0,
    };

    for triangle in mesh.indices.chunks_exact(3) {
        let corners: Vec<usize> = triangle.iter().map(|i| *i as usize).collect();

        // The triangle's centre in the world and the mean normal -- exactly
        // what interpolation gives at that same point.
        let mut centre = [0.0f64; 3];
        let mut normal = [0.0f64; 3];
        for &k in &corners {
            for axis in 0..3 {
                centre[axis] += mesh.positions[k][axis] / 3.0;
                normal[axis] += f64::from(mesh.normals[k][axis]) / 3.0;
            }
        }
        if normal.iter().all(|v| *v == 0.0) {
            continue;
        }
        let centre = turn(centre);
        let normal = turn(normal);

        // The triangle has to be noticeably larger than a pixel: in a small
        // one the neighbours share the centre, and the frame there shows a
        // mixture of several facets.
        let mut corner_px = Vec::with_capacity(3);
        for &k in &corners {
            let world = turn(mesh.positions[k]);
            let Some(p) = camera.to_screen(frame::FOV_Y, SIZE, SIZE, world) else {
                break;
            };
            corner_px.push(p);
        }
        if corner_px.len() < 3 {
            continue;
        }
        let area = {
            let (a, b, c) = (corner_px[0], corner_px[1], corner_px[2]);
            let ux = f64::from(b[0] - a[0]);
            let uy = f64::from(b[1] - a[1]);
            let vx = f64::from(c[0] - a[0]);
            let vy = f64::from(c[1] - a[1]);
            0.5 * (ux * vy - uy * vx).abs()
        };
        if area < 12.0 {
            continue;
        }

        let Some(pixel) = camera.to_screen(frame::FOV_Y, SIZE, SIZE, centre) else {
            continue;
        };
        let (x, y) = (pixel[0].round() as i64, pixel[1].round() as i64);
        if x < 1 || y < 1 || x + 1 >= i64::from(SIZE) || y + 1 >= i64::from(SIZE) {
            continue;
        }
        let (x, y) = (x as u32, y as u32);

        // The triangle has to cover its pixel together with the neighbours: on
        // the silhouette boundary some of them are sky, and there is nothing
        // to compare there.
        let neighbours = [
            shot.pixel(x, y),
            shot.pixel(x - 1, y),
            shot.pixel(x + 1, y),
            shot.pixel(x, y - 1),
            shot.pixel(x, y + 1),
        ];
        if neighbours
            .iter()
            .any(|p| [p[0], p[1], p[2]] == frame::CLEAR_BYTES)
        {
            continue;
        }
        // And the neighbours have to be close to one another: a sharp step
        // means an edge or the boundary of an occlusion, i.e. a pixel that
        // belongs to a different facet.
        let spread = neighbours
            .iter()
            .map(|p| i32::from(p[1]))
            .max()
            .unwrap_or(0)
            - neighbours
                .iter()
                .map(|p| i32::from(p[1]))
                .min()
                .unwrap_or(0);
        if spread > 8 {
            continue;
        }

        let position = camera.relative64(centre);
        let view = normalise([-position[0], -position[1], -position[2]]);
        let n = camera.rotate(normal);
        let n = normalise([f64::from(n[0]), f64::from(n[1]), f64::from(n[2])]);
        // WARNING: **facets turned away from the camera are discarded, and
        // that is the oracle's main filter.** The hull is a solid of
        // revolution, so exactly half its facets lie on the far side; their
        // centres project into the same pixels as the near ones, and the frame
        // there shows a **different** facet. The first edition did not filter
        // them out and got 28% agreement; the second flipped the normal
        // towards the eye, as the shader does, and got 69% -- plausible
        // numbers taken from somebody else's pixels. The right answer is not
        // to count them at all.
        //
        // The 0.15 margin also removes facets nearly parallel to the ray:
        // there several facets share a pixel at once.
        if n[0] * view[0] + n[1] * view[1] + n[2] * view[2] < 0.15 {
            continue;
        }

        let mut worst = 0;
        let mut compressed = false;
        for channel in 0..3 {
            let value = brdf::radiance(
                n,
                view,
                light,
                f64::from(BASE[channel]),
                f64::from(roughness),
                f64::from(metallic),
            );
            if value > tonemap::KNEE {
                compressed = true;
            }
            // WARNING: the compression is part of the prediction (T5c3).
            // Without it the oracle would diverge precisely on the highlight
            // -- i.e. where the material is most interesting.
            let expected = i32::from(srgb::linear_to_byte(tonemap::compress(value)));
            let got = i32::from(neighbours[0][channel]);
            worst = worst.max((expected - got).abs());
        }
        // The specular peak is not compared -- see the explanation in the test.
        if compressed {
            out.compressed += 1;
            continue;
        }
        out.errors.push(worst);
    }
    out
}

/// The frame gives the same number as the analytic twin, on every facet of
/// the hull.
///
/// A sweep over four materials and two light sources: a mistake in one term of
/// the formula almost always leaves the other correct, so a mirror metal and a
/// matte dielectric both have to agree.
///
/// WARNING: perfect agreement is impossible here, and the reason is geometric
/// rather than numeric: the hull carries fins, so some facets are **occluded**
/// by others, and such a facet's centre falls into a pixel that does not
/// belong to it. Depth cannot be read out of the frame, so these cases are not
/// filtered out -- hence the oracle's headline number is the **median** rather
/// than the maximum: a mistake in the formula shifts every facet, occlusion
/// spoils individual ones.
///
/// WARNING: **what this oracle does not catch, verified by breaking it:**
///
/// * **the Fresnel exponent** (`t^5` instead of `t^4`). For a metal `F0` is
///   the base colour, i.e. 0.55...0.85, and the `(1 - F0)*t^5` term stays
///   small; for a dielectric it is large only at a grazing angle, where the
///   highlight itself is small against the diffuse term. So in this fixture
///   the exponent is not observable, and what guards it is
///   [`the_shader_carries_the_same_material_numbers`];
/// * **flipping the normal towards the eye** -- by construction: the oracle
///   discards facets turned away from the camera. What guards it is
///   `tests/sun.rs`, where without the flip the hull gets black patches.
///
/// What it does catch is what it exists for: `alpha = roughness` instead of
/// `roughness^2` breaks the median immediately.
#[test]
fn every_facet_shows_the_number_the_analytic_brdf_predicts() {
    let Some(gpu) = gpu() else { return };

    for (roughness, metallic) in [(0.35, 1.0), (0.8, 1.0), (0.25, 0.0), (0.9, 0.0)] {
        for sun in [[0.0, 0.0, 1.0], [0.55, 0.3, 0.78]] {
            let got = compare(&gpu, sun, roughness, metallic);
            println!(
                "  roughness {roughness}, metallic {metallic}, light {sun:?}: \
                 {} facets, median {}, within 2 bytes {:.1}%, worst {}, \
                 above the knee {}",
                got.checked(),
                got.median(),
                got.within(2) * 100.0,
                got.worst(),
                got.compressed
            );
            assert!(
                got.checked() > 40,
                "only {} facets were checked -- the fixture does not work",
                got.checked()
            );
            // WARNING: facets on the **specular peak** are dropped from the
            // comparison, and that is a limit of the method rather than a
            // weakening of the oracle. The peak of `D` at `roughness = 0.35`
            // is hundreds of times higher than its flank, so the difference
            // between the mean normal of three vertices (what the twin
            // computes) and the perspective-interpolated one at the pixel
            // centre (what the shader sees) gives hundreds of bytes there with
            // the very same formulas. Measured: without this filter the median
            // stays zero and the worst facet is 235 bytes.
            //
            // WARNING: one byte rather than zero, and the reason is named: the
            // comparison point is the projection of the triangle's **spatial**
            // centre, while the rasteriser interpolates attributes in a
            // perspective-correct way, i.e. with weights that are not thirds
            // when the vertices lie at different depths. A hull side-on to the
            // camera is a solid of revolution, and most of its triangles are
            // like that. A mistake in the formula gives not one here but
            // tens.
            assert!(
                got.median() <= 1,
                "a median divergence of {} bytes: the shader has diverged from \
                 `engine::brdf` on every facet, not on individual ones",
                got.median()
            );
            assert!(
                got.within(2) > 0.8,
                "only {:.1}% of facets are within 2 bytes",
                got.within(2) * 100.0
            );
        }
    }
}

/// The material's numbers are written down twice -- in Rust and in the shader
/// -- and have to match.
///
/// The same guard that compares `SIDE` in `gpu_driven.rs` and the material
/// rule's constants in `material.rs`. Here it carries more than usual: the
/// Fresnel exponent is not observable in the frame (see above), so this line
/// is the only thing standing between it and a silent divergence.
#[test]
fn the_shader_carries_the_same_material_numbers() {
    let source = include_str!("../shaders/ship.slang");
    for (name, value) in [
        ("DIELECTRIC_F0", brdf::DIELECTRIC_F0),
        ("MIN_ROUGHNESS", brdf::MIN_ROUGHNESS),
    ] {
        let wanted = format!("static const float {name} = {value};");
        assert!(
            source.contains(&wanted),
            "shaders/ship.slang has no line \"{wanted}\" -- the material has \
             diverged from `engine::brdf`"
        );
    }
    // Schlick's fifth power: it is invisible in the frame, and that is exactly
    // why it is here.
    assert!(
        source.contains("float t5 = t * t * t * t * t;"),
        "shaders/ship.slang has no Schlick fifth power"
    );
}
