//! LROC WAC mosaic reader (stage T, step T2b).
//!
//! The oracles are split by what each can catch **on its own**, and by what
//! must be on disk for it:
//!
//! 1. **the label** -- the numbers all the arithmetic depends on. Checked
//!    separately from the pixels, because that is exactly what git holds (the
//!    66 MB mosaic is not, Q5), and because no error in it is visible in a
//!    picture;
//! 2. **a hand-built mosaic** -- registration, byte order, bilinear weights
//!    and refusal of special values, over a file whose every sample is known
//!    in advance. These oracles always run; they need no source;
//! 3. **the data itself** -- map orientation, which splits into **two** claims
//!    rather than one. Latitude is caught by "maria are darker than
//!    highlands"; that does **not** catch longitude (measured), which has its
//!    own pair of points. Both are skipped without the source -- and say what
//!    is missing.

use dem_cook::albedo::{Albedo, Header};
use std::path::{Path, PathBuf};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/wac")
        .join(name)
}

/// The mosaic itself, or `None` -- then the test says what is missing and does
/// not fail (Q5).
fn mosaic() -> Option<Albedo> {
    let path = data("wac_global_016p.img");
    match Albedo::read(&path) {
        Ok(map) => Some(map),
        Err(_) => {
            eprintln!(
                "SKIPPED: missing {}. How to put it back: data/wac/README.md",
                path.display()
            );
            None
        }
    }
}

/// Mean reflectance over a 5x5 degree square about a point.
///
/// A mean rather than a pixel: the mosaic was shot at incidence angles of
/// 53-70 degrees, so a single crater's shadow is darker than any mare, and one
/// sample says nothing about what lies under it.
fn box_mean(map: &Albedo, lat: f64, lon: f64) -> f64 {
    let degrees = std::f64::consts::PI / 180.0;
    let half = 2.5;
    let steps = 20;
    let mut sum = 0.0;
    for a in 0..steps {
        for b in 0..steps {
            let dl = -half + 2.0 * half * (f64::from(a) + 0.5) / f64::from(steps);
            let ds = -half + 2.0 * half * (f64::from(b) + 0.5) / f64::from(steps);
            sum += map.sample((lat + dl) * degrees, (lon + ds) * degrees);
        }
    }
    sum / f64::from(steps * steps)
}

/// The label gives exactly the numbers the reader rests on.
///
/// The last claim is not a restatement of the label but a cross-check of **two
/// of its fields against each other**: `MAP_SCALE` must equal the Moon's
/// circumference divided by the pixel count along the equator. That is the
/// check on `MAP_RESOLUTION`, which does not exist on its own: an error in it
/// would shift the whole map twofold and touch no other field.
#[test]
fn the_label_gives_the_numbers_the_reader_stands_on() {
    let bytes = std::fs::read(data("wac_global_016p.lbl")).expect("the label lives in git");
    let header = Header::parse(&bytes).expect("the label should have parsed");

    println!(
        "  {}x{} samples, {} px/degree, {:.2} m/pixel; pixels from byte {}",
        header.samples,
        header.lines,
        header.per_degree,
        header.metres_per_pixel,
        header.data_offset
    );

    assert_eq!(header.samples, 5760);
    assert_eq!(header.lines, 2880);
    assert_eq!(header.per_degree, 16.0);
    // `^IMAGE = 2` records of 23040 bytes: the pixels begin exactly where the
    // single label record ends. Zero here would mean the reader took the label
    // text for the picture's first row.
    assert_eq!(header.data_offset, 23_040);
    assert_eq!(bytes.len(), header.data_offset);

    // The circumference at a 1737.4 km radius, divided by 5760 equatorial
    // pixels.
    let moon_radius_m = 1_737_400.0;
    let along_equator = 2.0 * std::f64::consts::PI * moon_radius_m / header.samples as f64;
    let error = (header.metres_per_pixel - along_equator).abs() / along_equator;
    assert!(
        error < 1e-3,
        "MAP_SCALE {:.3} m/pixel against {along_equator:.3} from geometry -- {:.1}% apart",
        header.metres_per_pixel,
        error * 100.0
    );
}

