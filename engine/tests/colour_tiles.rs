//! A colour tile in the frame through the second bindless array (stage T, step
//! T3b).
//!
//! Three statements, and each catches its own bug.
//!
//! 1. **The colour reaches the pixel.** A frame with the tileset and a frame
//!    without it differ -- otherwise the asset was read, uploaded and ignored.
//! 2. **The map lies the right way round.** The tileset here is a ramp **by
//!    latitude**, and it must land in the frame as horizontal bands: brightness
//!    changes top to bottom and barely changes left to right. This catches
//!    swapped tile axes (`a` is the row, not the column) and a wrong tile index
//!    -- both bugs leave the frame plausible but not striped.
//! 3. **There is no seam between tiles.** The ramp is continuous over the whole
//!    sphere, so there must be no jump in brightness between neighbouring pixels
//!    larger than the ramp's own step. A tile 33 nodes wide covers dozens of
//!    pixels, so a shift of half a node would give a visible line there.
//!
//! The relief in all three is **flat zeros**. The question here is about colour,
//! and mountains would add their own shadows to the brightness, i.e. make every
//! oracle impure.

use engine::camera::Camera;
use engine::cubesphere::{Patch, FACES};
use engine::frame::{self, Frame};
use engine::gpu::Gpu;
use engine::scene::{Body, Scene, TileSet};
use engine::shot::{self, Shot};
use engine::srgb;
use engine::tiles::{self, Colour, Terrain, NODES, STORED};

const SIZE: u32 = 256;
const MOON_RADIUS_M: f64 = 1_737_400.0;

/// Levels in the pyramids: fewer for height than for colour -- exactly as in the
/// real assets (5 against 6, T2a). The numbers are smaller here only so that the
/// test does not cook thousands of tiles.
const HEIGHT_LEVELS: u32 = 2;
const COLOUR_LEVELS: u32 = 3;

fn gpu() -> Option<Gpu> {
    let gpu = Gpu::for_tests()?;
    if !gpu.bindless {
        eprintln!("SKIPPED: an adapter without bindless ({})", gpu.describe());
        return None;
    }
    Some(gpu)
}

/// Flat relief: the test asks about colour, and mountains would only get in the
/// way.
fn flat() -> Terrain {
    let grids = vec![vec![0i16; STORED * STORED]; Terrain::count(HEIGHT_LEVELS)];
    Terrain::build(HEIGHT_LEVELS, MOON_RADIUS_M, 0.5, tiles::NO_SEA, &grids)
}

/// Colour as a function of **latitude**: from dark at the south pole to light at
/// the north.
///
/// A function of direction, not of tile indices, and that is exactly why it is
/// continuous over the whole sphere: neighbouring tiles take it at the same
/// point and so give the same byte. That is, the fixture has no seam of its own,
/// and any seam in the frame is the frame's.
fn latitude_ramp() -> Colour {
    let mut grids = Vec::with_capacity(tiles::count(COLOUR_LEVELS));
    for level in 0..COLOUR_LEVELS {
        let side = 1u32 << level;
        for face in 0..FACES {
            for i in 0..side {
                for j in 0..side {
                    let patch = Patch { face, level, i, j };
                    let mut tile = Vec::with_capacity(NODES * NODES);
                    for a in 0..NODES {
                        for b in 0..NODES {
                            let unit = patch.vertex(a, b, 1.0);
                            let z = unit[2] / (unit.iter().map(|v| v * v).sum::<f64>()).sqrt();
                            // 0.1 ... 0.9 from pole to pole: the ends of the
                            // scale stay free so that quantisation runs into
                            // neither zero nor 255.
                            tile.push((255.0 * (0.5 + 0.4 * z)) as u8);
                        }
                    }
                    grids.push(tile);
                }
            }
        }
    }
    Colour::build(COLOUR_LEVELS, 1, 0.25, false, &grids)
}

