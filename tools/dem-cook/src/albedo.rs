//! LROC WAC global mosaic reader: lunar reflectance (stage T, T2b).
//!
//! The second surface source alongside LOLA, with the same shape of step as
//! R5a: first the data on disk and a reader with an oracle, and only then
//! something done with them. The grid geometry is shared with the heights --
//! the same simple cylindrical projection, the same pixel registration -- so
//! registration lives in [`crate::index_of`] rather than in two copies.
//!
//! ## Three differences from LOLA, each of which would break reading silently
//!
//! 1. **The label is embedded, not separate.** No `.LBL` file exists beside it
//!    (the server returns 404 for one): the PDS3 header sits at the head of
//!    the `.IMG` itself, and where it ends and the pixels begin is stated by
//!    the header -- `^IMAGE = 2` records of `RECORD_BYTES`. Reading the file
//!    from offset zero means taking the label text for the picture's first
//!    row;
//! 2. **samples are reals, not integers.** `SAMPLE_TYPE = PC_REAL`,
//!    `SAMPLE_BITS = 32`, and the value is reflectance (0.02 in the maria,
//!    0.05 in the highlands) rather than storage units with a scale;
//! 3. **the format has special values** -- `CORE_NULL` and four saturations,
//!    given as bit patterns (`16#FF7FFFFB#` and neighbours). As `f32` those
//!    are -3.4e38, a number bilinear sampling would smear across four nodes
//!    and quantisation would clamp to zero. It would look like a black patch
//!    of the right shape.
//!
//! ## What the reader does with empty pixels: nothing, loudly
//!
//! There is no fill rule here, and adding one in advance is not allowed -- it
//! would be a guess about data we have not seen. Measured instead: in
//! `WAC_GLOBAL_E000N1800_016P` there is **not one** special value; all
//! 16,588,800 samples are real. So the reader **counts them and fails** if
//! even one occurs: a product with holes is a different product and must not
//! be cooked silently.

use std::path::Path;

use crate::{label_values, number};

/// How many bytes of the file's head are read as label text.
///
/// Not `RECORD_BYTES` -- that still has to come from somewhere, and the only
/// somewhere is the label itself. The chicken and egg resolve like this: take
/// deliberately more than the label can be (the real one is 4 KB of text in a
/// 23 KB record), read the keys out of that, and **only then** trust their
/// numbers.
const LABEL_PROBE_BYTES: usize = 64 * 1024;

/// PDS3 special values: empty and four saturations.
///
/// Patterns rather than floating-point numbers: comparing an `f32` for
/// equality against -3.4e38 is possible, but the bits say the same thing with
/// no question of rounding, and bits are how the label states them.
const SPECIAL: [u32; 5] = [
    0xFF7F_FFFB,
    0xFF7F_FFFC,
    0xFF7F_FFFD,
    0xFF7F_FFFE,
    0xFF7F_FFFF,
];

/// A reflectance grid in the simple cylindrical projection.
#[derive(Clone, Debug)]
pub struct Albedo {
    /// Samples along longitude (`LINE_SAMPLES`).
    pub samples: usize,
    /// Rows along latitude (`LINES`).
    pub lines: usize,
    /// Samples per degree (`MAP_RESOLUTION`).
    pub per_degree: f64,
    /// The samples themselves, row by row from north to south.
    pub raw: Vec<f32>,
}

/// What the reader takes from the label before touching any pixels.
///
/// Its own type, because the label is checked **separately from the picture**:
/// git holds the product's head (`data/wac/wac_global_016p.lbl`), exactly
/// those 23 KB, while the 66 MB file itself does not (Q5). So header parsing
/// must be callable without the data, or there would be nothing to check it
/// with.
#[derive(Clone, Debug)]
pub struct Header {
    pub samples: usize,
    pub lines: usize,
    pub per_degree: f64,
    /// Metres per pixel per the label (`MAP_SCALE`).
    pub metres_per_pixel: f64,
    /// Which byte of the file the pixels start at.
    pub data_offset: usize,
}

impl Header {
    /// Parse the embedded label from the head of the file.
    pub fn parse(head: &[u8]) -> Result<Header, String> {
        let probe = &head[..head.len().min(LABEL_PROBE_BYTES)];
        // `from_utf8_lossy`, not `from_utf8`: the record's tail is padded with
        // zeros, and the pixels themselves may already start after the label.
        // The text we care about is ASCII at the front, and this cannot
        // corrupt it.
        let text = String::from_utf8_lossy(probe);
        let values = label_values(&text);

        let kind = values
            .get("SAMPLE_TYPE")
            .map(String::as_str)
            .unwrap_or_default();
        if kind != "PC_REAL" {
            return Err(format!(
                "expected PC_REAL -- no other sample type is read here, \
                 and the label says {kind:?}"
            ));
        }
        let bits = number(&values, "SAMPLE_BITS")?;
        if bits != 32.0 {
            return Err(format!(
                "expected 32 bits per sample, the label says {bits}"
            ));
        }
        // One band is not a simplification but the product itself: WAC GLOBAL
        // is shot through a single filter (643 nm). A multi-band mosaic would
        // be laid out differently (`BAND_STORAGE_TYPE`) and must not be read
        // with this code.
        let bands = number(&values, "BANDS")?;
        if bands != 1.0 {
            return Err(format!("expected one band, the label says {bands}"));
        }
        let projection = values
            .get("MAP_PROJECTION_TYPE")
            .map(String::as_str)
            .unwrap_or_default();
        if projection != "EQUIRECTANGULAR" {
            return Err(format!(
                "expected EQUIRECTANGULAR -- the reader\'s registration is \
                 derived from it, and the label says {projection:?}"
            ));
        }

        let record_bytes = number(&values, "RECORD_BYTES")? as usize;
        let first_record = number(&values, "^IMAGE")? as usize;
        if first_record == 0 {
            return Err("^IMAGE = 0 -- PDS3 records are numbered from one".to_string());
        }

        Ok(Header {
            samples: number(&values, "LINE_SAMPLES")? as usize,
            lines: number(&values, "LINES")? as usize,
            per_degree: number(&values, "MAP_RESOLUTION")?,
            metres_per_pixel: number(&values, "MAP_SCALE")?,
            data_offset: (first_record - 1) * record_bytes,
        })
    }
}

