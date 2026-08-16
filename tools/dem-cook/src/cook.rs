//! Surface cooker: LOLA and LROC WAC to cubesphere tiles (R5b; stage T, T2d).
//!
//! The first asset-pipeline tool in Rust. The only cooker before it is in C
//! (`make cook`, the ephemeris), and the shape of the step is the same:
//! offline, own format, version in the header, deterministic output.
//!
//! ## What it does, in three lines
//!
//! For every patch of every pyramid level it takes the direction of every grid
//! node (`cubesphere::Patch::vertex` on the unit sphere), asks the source for
//! the value along that direction, and stores it as an integer: height in half
//! metres, the same units LOLA stores it in; colour in fractions of [`SCALE`].
//!
//! ## Two sources, one traversal
//!
//! The traversal is shared deliberately (`tiles::node_direction`): **a colour
//! node must lie exactly where a height node lies**. Otherwise colour and
//! terrain would shift half a node against each other, and it would look like
//! a source error rather than two different traversals. The pyramid depths
//! differ meanwhile (5 against 6) -- which is exactly why depth is a parameter
//! and not a constant of the traversal.
//!
//! ## Why the output is deterministic, and why that is not luck
//!
//! The traversal order is fixed, patch vertices are bit-identical on both
//! sides of an edge (R2b), and grid sampling is a pure function of direction.
//! So the same input gives the same byte and the file's SHA is stable across
//! runs. That is checked rather than proclaimed:
//! `tools/dem-cook/tests/cook.rs`.
//!
//! `libm` is allowed here without reservation -- rule 4 of stage R: the cooker
//! is offline and CPU outside the integrator.

use crate::albedo::Albedo;
use crate::bmng::{self, Mosaic};
use crate::etopo::{self, Relief};
use crate::Grid;
use engine::cubesphere::{Patch, FACES, SIDE};
use engine::tiles::{self, node_direction, Colour, Terrain, HALO, NODES, STORED};
use std::path::Path;

/// What sample 255 in a colour tile equals -- reflectance.
///
/// A constant rather than a percentile computed on the fly, because the
/// cooker's output must be predictable: a number chosen from the data itself
/// would silently change **every** byte of the asset after an edit to one
/// crater. The measured basis is the WAC mosaic's distribution: median 0.044,
/// p99.9 = 0.197, a tail to 0.599 covering 0.09% of pixels
/// (`engine::tiles::Colour`).
///
/// How many nodes actually saturated is printed in the cooker's report, so the
/// choice is checked by a number rather than left a guess in the code.
pub const SCALE: f32 = 0.25;

/// Cook a tileset from the LOLA grid.
pub fn cook(source: &Path, out: &Path, levels: u32) -> Result<String, String> {
    let grid = Grid::read(source)?;
    let terrain = build(&grid, levels);
    let bytes = terrain.to_bytes();

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;

    Ok(format!(
        "{} -- {} levels, {} tiles, {:.1} MiB; lowest point {:.1} m",
        out.display(),
        levels,
        Terrain::count(levels),
        bytes.len() as f64 / (1024.0 * 1024.0),
        terrain.lowest_m()
    ))
}

/// The tile pyramid from a grid, without writing to disk, so a test can
/// compare it twice.
pub fn build(grid: &Grid, levels: u32) -> Terrain {
    // The storage units are the source's: translating them would mean
    // rounding twice where zero times suffices.
    let scale = grid.scale_m as f32;
    // The Moon has no sea, and the sentinel says so outright: the material
    // rule applies everywhere on it, as it did before T7f.
    Terrain::build(
        levels,
        grid.reference_m,
        scale,
        tiles::NO_SEA,
        &height_grids(grid, levels),
    )
}

/// Height grids **with a halo** -- exactly what `Terrain::build` accepts.
///
/// Separate from [`build`] not for structure but for the oracle: since format
/// version 4 the halo does not reach the file (the slope is baked and the
/// gradient moved into the writer), so "the halo really is a neighbour's node"
/// can only be checked here, before it is consumed. Checked by
/// `tests/cook.rs::the_halo_holds_the_neighbours_own_node`.
pub fn height_grids(grid: &Grid, levels: u32) -> Vec<Vec<i16>> {
    let mut grids = Vec::with_capacity(Terrain::count(levels));
    for level in 0..levels {
        let side = 1u32 << level;
        for face in 0..FACES {
            for i in 0..side {
                for j in 0..side {
                    let patch = Patch { face, level, i, j };
                    let mut tile = Vec::with_capacity(STORED * STORED);
                    for a in 0..STORED as isize {
                        for b in 0..STORED as isize {
                            let (a, b) = (a - HALO as isize, b - HALO as isize);
                            // Unit sphere: height depends on direction, not
                            // radius, and the direction here is bit-identical
                            // to the neighbouring patch's on a shared edge.
                            let metres = match node_direction(&patch, a, b) {
                                Some(unit) => grid.sample_direction_m(unit),
                                // Halo corner: there is no across-edge
                                // neighbour there, and nobody reads it
                                // (`engine::tiles`).
                                None => 0.0,
                            };
                            tile.push(quantise(metres, grid.scale_m));
                        }
                    }
                    grids.push(tile);
                }
            }
        }
    }
    grids
}