/// The Moon in frame, lit from the camera's side.
///
/// Light from the eye deliberately: the diffuse term then barely changes across
/// the disc, and a difference of brightness in the frame is a difference of
/// **colour** rather than of a cosine.
fn moon(tiles: TileSet, altitude: f64) -> Scene {
    let eye = [MOON_RADIUS_M + altitude, 0.0, 0.0];
    // The frame's vertical is world `+z`, i.e. north. That is exactly why a ramp
    // by latitude must land as horizontal bands.
    let mut scene = Scene::new(Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]));
    scene.sun = [1.0, 0.0, 0.0];
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

/// The brightness of a pixel, or `None` for empty sky.
fn lit(shot: &Shot, x: u32, y: u32) -> Option<f64> {
    let p = shot.pixel(x, y);
    if [p[0], p[1], p[2]] == frame::CLEAR_BYTES {
        return None;
    }
    Some((f64::from(p[0]) + f64::from(p[1]) + f64::from(p[2])) / 3.0)
}

/// A pair of screenshots from the same camera: with colour and without it.
fn pair(gpu: &Gpu, altitude: f64) -> (Shot, Shot) {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("colour shot"),
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
    let painted = frame
        .load_surface(gpu, &flat(), Some(&latitude_ramp()))
        .expect("the surface with colour should have loaded");
    let plain = frame
        .load_surface(gpu, &flat(), None)
        .expect("the surface without colour should have loaded");

    let mut take = |id| {
        let scene = moon(TileSet::Loaded(id), altitude);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("shot"),
            });
        frame.draw(gpu, &mut encoder, &view, SIZE, SIZE, &scene);
        shot::read_back(gpu, encoder, &texture, SIZE, SIZE).expect("the frame should have drawn")
    };

    (take(painted), take(plain))
}

/// The colour reaches the pixel, and a frame without it is different.
#[test]
fn the_colour_changes_the_frame() {
    let Some(gpu) = gpu() else { return };
    let (with, without) = pair(&gpu, 3.0e5);

    let mut changed = 0;
    let mut surface = 0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            if lit(&with, x, y).is_some() {
                surface += 1;
                if with.pixel(x, y) != without.pixel(x, y) {
                    changed += 1;
                }
            }
        }
    }
    println!("  {changed} of {surface} surface pixels changed");

    assert!(surface > 1000, "the disc is too small: {surface} pixels");
    assert!(
        changed * 10 > surface * 9,
        "the colour changed only {changed} of {surface} pixels"
    );
}

/// A ramp by latitude shows as horizontal bands, not vertical ones.
///
/// The oracle is the **ratio** of the vertical spread to the horizontal one. The
/// absolute values would say nothing here: they depend both on the colour scale
/// and on the diffuse term, while the ratio does not.
#[test]
fn a_latitude_ramp_shows_as_horizontal_bands() {
    let Some(gpu) = gpu() else { return };
    let (with, _) = pair(&gpu, 3.0e5);

    // The mean brightness of a row and of a column -- over those pixels where
    // there is surface.
    let mean = |along_row: bool, k: u32| {
        let mut sum = 0.0;
        let mut count = 0;
        for other in 0..SIZE {
            let (x, y) = if along_row { (other, k) } else { (k, other) };
            if let Some(value) = lit(&with, x, y) {
                sum += value;
                count += 1;
            }
        }
        (count > 20).then(|| sum / f64::from(count))
    };

    let rows: Vec<f64> = (0..SIZE).filter_map(|k| mean(true, k)).collect();
    let columns: Vec<f64> = (0..SIZE).filter_map(|k| mean(false, k)).collect();
    let spread = |values: &[f64]| {
        let lo = values.iter().cloned().fold(f64::MAX, f64::min);
        let hi = values.iter().cloned().fold(f64::MIN, f64::max);
        hi - lo
    };
    let (vertical, horizontal) = (spread(&rows), spread(&columns));
    println!("  spread across rows {vertical:.1}, across columns {horizontal:.1}");

    assert!(
        vertical > 4.0 * horizontal,
        "a ramp by latitude gave a spread of {vertical:.1} top to bottom and \
         {horizontal:.1} across -- the map lies the wrong way round"
    );
    // And the direction: north at the top of the frame, i.e. the upper rows are
    // lighter than the lower ones.
    assert!(
        rows[0] > rows[rows.len() - 1],
        "north came out darker than south: {:.1} against {:.1}",
        rows[0],
        rows[rows.len() - 1]
    );
}

