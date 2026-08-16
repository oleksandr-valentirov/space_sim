//! Earth height cooker: ETOPO to a cubesphere tileset (stage T, step T7d).
//!
//! The oracles here do not repeat `cook.rs` (the Moon): the format, the halo
//! and level stitching are already proved there and do not depend on the
//! source. What is proved is what is **different** about Earth:
//!
//! 1. **the chain** -- the source is five times finer than the deepest level's
//!    node and thirty thousand times finer than the zeroth's, so a coarse
//!    level must average rather than take a pixel;
//! 2. **the coastline** -- what the step exists for: the sign of the height in
//!    the tile must match the sign in the source, in coordinates;
//! 3. **reference radius and units** -- the metre and 6,371,010 m, not half a
//!    metre and the lunar radius.
//!
//! Anything needing the product itself is skipped without it.

use dem_cook::bmng::Mosaic;
use dem_cook::cook::{build_earth, build_earth_colour};
use dem_cook::etopo::{Relief, REFERENCE_M};
use engine::cubesphere::{self, Patch, SIDE};
use engine::tiles::{self as tiles, Colour};
use std::path::{Path, PathBuf};

fn source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/etopo/etopo_2022_60s_surface.tif")
}

/// The ETOPO grid, or `None` -- then the test says what is missing and does
/// not fail (Q5).
fn relief() -> Option<Relief> {
    match Relief::read(&source()) {
        Ok(grid) => Some(grid),
        Err(_) => {
            eprintln!(
                "SKIPPED: missing {}. How to put it back: data/etopo/README.md",
                source().display()
            );
            None
        }
    }
}

/// Two cooker runs give byte-for-byte the same.
///
/// Two pyramids of two levels are cooked rather than six: determinism does not
/// depend on depth, and a test has no business costing minutes.
#[test]
fn cooking_twice_gives_the_same_bytes() {
    let Some(grid) = relief() else { return };

    let first = build_earth(&grid, 2).to_bytes();
    let second = build_earth(&grid, 2).to_bytes();

    assert_eq!(first, second);
}

/// The units and reference radius are Earth's, not the Moon's.
///
/// A small thing, easy to miss and impossible to see in frame: terrain with
/// the lunar scale of 0.5 is simply half as tall, and with the lunar radius it
/// is a surface sunk by four and a half thousand kilometres.
#[test]
fn the_asset_carries_earths_own_numbers() {
    let Some(grid) = relief() else { return };

    let terrain = build_earth(&grid, 2);

    assert_eq!(terrain.scale_m, 1.0);
    assert_eq!(terrain.reference_m, REFERENCE_M);
}

/// Every tile node is the same number the source gives, read by another path.
///
/// Another path: the tile is read through `Terrain::node` and the source
/// through `sample_direction_m` along the direction of the same patch vertex.
/// They must agree exactly, because between them lies only the rounding to a
/// metre, which both do.
#[test]
fn every_node_is_the_source_read_a_second_way() {
    let Some(grid) = relief() else { return };

    let levels = 2;
    let terrain = build_earth(&grid, levels);
    let chain = grid.chain();
    // The deepest pyramid level reads the grid the chain gave it; for level 1
    // that is not ETOPO itself, and taking `grid` here would check something
    // else.
    let rads = chain.iter().map(Relief::pixel_rad).collect::<Vec<f64>>();
    let source = &chain[dem_cook::cook::source_for(&rads, levels - 1)];

    let patch = Patch {
        face: 2,
        level: levels - 1,
        i: 1,
        j: 0,
    };
    let index = terrain.index(&patch).expect("patch in the pyramid");
    for a in (0..=SIDE).step_by(7) {
        for b in (0..=SIDE).step_by(7) {
            let unit = patch.vertex(a, b, 1.0);
            let expect = source.sample_direction_m(unit).round();
            let got = f64::from(terrain.node(index, a as i32, b as i32));
            assert_eq!(got, expect, "node ({a}, {b})");
        }
    }
}

