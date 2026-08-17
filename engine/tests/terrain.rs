//! A tile in the frame through bindless (ROADMAP-PLANETS.md, R5c).
//!
//! Three claims, and each catches its own mistake.
//!
//! 1. **The height reaches the vertex.** A frame with terrain and a frame
//!    without it differ -- otherwise the tile was read, uploaded and ignored.
//! 2. **Array indexing works.** An array of one element proves nothing: what
//!    is checked is that **different tiles give different pixels** -- by
//!    rotating the camera about the light axis, which leaves a smooth sphere
//!    the same and terrain not.
//! 3. **The terrain shows as shadow.** A shot on the terminator: where the sun
//!    grazes the surface, a slope is either lit or it is not. What is measured
//!    is the sharpness of the brightness steps (total variation), not the
//!    number of shades: on the terminator most of the terrain goes into
//!    shadow, so there are **fewer** shades there and far more steps between
//!    them.
//!
//! A device without bindless skips these tests with an explanation. This is
//! not a silent skip: all three targets of the project have bindless
//! (PROJECT.md section 7), so its absence means a backend that is not a target
//! anyway.

use dem_cook::cook::build;
use dem_cook::Grid;
use engine::camera::Camera;
use engine::cubesphere::{Patch, FACES, SIDE};
use engine::frame::{self, Frame};
use engine::gpu::Gpu;
use engine::lod;
use engine::scene::{Body, Scene, TerrainId, TileSet};
use engine::shot::{self, Shot};
use engine::tiles::{self, Terrain, HALO, NODES, STORED};
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
    let grid = Grid::read(&path).expect("the LOLA grid should have been read");
    build(&grid, LEVELS)
}

/// The Moon in frame: the camera at altitude `altitude` above direction
/// `direction`.
fn moon(direction: [f64; 3], altitude: f64, tiles: TileSet) -> Scene {
    let length = (direction.iter().map(|v| v * v).sum::<f64>()).sqrt();
    let unit = direction.map(|v| v / length);
    let distance = MOON_RADIUS_M + altitude;
    let eye = unit.map(|v| v * distance);

    // The frame's vertical is the **light axis**, not the world z. That is
    // what makes a rotation about the light a symmetry of the frame: the whole
    // configuration turns along with the eye, so a smooth sphere has to give
    // the same picture. With a fixed z it did not -- and that is exactly why
    // the first version of this test caught nothing.
    let mut scene = Scene::new(Camera::look_at(eye, [0.0, 0.0, 0.0], light()));
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

/// The direction to a point at a latitude and longitude in degrees.
fn towards(lat: f64, lon: f64) -> [f64; 3] {
    let (a, b) = (lat.to_radians(), lon.to_radians());
    [a.cos() * b.cos(), a.cos() * b.sin(), a.sin()]
}

/// The unit direction to the light source -- the same one as in the frame.
fn light() -> [f64; 3] {
    let l = frame::LIGHT_DIR.map(f64::from);
    let n = l.iter().map(|v| v * v).sum::<f64>().sqrt();
    l.map(|v| v / n)
}

/// A direction at angle `tilt` to the light, rotated about it by `turn`.
///
/// The key property: **a rotation about the light axis leaves the
/// illumination unchanged**. A smooth sphere then gives the bitwise same
/// picture -- the configuration "light, eye, vertical" maps onto itself.
/// Terrain does not, because it is not symmetric. That is exactly what the
/// array-indexing check rests on.
fn around_light(tilt: f64, turn: f64) -> [f64; 3] {
    let l = light();
    // Any vector not parallel to `l` gives the first basis vector.
    let seed = if l[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let unit = |v: [f64; 3]| {
        let n = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        v.map(|x| x / n)
    };
    let e1 = unit(cross(l, seed));
    let e2 = cross(l, e1);

    let (c, s) = (tilt.to_radians().cos(), tilt.to_radians().sin());
    let (ct, st) = (turn.to_radians().cos(), turn.to_radians().sin());
    [0, 1, 2].map(|k| c * l[k] + s * (ct * e1[k] + st * e2[k]))
}

/// Terrain made of pure zeroes: the surface is exactly a sphere of radius
/// `reference_m`.
fn flat_terrain() -> Terrain {
    let grids = vec![vec![0i16; STORED * STORED]; Terrain::count(LEVELS)];
    Terrain::build(LEVELS, MOON_RADIUS_M, 1.0, tiles::NO_SEA, &grids)
}

/// How many pixels differ between two shots.
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

/// A shot of the scene with terrain and without it, from the same camera.
fn pair(gpu: &Gpu, direction: [f64; 3], altitude: f64) -> (Shot, Shot) {
    pair_of(gpu, &terrain(), direction, altitude)
}

/// The same, but with terrain given up front -- so that a **flat** one can be
/// passed in.
fn pair_of(gpu: &Gpu, relief: &Terrain, direction: [f64; 3], altitude: f64) -> (Shot, Shot) {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("terrain shot"),
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

    // One `Frame` for both shots: the terrain is loaded into it, and that is
    // exactly why a "loaded but not drawn" gap is impossible here.
    let mut frame = Frame::new(gpu, shot::FORMAT);
    let id = frame
        .load_terrain(gpu, relief)
        .expect("the terrain should have loaded");

    let mut take = |tiles: TileSet| {
        let scene = moon(direction, altitude, tiles);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("shot"),
            });
        frame.draw(gpu, &mut encoder, &view, SIZE, SIZE, &scene);
        shot::read_back(gpu, encoder, &texture, SIZE, SIZE).expect("the frame should have drawn")
    };

    let with = take(TileSet::Loaded(id));
    let without = take(TileSet::Smooth);
    (with, without)
}

