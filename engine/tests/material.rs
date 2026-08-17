//! The material rule in the frame (stage T, step T4b).
//!
//! The twin in the shader is checked the same way `engine::cull` is checked
//! against `cull.slang`: by a number, not by eye. But a frame is not a compute
//! pass, and the multiplier cannot be read out of it directly, so the fixture
//! is built so that everything else in the brightness is known in advance.
//!
//! **A ramp of constant slope.** The tileset is linear in fractions of a face,
//! so `slope_at` gives the same number at every node (proved separately --
//! `tiles::tests::the_slope_of_a_ramp_is_the_ramp`). Hence the
//! [`material::tint`] multiplier is constant across the whole disc, and it can
//! be computed on the CPU before a single shot is taken.
//!
//! **The camera is so far away that there is no procedural detail at all.**
//! Not "little" but exactly zero: at 3e5 m the coarsest octave takes 2.5
//! pixels while [`detail::FADE_LO_PX`] is 4, so `octave_weight` returns zero
//! and the loop breaks on the very first one. That leaves the slope term alone
//! in the multiplier.
//!
//! **The colour is a constant.** All the brightness difference between the two
//! shots then belongs to the multiplier and the facet's tilt, not to the
//! mosaic.
//!
//! One contaminant remains, and it is computed rather than dismissed: the ramp
//! tilts the surface by `atan(slope)`, i.e. it changes the diffuse term. At a
//! slope of 0.12 that is 6.8 deg and 0.68% of brightness, against 24% from the
//! rule itself.
//!
//! WARNING: **the shot's bytes are decoded before any division** (T5a). The
//! target encodes gamma, so a ratio of bytes is not a ratio of brightnesses,
//! and a tolerance of "one unit" is meaningless: a byte step costs three times
//! more near the light tones than near the dark ones. The tolerances here are
//! expressed through [`byte_quantum`], and that is not over-caution -- the
//! field in the frame is nearly constant, so every pixel rounds the same way
//! and the error is not averaged out by any number of pixels.

use engine::camera::Camera;
use engine::cubesphere::{Patch, FACES, SIDE};
use engine::frame::{self, Frame};
use engine::gpu::Gpu;
use engine::scene::{Body, Scene, TileSet};
use engine::shot::{self, Shot};
use engine::tiles::{self, Colour, Terrain, HALO, NODES, STORED};
use engine::{detail, material, srgb};

const SIZE: u32 = 256;
const MOON_RADIUS_M: f64 = 1_737_400.0;
/// Levels in the fixture's pyramids.
///
/// Two numbers rather than one: step T4 promises that pyramid depth does not
/// enter the colour, and that can only be checked with two pyramids of the
/// same terrain.
const LEVELS: u32 = 3;
const OTHER_LEVELS: u32 = 2;

/// The ramp's slope.
///
/// Clamped from both sides. **From below** by quantisation: the shot is eight
/// bits, and one byte step costs about a per cent of brightness, so a 5%
/// signal would leave only four times the error to measure. **From above** by
/// the contaminant: the ramp tilts the surface itself, and the diffuse term
/// with it. 0.12 gives a signal of 24% against a contaminant of 0.68% and a
/// quantum of 1.0%.
const SLOPE: f64 = 0.12;

/// Metres per storage unit.
///
/// Not one: a constant slope over the whole body inevitably accumulates relief
/// (0.12 over a quarter of a great circle is 327 km), and in `i16` that only
/// fits with a coarse scale. This is a property of the fixture, not of the
/// body.
const SCALE_M: f32 = 16.0;

/// The brightness of the constant colour, storage units.
const FLAT_COLOUR: u8 = 160;

/// The altitude from which the procedural terrain is visible.
///
/// Clamped from above by the fade: the coarsest octave (3393 m) has to take
/// more than [`detail::FADE_LO_PX`] pixels, i.e. the camera has to be below
/// 188 km. Four kilometres leave five of the six octaves alive.
const NEAR_ALTITUDE: f64 = 4.0e3;

fn gpu() -> Option<Gpu> {
    let gpu = Gpu::for_tests()?;
    if !gpu.bindless {
        eprintln!("SKIPPED: adapter without bindless ({})", gpu.describe());
        return None;
    }
    Some(gpu)
}

/// Height units per fraction of a face along `y`; along `x` it is half that.
///
/// Derived backwards from the slope wanted:
/// `slope = sqrt(g^2 + (2g)^2) * scale / (pi/2 * R)`.
fn gradient() -> f64 {
    SLOPE * std::f64::consts::FRAC_PI_2 * MOON_RADIUS_M / (5f64.sqrt() * f64::from(SCALE_M))
}

