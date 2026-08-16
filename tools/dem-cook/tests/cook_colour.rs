//! Colour cooker: same input, same byte, same node (T2d).
//!
//! The oracles take the same shape as the height cooker's (`cook.rs`), but
//! **three of the four need no source**: `Albedo` is plain fields, so a grid
//! can be built by hand. That is deliberate rather than convenient. The WAC
//! mosaic is not in git (Q5), and a cooker watched only by checks that skip
//! without it would be watched nowhere but on one machine.
//!
//! 1. **stability** -- two runs give byte-for-byte the same;
//! 2. **two paths, one number** -- the colour in the tile and the colour read
//!    from the source by latitude and longitude agree. The paths really do
//!    differ: the cooker goes through `Patch::vertex` and `sample_direction`,
//!    the test through an explicit translation of a direction into angles;
//! 3. **no seam** -- a node on two patches' shared edge carries the same byte
//!    in both tiles. This is the property the terrain rests on (R2b, R7b), and
//!    colour needs it no less: a one-byte difference on an edge is a visible
//!    line;
//! 4. **the scale did not eat the contrast** -- the only claim needing the
//!    real mosaic, and the only one that asks about the choice of `SCALE` at
//!    all: the previous three would pass with a scale where mare and highland
//!    differ by five units out of 255.

use dem_cook::albedo::Albedo;
use dem_cook::cook::{build_colour, source_for, SCALE};
use engine::cubesphere::{Patch, FACES, SIDE};
use engine::tiles;

const LEVELS: u32 = 3;

/// A grid in which reflectance is a smooth function of position.
///
/// Smooth on purpose: a step would give the same bytes on both sides of an
/// edge simply because everything there is equal, and the third oracle would
/// become vacuous. The period is chosen so that several waves fall on one cube
/// face -- then neighbouring nodes really do differ.
fn painted() -> Albedo {
    let (samples, lines) = (720usize, 360usize);
    let per_degree = 2.0;
    let mut raw = Vec::with_capacity(samples * lines);
    for line in 0..lines {
        for sample in 0..samples {
            let lat = 90.0 - (line as f64 + 0.5) / per_degree;
            let lon = (sample as f64 + 0.5) / per_degree;
            let radians = std::f64::consts::PI / 180.0;
            // The 0.02 to 0.18 range is where the real mosaic lives, so the
            // quantisation here is as coarse as in the shipping asset.
            let wave = (3.0 * lon * radians).sin() * (2.0 * lat * radians).cos();
            raw.push((0.1 + 0.08 * wave) as f32);
        }
    }
    Albedo {
        samples,
        lines,
        per_degree,
        raw,
    }
}

/// Two cooker runs give byte-for-byte the same.
#[test]
fn cooking_twice_gives_the_same_bytes() {
    let map = painted();
    let (first, saturated) = build_colour(&map, LEVELS);
    let (second, again) = build_colour(&map, LEVELS);

    assert_eq!(saturated, again);
    assert_eq!(
        first.to_bytes(),
        second.to_bytes(),
        "the two cooker runs diverged"
    );
    // The fixture does not reach the scale: saturation here would mean the
    // test measures clamping rather than quantisation.
    assert_eq!(
        saturated, 0,
        "the fixture saturated -- {SCALE} is too small"
    );
}

/// The tile's colour equals the source's colour in the same direction.
///
/// WARNING: the source here is **the chain grid the level itself would take**
/// (T3c), not always the finest. Comparing against the finest would pass even
/// now, and that is exactly how it was before T3c: after averaging and
/// quantisation a smooth fixture gives the same bytes, so the test would
/// silently stop asking about the coarse levels.
#[test]
fn every_node_is_the_source_read_a_second_way() {
    let map = painted();
    let chain = map.chain();
    let (colour, _) = build_colour(&map, LEVELS);

    let mut checked = 0;
    for level in 0..LEVELS {
        let side = 1u32 << level;
        for face in 0..FACES {
            for i in (0..side).step_by(3) {
                for j in (0..side).step_by(3) {
                    let patch = Patch { face, level, i, j };
                    let index = tiles::index(LEVELS, &patch).expect("the tile exists");
                    for (a, b) in [(0usize, 0usize), (1, 7), (SIDE / 2, SIDE / 3), (SIDE, SIDE)] {
                        let unit = colour.node(index, a as i32, b as i32, 0);

                        // The second path: direction -> angles -> grid, with
                        // no call from the cooker.
                        let rads = chain.iter().map(Albedo::pixel_rad).collect::<Vec<f64>>();
                        let source = &chain[source_for(&rads, level)];
                        let [x, y, z] = patch.vertex(a, b, 1.0);
                        let flat = (x * x + y * y).sqrt();
                        let want = source.sample(z.atan2(flat), y.atan2(x));
                        let want = (want / f64::from(SCALE) * 255.0).round() as u8;

                        assert_eq!(
                            unit, want,
                            "patch {patch:?}, node ({a}, {b}): {unit} in the tile, {want} from the source"
                        );
                        checked += 1;
                    }
                }
            }
        }
    }
    assert!(checked > 100, "only {checked} nodes were checked");
}