/// The height reaches the vertex: a frame with terrain is not the same as one
/// without.
#[test]
fn the_terrain_changes_the_frame() {
    let Some(gpu) = gpu() else { return };
    // The lit side, and that is no detail: on the night side the illumination
    // is constant (`shade = 0.05`), so terrain is invisible there with a tile
    // and without one. The first version of this test stood over the Aitken
    // basin -- and that lies exactly opposite the light.
    let (with, without) = pair(&gpu, around_light(35.0, 0.0), 5.0e4);

    let moved = different(&with, &without);
    let all = (SIZE * SIZE) as usize;
    println!("  lit side from 50 km: {moved} differing pixels out of {all}");
    assert!(
        moved > all / 20,
        "the terrain changed only {moved} pixels out of {all} -- the tile \
         either never reached the vertex shader or reached it as zeroes"
    );
}

/// Different tiles give different pixels -- otherwise array indexing is worth
/// nothing.
///
/// The oracle is **a symmetry that only terrain breaks**. Rotating the camera
/// about the light axis leaves the illumination unchanged, so a smooth sphere
/// gives the same picture from two such positions: symmetric surface,
/// symmetric light. Terrain has no such symmetry, and its pictures diverge.
///
/// An implementation in which every patch reads one tile is symmetric too --
/// it would give identical frames just as a smooth sphere does. So the claim
/// here is not "something changed" but "it changed exactly where terrain
/// breaks the symmetry".
#[test]
fn different_tiles_give_different_pixels() {
    let Some(gpu) = gpu() else { return };

    let altitude = 2.0e5;
    let (with_a, without_a) = pair(&gpu, around_light(35.0, 0.0), altitude);
    let (with_b, without_b) = pair(&gpu, around_light(35.0, 120.0), altitude);

    let smooth_moved = different(&without_a, &without_b);
    let terrain_moved = different(&with_a, &with_b);
    let all = (SIZE * SIZE) as usize;

    println!(
        "  120 deg turn about the light: smooth sphere {smooth_moved} \
         differing pixels, terrain {terrain_moved} out of {all}"
    );

    // The smooth sphere has to stay nearly the same. Not bitwise: the camera
    // basis is computed through cross products, and a rotation gives different
    // last bits. Measured -- 808 pixels out of 65536, i.e. 1.2%; the 2%
    // threshold is what pins that down.
    assert!(
        smooth_moved < all / 50,
        "the smooth sphere changed by {smooth_moved} pixels under a rotation \
         about the light -- then the symmetry this test rests on does not hold"
    );
    assert!(
        terrain_moved > all / 10,
        "the terrain changed by only {terrain_moved} pixels out of {all} under \
         a rotation about the light -- every patch reads the same tile"
    );
}