/// A ramp, linear in fractions of a face: `g*x + 2g*y`, shifted to zero under
/// the camera.
///
/// The same shape as the `tiles::tests::ramp` fixture, and for the same
/// reason: the slope of such a grid is known analytically and depends neither
/// on the level, nor on the node, nor on whether the patch is deeper than the
/// pyramid.
///
/// WARNING: **the subtracted constant is not cosmetic, and without it the
/// fixture is quiet and wrong.** A constant slope over the whole body
/// accumulates relief, and under the chosen node the ramp stood on a pedestal
/// of 98 km. Two things in the frame are measured from the **reference
/// sphere**, not from the ground: `Frame::near_for` (i.e. the near plane) and
/// `distance` in the shader, which is taken from the undisplaced vertex. So a
/// camera raised 30 km above the ground looked, to the octave fade, like a
/// camera at 128 km -- and almost no procedural detail was left in the frame.
/// The mistake looked like "the rule does not work".
fn ramp(levels: u32) -> Terrain {
    ramp_at_sea(levels, tiles::NO_SEA)
}

/// The same ramp with a sea level given up front (T7f).
///
/// Two tilesets built from it differ by **exactly one word of the header**, so
/// everything their frames differ by is the material multiplier: geometry,
/// slope and detail are bitwise identical in both.
fn ramp_at_sea(levels: u32, sea_units: f32) -> Terrain {
    let g = gradient();
    // The ramp's value at the node the camera stands over.
    let pedestal = g * (VIEW_X + 2.0 * VIEW_Y);
    let mut grids = Vec::with_capacity(Terrain::count(levels));
    for level in 0..levels {
        let side = 1u32 << level;
        for _face in 0..FACES {
            for i in 0..side {
                for j in 0..side {
                    let span = f64::from(SIDE as u32 * side);
                    let mut grid = Vec::with_capacity(STORED * STORED);
                    for a in 0..STORED {
                        for b in 0..STORED {
                            let a = a as isize - HALO as isize;
                            let b = b as isize - HALO as isize;
                            let x = (i as isize * SIDE as isize + a) as f64 / span;
                            let y = (j as isize * SIDE as isize + b) as f64 / span;
                            grid.push((g * x + 2.0 * g * y - pedestal) as i16);
                        }
                    }
                    grids.push(grid);
                }
            }
        }
    }
    Terrain::build(levels, MOON_RADIUS_M, SCALE_M, sea_units, &grids)
}

/// Flat zeroes: slope zero, detail zero, multiplier exactly one.
fn flat() -> Terrain {
    let grids = vec![vec![0i16; STORED * STORED]; Terrain::count(LEVELS)];
    Terrain::build(LEVELS, MOON_RADIUS_M, SCALE_M, tiles::NO_SEA, &grids)
}

/// The colour is the same constant everywhere, halo included.
///
/// There is deliberately no marker in the halo here, unlike in
/// `colour_tiles.rs`: that test asks about addressing, this one about
/// brightness, and any non-uniformity of the mosaic would contaminate the
/// measurement itself.
fn plain_colour(levels: u32) -> Colour {
    let grids = vec![vec![FLAT_COLOUR; NODES * NODES]; tiles::count(levels)];
    Colour::build(levels, 1, 0.25, false, &grids)
}

/// The node the camera stands over.
///
/// Not the centre of a face, and that is not taste: the whole engine fixture
/// once stood exactly over it -- the one point where wrong geometry gives the
/// right answer -- and D13 and D14 lived in it unseen. The fractions of a face
/// here are 0.40 and 0.60, i.e. +-9 deg from the centre; the face seam stays
/// outside the frame at that.
///
/// A node rather than an arbitrary direction, for a different reason: for a
/// node the **terrain height** is known (`Terrain::height_m`), and without it
/// there is nothing to measure the camera from -- the ramp lifts the surface
/// by a hundred kilometres.
fn view_patch() -> Patch {
    Patch {
        face: 0,
        level: 2,
        i: 1,
        j: 2,
    }
}
const VIEW_A: usize = 19;
const VIEW_B: usize = 13;
/// The same coordinates in fractions of a face -- `(i*SIDE + a) / (SIDE*2^level)`.
const VIEW_X: f64 = (1.0 * 32.0 + 19.0) / (32.0 * 4.0);
const VIEW_Y: f64 = (2.0 * 32.0 + 13.0) / (32.0 * 4.0);