/// A hand-built grid: every sample equals its row number.
///
/// A file with an embedded label, like the real product, but 8x4 samples and
/// values known in advance. Such an oracle catches what no check on real data
/// catches: `at` outside the grid, half-cell registration and the bilinear
/// weights -- on the real mosaic all three would give plausible numbers.
fn hand_made(values: &[f32], samples: usize, lines: usize) -> Vec<u8> {
    // The record deliberately does **not** equal a picture row, though it does
    // in the real product: then the offset to the pixels really is computed
    // from two label fields rather than coinciding with something the reader
    // already knows.
    let record = 1024;
    let label = format!(
        "PDS_VERSION_ID = PDS3\r\n\
         RECORD_TYPE   = FIXED_LENGTH\r\n\
         RECORD_BYTES  = {record}\r\n\
         LABEL_RECORDS = 1\r\n\
         ^IMAGE        = 2\r\n\
         OBJECT = IMAGE_MAP_PROJECTION\r\n\
         MAP_PROJECTION_TYPE = EQUIRECTANGULAR\r\n\
         MAP_RESOLUTION = {res} <PIX/DEG>\r\n\
         MAP_SCALE = 1.0 <METERS/PIXEL>\r\n\
         END_OBJECT = IMAGE_MAP_PROJECTION\r\n\
         OBJECT = IMAGE\r\n\
         LINES = {lines}\r\n\
         LINE_SAMPLES = {samples}\r\n\
         SAMPLE_TYPE = PC_REAL\r\n\
         SAMPLE_BITS = 32\r\n\
         BANDS = 1\r\n\
         END_OBJECT = IMAGE\r\n\
         END\r\n",
        res = samples as f64 / 360.0,
    );
    assert!(label.len() <= record, "the label did not fit in the record");

    let mut bytes = label.into_bytes();
    bytes.resize(record, 0);
    for v in values {
        bytes.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    bytes
}

fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, bytes).expect("the temporary file should have written");
    path
}

#[test]
fn a_hand_made_mosaic_reads_back_exactly() {
    let (samples, lines) = (8usize, 4usize);
    // Value = row number: then sampling in latitude must give the latitude
    // itself, and sampling in longitude must move nothing.
    let values: Vec<f32> = (0..lines)
        .flat_map(|line| (0..samples).map(move |_| line as f32))
        .collect();
    let path = write_temp(
        "space_sim_wac_rows.img",
        &hand_made(&values, samples, lines),
    );
    let map = Albedo::read(&path).expect("the hand-built mosaic should have read");
    std::fs::remove_file(&path).ok();

    assert_eq!((map.samples, map.lines), (samples, lines));
    assert_eq!(map.per_degree, samples as f64 / 360.0);
    assert_eq!(map.measured(), (0.0, (lines - 1) as f32));

    // Row centres: the latitude of row `l`'s centre is
    // `90 - (l + 0.5)/per_degree` degrees, and sampling there must give exactly
    // `l`, with no interpolation.
    let degrees = std::f64::consts::PI / 180.0;
    for line in 0..lines {
        let lat = (90.0 - (line as f64 + 0.5) / map.per_degree) * degrees;
        let got = map.sample(lat, 0.0);
        assert!(
            (got - line as f64).abs() < 1e-9,
            "the centre of row {line} gave {got}, but must give {line}"
        );
    }

    // Exactly between two row centres, a half. That is the weight check: a
    // half-cell shift would make this a whole number.
    let between = (90.0 - 1.0 / map.per_degree) * degrees;
    let got = map.sample(between, 0.0);
    assert!(
        (got - 0.5).abs() < 1e-9,
        "between the centres of rows 0 and 1 sampling gave {got}, but must give 0.5"
    );

    // Longitude wraps, latitude clamps -- both edges of the grid.
    assert_eq!(map.at(0, samples as i64), map.at(0, 0));
    assert_eq!(map.at(-1, 0), map.at(0, 0));
    assert_eq!(map.at(lines as i64, 0), map.at(lines as i64 - 1, 0));
}