/// The terrain shows as shadow: on the terminator the slopes scatter the
/// illumination.
///
/// Measured by **the number of distinct shades**. On a smooth sphere the
/// illumination is a smooth function of the normal, i.e. a few gentle
/// gradations; on terrain every facet has its own slope, and the shade count
/// jumps. This is the same claim as "shows as shadow", but as a number rather
/// than by eye.
#[test]
fn on_the_terminator_the_relief_shows_as_shade() {
    let Some(gpu) = gpu() else { return };

    // The terminator is the direction at 90 deg to the light: there the sun
    // falls exactly along the surface, and the slightest slope decides whether
    // a hillside is lit or not.
    let (with, without) = pair(&gpu, around_light(70.0, 0.0), 1.2e6);

    // What is measured is **sharpness**, not the number of shades. A smooth
    // sphere has a gentle gradient: neighbouring pixels differ by one or two.
    // On terrain every facet has its own slope, and brightness jumps at its
    // edge. A shade count is no good here at all -- on the terminator most of
    // the terrain goes into shadow and merges into one level, i.e. there are
    // FEWER shades.
    let sharp = |shot: &Shot| {
        let mut count = 0usize;
        // Total variation of brightness along the rows: the sum of the
        // absolute steps between neighbouring pixels.
        for y in 0..shot.height {
            for x in 1..shot.width {
                let (a, b) = (shot.pixel(x - 1, y), shot.pixel(x, y));
                if [a[0], a[1], a[2]] == frame::CLEAR_BYTES
                    || [b[0], b[1], b[2]] == frame::CLEAR_BYTES
                {
                    continue;
                }
                count += usize::from(a[2].abs_diff(b[2]));
            }
        }
        count
    };

    let rough = sharp(&with);
    let smooth = sharp(&without);
    println!(
        "  total brightness variation on the terminator: terrain {rough}, \
         smooth sphere {smooth}"
    );

    // The shots go to disk: when this eventually turns red, there will be
    // something to look at.
    let out = Path::new("build/r5c");
    let _ = with.write_png(&out.join("terminator_terrain.png"));
    let _ = without.write_png(&out.join("terminator_smooth.png"));

    // WARNING: the threshold is **1.7, not 3**, and that is not a weakening of
    // the test but a correction of what it measured (T7f). Before the normal
    // was fixed the number was 39179 against 4745, i.e. x8.25, and almost all
    // of it came from something other than the terrain: the fragment normal
    // was the triangle facet **together with the sphere's curvature**, so the
    // Moon came out a faceted ball, and it was the facets that gave that
    // variation. With a clean normal 9100 against 4745 is left -- that is the
    // terrain's slope and nothing else.
    //
    // What now guards the faceting artefact instead of this threshold is
    // `a_flat_tileset_shades_like_a_smooth_sphere`: at zero heights the
    // difference has to be identically zero, and a faceted ball gives it away
    // immediately.
    assert!(
        rough * 10 > smooth * 17,
        "the terrain gave variation {rough} against {smooth} for the smooth \
         one -- no shadows from slope are visible"
    );
}