/// There is no seam between tiles: neighbouring pixels do not jump.
///
/// The camera is low deliberately -- then the frame holds dozens of patches, i.e.
/// dozens of tile boundaries, and each of them crosses the disc. The threshold is
/// in units of brightness: the ramp changes by ~0.5 units per pixel at this
/// scale, so a jump of five units is not the ramp but a boundary.
#[test]
fn the_tile_boundaries_leave_no_seam() {
    let Some(gpu) = gpu() else { return };
    let (with, _) = pair(&gpu, 1.0e5);

    let mut worst = 0.0f64;
    let mut jumps = 0;
    let mut pairs = 0;
    let mut surface = 0;
    for y in 1..SIZE - 1 {
        for x in 1..SIZE - 1 {
            let Some(here) = lit(&with, x, y) else {
                continue;
            };
            surface += 1;
            // Only pixels whose neighbours are on the surface too: the edge of
            // the disc is a legitimate jump into the sky, and the test does not
            // ask about it.
            for (dx, dy) in [(1u32, 0u32), (0, 1)] {
                let Some(there) = lit(&with, x + dx, y + dy) else {
                    continue;
                };
                let jump = (here - there).abs();
                worst = worst.max(jump);
                pairs += 1;
                if jump > 5.0 {
                    jumps += 1;
                }
            }
        }
    }
    println!(
        "  {surface} surface pixels, largest jump {worst:.1} units, \
         {jumps} of {pairs} pairs"
    );

    // The disc must cover the frame: a check for "no jumps" on empty sky would
    // pass flawlessly and say nothing.
    assert!(
        surface * 10 > (SIZE * SIZE) as usize * 9,
        "only {surface} pixels of surface"
    );
    assert!(pairs > 5000, "only {pairs} pixel pairs were checked");
    assert_eq!(
        jumps, 0,
        "{jumps} jumps were found -- that is a seam between tiles"
    );
}

/// A constant mosaic: the same storage unit in every node.
fn plain(value: u8, scale: f32) -> Colour {
    let grids = vec![vec![value; NODES * NODES]; tiles::count(COLOUR_LEVELS)];
    Colour::build(COLOUR_LEVELS, 1, scale, false, &grids)
}

