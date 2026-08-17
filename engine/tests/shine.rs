//! The planet lights the shadowed side of the ship (stage T, step T6).
//!
//! Three claims, and all three were named in ROADMAP before anything was
//! written: in low orbit the shadowed side of the hull is **not black**, the
//! colour of the shine is the colour of the surface below it (**over a mare
//! and over a highland it differs, and that is a number**), and over the night
//! side the shine **goes out**.
//!
//! ## The mask is taken from a frame without the planet
//!
//! The question here is about the pixels **the light source does not reach**:
//! those were exactly `[0, 0, 0]` before T6, because ambient in the frame is
//! zero (PROJECT.md section 7). They can only be found in a frame with no
//! planet at all; classifying by colour is impossible for the same reason it
//! had to be fixed in `tests/sun.rs`: a black hull pixel and a black shadow
//! pixel are indistinguishable.
//!
//! ## Mare against highland is checked by **turning the body**
//!
//! The ship, the camera, the light source and the planet itself stay bitwise
//! identical -- only `Body::orientation` changes, i.e. which part of the asset
//! ends up under the ship. Moving the ship along the surface would be a weaker
//! check: the direction "down" and the light angle would travel with the
//! place, and the ratio of brightnesses would stop being a ratio of
//! reflectances.
//!
//! Incidentally this is the only oracle for the **rotation into the body's
//! frame** in `shine_of`: a forgotten rotation leaves both frames identical.

use engine::camera::Camera;
use engine::cubesphere::FACES;
use engine::frame::{self, Frame};
use engine::gpu::Gpu;
use engine::scene::{Body, Scene, Ship, TileSet};
use engine::shot::{self, Shot};
use engine::srgb;
use engine::tiles::{self, Colour, Terrain, STORED};

const SIZE: u32 = 256;
const MOON_RADIUS_M: f64 = 1_737_400.0;

/// The ship's altitude above the surface, metres. Low orbit is exactly the
/// case the step talks about: the planet's disc takes up nearly a hemisphere.
const ALTITUDE_M: f64 = 100_000.0;

/// The ship and the distance to it, metres.
const HEIGHT_M: f64 = 20.0;
const RANGE_M: f64 = 45.0;

/// One level per pyramid: the test's question is not about the pyramid but
/// about where the albedo comes from.
const LEVELS: u32 = 1;

/// The fixture's reflectances -- measured numbers of the Moon, not invented
/// ones.
///
/// The median of the LROC WAC mosaic is 0.044, and the mare-highland contrast
/// is roughly 0.021 against 0.12 (T2c). Those are what lie in the fixture, so
/// the ratio in the frame has to be the ratio of these two.
const MARE: f64 = 0.021;
const HIGHLAND: f64 = 0.12;
const SCALE: f32 = 0.25;

fn gpu() -> Option<Gpu> {
    let gpu = Gpu::for_tests()?;
    if !gpu.bindless {
        eprintln!("SKIPPED: adapter without bindless ({})", gpu.describe());
        return None;
    }
    Some(gpu)
}

/// The light by day: the sub-ship point is lit (`cos = 0.6`).
const SUN_DAY: [f64; 3] = [0.6, 0.0, 0.8];
/// The light by night: the same, mirrored across the terminator.
const SUN_NIGHT: [f64; 3] = [-0.6, 0.0, 0.8];

/// The scene: the ship in low orbit, the planet below it (or absent).
///
/// The camera stands **off-axis and from below**: from above it would see only
/// the side facing the light source, and there would be nothing to ask about
/// the shadowed side.
fn scene(sun: [f64; 3], body: Option<Body>, roughness: f32, metallic: f32) -> Scene {
    let centre = [MOON_RADIUS_M + ALTITUDE_M, 0.0, 0.0];
    let eye = [
        centre[0] - RANGE_M * 0.30,
        centre[1] - RANGE_M * 0.75,
        centre[2] - RANGE_M * 0.59,
    ];
    // "Up" for the camera is away from the planet: otherwise the frame is
    // upside down, which makes reading the PNG by eye awkward for no gain.
    let camera = Camera::look_at(eye, centre, [1.0, 0.0, 0.0]);

    let mut scene = Scene::new(camera);
    scene.sun = sun;
    if let Some(body) = body {
        scene.bodies.push(body);
    }
    scene.ships.push(Ship {
        centre,
        orientation: [1.0, 0.0, 0.0, 0.0],
        height_m: HEIGHT_M,
        extent_m: 0.5 * HEIGHT_M,
        colour: [0.72, 0.74, 0.78, 1.0],
        roughness,
        metallic,
    });
    scene
}

