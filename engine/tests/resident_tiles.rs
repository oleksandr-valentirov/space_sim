//! The resident tile set: what is bound, and how often (ROADMAP.md, Y1b/Y1c).
//!
//! Debt D19 charged the frame for the **length** of the bindless array rather
//! than for what was drawn -- 26,616 views across two bodies, 1.05-1.28 ms
//! every frame even when a body covered six pixels. Y1b binds only the tiles
//! the frame reads; Y1c stops rebuilding that binding when the set has not
//! moved.
//!
//! Two claims, and the second is the one that is easy to get wrong.
//!
//! 1. **A still camera rebuilds the group once, ever.** This is a statement
//!    about work *not* done, so it is counted rather than timed: a timing that
//!    happens to be fast proves nothing about the next machine.
//! 2. **The memo never serves a stale group.** The cheapest way to break Y1c
//!    is to compare the wrong thing and keep a binding built for another
//!    camera -- which draws someone else's tiles, silently and plausibly. So
//!    the same camera is visited twice with a different one in between, and
//!    the two shots must be **bitwise** equal.
//!
//! A device without bindless skips these with a word, exactly as
//! `tests/terrain.rs` does and for the same reason: all three target backends
//! have it (PROJECT.md section 7).

use dem_cook::cook::build;
use dem_cook::Grid;
use engine::camera::Camera;
use engine::frame::Frame;
use engine::gpu::Gpu;
use engine::scene::{Body, Scene, TerrainId, TileSet};
use engine::shot::{self, Shot};
use engine::tiles::Terrain;
use std::path::Path;

const SIZE: u32 = 256;
const MOON_RADIUS_M: f64 = 1_737_400.0;
const LEVELS: u32 = 4;

fn gpu() -> Option<Gpu> {
    let gpu = Gpu::for_tests()?;
    if !gpu.bindless {
        eprintln!("SKIPPED: adapter without bindless ({})", gpu.describe());
        return None;
    }
    Some(gpu)
}

fn terrain() -> Terrain {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/lola/ldem_4.img");
    let grid = Grid::read(&path).expect("the LOLA grid should have read");
    build(&grid, LEVELS)
}

/// The Moon seen from `altitude`, off any axis of symmetry.
///
/// Not straight down at a face centre: that is the one direction where a wrong
/// distance to a patch still gives the right level, and two debts (D13, D14)
/// once lived there unseen. The rule out of it (CLAUDE.md) is that a check of
/// body geometry needs an asymmetric direction and a small altitude, and the
/// altitudes below supply the second half.
fn moon(altitude: f64, tiles: TileSet) -> Scene {
    let unit = {
        let v = [0.61, 0.44, 0.66];
        let n = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        v.map(|x| x / n)
    };
    let distance = MOON_RADIUS_M + altitude;
    let eye = unit.map(|v| v * distance);

    let mut scene = Scene::new(Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]));
    scene.bodies.push(Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: MOON_RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles,
        colour: engine::frame::COLOUR,
        air: None,
    });
    scene
}

/// One frame through a `Frame` that outlives the call.
///
/// `shot::take_scene` builds its own `Frame` per call, which is exactly what
/// cannot be used here: the counter being measured lives in the frame, and a
/// fresh frame would report one rebuild every time by construction.
fn draw(gpu: &Gpu, frame: &mut Frame, scene: &Scene) -> Shot {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("resident tiles"),
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

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("resident tiles"),
        });
    frame.draw(gpu, &mut encoder, &view, SIZE, SIZE, scene);
    shot::read_back(gpu, encoder, &texture, SIZE, SIZE).expect("the shot should have read back")
}

fn different(a: &Shot, b: &Shot) -> usize {
    let mut count = 0;
    for y in 0..a.height {
        for x in 0..a.width {
            if a.pixel(x, y) != b.pixel(x, y) {
                count += 1;
            }
        }
    }
    count
}

/// A still camera builds the binding once and then never again.
///
/// The number that matters is the second one: ten identical frames must add
/// **zero**. Were the count merely "small", the group would still be rebuilt
/// per frame and Y1c would have bought nothing -- and the frame time would not
/// say so loudly enough to notice on a fast machine.
#[test]
fn a_still_camera_builds_the_binding_once() {
    let Some(gpu) = gpu() else { return };

    let mut frame = Frame::new(&gpu, shot::FORMAT);
    let id = frame
        .load_surface(&gpu, &terrain(), None)
        .expect("the terrain should have loaded");
    let scene = moon(50.0e3, TileSet::Loaded(id));

    let _ = draw(&gpu, &mut frame, &scene);
    let after_first = frame.tile_rebuilds();
    assert_eq!(
        after_first, 1,
        "the first frame should build the binding exactly once, not {after_first} times"
    );

    for _ in 0..10 {
        let _ = draw(&gpu, &mut frame, &scene);
    }
    assert_eq!(
        frame.tile_rebuilds(),
        after_first,
        "ten identical frames rebuilt the binding {} times",
        frame.tile_rebuilds() - after_first
    );
}