/// Zero heights are drawn exactly as a smooth sphere is -- byte for byte.
///
/// An oracle on the **normal**, and it was put in for a mistake it would have
/// caught itself (T7f). The fragment normal was taken from the screen-space
/// derivatives of the world position, i.e. it was the **triangle's** normal.
/// Into it went not only the terrain's slope but also the break of the
/// sphere's own tessellation -- and from afar, when a grid cell is larger than
/// the terrain under it, it was that break that stayed in the frame: the Earth
/// from 1e6 m came out as 135-pixel tiles with a 3% brightness jump, and the
/// Moon as a faceted ball.
///
/// Here this is easiest to see: if the heights are zero everywhere, then the
/// surface **is** the sphere, and the two pipelines are obliged to draw the
/// same frame. Any difference in it is exactly what the normal invented on its
/// own.
///
/// WARNING: the main tolerance here is **on the depth of the divergence, not
/// on its count**: no pixel may differ by more than one byte in a channel. The
/// faceted ball gave 3% (seven bytes), i.e. it failed precisely this condition
/// rather than the counter. One byte is legitimate, and the reason is exact:
/// `lift` comes out an exact zero, so the geometry is bitwise identical in
/// both branches, but the terrain one normalises the vector twice
/// (`normalize(sphere + 0)`, and then once more in `shade`) while the smooth
/// one does it once. Double normalisation in `f32` is not an identity, and one
/// ULP on a brightness slope comes out as one byte.
#[test]
fn a_flat_tileset_shades_like_a_smooth_sphere() {
    let Some(gpu) = gpu() else { return };

    // The same angle as in the terminator test: there the sphere's curvature
    // in frame is greatest, i.e. the faceting artefact is most visible.
    let (flat, sphere) = pair_of(&gpu, &flat_terrain(), around_light(70.0, 0.0), 1.2e6);

    let mut count = 0usize;
    let mut worst = 0u8;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (a, b) = (flat.pixel(x, y), sphere.pixel(x, y));
            if a != b {
                count += 1;
                for k in 0..3 {
                    worst = worst.max(a[k].abs_diff(b[k]));
                }
            }
        }
    }
    println!(
        "  differing pixels: {count} out of {}, deepest divergence {worst} bytes",
        SIZE * SIZE
    );

    let out = Path::new("build/t7f");
    let _ = flat.write_png(&out.join("flat_tiles.png"));
    let _ = sphere.write_png(&out.join("smooth_sphere.png"));

    assert!(
        worst <= 1,
        "zero terrain diverged from the sphere by {worst} bytes: the normal \
         knows about the tessellation"
    );
    assert!(
        count * 100 < (SIZE * SIZE) as usize,
        "{count} pixels diverged -- that is no longer rounding"
    );
}

// ---------------------------------------------------------------------------
// A subrectangle of someone else's tile (R7a, the GPU half)

/// How many levels a pyramid has that the patches find **insufficient**.
const SHALLOW: u32 = 2;
/// How many levels a pyramid has in which every patch has its **own** tile.
const DEEP: u32 = 4;
/// Metres per storage unit. One metre: then storage units and metres are the
/// same number, and the integrality check below reads without conversion.
const UNIT_M: f32 = 1.0;
/// The frame side of this check -- larger than the shared [`SIZE`], and
/// deliberately so.
///
/// The level is chosen by screen-space error, so the depth of the set is
/// bought either by a low altitude or by a tall frame. A low one would cost a
/// camera inside the terrain (a range of +-1 km), a tall frame costs nothing
/// but the read-back. A thousand and twenty-four pixels give level 3 from
/// twenty kilometres -- i.e. **two** steps below the pyramid rather than one:
/// the modulo in the window offset is checked where it no longer reduces to
/// `i % 2`.
const SUBRECT_SIZE: u32 = 1024;

/// The node height is a **steep ramp** in fractions of a face:
/// `4096*(x + y)` units.
///
/// Three requirements collide here, and the ramp is the only shape satisfying
/// all three.
///
/// 1. **Bilinear sampling must reproduce the field exactly**, otherwise the
///    deep tile would store rounding and the bitwise equality would break for
///    a reason other than the one the test looks for. A linear function is
///    reproduced exactly.
/// 2. **The slope must be the same over any difference base.** Ever since
///    terrain entered the level choice (R7c), two pyramids of different depth
///    give different sets of patches -- and then comparing frames makes no
///    sense. For a linear field the gradient does not depend on the base at
///    all, and every multiplier here is a power of two, so the equality comes
///    out bitwise rather than "almost".
/// 3. **The lighting must see the terrain.** The first version of this ramp
///    gave 512 m per quarter circle, a slope of 2e-4, and changed **not one**
///    pixel against a smooth sphere. Here the range is 8192 m and the slope
///    2.1e-3 -- more than an order larger, and the frame changes visibly.
///
/// The multiplier `128 >> level` keeps the values integral at every level up
/// to the seventh and within `i16` (ceiling 8192).
fn ramp_units(level: u32, u: i64, v: i64) -> i16 {
    ((u + v) * i64::from(128u32 >> level)) as i16
}

