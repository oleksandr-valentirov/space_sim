//! Yale BSC5 into our star asset (ROADMAP, stage Z, Z2).
//!
//! ## The source, and why it is read by hand
//!
//! BSC5 is a fixed-width text table, one line per star, described byte by byte
//! in the ReadMe that ships beside it. The fields this cooker wants sit at
//! columns 76-90 (right ascension and declination, J2000), 103-107 (visual
//! magnitude) and 110-114 (B-V colour index). Everything else -- proper
//! motions, parallaxes, spectral types, double star separations -- is read
//! past. Reading twenty columns with a library would mean importing thousands
//! of lines to avoid writing twenty, and the twenty are the ones the oracle
//! sits on.
//!
//! ## Blank lines are the format, not corruption
//!
//! Stars removed from the catalogue keep their line and their Harvard Revised
//! number with every measured field blank; the ReadMe says so under Note (1).
//! They are skipped, so the count in the finished asset is **not** 9110 --
//! worth knowing before someone reads the difference as a parsing bug.
//! Measured on the distributed file: **9096 stars out of 9110 lines, 14
//! skipped**, magnitudes -1.46 (Sirius) to 7.96, 177 KiB.
//!
//! ## Where the magnitude cut is
//!
//! Nowhere: the whole catalogue is kept. BSC5 is already cut at about 6.5
//! magnitudes -- naked-eye visibility -- by its own definition, and the result
//! is under 200 KiB. A second cut here would be a decision about what the sky
//! looks like, and that decision belongs to whoever draws it with an exposure
//! in hand (Z1), not to the file.

use engine::stars::{declination, direction, right_ascension, Catalogue, Star};

/// A fixed-width field, by the **one-based inclusive** columns the ReadMe
/// uses.
///
/// One-based because every reference to this format is: transcribing "103-107"
/// as `102..107` at each of nine call sites is nine chances to be off by one,
/// and the compiler cannot tell. `None` when the line stops short -- trailing
/// blanks are trimmed in the distributed file, so a line ending before column
/// 197 is normal and not damage.
fn field(line: &[u8], first: usize, last: usize) -> Option<&str> {
    let (from, to) = (first - 1, last);
    if line.len() < to {
        return None;
    }
    std::str::from_utf8(&line[from..to]).ok()
}

/// A blank field means "this star was withdrawn", not "this number is zero".
fn number(line: &[u8], first: usize, last: usize) -> Option<f64> {
    let text = field(line, first, last)?.trim();
    if text.is_empty() {
        return None;
    }
    text.parse::<f64>().ok()
}