/// The planet below the ship: smooth, in its own colour.
fn smooth(colour: [f32; 4]) -> Body {
    Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: MOON_RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: TileSet::Smooth,
        colour,
        air: None,
    }
}

/// Flat terrain: the question is about colour, and mountains would only add
/// shadows of their own.
fn flat() -> Terrain {
    let grids = vec![vec![0i16; STORED * STORED]; Terrain::count(LEVELS)];
    Terrain::build(LEVELS, MOON_RADIUS_M, 0.5, tiles::NO_SEA, &grids)
}

/// An asset in which the `+X` face is mare and the `-X` face is highland.
///
/// A constant per face rather than a map: the test's question is whether the
/// albedo is taken from the asset and from the right place, and a constant
/// answers it with no interpolation at all. The remaining faces carry a third
/// number, so a "took the wrong face" mistake gives not the second value but a
/// foreign one.
fn two_zones() -> Colour {
    let byte = |reflectance: f64| (reflectance / f64::from(SCALE) * 255.0).round() as u8;
    let mut grids = Vec::with_capacity(tiles::count(LEVELS));
    for face in 0..FACES {
        let value = match face {
            0 => byte(MARE),
            1 => byte(HIGHLAND),
            _ => byte(0.5 * (MARE + HIGHLAND)),
        };
        grids.push(vec![value; Colour::tile_len(1)]);
    }
    // Level-0 tiles, one per face, in face order (`tiles::index`).
    assert_eq!(grids.len(), tiles::count(LEVELS));
    Colour::build(LEVELS, 1, SCALE, false, &grids)
}

/// A studio: one texture, one frame, any number of scenes.
struct Studio {
    gpu: Gpu,
    frame: Frame,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Studio {
    fn new(gpu: Gpu) -> Studio {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shine shot"),
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
        let frame = Frame::new(&gpu, shot::FORMAT);
        Studio {
            gpu,
            frame,
            texture,
            view,
        }
    }

    fn take(&mut self, scene: &Scene) -> Shot {
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("shine"),
            });
        self.frame
            .draw(&self.gpu, &mut encoder, &self.view, SIZE, SIZE, scene);
        shot::read_back(&self.gpu, encoder, &self.texture, SIZE, SIZE)
            .expect("the frame should have come out")
    }
}

/// The hull pixels the light source never reached -- from the frame with no
/// planet in it.
///
/// Those were exactly black before T6, so they are what this is about. The
/// mask's size comes back along with it: a check with three pixels under the
/// mask is checking noise.
fn unlit_mask(studio: &mut Studio, sun: [f64; 3], roughness: f32, metallic: f32) -> Vec<bool> {
    let alone = studio.take(&scene(sun, None, roughness, metallic));
    let mut mask = vec![false; (SIZE * SIZE) as usize];
    let mut count = 0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let p = alone.pixel(x, y);
            let sky = [p[0], p[1], p[2]] == frame::CLEAR_BYTES;
            if !sky && p[0] == 0 && p[1] == 0 && p[2] == 0 {
                mask[(y * SIZE + x) as usize] = true;
                count += 1;
            }
        }
    }
    assert!(
        count > 300,
        "there is almost no shadowed side in the frame: {count} pixels"
    );
    mask
}

/// The mean linear light per channel under the mask.
///
/// Linear rather than bytes: the shot's target is sRGB (T5a), and dividing
/// bytes would mean measuring the transfer function instead of the
/// brightness.
fn mean_linear(shot: &Shot, mask: &[bool]) -> [f64; 3] {
    let mut sum = [0.0; 3];
    let mut count = 0.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            if !mask[(y * SIZE + x) as usize] {
                continue;
            }
            let p = shot.pixel(x, y);
            for c in 0..3 {
                sum[c] += srgb::byte_to_linear(p[c]);
            }
            count += 1.0;
        }
    }
    assert!(count > 0.0);
    [sum[0] / count, sum[1] / count, sum[2] / count]
}

/// How many pixels under the mask stopped being exactly black.
fn fraction_lit(shot: &Shot, mask: &[bool]) -> f64 {
    let mut lit = 0.0;
    let mut count = 0.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            if !mask[(y * SIZE + x) as usize] {
                continue;
            }
            let p = shot.pixel(x, y);
            count += 1.0;
            if p[0] != 0 || p[1] != 0 || p[2] != 0 {
                lit += 1.0;
            }
        }
    }
    lit / count
}

