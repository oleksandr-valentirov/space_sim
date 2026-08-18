//! Surface cooker: command line (R5b; stage T, T2d).
//!
//! The work itself is in [`dem_cook::cook`]; this is argument parsing only.
//! Split not out of love for layers: a test cannot call a function out of a
//! binary, and output determinism has to be checked by calling it, twice.
//!
//!     cargo run -p dem-cook                       data/lola  → assets/moon.dem
//!     cargo run -p dem-cook -- --colour           data/wac   → assets/moon.col
//!     cargo run -p dem-cook -- --body earth       data/etopo → assets/earth.dem
//!     cargo run -p dem-cook -- --body earth --colour  data/bmng → assets/earth.col
//!
//! The body is a flag rather than a separate binary: they share exactly as
//! much as they should -- the cubesphere traversal and the tile format.

use dem_cook::cook::{cook, cook_colour, cook_earth, cook_earth_colour};
use std::path::PathBuf;

/// How many height pyramid levels to cook by default.
///
/// A measured number, not a taste. LDEM_4 gives 7581 m per sample; a level `L`
/// patch cell on the Moon is `(pi*R/2) / (SIDE*2^L)`, i.e. 85 km at level 0
/// and 5.3 km at level 4. So level 4 is already finer than the source, and
/// level 5 would bring no new number -- only a fourfold larger file.
const DEFAULT_LEVELS: u32 = 5;

/// How many colour pyramid levels -- one more, also measured (T2a).
///
/// The source is twice as fine as LOLA (1.9 km against 7.6 km per pixel), so a
/// sixth level has something to take: 3.8 km per node. A seventh would cost
/// 256 MiB of video memory against 32 and a fourfold longer load, and reaches
/// the screen no better either way -- the material rule (T4) closes that
/// gap.
const DEFAULT_COLOUR_LEVELS: u32 = 6;

/// How many pyramid levels for Earth's **shape** -- six, measured at T7.
///
/// A level 6 node covers 9.77 km of Earth, five times coarser than the source
/// (1.85 km). That is visible in colour and not in shape: at this node the
/// coastline is where it should be, and metre-scale relief has nothing to be
/// built on either way. Deepening it is a separate decision, to be made on a
/// sharp Earth rather than before one (ROADMAP, after X5e).
const DEFAULT_EARTH_LEVELS: u32 = 6;

/// How many pyramid levels for Earth's **colour** -- eight (X5e).
///
/// Eight because eight is where the source runs out: a level 8 node covers
/// 2.45 km against Blue Marble's 1.85 km per pixel, and a ninth (1.22 km) would
/// be finer than the source cell, i.e. a fourfold larger file carrying nothing
/// (see `tiles.rs`). Six saw 3.5% of the mosaic's pixels; this sees all of
/// them.
///
/// What used to forbid it was video memory -- the whole pyramid was resident,
/// and eight levels measure 1.5 GiB (NVIDIA) or 2.0 (RADV) for this pyramid
/// alone (X5a). Since X5b/X5d the GPU holds a pool of slots and the file is
/// read a tile at a time, so depth costs disk (571 MB) and cooking time, not
/// VRAM.
const DEFAULT_EARTH_COLOUR_LEVELS: u32 = 8;

fn main() {
    let mut colour = false;
    let mut earth = false;
    let mut source: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut levels: Option<u32> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--colour" => colour = true,
            "--body" => match args.next().as_deref() {
                Some("earth") => earth = true,
                Some("moon") => earth = false,
                other => {
                    eprintln!("--body wants moon or earth, not {other:?}");
                    std::process::exit(2);
                }
            },
            "--source" => source = Some(args.next().expect("--source wants a path").into()),
            "--out" => out = Some(args.next().expect("--out wants a path").into()),
            "--levels" => {
                levels = Some(
                    args.next()
                        .expect("--levels wants a number")
                        .parse()
                        .expect("--levels wants a number"),
                )
            }
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
    }

    // The defaults depend on what is being cooked: different sources,
    // different pyramid depths, and they must not be confused silently.
    let (default_source, default_out, default_levels) = match (earth, colour) {
        (false, false) => ("data/lola/ldem_4.img", "assets/moon.dem", DEFAULT_LEVELS),
        (false, true) => (
            "data/wac/wac_global_016p.img",
            "assets/moon.col",
            DEFAULT_COLOUR_LEVELS,
        ),
        (true, false) => (
            "data/etopo/etopo_2022_60s_surface.tif",
            "assets/earth.dem",
            DEFAULT_EARTH_LEVELS,
        ),
        (true, true) => (
            "data/bmng/world.topo.bathy.200407.jpg",
            "assets/earth.col",
            DEFAULT_EARTH_COLOUR_LEVELS,
        ),
    };
    let source = source.unwrap_or_else(|| PathBuf::from(default_source));
    let out = out.unwrap_or_else(|| PathBuf::from(default_out));
    let levels = levels.unwrap_or(default_levels);

    let result = match (earth, colour) {
        (false, false) => cook(&source, &out, levels),
        (false, true) => cook_colour(&source, &out, levels),
        (true, false) => cook_earth(&source, &out, levels),
        (true, true) => cook_earth_colour(&source, &out, levels),
    };

    match result {
        Ok(report) => println!("{report}"),
        Err(message) => {
            eprintln!("the cooker failed: {message}");
            std::process::exit(1);
        }
    }
}