/// Moving the camera rebuilds the binding, and coming back does too.
///
/// The point is not that the count grows -- it is that it grows *by the
/// change*. A binding held across a camera move would draw the old camera's
/// tiles, which is why the shot is compared as well: the same camera visited
/// twice, with another in between, must give the same pixels down to the byte.
#[test]
fn the_binding_follows_the_set_and_never_lags_behind_it() {
    let Some(gpu) = gpu() else { return };

    let mut frame = Frame::new(&gpu, shot::FORMAT);
    let id = frame
        .load_surface(&gpu, &terrain(), None)
        .expect("the terrain should have loaded");

    // Two altitudes far enough apart that the level criterion answers
    // differently -- otherwise the test would be about one set visited twice.
    let near = moon(20.0e3, TileSet::Loaded(id));
    let far = moon(5.0e6, TileSet::Loaded(id));

    let first = draw(&gpu, &mut frame, &near);
    let after_near = frame.tile_rebuilds();

    let _ = draw(&gpu, &mut frame, &far);
    assert!(
        frame.tile_rebuilds() > after_near,
        "a camera that moved five thousand kilometres did not change the set"
    );

    let second = draw(&gpu, &mut frame, &near);
    assert_eq!(
        different(&first, &second),
        0,
        "the same camera gave a different picture after a detour -- the binding \
         was kept when the set had moved"
    );
}

/// A deeper pyramid does not enlarge what the frame binds (Y1e).
///
/// This is the claim the rest of stage Y stands on, and it is the converse of
/// the measurement that opened debt D19. T8 varied exactly this -- pyramid
/// depth from 1 to 6 levels, 6 to 8190 views -- at a camera 1e9 m away, and
/// the frame time rose linearly with the count at about 51 ns per texture,
/// even though the body covered a handful of pixels. If that line is now flat,
/// clouds and night lights can each bring a pyramid without bringing a
/// millisecond.
///
/// Counted rather than timed, deliberately. A frame time proves the claim on
/// one machine on one afternoon; the number of views bound is what the driver
/// is actually charging for, and it is the same everywhere.
///
/// The camera is far enough that the level criterion stops at the coarsest
/// level whatever the pyramid holds -- which is the point. A near camera would
/// legitimately read more tiles from a deeper pyramid, and then the test would
/// be measuring the criterion instead of the binding.
#[test]
fn a_deeper_pyramid_does_not_enlarge_what_is_bound() {
    let Some(gpu) = gpu() else { return };

    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/lola/ldem_4.img");
    let grid = Grid::read(&path).expect("the LOLA grid should have read");

    let mut bound = Vec::new();
    let mut declared = Vec::new();
    for levels in [2u32, 3, 4] {
        let terrain = build(&grid, levels);
        declared.push(Terrain::count(levels));

        let mut frame = Frame::new(&gpu, shot::FORMAT);
        let id = frame
            .load_surface(&gpu, &terrain, None)
            .expect("the terrain should have loaded");
        let _ = draw(&gpu, &mut frame, &moon(1.0e9, TileSet::Loaded(id)));
        bound.push(frame.resident_tiles()[0]);
    }

    assert!(
        declared[2] > declared[0] * 4,
        "the pyramids did not actually differ in depth: {declared:?}"
    );
    assert_eq!(
        bound[0], bound[1],
        "a level of pyramid depth changed what is bound: {bound:?} for {declared:?} declared"
    );
    assert_eq!(
        bound[1], bound[2],
        "a level of pyramid depth changed what is bound: {bound:?} for {declared:?} declared"
    );

    // Without this the test would pass on a build that binds everything and
    // simply declares the same everywhere -- constant is not the claim, small
    // is. Before Y1b the deepest row bound all 510 views; it now binds the
    // handful the coarsest level asks for.
    assert!(
        bound[2] * 10 < declared[2],
        "{} views bound out of {} declared -- that is not a resident set",
        bound[2],
        declared[2]
    );
}

/// A body whose terrain is swapped rebuilds the binding, even standing still.
///
/// The set of tile *indices* is unchanged by such a swap -- the same patches,
/// the same levels, the same numbers -- so a memo keyed on indices alone would
/// happily keep pointing at the old pyramid's textures. Hence the terrain id
/// is part of the key, and hence this test.
#[test]
fn pointing_a_body_at_another_surface_rebuilds_the_binding() {
    let Some(gpu) = gpu() else { return };

    let mut frame = Frame::new(&gpu, shot::FORMAT);
    let real = frame
        .load_surface(&gpu, &terrain(), None)
        .expect("the terrain should have loaded");

    // The same pyramid depth, all zeroes: a surface that is exactly the
    // reference sphere. Same indices, different textures -- which is the whole
    // point.
    let flat = {
        let grids =
            vec![vec![0i16; engine::tiles::STORED * engine::tiles::STORED]; Terrain::count(LEVELS)];
        Terrain::build(LEVELS, MOON_RADIUS_M, 1.0, engine::tiles::NO_SEA, &grids)
    };
    let smooth = frame
        .load_surface(&gpu, &flat, None)
        .expect("the flat terrain should have loaded");

    let rough_shot = draw(&gpu, &mut frame, &moon(50.0e3, TileSet::Loaded(real)));
    let after_rough = frame.tile_rebuilds();

    let flat_shot = draw(&gpu, &mut frame, &moon(50.0e3, TileSet::Loaded(smooth)));
    assert!(
        frame.tile_rebuilds() > after_rough,
        "swapping the surface under a still camera did not rebuild the binding"
    );
    assert!(
        different(&rough_shot, &flat_shot) > 0,
        "the two surfaces drew the same picture, so the swap did not reach the GPU"
    );

    // `TerrainId` is used rather than ignored -- a warning here would mean the
    // handles were never distinct.
    assert_ne!(real, TerrainId(usize::MAX));
}
