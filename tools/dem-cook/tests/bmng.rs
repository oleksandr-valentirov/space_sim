//! Blue Marble mosaic reader (stage T, step T7c).
//!
//! The principal oracle here is **not our own**, deliberately: colour proves
//! nothing about itself ("looks like Earth" is not a check). The neighbouring
//! product proves it -- where ETOPO says water, the mosaic must be blue. One
//! check catches a half-pixel shift, a reversed row, swapped channels and the
//! wrong longitude origin alike.
//!
//! The remaining oracles hold what the mask agreement does not see: the grid
//! geometry (compared against the ETOPO label, i.e. a file in git), the sample
//! space (sRGB there and back) and the chain of coarser grids.

use dem_cook::bmng::{self, Mosaic};
use dem_cook::etopo::{Header, Relief};
use std::path::{Path, PathBuf};

fn data(dir: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data")
        .join(dir)
        .join(name)
}

/// The mosaic itself, or `None` -- then the test says what is missing and does
/// not fail (Q5).
fn mosaic() -> Option<Mosaic> {
    let path = data("bmng", "world.topo.bathy.200407.jpg");
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

/// sRGB there and back -- over all 256 inputs, because there are exactly 256.
///
/// Not a check of the decoder but of **our** pair of conversions: colour goes
/// from the file into linear light and returns as a tile byte, and if the two
/// functions are not inverses, Earth's surface shifts in brightness as a whole
/// -- uniformly, i.e. invisibly, until the source is put beside it.
#[test]
fn srgb_round_trips_on_every_byte() {
    let table = (0..=255u8).map(|b| {
        let x = f64::from(b) / 255.0;
        let linear = if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        };
        (b, bmng::to_srgb(linear))
    });

    for (before, after) in table {
        assert_eq!(before, after, "{before} -> linear -> {after}");
    }
}

/// The grid geometry must match ETOPO -- the choice of pair rests on that.
///
/// Compared against the **label** rather than the second product: the label
/// lives in git, so this side of the check always exists.
#[test]
fn the_grid_matches_the_dem() {
    let Some(map) = mosaic() else { return };
    let dem = Header::read(&data("etopo", "etopo_2022_60s_surface.lbl")).expect("the label in git");

    assert_eq!(map.samples, dem.samples);
    assert_eq!(map.lines, dem.lines);
    assert!(
        (map.per_degree - dem.per_degree).abs() < 1e-9,
        "{} samples per degree against {}",
        map.per_degree,
        dem.per_degree
    );
}

/// Named points: the ocean dark and blue, the desert light and warm, ice
/// white.
///
/// The numbers come from an independent reader (Python + PIL) that walked
/// **the same** path: sRGB to linear, bilinear weights in linear, back to a
/// byte. Compared in bytes, because in linear units there is nothing to check
/// them against by eye.
///
/// WARNING: comparing against a **pixel** sample is not allowed here: the
/// bilinear weights take four neighbours, and in the Sahara that is a
/// difference of two bytes out of 197. The oracle must repeat the arithmetic,
/// not only the source.
#[test]
fn named_points_match_an_independent_reader() {
    let Some(map) = mosaic() else { return };
    let degrees = std::f64::consts::PI / 180.0;

    for (name, lat, lon, expect) in [
        ("mid Pacific", 0.0, -140.0, [5u8, 16, 43]),
        ("Sahara", 23.0, 13.0, [198, 158, 110]),
        ("Amazonia", -3.0, -60.0, [87, 90, 56]),
        ("Greenland", 72.0, -40.0, [252, 254, 253]),
    ] {
        let got = map.sample(lat * degrees, lon * degrees);
        for channel in 0..3 {
            let byte = bmng::to_srgb(got[channel]);
            assert!(
                byte.abs_diff(expect[channel]) <= 1,
                "{name}, channel {channel}: {byte} against {}",
                expect[channel]
            );
        }
    }
}