/// Cook the catalogue text into stars.
///
/// Takes the text rather than a path so the oracle can run on three lines
/// instead of a megabyte, and so the caller decides where the file comes from:
/// the raw data is outside git (Q5) and fetching it is debt D18.
pub fn cook(text: &str) -> Result<Catalogue, String> {
    let mut stars = Vec::new();

    for line in text.lines() {
        let line = line.as_bytes();

        // Position first: a line with no right ascension is a withdrawn star,
        // and there is nothing further to read on it.
        let (Some(ra_h), Some(ra_m), Some(ra_s)) = (
            number(line, 76, 77),
            number(line, 78, 79),
            number(line, 80, 83),
        ) else {
            continue;
        };
        let (Some(dec_d), Some(dec_m), Some(dec_s)) = (
            number(line, 85, 86),
            number(line, 87, 88),
            number(line, 89, 90),
        ) else {
            continue;
        };
        // The sign is its own column, and it is the one field where a wrong
        // guess is silent: a star at -16 degrees read as +16 is still a
        // perfectly ordinary star, in the wrong hemisphere.
        let sign = match field(line, 84, 84) {
            Some("-") => -1.0,
            _ => 1.0,
        };

        // A star without a magnitude cannot be drawn at all, so it is dropped
        // rather than given a default: an invented brightness would put a
        // star of the wrong size on the sky, and nothing downstream could
        // tell it from a real one.
        let Some(magnitude) = number(line, 103, 107) else {
            continue;
        };

        stars.push(Star {
            dir: direction(
                right_ascension(ra_h, ra_m, ra_s),
                declination(sign, dec_d, dec_m, dec_s),
            ),
            magnitude: magnitude as f32,
            // Blank B-V becomes zero, which is white. The catalogue has
            // genuine zeroes too (Vega, Sirius), and the two are deliberately
            // indistinguishable: both draw the same.
            colour_index: number(line, 110, 114).unwrap_or(0.0) as f32,
        });
    }

    if stars.is_empty() {
        return Err("no star in this text has both a position and a magnitude".to_string());
    }
    Ok(Catalogue { stars })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real lines, copied byte for byte out of the distributed file.
    ///
    /// Real rather than hand-typed: the columns are the thing under test, and
    /// a line written to fit the parser would test the parser against itself.
    const SIRIUS: &str = "2491  9Alp CMaBD-16 1591  48915151881 257I   5423           064044.6-163444064508.9-164258227.22-08.88-1.46   0.00 -0.05 -0.03   A1Vm               -0.553-1.205 +.375-008SBO    13 10.3  11.2AB   4*";
    const POLARIS: &str = " 424  1Alp UMiBD+88    8   8890   308 907    1477  Alp UMi  012233.7+884626023148.7+891551123.28 26.46 2.02  +0.60 +0.38 +0.31   F7:Ib-II          v+0.038-0.015 +.007-017SBO    17  6.8  18.4AB   5*";
    /// A withdrawn star: the number is there, every measured field is blank.
    const WITHDRAWN: &str = "  92 NOVA 1572                                     B Cas                                                                                                                                            *";

    /// The named stars land where an independent source puts them.
    ///
    /// This is the step's oracle, and what makes it one is that the expected
    /// values are written here in the form a catalogue-independent reference
    /// publishes them -- hours and degrees -- rather than copied out of the
    /// same columns the parser reads. An off-by-one in a column would move a
    /// star and still parse.
    #[test]
    fn the_named_stars_land_where_an_independent_source_puts_them() {
        let cooked = cook(&format!("{SIRIUS}\n{POLARIS}")).expect("two stars should cook");
        assert_eq!(cooked.stars.len(), 2);

        // Sirius: 06h 45m 08.9s, -16 deg 42' 58", V = -1.46.
        let sirius = cooked.stars[0];
        let expected = direction(
            right_ascension(6.0, 45.0, 8.9),
            declination(-1.0, 16.0, 42.0, 58.0),
        );
        for k in 0..3 {
            assert!(
                (sirius.dir[k] - expected[k]).abs() < 1.0e-6,
                "Sirius is at {:?} instead of {expected:?}",
                sirius.dir
            );
        }
        assert!((sirius.magnitude - (-1.46)).abs() < 1.0e-5);
        // Southern declination, and the sign column is the only thing saying
        // so -- this is the assertion that fails if it is misread.
        assert!(
            sirius.dir[2] < 0.0,
            "Sirius came out in the wrong hemisphere"
        );

        // Polaris: 02h 31m 48.7s, +89 deg 15' 51", V = 2.02. Nearly on the
        // axis, which is why it is here: it pins the pole convention.
        let polaris = cooked.stars[1];
        assert!((polaris.magnitude - 2.02).abs() < 1.0e-5);
        assert!(
            polaris.dir[2] > 0.999,
            "Polaris should be within a degree of +z, and is at {:?}",
            polaris.dir
        );
        assert!((polaris.colour_index - 0.60).abs() < 1.0e-5);
    }

    /// A withdrawn star is skipped rather than placed at the origin.
    ///
    /// The failure this prevents is not a crash: blank fields parsed as zero
    /// would put fourteen stars at right ascension zero, declination zero --
    /// a small false constellation in Pisces that looks like data.
    #[test]
    fn a_withdrawn_star_is_skipped_and_not_placed_at_the_origin() {
        let cooked =
            cook(&format!("{SIRIUS}\n{WITHDRAWN}\n{POLARIS}")).expect("two stars should cook");
        assert_eq!(
            cooked.stars.len(),
            2,
            "the withdrawn line should not have become a star"
        );
    }

    /// A text with nothing readable in it is an error, not an empty sky.
    #[test]
    fn a_file_with_no_stars_is_an_error() {
        cook(WITHDRAWN).expect_err("a catalogue of withdrawn stars should be refused");
    }
}