/// A coarse level reads the chain's averaged grid rather than ETOPO itself
/// (T3c).
///
/// WARNING: **the two oracles that suggest themselves here both fail**, and
/// that is worth knowing in advance:
///
/// - *neighbour variance*: at level 0 a node covers 312 km, and neighbouring
///   nodes legitimately differ by four kilometres -- shelf against ocean
///   floor. Measured 3924 m, and that is a truth about Earth;
/// - *closeness to an area mean*: that "mean" itself has to be estimated by
///   sampling, and at 11x11 points its own noise (+/-360 m) exceeds the
///   difference it should show. Measured: 290 m against 239 m, i.e. the oracle
///   answers its own question with noise.
///
/// What works instead is the chain's own invariant, and it is exact: level 0
/// must take a **non-zeroth** grid, and a tile node must equal bitwise a
/// sample from exactly that one.
#[test]
fn a_coarse_level_reads_a_reduced_grid() {
    let Some(grid) = relief() else { return };

    let chain = grid.chain();
    let rads = chain.iter().map(Relief::pixel_rad).collect::<Vec<f64>>();
    let chosen = dem_cook::cook::source_for(&rads, 0);
    assert!(
        chosen > 0,
        "level 0 reads ETOPO itself -- the chain did not reach the 312 km node"
    );

    // And that grid really is coarser than the node by no more than one chain
    // step: coarser would give level 0 less detail than it can carry.
    let node_rad = std::f64::consts::FRAC_PI_2 / SIDE as f64;
    assert!(chain[chosen].pixel_rad() <= node_rad);
    assert!(chain[chosen + 1].pixel_rad() > node_rad);

    let terrain = build_earth(&grid, 1);
    let patch = Patch {
        face: 0,
        level: 0,
        i: 0,
        j: 0,
    };
    let index = terrain.index(&patch).expect("patch in the pyramid");
    for a in (0..=SIDE).step_by(7) {
        for b in (0..=SIDE).step_by(7) {
            let unit = cubesphere::vertex(
                patch.face,
                cubesphere::parameter(a, SIDE, true),
                cubesphere::parameter(b, SIDE, true),
                1.0,
            );
            let expect = chain[chosen].sample_direction_m(unit).round();
            let got = f64::from(terrain.node(index, a as i32, b as i32));
            assert_eq!(got, expect, "node ({a}, {b})");
        }
    }
}

/// The coastline in the tileset stands where it stands in the source.
///
/// This is the check the step was made for (T7). The sign of the height rather
/// than the value: between tile and source stands the chain, so the numbers
/// differ legitimately -- but land turned sea would mean a shifted grid.
///
/// The points are taken on both sides of a coast and deep inside both media,
/// including an inland sea (the Caspian) -- the case that catches a mirrored
/// longitude.
#[test]
fn the_coastline_lands_where_the_source_has_it() {
    let Some(grid) = relief() else { return };

    let terrain = build_earth(&grid, 6);
    let degrees = std::f64::consts::PI / 180.0;

    for (name, lat, lon, land) in [
        ("Sahara", 23.0, 13.0, true),
        ("Tibet", 32.0, 88.0, true),
        ("Amazonia", -3.0, -60.0, true),
        ("Antarctica", -80.0, 0.0, true),
        ("mid Pacific", 0.0, -140.0, false),
        ("Atlantic", 30.0, -40.0, false),
        ("Caspian", 42.0, 51.0, false),
        ("Arctic Ocean", 89.0, 0.0, false),
    ] {
        let (lat, lon) = (lat * degrees, lon * degrees);
        let unit = [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()];

        // From a direction to a tile node: `locate` gives the face and the
        // place on it, and the deepest level's patch is simply the integer
        // part of that place in the grid.
        let place = cubesphere::locate(unit);
        let nodes = Patch::face_nodes(5);
        let (u, v) = (place.s * nodes as f64, place.t * nodes as f64);
        let patch = Patch {
            face: place.face,
            level: 5,
            i: (u as usize / SIDE).min((1 << 5) - 1) as u32,
            j: (v as usize / SIDE).min((1 << 5) - 1) as u32,
        };
        let height = terrain.height_m(&patch, u as usize % SIDE, v as usize % SIDE);

        assert_eq!(
            height >= 0.0,
            land,
            "{name}: the tileset gives {height:.0} m, the source {:.0} m",
            grid.sample_direction_m(unit)
        );
    }
}

// -- Colour (T7e) ---------------------------------------------------------

/// The BMNG mosaic, or `None` -- then the test says what is missing and does
/// not fail (Q5).
fn mosaic() -> Option<Mosaic> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/bmng/world.topo.bathy.200407.jpg");
    match Mosaic::read(&path) {
        Ok(map) => Some(map),
        Err(_) => {
            eprintln!(
                "SKIPPED: missing {}. How to put it back: data/bmng/README.md",
                path.display()
            );
            None
        }
    }
}