/// Cook Earth's height tileset from ETOPO.
///
/// Separate from [`cook`] rather than a flag inside it: what they share is the
/// traversal (`tiles::node_direction`) and the format, while the sources
/// differ in everything -- units, reference radius, chain, longitude
/// registration.
pub fn cook_earth(source: &Path, out: &Path, levels: u32) -> Result<String, String> {
    let relief = Relief::read(source)?;
    let terrain = build_earth(&relief, levels);
    let bytes = terrain.to_bytes();

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;

    Ok(format!(
        "{} -- {} levels, {} tiles, {:.1} MiB; lowest point {:.1} m, land {:.2}%",
        out.display(),
        levels,
        Terrain::count(levels),
        bytes.len() as f64 / (1024.0 * 1024.0),
        terrain.lowest_m(),
        100.0 * relief.land_fraction(),
    ))
}

/// Earth's height pyramid, without writing to disk, so a test can compare it
/// twice.
pub fn build_earth(relief: &Relief, levels: u32) -> Terrain {
    let chain = relief.chain();
    let rads = chain.iter().map(Relief::pixel_rad).collect::<Vec<f64>>();

    let mut grids = Vec::with_capacity(Terrain::count(levels));
    for level in 0..levels {
        let source = &chain[source_for(&rads, level)];
        let side = 1u32 << level;
        for face in 0..FACES {
            for i in 0..side {
                for j in 0..side {
                    let patch = Patch { face, level, i, j };
                    let mut tile = Vec::with_capacity(STORED * STORED);
                    for a in 0..STORED as isize {
                        for b in 0..STORED as isize {
                            let (a, b) = (a - HALO as isize, b - HALO as isize);
                            let metres = match node_direction(&patch, a, b) {
                                Some(unit) => source.sample_direction_m(unit),
                                // Halo corner: there is no across-edge
                                // neighbour there, and nobody reads it
                                // (`engine::tiles`).
                                None => 0.0,
                            };
                            // The storage unit is the metre, the one the grid
                            // already uses; this rounding is the second and
                            // last.
                            tile.push(quantise(metres, 1.0));
                        }
                    }
                    grids.push(tile);
                }
            }
        }
    }

    // Sea level is exactly zero, and that is not a convention: ETOPO measures
    // heights from the geoid and the storage unit here is the metre. So "below
    // zero" in this tileset means "under water" by the source's construction,
    // not by a threshold of ours. Measured on the cooked asset: 72.0% of nodes
    // are below zero, against a true ocean fraction of 71%.
    Terrain::build(levels, etopo::REFERENCE_M, 1.0, 0.0, &grids)
}

/// Cook the colour tileset from the WAC mosaic.
pub fn cook_colour(source: &Path, out: &Path, levels: u32) -> Result<String, String> {
    let map = Albedo::read(source)?;
    let (colour, saturated) = build_colour(&map, levels);
    let bytes = colour.to_bytes();

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;

    let nodes = tiles::count(levels) * NODES * NODES;
    Ok(format!(
        "{} -- {levels} levels, {} tiles, {:.1} MiB; scale {SCALE}, saturated \
         {saturated} nodes of {nodes} ({:.4}%)",
        out.display(),
        tiles::count(levels),
        bytes.len() as f64 / (1024.0 * 1024.0),
        100.0 * saturated as f64 / nodes as f64,
    ))
}

/// The colour tile pyramid, without writing to disk, so a test can compare it
/// twice.
///
/// Alongside it, how many nodes came up against [`SCALE`]: that is the cost of
/// the scale choice, and it is paid in the same bytes as the asset itself.
pub fn build_colour(map: &Albedo, levels: u32) -> (Colour, usize) {
    let chain = map.chain();
    let rads = chain.iter().map(Albedo::pixel_rad).collect::<Vec<f64>>();
    let mut saturated = 0usize;
    let mut grids = Vec::with_capacity(tiles::count(levels));
    for level in 0..levels {
        let source = &chain[source_for(&rads, level)];
        let side = 1u32 << level;
        for face in 0..FACES {
            for i in 0..side {
                for j in 0..side {
                    let patch = Patch { face, level, i, j };
                    let mut tile = Vec::with_capacity(NODES * NODES);
                    for a in 0..NODES {
                        for b in 0..NODES {
                            // A colour tile carries no halo (W4): colour has
                            // no gradient, and a sample at the patch edge has
                            // zero weight on it.
                            let value = source.sample_direction(patch.vertex(a, b, 1.0));
                            let unit = quantise_colour(value);
                            if unit == u8::MAX {
                                saturated += 1;
                            }
                            tile.push(unit);
                        }
                    }
                    grids.push(tile);
                }
            }
        }
    }
    (Colour::build(levels, 1, SCALE, false, &grids), saturated)
}

