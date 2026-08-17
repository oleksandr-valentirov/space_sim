//! The star catalogue as an asset (ROADMAP.md, stage Z, step Z2).
//!
//! A catalogue rather than noise, by rule 8 of stage R -- data with provenance
//! and a licence. Procedural noise would give stars; it would not give a sky.
//! Orion has to be Orion, or the background stops being an orientation check
//! and becomes wallpaper.
//!
//! ## What is kept, and what is thrown away
//!
//! Yale BSC5 carries 197 columns per star: proper motions, parallaxes, double
//! star separations, spectral types, radial velocities. A frame needs three
//! numbers -- **where, how bright, what colour** -- and the rest would be a
//! promise the renderer never keeps.
//!
//! Proper motion is the one omission worth naming, because it looks like an
//! oversight and is not. The largest in the catalogue is about 10 arcsec/yr
//! (Barnard's star, and it is too faint to be here at all); over the two
//! centuries the ephemeris covers that is half an arcminute, against a star
//! drawn a pixel wide at a field of view of sixty degrees. The sky does not
//! move within this game's horizon, and storing a velocity nobody integrates
//! would be a field with no reader (CLAUDE.md).
//!
//! ## The frame the directions are in
//!
//! Equatorial J2000, the same frame the catalogue's own RA/Dec are given in,
//! turned into unit vectors once by the cooker so the runtime never sees an
//! angle. **This is the assumption the whole asset rests on:** the world axes
//! of the simulation are taken to be that frame. They are -- the ephemeris is
//! read in it -- but nothing in the file says so, which is exactly why it is
//! said here.

/// The file signature. Eight bytes like every other asset of ours, so a wrong
/// file is caught by its first word rather than by a nonsensical star count.
pub const MAGIC: [u8; 8] = *b"SSTAR\0\0\0";

/// Format version.
pub const VERSION: u32 = 1;

/// Magic, version, count.
const HEADER_BYTES: usize = 8 + 4 + 4;

/// Bytes per star: three direction components, a magnitude, a colour index.
const STAR_BYTES: usize = 5 * 4;

/// One star: where it is, how bright it is, what colour it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Star {
    /// Unit direction in equatorial J2000, world axes.
    pub dir: [f32; 3],
    /// Apparent visual magnitude. **Smaller is brighter**, and the scale is
    /// logarithmic: five magnitudes are a factor of a hundred in flux. The
    /// conversion belongs to whoever draws, not here -- it needs the exposure
    /// (Z1), and a brightness stored in the file would bake one exposure into
    /// the asset for ever.
    pub magnitude: f32,
    /// The B-V colour index: negative is blue, positive is red. Zero for the
    /// stars whose colour the catalogue does not give, which is what the
    /// catalogue itself means by a blank field -- Vega's B-V is 0.00 too, and
    /// the two cases are indistinguishable here on purpose. Nothing downstream
    /// can tell them apart either: both draw white.
    pub colour_index: f32,
}

/// The whole catalogue.
#[derive(Debug)]
pub struct Catalogue {
    pub stars: Vec<Star>,
}

impl Catalogue {
    /// The file bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_BYTES + self.stars.len() * STAR_BYTES);
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(self.stars.len() as u32).to_le_bytes());
        for star in &self.stars {
            for value in star.dir {
                out.extend_from_slice(&value.to_le_bytes());
            }
            out.extend_from_slice(&star.magnitude.to_le_bytes());
            out.extend_from_slice(&star.colour_index.to_le_bytes());
        }
        out
    }

    /// Parse the file bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Catalogue, String> {
        if bytes.len() < HEADER_BYTES {
            return Err(format!("{} bytes is not even a header", bytes.len()));
        }
        if bytes[..8] != MAGIC {
            return Err("wrong signature: this is not a star catalogue".to_string());
        }
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let version = word(8);
        if version != VERSION {
            return Err(format!(
                "format version {version}, while this engine reads {VERSION}"
            ));
        }
        let count = word(12) as usize;
        let wanted = HEADER_BYTES + count * STAR_BYTES;
        // The length is checked against the count rather than trusted, because
        // a truncated download is the failure this format will actually meet:
        // the raw catalogue is fetched from a mirror (debt D18), and half a
        // file parses perfectly up to the byte where it stops.
        if bytes.len() != wanted {
            return Err(format!(
                "{count} stars need {wanted} bytes, and the file has {}",
                bytes.len()
            ));
        }

        let float = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let mut stars = Vec::with_capacity(count);
        for i in 0..count {
            let at = HEADER_BYTES + i * STAR_BYTES;
            stars.push(Star {
                dir: [float(at), float(at + 4), float(at + 8)],
                magnitude: float(at + 12),
                colour_index: float(at + 16),
            });
        }
        Ok(Catalogue { stars })
    }
}