/// The unit direction to that node.
fn view_unit() -> [f64; 3] {
    let v = view_patch().vertex(VIEW_A, VIEW_B, 1.0);
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / n, v[1] / n, v[2] / n]
}

/// The Moon, lit from exactly the camera's side.
///
/// Light along the view means the diffuse term is nearly constant across the
/// disc, and the difference between the two shots belongs to the material
/// multiplier rather than to the cosine.
///
/// `altitude` is counted from the **surface under the camera**, not from the
/// reference radius: otherwise at a low altitude the camera would end up
/// inside the ramp.
fn scene(tiles: TileSet, terrain: &Terrain, altitude: f64) -> Scene {
    let unit = view_unit();
    let ground = terrain.height_m(&view_patch(), VIEW_A, VIEW_B);
    let distance = MOON_RADIUS_M + ground + altitude;
    let eye = [unit[0] * distance, unit[1] * distance, unit[2] * distance];
    let mut scene = Scene::new(Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]));
    scene.sun = unit;
    scene.bodies.push(Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: MOON_RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles,
        colour: frame::COLOUR,
        air: None,
    });
    scene
}

/// The brightness of a pixel in **linear light**, or `None` for empty sky.
///
/// WARNING: the byte from the shot is decoded, and without that this whole
/// module would be lying. Since T5a the target encodes gamma, so a ratio of
/// two bytes is a ratio of two gamma-encoded numbers, while the material
/// multiplier is linear by construction. The scale stays 0...255 only so that
/// the numbers in the messages are recognisable.
fn lit(shot: &Shot, x: u32, y: u32) -> Option<f64> {
    let p = shot.pixel(x, y);
    if [p[0], p[1], p[2]] == frame::CLEAR_BYTES {
        return None;
    }
    let mean =
        (srgb::byte_to_linear(p[0]) + srgb::byte_to_linear(p[1]) + srgb::byte_to_linear(p[2]))
            / 3.0;
    Some(mean * 255.0)
}

/// The width of one shot byte in the same linear units as [`lit`].
///
/// WARNING: needed precisely because the target encodes gamma (T5a): one byte
/// step costs three times less near the dark tones than near the light ones,
/// so a tolerance of "one unit of brightness" no longer means anything. And it
/// does not average out: where the field in the frame is constant, every pixel
/// rounds **the same way**, however many of them there are.
fn byte_quantum(value: f64) -> f64 {
    let byte = srgb::linear_to_byte(value / 255.0);
    let up = srgb::byte_to_linear(byte.saturating_add(1));
    let down = srgb::byte_to_linear(byte.saturating_sub(1));
    (up - down) / 2.0 * 255.0
}

/// The mean brightness and the spread in the central window of the frame.
///
/// A window rather than the whole disc: near the limb the cosine falls off,
/// and any comparison there would be measuring geometry. In the centre the
/// surface faces the camera and the light source alike in both shots.
fn window(shot: &Shot, half: u32) -> (f64, f64) {
    let mid = SIZE / 2;
    let mut values = Vec::new();
    for y in mid - half..mid + half {
        for x in mid - half..mid + half {
            if let Some(value) = lit(shot, x, y) {
                values.push(value);
            }
        }
    }
    assert!(
        values.len() > (2 * half * 2 * half) as usize * 9 / 10,
        "the centre of the frame is not covered by the surface: {} of {}",
        values.len(),
        4 * half * half
    );
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let spread = values.iter().map(|v| (v - mean).abs()).sum::<f64>() / values.len() as f64;
    (mean, spread)
}

/// A shot of the scene with the given terrain and a constant colour.
fn take(gpu: &Gpu, terrain: &Terrain, altitude: f64) -> Shot {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("material shot"),
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

    let mut frame = Frame::new(gpu, shot::FORMAT);
    let id = frame
        .load_surface(gpu, terrain, Some(&plain_colour(terrain.levels)))
        .expect("the surface should have loaded");
    let scene = scene(TileSet::Loaded(id), terrain, altitude);
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("material shot"),
        });
    frame.draw(gpu, &mut encoder, &view, SIZE, SIZE, &scene);
    shot::read_back(gpu, encoder, &texture, SIZE, SIZE).expect("the frame should have drawn")
}