/// The shallow pyramid: its own data on both levels, nothing deeper.
///
/// The halo here is a continuation of the same ramp rather than poison: ever
/// since slope entered the level choice, the edge nodes of a patch read the
/// halo **through `slope_at`**, and `i16::MIN` beyond the edge would give an
/// infinite slope and a division up to the ceiling. That is the "loud break"
/// the poison used to be there for.
fn shallow_relief() -> Terrain {
    let mut grids = Vec::with_capacity(Terrain::count(SHALLOW));
    for level in 0..SHALLOW {
        let side = 1u32 << level;
        for _face in 0..FACES {
            for i in 0..side {
                for j in 0..side {
                    let mut grid = vec![0i16; STORED * STORED];
                    for a in 0..STORED {
                        for b in 0..STORED {
                            let u = i64::from(i) * SIDE as i64 + a as i64 - HALO as i64;
                            let v = i64::from(j) * SIDE as i64 + b as i64 - HALO as i64;
                            grid[a * STORED + b] = ramp_units(level, u, v);
                        }
                    }
                    grids.push(grid);
                }
            }
        }
    }
    Terrain::build(SHALLOW, MOON_RADIUS_M, UNIT_M, tiles::NO_SEA, &grids)
}

/// A deep pyramid of **the same field**: each of its tiles is what
/// [`Terrain::height_m`] reads out of the shallow one for the same patch.
///
/// So the question the test asks is this: will the GPU read out of the shallow
/// pyramid the same thing the CPU already put into the deep one. Levels 0 and
/// 1 come out a literal copy (there `height_m` takes the node exactly), levels
/// 2 and 3 a bilinear subrectangle of the ancestor.
fn deep_relief(shallow: &Terrain) -> Terrain {
    let mut grids = Vec::with_capacity(Terrain::count(DEEP));
    for level in 0..DEEP {
        let side = 1u32 << level;
        for face in 0..FACES {
            for i in 0..side {
                for j in 0..side {
                    let patch = Patch { face, level, i, j };
                    // The halo comes from the same ramp analytically:
                    // `height_m` deliberately never goes beyond the edge of
                    // the grid, but `slope_at` reads there, and without it the
                    // slope at the patch edge would go mad.
                    let mut grid = vec![0i16; STORED * STORED];
                    for a in 0..STORED {
                        for b in 0..STORED {
                            let u = i64::from(i) * SIDE as i64 + a as i64 - HALO as i64;
                            let v = i64::from(j) * SIDE as i64 + b as i64 - HALO as i64;
                            grid[a * STORED + b] = ramp_units(level, u, v);
                        }
                    }
                    for a in 0..NODES {
                        for b in 0..NODES {
                            let value = shallow.height_m(&patch, a, b) / f64::from(UNIT_M);
                            // A guard on the construction of the fixture
                            // itself: if the height step ever stops dividing
                            // into the weights, the tile will start rounding
                            // and the bitwise equality below will break for an
                            // entirely different reason.
                            assert_eq!(
                                value,
                                value.round(),
                                "{patch:?} node ({a}, {b}): {value} is not an \
                                 integer -- the ramp multiplier does not cover \
                                 the sampling weights"
                            );
                            grid[(a + HALO) * STORED + b + HALO] = value as i16;
                        }
                    }
                    grids.push(grid);
                }
            }
        }
    }
    Terrain::build(DEEP, MOON_RADIUS_M, UNIT_M, tiles::NO_SEA, &grids)
}