/// Earth's colour header carries what distinguishes it from the Moon.
///
/// Four channels, a scale of one and the **sRGB space** -- the last new in the
/// format (`Colour::srgb`, since format version 2). Without that field a
/// "colour" byte would mean different things for the two bodies, and which
/// could only be learned from the channel count, i.e. by guessing.
#[test]
fn the_colour_asset_says_what_space_it_is_in() {
    let Some(map) = mosaic() else { return };

    let colour = build_earth_colour(&map, 1);

    assert_eq!(colour.channels, 4);
    assert_eq!(colour.scale, 1.0);
    assert!(colour.srgb);

    let read = Colour::from_bytes(&colour.to_bytes()).expect("our own file");
    assert_eq!(read.srgb, colour.srgb);
    assert_eq!(read.channels, colour.channels);
}

/// Every colour node is the source read by another path, and in sRGB.
///
/// The chain here is the same as for the heights, so the comparison must be
/// against the grid `source_for` chose rather than against the mosaic.
#[test]
fn every_colour_node_is_the_source_read_a_second_way() {
    let Some(map) = mosaic() else { return };

    let levels = 2;
    let colour = build_earth_colour(&map, levels);
    let chain = map.chain();
    let rads = chain.iter().map(Mosaic::pixel_rad).collect::<Vec<f64>>();
    let source = &chain[dem_cook::cook::source_for(&rads, levels - 1)];

    let patch = Patch {
        face: 3,
        level: levels - 1,
        i: 0,
        j: 1,
    };
    let index = tiles::index(colour.levels, &patch).expect("patch in the pyramid");
    for a in (0..=SIDE).step_by(7) {
        for b in (0..=SIDE).step_by(7) {
            let linear = source.sample_direction(patch.vertex(a, b, 1.0));
            for channel in 0..3u32 {
                let expect = dem_cook::bmng::to_srgb(linear[channel as usize]);
                let got = colour.node(index, a as i32, b as i32, channel);
                assert_eq!(got, expect, "node ({a}, {b}), channel {channel}");
            }
            // The fourth channel exists only because there is no three-byte
            // texture.
            assert_eq!(colour.node(index, a as i32, b as i32, 3), u8::MAX);
        }
    }
}

/// What the CPU reads stays linear -- regardless of how the byte is stored.
///
/// That is the reason for the `srgb` field: planetshine (T6) asks about light,
/// and over a dark ocean the difference between byte and light is
/// twentyfold.
#[test]
fn what_the_cpu_reads_is_linear() {
    let Some(map) = mosaic() else { return };

    let colour = build_earth_colour(&map, 1);
    let degrees = std::f64::consts::PI / 180.0;
    let (lat, lon) = (0.0f64, -140.0 * degrees);
    let unit = [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()];

    // Level zero is averaged by the chain, so the comparison is against the
    // tile itself rather than the mosaic: the question here is not "which
    // number" but "in which space".
    let (a, b) = (0, 0);
    let index = tiles::index(
        colour.levels,
        &Patch {
            face: cubesphere::locate(unit).face,
            level: 0,
            i: 0,
            j: 0,
        },
    )
    .expect("level zero always exists");

    for channel in 0..3u32 {
        let byte = f64::from(colour.node(index, a, b, channel)) / 255.0;
        let linear = colour.reflectance(index, a, b, channel);
        assert!(
            linear < byte,
            "channel {channel}: {linear} is not darker than byte {byte} -- sRGB was not decoded"
        );
    }
}

/// Colour and height sit at one node: the sea is blue where it is below zero.
///
/// The same coastline check, but now **between two assets** rather than
/// between an asset and a source: both tilesets share the pyramid geometry and
/// the traversal, so a discrepancy here would mean a half-node shift -- exactly
/// what the shared `direction` is written against.
#[test]
fn colour_and_height_agree_on_the_shore() {
    let Some(map) = mosaic() else { return };
    let Some(grid) = relief() else { return };

    let levels = 3;
    let colour = build_earth_colour(&map, levels);
    let terrain = build_earth(&grid, levels);

    let patch = Patch {
        face: 0,
        level: levels - 1,
        i: 2,
        j: 1,
    };
    let ci = tiles::index(colour.levels, &patch).expect("patch in the pyramid");
    let ti = terrain.index(&patch).expect("patch in the pyramid");

    let mut agree = 0;
    let mut total = 0;
    for a in 0..=SIDE {
        for b in 0..=SIDE {
            let height = f64::from(terrain.node(ti, a as i32, b as i32));
            let blue = colour.node(ci, a as i32, b as i32, 2);
            let red = colour.node(ci, a as i32, b as i32, 0);
            if (height < 0.0) == (blue > red) {
                agree += 1;
            }
            total += 1;
        }
    }

    let fraction = f64::from(agree) / f64::from(total);
    assert!(
        fraction > 0.9,
        "colour and height agree on only {:.1}% of nodes -- the tilesets are shifted",
        100.0 * fraction
    );
}