/// The multiplier from the frame agrees with the multiplier from the CPU twin.
///
/// This is the check that the two copies of the rule -- in `engine::material`
/// and in `patch.slang` -- have not diverged.
#[test]
fn the_frame_shows_the_multiplier_the_rule_predicts() {
    let Some(gpu) = gpu() else { return };
    const ALTITUDE: f64 = 3.0e5;

    // At this altitude the detail has to be exactly zero -- otherwise the
    // prediction is incomplete. Checked rather than assumed.
    let focal = f64::from(SIZE) / 2.0 / (30f64.to_radians()).tan();
    let base = detail::base_m(MOON_RADIUS_M);
    let weight = detail::octave_weight(base, ALTITUDE, focal);
    assert_eq!(
        weight, 0.0,
        "at {ALTITUDE:.0} m the detail is still alive: weight {weight}"
    );

    let tint = material::tint(SLOPE, 0.0, false);
    // The diffuse contaminant: the ramp tilts the facet by `atan(slope)`, and
    // the lighting in the shader is `0.05 + 0.95*cos`.
    let cos = 1.0 / (1.0 + SLOPE * SLOPE).sqrt();
    let predicted = tint * (0.05 + 0.95 * cos) / (0.05 + 0.95);
    assert!(
        (predicted - 1.0).abs() > 0.04,
        "the fixture is toothless: the rule changes the brightness by only \
         {:.3}%",
        (predicted - 1.0) * 100.0
    );

    let (sloped, _) = window(&take(&gpu, &ramp(LEVELS), ALTITUDE), 32);
    let (level, _) = window(&take(&gpu, &flat(), ALTITUDE), 32);
    let measured = sloped / level;
    println!(
        "  slope {SLOPE}: multiplier {tint:.4}, with the diffuse contaminant \
         {predicted:.4}, in frame {measured:.4} ({level:.1} -> {sloped:.1} units)"
    );

    // The tolerance is not taste but quantisation: both shots sit on a nearly
    // constant brightness, so each rounds wholly in one direction, and the
    // difference of the two roundings gives exactly this bound.
    let tolerance = (byte_quantum(sloped) / sloped + byte_quantum(level) / level) / 2.0;
    println!(
        "  tolerance from the byte quantum: {:.3}%",
        tolerance * 100.0
    );
    assert!(
        (measured / predicted - 1.0).abs() < tolerance,
        "the frame gave {measured:.4} against the predicted {predicted:.4} at a \
         tolerance of {tolerance:.4} -- the rule in the shader has diverged from \
         `engine::material`"
    );
}

/// Under water the rule does nothing -- and does exactly nothing, not "almost"
/// (T7f).
///
/// The same ramp, the same frame, one single difference in the tileset header:
/// a sea level above any height that can be written into an `i16`. So the
/// slope, the geometry and the detail stayed the same, and the only thing that
/// could have changed is the multiplier.
///
/// What for: the rule tints a slope, and under water what is seen in the frame
/// is the surface of the sea, not the slope of the floor. On Earth this is no
/// detail -- it is measured (`--example slope_histogram assets/earth.dem`)
/// that the seabed is **steeper** than the land: a median of 0.0071 against
/// 0.0030, a ninetieth percentile of 0.0333 against 0.0201. Without this
/// branch the rule would draw mid-ocean ridges on top of flat water, and
/// brighter than mountains on dry land.
#[test]
fn under_water_the_rule_does_nothing() {
    let Some(gpu) = gpu() else { return };
    const ALTITUDE: f64 = 3.0e5;

    // The same diffuse contaminant as in the neighbouring test, but **without**
    // the multiplier: under water it has to be exactly one.
    let cos = 1.0 / (1.0 + SLOPE * SLOPE).sqrt();
    let predicted = (0.05 + 0.95 * cos) / (0.05 + 0.95);

    let drowned = ramp_at_sea(LEVELS, f32::from(i16::MAX));
    let (sunk, _) = window(&take(&gpu, &drowned, ALTITUDE), 32);
    let (dry, _) = window(&take(&gpu, &ramp(LEVELS), ALTITUDE), 32);
    let (level, _) = window(&take(&gpu, &flat(), ALTITUDE), 32);
    let measured = sunk / level;
    println!(
        "  under water {measured:.4} against the predicted {predicted:.4}; \
         above water {:.4}",
        dry / level
    );

    // First, that the fixture measures anything at all: the dry and the
    // drowned frames have to differ. Otherwise this test would pass even with
    // the rule switched off everywhere.
    assert!(
        (dry - sunk).abs() > byte_quantum(dry),
        "the dry and drowned frames are identical ({dry:.1} against {sunk:.1}): \
         the fixture does not tell the branches apart"
    );

    let tolerance = (byte_quantum(sunk) / sunk + byte_quantum(level) / level) / 2.0;
    assert!(
        (measured / predicted - 1.0).abs() < tolerance,
        "under water the frame gave {measured:.4} against {predicted:.4} at a \
         tolerance of {tolerance:.4} -- the rule did not switch off"
    );
}

