//! The LOLA reader reproduces what the label printed (R5a).
//!
//! The oracle here is **not ours**: the LOLA team published `MINIMUM` and
//! `MAXIMUM` beside the data itself. The reader must arrive at the same
//! numbers from raw bytes -- and that catches byte order, scale and grid size
//! in one claim.

use dem_cook::Grid;
use std::path::Path;

fn moon() -> Grid {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/lola/ldem_4.img");
    Grid::read(&path).expect("the LOLA grid should have read")
}

/// Bounds computed from the data equal the published ones -- exactly.
#[test]
fn the_measured_extremes_are_the_published_ones() {
    let grid = moon();
    let (low, high) = grid.measured();

    println!(
        "  {}x{} samples, {:.2} m/pixel; bounds {low} .. {high} = {:.1} .. {:.1} m",
        grid.samples,
        grid.lines,
        grid.metres_per_pixel,
        f64::from(low) * grid.scale_m,
        f64::from(high) * grid.scale_m
    );

    // A measured discrepancy worth knowing: the maximum matches **exactly**
    // (21008), the minimum by **one** unit (-17757 against -17758), i.e. half
    // a metre. The label itself says it describes "binary resampling to pixel
    // registration", so the published bounds were most likely computed before
    // resampling. One unit is one storage quantum, which is exactly why the
    // tolerance is this: larger would hide a swapped byte order (which would
    // diverge by thousands), smaller would fail on a property of the product
    // itself.
    assert!(
        (low - grid.published.0).abs() <= 1 && (high - grid.published.1).abs() <= 1,
        "measured bounds ({low}, {high}) diverge from the label {:?} by more \
         than a quantum -- either the byte order or the wrong part of the file",
        grid.published
    );
    // The scale is applied: in metres this is ten kilometres, not twenty.
    let relief = f64::from(high - low) * grid.scale_m;
    assert!(
        (19_000.0..20_000.0).contains(&relief),
        "terrain range {relief:.0} m -- the Moon has about 19.4 km"
    );
}

/// The map lies the right way up -- by two known lunar asymmetries.
///
/// Without this the first check catches nothing about **orientation**: a map
/// flipped in latitude or counted westwards has the same bounds. The oracles
/// here are not individual pixels (at 7.6 km/pixel they are smoothed) but
/// large known facts that an error of a few cells will not move:
///
/// 1. **The near side is lower than the far side** by about two kilometres --
///    the offset of the Moon's centre of figure from its centre of mass.
///    Measured here: mean hemisphere height about 0 comes to **-1105 m**,
///    about 180 to **+608 m**, a difference of **1713 m**. Catches both the
///    direction longitude is counted in and where it starts;
/// 2. **The South Pole-Aitken basin** (-60, 200E) is the Moon's deepest place;
///    its mirror in latitude (+60, 200E) is far-side highlands. Measured:
///    **-5577 m** against **+3802 m**, i.e. 9.4 km apart. A flipped latitude
///    would swap them, and that cannot go unnoticed.
#[test]
fn the_map_lies_the_right_way_round() {
    let grid = moon();
    let at = |lat: f64, lon: f64| grid.sample_m(lat.to_radians(), lon.to_radians());

    // Mean hemisphere height about a given longitude, weighted by the cosine
    // of latitude: otherwise the poles, where cells are narrower, would count
    // as much.
    let hemisphere = |centre: f64| {
        let (mut sum, mut weight) = (0.0, 0.0);
        for line in 0..grid.lines {
            for sample in 0..grid.samples {
                let lat = 90.0 - (line as f64 + 0.5) / grid.per_degree;
                let lon = (sample as f64 + 0.5) / grid.per_degree;
                let away = ((lon - centre + 540.0) % 360.0 - 180.0).abs();
                if away < 90.0 {
                    let w = lat.to_radians().cos();
                    sum += at(lat, lon) * w;
                    weight += w;
                }
            }
        }
        sum / weight
    };

    let near_side = hemisphere(0.0);
    let far_side = hemisphere(180.0);
    let aitken = at(-60.0, 200.0);
    let mirror = at(60.0, 200.0);

    println!(
        "  hemispheres: near {near_side:.0} m, far {far_side:.0} m \
         (difference {:.0} m); Aitken {aitken:.0} m against mirror {mirror:.0} m",
        far_side - near_side
    );

    assert!(
        far_side - near_side > 1000.0,
        "the far side is not higher than the near side ({far_side:.0} against \
         {near_side:.0}) -- longitude is counted the wrong way or from the \
         wrong meridian"
    );
    assert!(
        aitken < -3000.0 && mirror > 2000.0,
        "latitude is flipped: Aitken {aitken:.0} m, mirror {mirror:.0} m"
    );
}

/// A direction and a pair of angles give the same sample.
///
/// The cubesphere works in directions, and the translation must live in one
/// place. Checked at the poles and on the longitude seam, i.e. where `atan2`
/// changes branch.
#[test]
fn a_direction_and_a_pair_of_angles_read_the_same_sample() {
    let grid = moon();
    for (lat, lon) in [
        (0.0_f64, 0.0_f64),
        (0.0, 179.9),
        (0.0, 180.1),
        (45.0, 270.0),
        (-89.0, 33.0),
        (89.5, 200.0),
    ] {
        let (a, b) = (lat.to_radians(), lon.to_radians());
        let direction = [a.cos() * b.cos(), a.cos() * b.sin(), a.sin()];
        let by_angles = grid.sample_m(a, b);
        let by_direction = grid.sample_direction_m(direction);
        assert!(
            (by_angles - by_direction).abs() < 1e-6,
            "({lat}, {lon}): {by_angles} against {by_direction}"
        );
    }
}

/// Pixel registration: the first sample's centre sits half a cell inside.
///
/// A number rather than an argument: without the half-pixel shift the lunar
/// map would slide by 3790 m, exactly half a cell of this product.
#[test]
fn the_grid_is_pixel_registered_and_the_half_pixel_is_worth_metres() {
    let grid = moon();
    let (line, sample) = grid.index_of(90.0_f64.to_radians(), 0.0);
    assert!(
        (line - (-0.5)).abs() < 1e-9,
        "the north pole should have landed on the first row's edge, but landed \
         on {line}"
    );
    assert!(
        (sample - (-0.5)).abs() < 1e-9,
        "zero longitude should have landed on the first column\'s edge, but \
         landed on {sample}"
    );

    let half = grid.metres_per_pixel / 2.0;
    println!("  half a pixel of this product is {half:.0} m at the equator");
    assert!((3700.0..3900.0).contains(&half));
}