/// The pixel carries exactly the reflectance the mosaic measured (T5b).
///
/// This is the stage's most direct oracle and the first that became possible at
/// all: before T5b the shader held the stub `terrain.y = 1`, i.e. the frame drew
/// **storage units** rather than albedo, and asking about a physical number made
/// no sense. Now the multiplier is `Colour::scale`, and the whole chain is
/// checked by one equation.
///
/// The fixture clears everything but the albedo itself out of the way:
///
/// * the relief is flat, so the material rule gives exactly one;
/// * the mosaic is constant, so sampling and windows add nothing;
/// * the light is along the view and the body is far away -- at the centre of the
///   frame the normal looks both at the camera and at the light source, i.e. the
///   diffuse term is exactly one, and the shader's `0.05 + 0.95*cos` factors do
///   not enter the prediction at all.
///
/// What is left is `byte = srgb(unit * scale)` -- and that is the number the test
/// compares.
#[test]
fn the_pixel_carries_the_reflectance_the_mosaic_measured() {
    let Some(gpu) = gpu() else { return };

    // Three reflectances covering the Moon's range: a dark mare, a typical
    // highland, the bright ray of a fresh crater.
    for (value, scale) in [(45u8, 0.25f32), (160, 0.25), (255, 0.25)] {
        let colour = plain(value, scale);
        let expected_reflectance = colour.reflectance(0, 0, 0, 0);
        let expected = srgb::linear_to_byte(expected_reflectance);

        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("reflectance"),
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
        let id = frame
            .load_surface(&gpu, &flat(), Some(&colour))
            .expect("the surface should have loaded");
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("reflectance"),
            });
        let scene = moon(TileSet::Loaded(id), 3.0e5);
        frame.draw(&gpu, &mut encoder, &view, SIZE, SIZE, &scene);
        let shot = shot::read_back(&gpu, encoder, &texture, SIZE, SIZE).expect("a frame");

        let got = shot.pixel(SIZE / 2, SIZE / 2)[0];
        println!(
            "  unit {value} x {scale} = {expected_reflectance:.4}: expected byte \
             {expected}, in the frame {got}"
        );
        assert!(
            got.abs_diff(expected) <= 1,
            "a reflectance of {expected_reflectance:.4} should have given byte \
             {expected}, but the frame gave {got}"
        );
    }
}

/// A constant four-channel mosaic in sRGB -- what Earth carries (T7e).
fn plain_rgba(rgb: [u8; 3]) -> Colour {
    let mut tile = Vec::with_capacity(NODES * NODES * 4);
    for _ in 0..NODES * NODES {
        tile.extend_from_slice(&[rgb[0], rgb[1], rgb[2], u8::MAX]);
    }
    let grids = vec![tile; tiles::count(COLOUR_LEVELS)];
    Colour::build(COLOUR_LEVELS, 4, 1.0, true, &grids)
}

/// A four-channel tileset paints its own colour, not its first channel (T7g).
///
/// Three things are caught by one fixture, and each of them was impossible
/// before:
///
/// 1. **the channels are not swapped.** The values are deliberately different --
///    red smaller than blue, as in a real ocean. Swapping `r` and `b` gives the
///    same frame on any grey test and is visible only here;
/// 2. **sRGB is decoded exactly once.** The byte in the tile is encoded, the
///    hardware decodes it when reading the texel, and the target encodes it back.
///    So the frame must return **the same byte** that lies in the asset: a double
///    decode would give a noticeably darker pixel, none at all a lighter one;
/// 3. **the single-channel tileset did not break.** The branch on `terrain.z`
///    lives in the fragment stage, and the neighbouring tests above check its
///    other half -- the grey Moon stayed grey.
#[test]
fn a_four_channel_tileset_paints_its_own_colour() {
    let Some(gpu) = gpu() else { return };

    // BMNG ocean: dark, blue, with different channels.
    for rgb in [[5u8, 17, 43], [197, 155, 107]] {
        let colour = plain_rgba(rgb);

        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rgba tiles"),
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
        let id = frame
            .load_surface(&gpu, &flat(), Some(&colour))
            .expect("the surface should have loaded");
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("rgba tiles"),
            });
        let scene = moon(TileSet::Loaded(id), 3.0e5);
        frame.draw(&gpu, &mut encoder, &view, SIZE, SIZE, &scene);
        let shot = shot::read_back(&gpu, encoder, &texture, SIZE, SIZE).expect("a frame");

        let got = shot.pixel(SIZE / 2, SIZE / 2);
        println!(
            "  asset {rgb:?} -> frame [{}, {}, {}]",
            got[0], got[1], got[2]
        );
        for (channel, &in_frame) in got.iter().take(3).enumerate() {
            let expected = srgb::linear_to_byte(colour.reflectance(0, 0, 0, channel as u32));
            assert!(
                in_frame.abs_diff(expected) <= 1,
                "channel {channel}: expected {expected}, in the frame {in_frame}"
            );
        }
    }
}