/// A unit direction from right ascension and declination, both in radians.
///
/// Here rather than in the cooker because the reader's test needs it too: an
/// oracle that checks a star's direction has to say where that star should be,
/// and saying it in RA/Dec is the only form an independent source publishes.
pub fn direction(ra: f64, dec: f64) -> [f32; 3] {
    let (sin_dec, cos_dec) = dec.sin_cos();
    let (sin_ra, cos_ra) = ra.sin_cos();
    [
        (cos_dec * cos_ra) as f32,
        (cos_dec * sin_ra) as f32,
        sin_dec as f32,
    ]
}

/// Right ascension in radians from hours, minutes and seconds.
pub fn right_ascension(h: f64, m: f64, s: f64) -> f64 {
    (h + m / 60.0 + s / 3600.0) * std::f64::consts::PI / 12.0
}

/// Declination in radians from a sign and degrees, arcminutes, arcseconds.
pub fn declination(sign: f64, d: f64, m: f64, s: f64) -> f64 {
    sign * (d + m / 60.0 + s / 3600.0).to_radians()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Catalogue {
        Catalogue {
            stars: vec![
                Star {
                    dir: direction(
                        right_ascension(6.0, 45.0, 8.9),
                        declination(-1.0, 16.0, 42.0, 58.0),
                    ),
                    magnitude: -1.46,
                    colour_index: 0.0,
                },
                Star {
                    dir: [0.0, 0.0, 1.0],
                    magnitude: 2.02,
                    colour_index: 0.6,
                },
            ],
        }
    }

    /// What is written comes back, to the bit.
    ///
    /// Bitwise rather than approximate, and that is the point of storing `f32`
    /// rather than angles: a catalogue that survives a round trip only to
    /// within a tolerance would put the sky in a slightly different place on
    /// every recook, and a save would stop reproducing its own screenshot.
    #[test]
    fn a_catalogue_survives_the_round_trip_exactly() {
        let before = sample();
        let after = Catalogue::from_bytes(&before.to_bytes()).expect("it should read back");

        assert_eq!(after.stars.len(), before.stars.len());
        for (a, b) in after.stars.iter().zip(&before.stars) {
            assert_eq!(a, b, "a star changed across the round trip");
        }
    }

    /// A truncated file is refused rather than half-read.
    #[test]
    fn half_a_file_is_an_error_and_not_half_a_sky() {
        let bytes = sample().to_bytes();
        let half = &bytes[..bytes.len() - STAR_BYTES / 2];

        let error = Catalogue::from_bytes(half).expect_err("a short file should be refused");
        assert!(
            error.contains("bytes"),
            "the error should say what was expected: {error}"
        );
    }

    /// Someone else's file is refused by its signature.
    #[test]
    fn another_asset_is_not_read_as_a_sky() {
        let mut bytes = sample().to_bytes();
        bytes[..8].copy_from_slice(b"SSDEM\0\0\0");

        let error = Catalogue::from_bytes(&bytes).expect_err("a foreign file should be refused");
        assert!(error.contains("signature"), "unexpected error: {error}");
    }

    /// The directions are unit vectors, and the poles land on the axis.
    ///
    /// The check that catches a swapped sine and cosine: at declination +90
    /// every right ascension must give the same point, and it must be `+z`. A
    /// swap leaves the vector unit-length, so length alone would pass.
    #[test]
    fn the_pole_is_on_the_axis_from_every_right_ascension() {
        for hours in 0..24 {
            let dir = direction(
                right_ascension(f64::from(hours), 0.0, 0.0),
                declination(1.0, 90.0, 0.0, 0.0),
            );
            let length = (dir.iter().map(|v| v * v).sum::<f32>()).sqrt();
            assert!((length - 1.0).abs() < 1e-6, "not a unit vector: {dir:?}");
            assert!(dir[2] > 0.999_999, "the pole drifted off the axis: {dir:?}");
        }
    }
}