/// Two patches' shared edge carries the same byte.
///
/// This is "there is no seam between tiles": the left one's last node is the
/// right one's zeroth, and the cooker takes them in a bitwise identical
/// direction. It fails if the coordinates inside a patch have shifted.
///
/// WARNING: **stage W removed half of this test** (step W4): there was also a
/// comparison of the halo against the neighbour's grid, checking the search
/// for a neighbour across a cube edge. The colour tileset no longer has a halo
/// -- it was never read there -- so there is nothing to check. The same search
/// stays checked for the terrain
/// (`cook.rs::the_halo_holds_the_neighbours_own_node`), where the halo really
/// is used.
#[test]
fn the_shared_edge_agrees_with_the_neighbour() {
    let map = painted();
    let (colour, _) = build_colour(&map, LEVELS);

    let level = LEVELS - 1;
    let side = 1u32 << level;
    let mut pairs = 0;
    for face in 0..FACES {
        for i in 0..side - 1 {
            for j in 0..side {
                let left = Patch { face, level, i, j };
                let right = Patch {
                    face,
                    level,
                    i: i + 1,
                    j,
                };
                let (l, r) = (
                    tiles::index(LEVELS, &left).expect("the tile exists"),
                    tiles::index(LEVELS, &right).expect("the tile exists"),
                );

                for b in [0i32, 1, SIDE as i32 / 2, SIDE as i32] {
                    // Shared edge: the left one's last node is the right
                    // one's zeroth.
                    assert_eq!(
                        colour.node(l, SIDE as i32, b, 0),
                        colour.node(r, 0, b, 0),
                        "edge between {left:?} and {right:?}, node {b}"
                    );
                }
                pairs += 1;
            }
        }
    }
    assert!(pairs > 0, "no neighbour pair was checked");
}

/// Quantisation leaves the Moon its contrast rather than making it flat grey.
///
/// The only check here needing the real source, and the only one that asks
/// about the **choice of scale**: everything above would pass at `SCALE = 1.0`,
/// where mare and highland would differ by five units out of 255. The pyramid
/// is deliberately shallow -- the question is about the range of values, not
/// the depth, and 96 tiles answer it as well as 8190.
#[test]
fn the_moon_keeps_its_contrast_through_the_quantisation() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/wac/wac_global_016p.img");
    let Ok(map) = Albedo::read(&path) else {
        eprintln!(
            "SKIPPED: missing {}. How to put it back: data/wac/README.md",
            path.display()
        );
        return;
    };

    let (colour, saturated) = build_colour(&map, 2);
    let (mut low, mut high) = (u8::MAX, u8::MIN);
    for index in 0..tiles::count(2) {
        for a in 0..=SIDE as i32 {
            for b in 0..=SIDE as i32 {
                let unit = colour.node(index, a, b, 0);
                low = low.min(unit);
                high = high.max(unit);
            }
        }
    }
    println!("  nodes {low} .. {high} of 255; saturated {saturated}");

    // Saturation occurs at **any** depth, and that is not a property of
    // fineness: a coarse pyramid level averages nothing, it takes the same
    // point bilinear sample, merely less often. So zero cannot be expected here
    // -- the first version of this check did expect it, and failed on four
    // nodes. Measured: 4 of 36,750 at two levels (0.011%) and 552 of
    // 10,032,750 at six (0.0055%) -- both an order below the 0.09% of raw
    // pixels above 0.2, because sampling between source pixels averages four
    // neighbours.
    let nodes = tiles::count(2) * (SIDE + 1) * (SIDE + 1);
    assert!(
        saturated * 1000 < nodes,
        "saturated {saturated} nodes of {nodes} -- above a per-mille, the scale is too small"
    );
    assert!(
        high - low > 60,
        "node range {low}..{high} -- the scale ate the surface contrast"
    );
}

/// A coarse level averages the source rather than picking one pixel from it
/// (T3c).
///
/// The source is a checkerboard with a two-pixel period, i.e. detail a level 0
/// node cannot carry at all: it covers hundreds of cells. There is one correct
/// answer there -- the **mean** -- and it is the same at every node.
///
/// Point sampling would give 0 or 255 at random instead, i.e. blotchy noise.
/// That is exactly how the distant Moon looked in the demo before this step,
/// and exactly what led to the chain of grids: every invariant held while the
/// picture was wrong.
#[test]
fn a_coarse_level_averages_the_source_instead_of_picking_a_pixel() {
    let (samples, lines) = (720usize, 360usize);
    let mut raw = Vec::with_capacity(samples * lines);
    for line in 0..lines {
        for sample in 0..samples {
            // A two-pixel period on both axes: the mean sits exactly in the
            // middle.
            let dark = (line + sample).is_multiple_of(2);
            raw.push(if dark { 0.05f32 } else { 0.15 });
        }
    }
    let map = Albedo {
        samples,
        lines,
        per_degree: 2.0,
        raw,
    };

    let (colour, _) = build_colour(&map, 2);
    let middle = (0.1 / f64::from(SCALE) * 255.0).round();

    let (mut low, mut high) = (u8::MAX, u8::MIN);
    for index in 0..tiles::count(1) {
        for a in 0..=SIDE as i32 {
            for b in 0..=SIDE as i32 {
                let unit = colour.node(index, a, b, 0);
                low = low.min(unit);
                high = high.max(unit);
            }
        }
    }
    println!("  level 0: nodes {low} .. {high}, source mean {middle}");

    // A tolerance of four units: the chain grid does not land exactly on a
    // checkerboard cell, so the mean wanders slightly. Point sampling would
    // give a spread from 51 to 153 -- two orders larger.
    assert!(
        f64::from(low) > middle - 4.0 && f64::from(high) < middle + 4.0,
        "level 0 nodes are spread {low}..{high} about {middle} -- that is point \
         sampling, not a mean"
    );
}