/// The relief reaches the colour, and it reaches it only from up close.
///
/// A distant frame of the ramp is flat: the multiplier is constant, because
/// there is no detail. A close one is not: the procedural relief gives +-7% of
/// brightness. The **geometric** contaminant here is negligible by
/// construction: the detail's own slope is `STEEPNESS * slope`, i.e. 1.4 deg,
/// and the shading from it does not reach even one per cent.
#[test]
fn the_relief_paints_only_when_the_camera_is_close() {
    let Some(gpu) = gpu() else { return };

    let (_, far) = window(&take(&gpu, &ramp(LEVELS), 3.0e5), 32);
    let (_, near) = window(&take(&gpu, &ramp(LEVELS), NEAR_ALTITUDE), 32);
    println!("  spread: {far:.2} from afar, {near:.2} units from up close");

    assert!(
        far < 1.5,
        "the distant frame should have been flat, but the spread is {far:.2} units"
    );
    assert!(
        near > 4.0 * far.max(0.5),
        "from up close the spread is only {near:.2} against {far:.2} -- the \
         relief does not paint"
    );
}

/// The rule's numbers are written down twice -- in Rust and in the shader --
/// and have to match.
///
/// The same guard that compares `SIDE` in `gpu_driven.rs`: no constant is
/// shared between Rust and Slang, so all that is left is to read the shader
/// file and compare a line. A mistake here neither crashes nor warns: it draws
/// a slightly different colour.
#[test]
fn the_shader_carries_the_same_numbers() {
    let source = include_str!("../shaders/patch.slang");
    for (name, value) in [
        ("SLOPE_GAIN", material::SLOPE_GAIN),
        ("SLOPE_REF", material::SLOPE_REF),
        ("RELIEF_GAIN", material::RELIEF_GAIN),
        ("MIN_TINT", material::MIN_TINT),
        ("MAX_TINT", material::MAX_TINT),
    ] {
        let wanted = format!("static const float {name} = {value:.2};");
        assert!(
            source.contains(&wanted),
            "shaders/patch.slang has no line \"{wanted}\" -- the material rule \
             has diverged from `engine::material`"
        );
    }
}

/// The depth of the colour pyramid does not repaint the slope.
///
/// This is the check named for T4 in advance: the colour has to be a function
/// of position on the body and of that alone, so re-cooking the asset with a
/// different number of levels has no right to change the frame. The ramp is
/// linear, so both pyramids describe **the same surface** -- the coarser one
/// simply on a sparser grid, and sampling between its nodes is linear and
/// exact.
///
/// The mistake this catches is concrete and was verified by breaking it: the
/// wavelength of the coarsest octave taken from `Terrain::step_m` instead of
/// from the body radius. It would look flawless on any single asset -- the
/// test fails by 30 units of brightness.
///
/// WARNING: what it does **not** catch, and this is worth knowing: a factor of
/// `window_step` inside the rule. At this altitude the patch is deeper than
/// both pyramids, so that factor is 2^-10 in one and 2^-9 in the other; the
/// rule then does not diverge but vanishes, and what fails instead is the
/// neighbouring test about relief. An oracle on "sameness" is blind to
/// mistakes that suppress the signal in **both** branches of the comparison.
#[test]
fn the_pyramid_depth_does_not_repaint_the_slope() {
    let Some(gpu) = gpu() else { return };

    let deep = take(&gpu, &ramp(LEVELS), NEAR_ALTITUDE);
    let shallow = take(&gpu, &ramp(OTHER_LEVELS), NEAR_ALTITUDE);

    let mut worst = 0.0f64;
    let mut sum = 0.0;
    let mut count = 0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (Some(a), Some(b)) = (lit(&deep, x, y), lit(&shallow, x, y)) else {
                continue;
            };
            worst = worst.max((a - b).abs());
            sum += (a - b).abs();
            count += 1;
        }
    }
    assert!(count > 50_000, "only {count} pixels were compared");
    let mean = sum / f64::from(count);
    println!("  {count} pixels: mean difference {mean:.3}, worst {worst:.1} units");

    // The bound is the quantum of the eight-bit scale in linear units; a rule
    // that read the pyramid step would give tens here.
    let quantum = byte_quantum(sum / f64::from(count) + 175.0);
    println!("  byte quantum at this brightness {quantum:.2}");
    assert!(
        worst <= 1.5 * quantum,
        "two pyramid depths gave different colours: up to {worst:.1} units at a \
         quantum of {quantum:.2}"
    );
}