/// The principal oracle: ETOPO's water mask and the mosaic's blue are the same
/// mask.
///
/// Not 100%, and it need not be: shaded forests in the mosaic are bluish too,
/// while shallows and salt flats are not. Measured over every twentieth pixel
/// of both products -- **97.95%** weighted by `cos(latitude)`; any grid shift
/// drops that number by tens of percent.
#[test]
fn the_mosaic_is_blue_where_the_dem_says_water() {
    let Some(map) = mosaic() else { return };
    let Ok(dem) = Relief::read(&data("etopo", "etopo_2022_60s_surface.tif")) else {
        eprintln!("SKIPPED: no ETOPO. How to put it back: data/etopo/README.md");
        return;
    };

    let degrees = std::f64::consts::PI / 180.0;
    let step = 20;
    let mut agree = 0.0;
    let mut total = 0.0;
    for line in (0..map.lines).step_by(step) {
        let lat = 90.0 - (line as f64 + 0.5) * 180.0 / map.lines as f64;
        let weight = (lat * degrees).cos();
        for sample in (0..map.samples).step_by(step) {
            let lon = -180.0 + (sample as f64 + 0.5) * 360.0 / map.samples as f64;
            let colour = map.sample(lat * degrees, lon * degrees);
            let water = dem.sample_m(lat * degrees, lon * degrees) < 0.0;
            let blue = colour[2] > colour[0] && colour[2] > colour[1];
            if water == blue {
                agree += weight;
            }
            total += weight;
        }
    }

    let fraction = agree / total;
    assert!(
        fraction > 0.95,
        "the masks agree on only {:.2}% -- the grids have diverged",
        100.0 * fraction
    );
}

/// The chain of coarser grids: it reaches the coarsest pyramid level's node
/// and does not move the mean colour on the way.
///
/// The first is what the chain exists for: a level 0 node covers 312 km, and
/// if the chain stopped at halving (10800 = 2^4 * 675) the coarsest grid would
/// be ten times finer than a node, i.e. level 0 would again take one point out
/// of thirty thousand pixels.
///
/// The second is what the chain has no right to do: it preserves the mean,
/// because it removes detail rather than changing the planet's brightness.
#[test]
fn the_chain_reaches_the_coarsest_node_and_keeps_the_mean() {
    let Some(map) = mosaic() else { return };

    let chain = map.chain();
    let node_rad = std::f64::consts::FRAC_PI_2 / 32.0;
    let coarsest = chain.last().expect("the chain is not empty");
    assert!(
        coarsest.pixel_rad() >= node_rad,
        "coarsest grid {}x{} is {:.4} rad per pixel against a {node_rad:.4} rad node",
        coarsest.samples,
        coarsest.lines,
        coarsest.pixel_rad()
    );

    // An unweighted mean rather than `mean()`: the box filter's equal blocks
    // preserve the sum exactly, so this is an **exact** invariant and the
    // tolerance here is only for `f32` rounding. A `cos(latitude)`-weighted
    // mean is not such an invariant -- on a fifteen-row grid a block centre's
    // weight no longer equals the mean weight over the block, and the 0.9% at
    // the coarsest level is a truth about the sphere rather than a chain
    // error.
    let flat = |level: &Mosaic| {
        let mut sum = [0.0f64; 3];
        for pixel in level.raw.chunks_exact(3) {
            for (channel, value) in pixel.iter().enumerate() {
                sum[channel] += f64::from(*value);
            }
        }
        sum.map(|s| s / (level.samples * level.lines) as f64)
    };

    let mean = flat(&map);
    for (index, level) in chain.iter().enumerate() {
        let here = flat(level);
        for channel in 0..3 {
            assert!(
                (here[channel] - mean[channel]).abs() < 1e-6,
                "level {index}, channel {channel}: {} against {}",
                here[channel],
                mean[channel]
            );
        }
    }
}
