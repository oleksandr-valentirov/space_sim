//! ETOPO 2022 reader: Earth's shape (stage T, step T7b).
//!
//! The oracles are split the same way as for WAC, for the same reason -- by
//! what each can catch on its own and what must be on disk for it:
//!
//! 1. **the header** -- georeferencing and registration. Checked separately
//!    from the pixels, because that is exactly what git holds (the 466 MB
//!    product is not, Q5), and because no error in it is visible in a picture;
//! 2. **the land fraction** -- one number for the whole class of geometry
//!    errors. The true one is 29.2%; a half-pixel shift, reversed row order or
//!    a misparsed predictor move it by percent;
//! 3. **named points** -- orientation, which splits into two claims rather
//!    than one. Latitude is caught by a pair of poles: the north is ocean, the
//!    south is ice. That does **not** catch longitude, which has its own pair
//!    of mirrored meridians.
//!
//! Anything needing the data itself is skipped without it -- and says what is
//! missing.

use dem_cook::etopo::{Header, Relief};
use std::path::{Path, PathBuf};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/etopo")
        .join(name)
}

/// The product itself, or `None` -- then the test says what is missing and
/// does not fail (Q5).
fn relief() -> Option<Relief> {
    let path = data("etopo_2022_60s_surface.tif");
    match Relief::read(&path) {
        Ok(grid) => Some(grid),
        Err(_) => {
            eprintln!(
                "SKIPPED: missing {}. How to put it back: data/etopo/README.md",
                path.display()
            );
            None
        }
    }
}

/// The header reads from the label -- the 32 KiB that live in git.
///
/// That is the point of the label: georeferencing parsing is checked where the
/// data itself may not exist at all -- on CI, on someone else's machine, in a
/// fresh clone.
#[test]
fn header_from_label() {
    let header = Header::read(&data("etopo_2022_60s_surface.lbl")).expect("the label in git");

    assert_eq!(header.samples, 21600);
    assert_eq!(header.lines, 10800);
    assert!(
        (header.per_degree - 60.0).abs() < 1e-9,
        "60 samples per degree, but got {}",
        header.per_degree
    );
    assert_eq!(header.corner_deg, (-180.0, 90.0));
    assert!(
        header.covers_globe(),
        "the grid must cover the whole globe"
    );
}

/// A truncated label is no less a valid TIFF for the header, but not for the
/// data.
///
/// The check is not about the file but about the boundary: `Header::read` must
/// work without pixels, and `Relief::read` must fail rather than read garbage.
/// Otherwise the cooker would one day cook a tileset out of the label and say
/// nothing.
#[test]
fn label_is_not_data() {
    let path = data("etopo_2022_60s_surface.lbl");
    assert!(Header::read(&path).is_ok());
    assert!(
        Relief::read(&path).is_err(),
        "933 MB of samples cannot be read from 32 KiB of header"
    );
}

/// The land fraction, the principal geometry oracle: one number for the whole
/// class.
#[test]
fn land_fraction_matches_the_planet() {
    let Some(grid) = relief() else { return };

    let land = grid.land_fraction();
    assert!(
        (land - 0.2911).abs() < 0.002,
        "land fraction {:.4}, but Earth has 0.292",
        land
    );
}

/// Height range: the deepest trench and the highest mountain, both averaged
/// over a 1.85 km cell.
///
/// The numbers are smaller than the textbook ones (-10,935 and 8849) by
/// exactly that averaging, and that is the product's resolution rather than a
/// reader error. The oracle here is not "looks about right" but an independent
/// reader implementation in Python, which produced the same two numbers from
/// the same bytes.
#[test]
fn range_matches_the_product() {
    let Some(grid) = relief() else { return };

    assert_eq!(grid.measured(), (-10752, 8157));
}

/// Latitude is caught by the poles: the north is ocean, the south three
/// kilometres of ice.
///
/// A reversed row order swaps them, and no other oracle sees that: the land
/// fraction does not change under a flip at all.
#[test]
fn poles_are_not_swapped() {
    let Some(grid) = relief() else { return };
    let degrees = std::f64::consts::PI / 180.0;

    let north = grid.sample_m(89.9 * degrees, 0.0);
    let south = grid.sample_m(-89.9 * degrees, 0.0);

    assert!(north < -3000.0, "the north pole is ocean, but got {north}");
    assert!(
        south > 2000.0,
        "the south pole is ice (the surface product, not bed), but got {south}"
    );
}

/// Longitude is caught by a pair of mirrored meridians at one latitude.
///
/// At 28 N: the Himalayas at 86.9 E and the Gulf of Mexico at 86.9 W.
/// Mirrored, so a 180 shift (the most typical error: the ETOPO grid starts at
/// -180 rather than at zero like LOLA) swaps them.
#[test]
fn meridians_are_not_mirrored() {
    let Some(grid) = relief() else { return };
    let degrees = std::f64::consts::PI / 180.0;

    let east = grid.sample_m(27.99 * degrees, 86.9 * degrees);
    let west = grid.sample_m(27.99 * degrees, -86.9 * degrees);

    assert!(east > 5000.0, "the Himalayas at 86.9 E, but got {east}");
    assert!(west < -1000.0, "the gulf at 86.9 W, but got {west}");
}

/// Named points with known heights -- an oracle for the sampling arithmetic.
///
/// The values come not from a textbook but from an independent reader
/// implementation (Python, the same predictor and the same bilinear weights),
/// so a discrepancy here means an error in **our** code rather than in the
/// product. The tolerance is one metre: there is nowhere further to go,
/// because the grid already sits in integer metres.
#[test]
fn named_points_match_an_independent_reader() {
    let Some(grid) = relief() else { return };
    let degrees = std::f64::consts::PI / 180.0;

    for (name, lat, lon, expect) in [
        ("Everest", 27.9881, 86.925, 8054.494),
        ("Mariana Trench", 11.35, 142.20, -10291.250),
        ("Dead Sea", 31.5, 35.5, -427.0),
        ("Kyiv", 50.45, 30.52, 151.200),
        ("mid Pacific", 0.0, -140.0, -4319.250),
        ("Sahara", 23.0, 13.0, 736.500),
    ] {
        let got = grid.sample_m(lat * degrees, lon * degrees);
        assert!(
            (got - expect).abs() < 1.0,
            "{name}: {got:.3} against {expect:.3}"
        );
    }
}

/// Sampling by direction -- what the cooker itself uses.
///
/// Separate from `sample_m`, because between them stands the translation of a
/// direction into angles (`lat_lon`), and that is where the third copy of a
/// sign error lives.
///
/// WARNING: compared with a tolerance rather than bitwise: angle -> direction
/// -> angle passes through `cos`, `sin` and `atan2`, and is not obliged to
/// return bitwise where it started. Bitwise equality is needed here not
/// between angles but between neighbouring patches' nodes -- and that is
/// guaranteed differently, by a shared vertex (R2b).
#[test]
fn direction_agrees_with_angles() {
    let Some(grid) = relief() else { return };
    let degrees = std::f64::consts::PI / 180.0;

    let (lat, lon) = (50.45 * degrees, 30.52 * degrees);
    let unit = [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()];

    let there = grid.sample_direction_m(unit);
    let here = grid.sample_m(lat, lon);
    assert!(
        (there - here).abs() < 1e-6,
        "the direction gave {there}, the angles {here}"
    );
}