/// **A patch deeper than the pyramid draws the same surface as a patch with a
/// tile of its own** (R7a).
///
/// This is the half of the R7a oracle that could not be written at the time of
/// the step: LOD did not descend below level zero, so patches deeper than the
/// pyramid never arose in the frame at all. Debt D13 closed that, and the
/// check became possible.
///
/// **The claim is the bitwise equality of two frames** taken from one camera
/// over two pyramids of **one height field**: the shallow one, where the patch
/// reads a subrectangle of its ancestor, and the deep one, where the same
/// patch has its own tile filled with what `Terrain::height_m` read out of the
/// shallow one. So the GPU is checked not against a second copy of the formula
/// but against the very CPU function the shader is declared to be a twin of.
/// There is no rounding along that path at all ([`UNIT_M`] is one metre, and
/// the fixture asserts integrality node by node), so no tolerance is needed.
///
/// The mistake this catches is exactly the one the step was made for: a patch
/// reading its ancestor's tile with **its own** local coordinates would
/// stretch the whole ancestor tile over itself, i.e. repeat the terrain in
/// every patch and tear it at every boundary. No tolerance is needed here --
/// such a mistake changes the frame entirely.
///
/// The third shot, the smooth one, stands against the opposite substitution:
/// two frames in which the height never reached the vertex at all are bitwise
/// equal too.
///
/// **There are four cameras, and that is not slack.** From twenty kilometres a
/// cap of a few degrees is visible, i.e. five patches out of forty get drawn
/// -- and whether a patch with an **asymmetric** window (`origin.x !=
/// origin.y`) is among them is down to chance. Measured on a single camera:
/// swapping `origin.x` and `origin.y` in the shader changed **not one** pixel,
/// even though three of the five drawn patches had different window
/// coordinates. One camera simply does not see half the addressing mistakes.
#[test]
fn a_patch_deeper_than_the_pyramid_draws_the_same_surface() {
    let Some(gpu) = gpu() else { return };

    // Twenty kilometres: at this altitude the set goes down to level 3 with a
    // 1024 px frame -- i.e. deeper than the shallow pyramid and finer than the
    // deep one. The altitude was not picked by eye: both bounds are checked
    // below, and the test turns red if the error criterion ever moves.
    let altitude = 2.0e4;

    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("subrect shot"),
        size: wgpu::Extent3d {
            width: SUBRECT_SIZE,
            height: SUBRECT_SIZE,
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

    // One frame for all three shots: both pyramids live in it at once, so
    // between the shots nothing changes at all except the handle.
    let mut frame = Frame::new(&gpu, shot::FORMAT);
    let field = shallow_relief();
    let shallow = frame
        .load_terrain(&gpu, &field)
        .expect("the shallow pyramid should have loaded");
    let deep = frame
        .load_terrain(&gpu, &deep_relief(&field))
        .expect("the deep pyramid should have loaded");

    let mut take = |direction: [f64; 3], tiles: TileSet| {
        let scene = moon(direction, altitude, tiles);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("subrect"),
            });
        frame.draw(
            &gpu,
            &mut encoder,
            &view,
            SUBRECT_SIZE,
            SUBRECT_SIZE,
            &scene,
        );
        shot::read_back(&gpu, encoder, &texture, SUBRECT_SIZE, SUBRECT_SIZE)
            .expect("the frame should have drawn")
    };

    let all = (SUBRECT_SIZE * SUBRECT_SIZE) as usize;
    let mut asymmetric = 0;

    for turn in [0.0, 90.0, 180.0, 270.0] {
        let direction = around_light(35.0, turn);
        let scene = moon(direction, altitude, TileSet::Smooth);

        // A check that the check is not empty. Without it the test would stay
        // green on a set made of bare faces -- i.e. in exactly the state it
        // was in before D13. It also counts the patches with an asymmetric
        // window: without a single one of those, equality of frames would say
        // nothing about the coordinates themselves.
        let selection = lod::select(
            &lod::Body::still([0.0, 0.0, 0.0], MOON_RADIUS_M),
            &scene.camera,
            lod::focal_px(frame::FOV_Y, f64::from(SUBRECT_SIZE)),
            None,
        );
        let deepest = selection
            .patches
            .iter()
            .map(|p| p.level)
            .max()
            .expect("the set is never empty");
        asymmetric += selection
            .patches
            .iter()
            .filter(|p| {
                let (_, origin, _) = field.window(p);
                origin[0] != origin[1]
            })
            .count();
        assert!(
            deepest >= SHALLOW,
            "turn {turn} deg: deepest level {deepest} -- no patch goes beyond \
             the shallow pyramid, i.e. the subrectangle is never read anywhere"
        );
        assert!(
            deepest < DEEP,
            "turn {turn} deg: deepest level {deepest} -- the deep pyramid does \
             not cover it either, and there is nothing to compare against"
        );

        let from_parent = take(direction, TileSet::Loaded(shallow));
        let from_own = take(direction, TileSet::Loaded(deep));
        let smooth = take(direction, TileSet::Smooth);

        let moved = different(&from_parent, &smooth);
        let apart = different(&from_parent, &from_own);
        println!(
            "  turn {turn} deg: {} patches down to level {deepest}, against the \
             smooth one {moved} differing out of {all}, against its own tile {apart}",
            selection.patches.len()
        );
        assert!(
            moved > all / 20,
            "turn {turn} deg: the terrain changed only {moved} pixels out of \
             {all} -- the height never reached the vertex, and the equality \
             below would have meant nothing"
        );
        assert_eq!(
            apart, 0,
            "turn {turn} deg: a patch deeper than the pyramid drew a different \
             surface from a patch with its own tile -- the window in the \
             ancestor's tile is in the wrong place"
        );
    }

    println!("  patches with an asymmetric window over four cameras: {asymmetric}");
    assert!(
        asymmetric > 0,
        "not one camera gave a patch whose window offset differs along the \
         axes -- swapping the coordinates would have gone unnoticed"
    );
}