impl Albedo {
    /// Read the mosaic: the label from the head of the same file, then the
    /// pixels.
    pub fn read(img: &Path) -> Result<Albedo, String> {
        let bytes = std::fs::read(img).map_err(|e| format!("{}: {e}", img.display()))?;
        let header = Header::parse(&bytes)?;

        let wanted = header.samples * header.lines * 4;
        let end = header.data_offset + wanted;
        if bytes.len() < end {
            return Err(format!(
                "{}: {} bytes, but the label promises {end} = {} + {}x{}x4",
                img.display(),
                bytes.len(),
                header.data_offset,
                header.samples,
                header.lines
            ));
        }

        let mut raw = Vec::with_capacity(header.samples * header.lines);
        let mut specials = 0usize;
        for chunk in bytes[header.data_offset..end].chunks_exact(4) {
            let word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            if SPECIAL.contains(&word) {
                specials += 1;
            }
            raw.push(f32::from_bits(word));
        }
        if specials > 0 {
            return Err(format!(
                "{}: {specials} PDS3 special values (empty or saturated). The \
                 reader deliberately has no fill rule -- this product has none",
                img.display()
            ));
        }

        Ok(Albedo {
            samples: header.samples,
            lines: header.lines,
            per_degree: header.per_degree,
            raw,
        })
    }

    /// A grid sample. Indices wrap in longitude and clamp in latitude --
    /// exactly how the sphere itself behaves.
    pub fn at(&self, line: i64, sample: i64) -> f32 {
        let line = line.clamp(0, self.lines as i64 - 1) as usize;
        let sample = sample.rem_euclid(self.samples as i64) as usize;
        self.raw[line * self.samples + sample]
    }

    /// Reflectance at an arbitrary point, bilinear between four samples.
    pub fn sample(&self, lat: f64, lon: f64) -> f64 {
        crate::bilinear(self.per_degree, lat, lon, |line, sample| {
            f64::from(self.at(line, sample))
        })
    }

    /// Reflectance along `direction` (not necessarily unit).
    pub fn sample_direction(&self, direction: [f64; 3]) -> f64 {
        let (lat, lon) = crate::lat_lon(direction);
        self.sample(lat, lon)
    }

    /// Angular size of a pixel, radians.
    pub fn pixel_rad(&self) -> f64 {
        std::f64::consts::PI / 180.0 / self.per_degree
    }

    /// The same grid, coarser by one chain step: each sample is a block mean.
    ///
    /// Returns `None` when there is nothing left to divide.
    /// [`crate::reduce_step`] says by how much; a mean rather than a sample,
    /// because this is not eyeball smoothing but removal of what a coarser
    /// grid can no longer represent.
    pub fn reduced(&self) -> Option<Albedo> {
        let step = crate::reduce_step(self.samples, self.lines)?;
        let (samples, lines) = (self.samples / step, self.lines / step);
        let mut raw = Vec::with_capacity(samples * lines);
        for line in 0..lines {
            for sample in 0..samples {
                let mut sum = 0.0f64;
                for dl in 0..step {
                    for ds in 0..step {
                        let l = step * line + dl;
                        let s = step * sample + ds;
                        sum += f64::from(self.raw[l * self.samples + s]);
                    }
                }
                raw.push((sum / (step * step) as f64) as f32);
            }
        }
        Some(Albedo {
            samples,
            lines,
            per_degree: self.per_degree / step as f64,
            raw,
        })
    }

    /// The chain of grids, each coarser than the last; the zeroth is this one.
    ///
    /// ## Why it exists at all, and why it is not cosmetic
    ///
    /// The cooker takes a **point** at each node, and at fine levels that is
    /// honest: the node there is finer than a source pixel. At coarse levels
    /// it is not. A level 0 node covers 85 km of the Moon, i.e. two thousand
    /// mosaic pixels, and point sampling picks one of them at random. It is
    /// visible in the demo: the distant Moon came out as blotchy noise instead
    /// of a map with visible maria.
    ///
    /// The chain removes that: a level takes the grid whose pixel is no longer
    /// finer than its node. The "a node on a shared edge is bitwise one"
    /// invariant stays intact meanwhile -- the grid choice depends **only on
    /// the level**, so both neighbours read the same grid at the same
    /// point.
    pub fn chain(&self) -> Vec<Albedo> {
        let mut out = vec![self.clone()];
        while let Some(next) = out.last().expect("the chain is not empty").reduced() {
            out.push(next);
        }
        out
    }

    /// Bounds computed from the data itself.
    pub fn measured(&self) -> (f32, f32) {
        let mut low = f32::MAX;
        let mut high = f32::MIN;
        for &v in &self.raw {
            low = low.min(v);
            high = high.max(v);
        }
        (low, high)
    }
}