/// The shadowed side is not black by day and exactly black by night.
///
/// Both halves are needed together. The first on its own would pass on the
/// "ambient 0.05" that stage T deliberately removed; the second on its own
/// would pass on a shine that does not exist at all.
#[test]
fn the_shadow_side_lights_up_over_a_day_side_and_goes_out_over_the_night() {
    let Some(gpu) = gpu() else { return };
    let mut studio = Studio::new(gpu);
    let (roughness, metallic) = (0.35f32, 0.0f32);

    let day_mask = unlit_mask(&mut studio, SUN_DAY, roughness, metallic);
    let day = studio.take(&scene(
        SUN_DAY,
        Some(smooth([0.6, 0.6, 0.6, 1.0])),
        roughness,
        metallic,
    ));
    let lit = fraction_lit(&day, &day_mask);
    let mean = mean_linear(&day, &day_mask);
    println!("  day: {lit:.3} of the shadowed side is lit, {mean:?}");
    // Not "all", and that is as it should be: the shine arrives from below, so
    // facets facing **away** from the planet stay exactly black -- the same
    // cosine condition as for the light source. Measured at 0.65 with this
    // camera.
    assert!(
        lit > 0.5,
        "the planet below the ship did not light the shadowed side: {lit:.3}"
    );
    assert!(
        mean.iter().all(|&v| v > 0.005),
        "the shine is there but invisible: {mean:?}"
    );

    let night_mask = unlit_mask(&mut studio, SUN_NIGHT, roughness, metallic);
    let night = studio.take(&scene(
        SUN_NIGHT,
        Some(smooth([0.6, 0.6, 0.6, 1.0])),
        roughness,
        metallic,
    ));
    let lit = fraction_lit(&night, &night_mask);
    println!("  night: {lit:.3} is lit");
    assert_eq!(
        lit, 0.0,
        "over the planet's night side the shadowed side of the hull glows"
    );
}

/// The shine carries the planet's colour, not grey.
#[test]
fn the_shine_is_the_colour_of_the_planet_below() {
    let Some(gpu) = gpu() else { return };
    let mut studio = Studio::new(gpu);
    let (roughness, metallic) = (0.35f32, 0.0f32);
    let mask = unlit_mask(&mut studio, SUN_DAY, roughness, metallic);

    let blue = studio.take(&scene(
        SUN_DAY,
        Some(smooth([0.15, 0.30, 0.90, 1.0])),
        roughness,
        metallic,
    ));
    let rust = studio.take(&scene(
        SUN_DAY,
        Some(smooth([0.90, 0.30, 0.15, 1.0])),
        roughness,
        metallic,
    ));
    let blue = mean_linear(&blue, &mask);
    let rust = mean_linear(&rust, &mask);
    println!("  blue planet {blue:?}, rust-coloured {rust:?}");

    assert!(
        blue[2] > 3.0 * blue[0],
        "over a blue planet the hull is not blue: {blue:?}"
    );
    assert!(
        rust[0] > 3.0 * rust[2],
        "over a rust-coloured planet the hull is not rust-coloured: {rust:?}"
    );
}

/// Over a mare the ship is lit more dimly than over a highland -- and by
/// exactly the factor the asset's reflectances differ by.
///
/// This is step T6's number. The ratio is predicted in advance from the
/// fixture rather than read from the frame: "dimmer" would pass on an
/// implementation that takes the albedo at random.
#[test]
fn over_the_mare_the_hull_is_dimmer_than_over_the_highland() {
    let Some(gpu) = gpu() else { return };
    let mut studio = Studio::new(gpu);
    let (roughness, metallic) = (0.35f32, 0.0f32);

    let surface = studio
        .frame
        .load_surface(&studio.gpu, &flat(), Some(&two_zones()))
        .expect("the surface with colour should have loaded");

    let mask = unlit_mask(&mut studio, SUN_DAY, roughness, metallic);

    // A 180 deg rotation about `z` puts the opposite face under the ship -- and
    // changes nothing else in the scene.
    let mut over = |orientation: [f64; 4]| {
        let body = Body {
            centre: [0.0, 0.0, 0.0],
            radius_m: MOON_RADIUS_M,
            orientation,
            tiles: TileSet::Loaded(surface),
            colour: frame::COLOUR,
            air: None,
        };
        let shot = studio.take(&scene(SUN_DAY, Some(body), roughness, metallic));
        mean_linear(&shot, &mask)
    };
    let mare = over([1.0, 0.0, 0.0, 0.0]);
    let highland = over([0.0, 0.0, 0.0, 1.0]);

    let measured = highland[1] / mare[1];
    let expected = HIGHLAND / MARE;
    println!("  mare {mare:?}, highland {highland:?}");
    println!("  ratio: {measured:.3} against {expected:.3} from the asset");
    assert!(
        (measured - expected).abs() < 0.1 * expected,
        "the brightness does not follow the asset: {measured:.3} against {expected:.3}"
    );
}