/// A PDS3 special value stops the read rather than travelling on as a number.
///
/// The check exists because the silent path here would look fine: -3.4e38 in
/// bilinear sampling gives a black patch of the right shape, and no other
/// oracle will ask about it.
#[test]
fn a_special_value_stops_the_reader() {
    let (samples, lines) = (8usize, 4usize);
    let mut values = vec![0.5f32; samples * lines];
    values[13] = f32::from_bits(0xFF7F_FFFB);
    let path = write_temp(
        "space_sim_wac_null.img",
        &hand_made(&values, samples, lines),
    );
    let result = Albedo::read(&path);
    std::fs::remove_file(&path).ok();

    let message = result
        .expect_err("the reader must have refused")
        .to_string();
    assert!(
        message.contains("1 PDS3 special values"),
        "wrong message: {message}"
    );
}

/// The maria are darker than the highlands -- and exactly where they really
/// are.
///
/// An oracle that asks about **orientation**: a map flipped in latitude has
/// the same dimensions, the same range and the same label. The numbers are
/// means over 5x5 degree squares rather than individual pixels: the mosaic was
/// shot at large incidence angles, so a single pixel in a crater's shadow is
/// darker than any mare.
///
/// WARNING: **this claim does not catch the sign of longitude, and that is
/// measured rather than guessed.** A flipped sign passed all four checks in
/// this file, because the near side's maria are scattered almost symmetrically
/// about the prime meridian: the mirror maps mare to mare, and the far side
/// onto itself. The sign has its own check below.
#[test]
fn the_maria_are_darker_than_the_highlands() {
    let Some(map) = mosaic() else {
        return;
    };

    let maria = [
        ("Serenitatis", 28.0, 17.5),
        ("Imbrium", 35.0, 345.0),
        ("Oceanus Procellarum", 18.0, 303.0),
        ("Tranquillitatis", 8.0, 31.0),
        ("Crisium", 17.0, 59.0),
    ];
    let highlands = [
        ("south of Ptolemaeus", -20.0, 355.0),
        ("far side, -10", -10.0, 180.0),
        ("far side, +10", 10.0, 200.0),
        ("far side, -25", -25.0, 150.0),
    ];

    let mut darkest_highland = f64::MAX;
    let mut brightest_mare = f64::MIN;
    for (name, lat, lon) in maria {
        let value = box_mean(&map, lat, lon);
        println!("  mare {name}: {value:.4}");
        brightest_mare = brightest_mare.max(value);
    }
    for (name, lat, lon) in highlands {
        let value = box_mean(&map, lat, lon);
        println!("  highland {name}: {value:.4}");
        darkest_highland = darkest_highland.min(value);
    }

    // A gap rather than mere inequality: measured 0.0267 against 0.0456, i.e.
    // a factor of 1.7. The 1.3 multiplier leaves margin for the choice of
    // points while still failing under any flip of the map -- there the
    // numbers swap places.
    assert!(
        darkest_highland > 1.3 * brightest_mare,
        "the brightest mare {brightest_mare:.4} and the darkest highland \
         {darkest_highland:.4} are not separated -- the map lies the wrong way"
    );
}

/// East is east: mirroring longitude breaks the map, and here is the point
/// that sees it.
///
/// This check appeared because the previous one **did not catch** the sign of
/// longitude, and that was measured: a flipped sign passed all four tests. The
/// cause is the Moon's own symmetry rather than a weak oracle: the near side's
/// maria lie almost symmetrically about the prime meridian, the far side
/// mirrors onto itself, so the mirror permutes "mare against highland" pairs
/// into one another.
///
/// They are told apart by a pair whose mirrored points belong to **different**
/// classes, found by a sweep over the whole map rather than from memory. The
/// best turned out to be a memorable one: **Mare Tranquillitatis (10 N,
/// 20 E)** and its mirror, **Copernicus (10 N, 20 W)**, a crater with a bright
/// ray system. Measured: 0.0207 against 0.0466, so a flipped sign would swap
/// dark for light by a factor of two.
#[test]
fn east_is_east_and_the_mirror_of_a_mare_is_a_bright_crater() {
    let Some(map) = mosaic() else {
        return;
    };

    let mare = box_mean(&map, 10.0, 20.0);
    let crater = box_mean(&map, 10.0, -20.0);
    println!("  Tranquillitatis (20 E): {mare:.4}; Copernicus (20 W): {crater:.4}");

    assert!(
        crater > 1.5 * mare,
        "20 E ({mare:.4}) and 20 W ({crater:.4}) are not separated -- the sign \
         of longitude or the prime meridian is wrong"
    );
}