/// An empty terrain is an error, not a smooth planet.
///
/// An empty pyramid of a given depth is a fixture for **ceiling** checks.
///
/// Zeroes rather than LOLA terrain, and that is not laziness: the question
/// here is about the number of textures, not their contents, and seven levels
/// from the source would mean forty million samples in a test that reads none
/// of them.
fn flat(levels: u32) -> Terrain {
    let grids = vec![vec![0i16; STORED * STORED]; Terrain::count(levels)];
    Terrain::build(levels, MOON_RADIUS_M, 0.5, tiles::NO_SEA, &grids)
}

/// The deepest pyramid we actually cook fits into the array.
///
/// Six levels is 8190 tiles, i.e. the Moon's **colour** tileset (T2a), and the
/// array ceiling was raised for exactly that. Without this claim the ceiling
/// would stay a number somebody once raised: the refusal check below would
/// pass at a ceiling of 4096 and at 64 alike.
#[test]
fn the_deepest_pyramid_we_actually_cook_fits() {
    let Some(gpu) = gpu() else { return };
    let mut frame = Frame::new(&gpu, shot::FORMAT);

    let deep = flat(6);
    assert_eq!(Terrain::count(6), 8190);
    let loaded = frame.load_terrain(&gpu, &deep);
    assert!(loaded.is_ok(), "8190 tiles did not fit: {loaded:?}");
}

/// A handle that does not exist must not quietly turn into `Smooth`: a planet
/// without mountains and a planet whose asset failed to load look the same.
#[test]
fn a_terrain_that_does_not_fit_is_refused_out_loud() {
    let Some(gpu) = gpu() else { return };
    let mut frame = Frame::new(&gpu, shot::FORMAT);

    // A pyramid larger than the array ceiling: 7 levels is 32766 tiles.
    let refused = frame.load_terrain(&gpu, &flat(7));
    println!("  oversized pyramid: {refused:?}");
    assert!(
        refused.is_err(),
        "an oversized terrain was accepted silently"
    );

    // And a non-existent handle in the scene simply draws no terrain -- but
    // does not crash either.
    let scene = moon(towards(0.0, 0.0), 1.0e5, TileSet::Loaded(TerrainId(42)));
    let taken = shot::take_scene(&gpu, 64, 64, &scene);
    assert!(taken.is_ok(), "a foreign handle brought the frame down");
}