/// Cook Earth's colour tileset from the BMNG mosaic.
pub fn cook_earth_colour(source: &Path, out: &Path, levels: u32) -> Result<String, String> {
    let map = Mosaic::read(source)?;
    let colour = build_earth_colour(&map, levels);
    let bytes = colour.to_bytes();

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;

    // Two means side by side, and this is a cross-check rather than a report
    // (T7h). The first is computed over the source's lat-lon grid, the second
    // over the coarsest cubesphere pyramid level -- an entirely different route
    // after three transformations. It is the second that the engine reads when
    // it builds the sky table.
    let mean = map.mean();
    let ours = colour.mean();
    Ok(format!(
        "{} -- {levels} levels, {} tiles, {:.1} MiB, four sRGB channels; \
         mean mosaic colour {:.4} {:.4} {:.4}, pyramid {:.4} {:.4} {:.4}",
        out.display(),
        tiles::count(levels),
        bytes.len() as f64 / (1024.0 * 1024.0),
        mean[0],
        mean[1],
        mean[2],
        ours[0],
        ours[1],
        ours[2],
    ))
}

/// Earth's colour tile pyramid, without writing to disk, so a test compares it
/// twice.
///
/// ## Four channels, three of which carry colour
///
/// No three-byte texture format exists in wgpu or in Vulkan (T2a), so the
/// fourth byte is there regardless. It is filled with `255` and read by
/// nobody: the water mask follows from the height for free (`h < 0`), and a
/// field nobody reads is worse than no field -- so nothing is put there "just
/// in case".
///
/// ## The byte stores sRGB, not linear light
///
/// Everything inside -- the bilinear weights and the chain -- is computed
/// linearly (`bmng::Mosaic`), while an sRGB-encoded byte goes into the tile.
/// The reason is numerical: the ocean's linear luminance is 0.0015, i.e. zero
/// in eight bits. The GPU decodes it on sampling (`Rgba8UnormSrgb`), for free,
/// and on the CPU --
/// `Colour::reflectance`.
pub fn build_earth_colour(map: &Mosaic, levels: u32) -> Colour {
    let chain = map.chain();
    let rads = chain.iter().map(Mosaic::pixel_rad).collect::<Vec<f64>>();

    let mut grids = Vec::with_capacity(tiles::count(levels));
    for level in 0..levels {
        let source = &chain[source_for(&rads, level)];
        let side = 1u32 << level;
        for face in 0..FACES {
            for i in 0..side {
                for j in 0..side {
                    let patch = Patch { face, level, i, j };
                    let mut tile = Vec::with_capacity(NODES * NODES * 4);
                    for a in 0..NODES {
                        for b in 0..NODES {
                            let linear = source.sample_direction(patch.vertex(a, b, 1.0));
                            for value in linear {
                                tile.push(bmng::to_srgb(value));
                            }
                            tile.push(u8::MAX);
                        }
                    }
                    grids.push(tile);
                }
            }
        }
    }

    // Scale one: the byte holds the whole colour range and there is nowhere to
    // compress it -- unlike the Moon's reflectance, of which 99.9% of samples
    // lie below 0.2 (`SCALE`).
    Colour::build(levels, 4, 1.0, true, &grids)
}

/// Which chain grid a pyramid level reads.
///
/// The coarsest whose pixel is not yet larger than this level's node. Angles,
/// not metres: a level `L` node is `(pi/2) / (SIDE*2^L)` radians regardless of
/// body radius, and a grid pixel is `pi/(180*per_degree)`. The radius cancels,
/// so the same number works for both the Moon and Earth.
///
/// A finer grid would give point sampling where a node covers thousands of
/// pixels (blotchy noise instead of a map); a coarser one would throw away
/// detail the node can still carry.
///
/// The parameter is the angles themselves rather than the chain: there are now
/// three different grid types (lunar heights, lunar mosaic, Earth heights and
/// colour) and one question to ask them.
pub fn source_for(pixel_rad: &[f64], level: u32) -> usize {
    let node_rad = std::f64::consts::FRAC_PI_2 / f64::from(SIDE as u32 * (1u32 << level));
    let mut best = 0;
    for (index, rad) in pixel_rad.iter().enumerate() {
        if *rad <= node_rad {
            best = index;
        }
    }
    best
}

/// Reflectance to one byte.
///
/// Clamped on both sides, and the two sides mean different things. Below,
/// negative source values (1.66% of the mosaic): photometric normalisation
/// noise, with zero the physical floor. Above, the tail past [`SCALE`], whose
/// saturation to white the cooker counts and prints.
fn quantise_colour(value: f64) -> u8 {
    let units = (value / f64::from(SCALE) * 255.0).round();
    units.clamp(0.0, 255.0) as u8
}

/// Metres to storage units, saturating rather than wrapping.
///
/// Wrapping here would be the worst possible: a 33 km mountain would turn into
/// a basin, and it would look plausible.
fn quantise(metres: f64, scale: f64) -> i16 {
    let units = (metres / scale).round();
    units.clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}
