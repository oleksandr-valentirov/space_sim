//! LOLA GDR reader: the raw lunar height grid (ROADMAP-PLANETS.md, R5a).
//!
//! The same shape of step as K5a with the GRAIL coefficients: first the data
//! in the repository and a reader with an oracle, and only then something done
//! with them. The oracle here is deliberately **not ours**: the PDS3 label
//! beside the file prints `MINIMUM` and `MAXIMUM` in the same units the data
//! itself uses, so the reader must reproduce published numbers from raw bytes.
//! That catches three classic mistakes at once, none of them visible by eye in
//! a picture:
//!
//! 1. **swapped byte order** -- LOLA writes `LSB_INTEGER`, and on a big-endian
//!    machine, or with a naive `from_be_bytes`, the minimum and maximum move
//!    by thousands of kilometres;
//! 2. **forgotten scale** -- heights are stored as integers of **half** a
//!    metre (`SCALING_FACTOR = 0.5`), so without it the terrain is twice as
//!    tall;
//! 3. **half-pixel shift** -- the grid is **pixel-registered**, and a
//!    half-cell offset gives a map that looks right and sits in the wrong
//!    place.
//!
//! The min/max oracle does not catch the third by itself, so it has its own
//! check: known crater coordinates must give known depths.
//!
//! ## What is not here
//!
//! **General PDS3 parsing.** Exactly the keys the arithmetic depends on are
//! read from the label, and the rest stays text for a human. Parsing the
//! format fully is a library, not twenty lines, and nobody here would call any
//! of its other capabilities.

pub mod albedo;
pub mod bmng;
pub mod cook;
pub mod etopo;

use std::collections::HashMap;
use std::path::Path;

/// Fractional sample indices for latitude and longitude, **radians**.
///
/// Shared by both sources -- LOLA and LROC WAC -- which is exactly why it
/// lives outside [`Grid`]: the grids differ (integer half-metres against real
/// reflectance) while the **registration is the same**, and it can be got
/// wrong exactly once for the whole project. Two copies would diverge by the
/// fourth edit, and a half-cell shift gives a map that looks right and sits in
/// the wrong place.
///
/// The grid is pixel-registered: the centre of the first sample lies half a
/// cell inside the range rather than on its edge. Hence the `- 0.5` in both
/// formulas -- the same offset the LOLA label calls
/// `LINE_PROJECTION_OFFSET = 359.5` and `SAMPLE_PROJECTION_OFFSET = 719.5`.
/// Forgetting it shifts the whole map by half a source cell.
pub fn index_of(per_degree: f64, lat: f64, lon: f64) -> (f64, f64) {
    let degrees = 180.0 / std::f64::consts::PI;
    let line = (90.0 - lat * degrees) * per_degree - 0.5;
    let sample = (lon * degrees).rem_euclid(360.0) * per_degree - 0.5;
    (line, sample)
}

/// Bilinear sampling from a cylindrical grid: four neighbours, weighted by the
/// fractional part of the index.
///
/// Bilinear rather than nearest: a cubesphere tile lands on this grid at an
/// arbitrary angle, and nearest-neighbour stair-stepping would become visible
/// exactly where the tile is finer than a source cell.
///
/// The caller supplies the value itself -- `value(row, sample)`; wrapping in
/// longitude and clamping in latitude stay its job too, because those are
/// properties of the grid, not of the interpolation.
pub fn bilinear(per_degree: f64, lat: f64, lon: f64, value: impl Fn(i64, i64) -> f64) -> f64 {
    let (line, sample) = index_of(per_degree, lat, lon);
    let (l0, s0) = (line.floor(), sample.floor());
    let (tl, ts) = (line - l0, sample - s0);
    let (l0, s0) = (l0 as i64, s0 as i64);

    let top = value(l0, s0) * (1.0 - ts) + value(l0, s0 + 1) * ts;
    let bottom = value(l0 + 1, s0) * (1.0 - ts) + value(l0 + 1, s0 + 1) * ts;
    top * (1.0 - tl) + bottom * tl
}

/// Latitude and longitude of a direction (not necessarily unit), radians.
///
/// A direction rather than a pair of angles is what arrives from the
/// cubesphere, and the translation must live in one place -- here, beside the
/// grids themselves.
pub fn lat_lon(direction: [f64; 3]) -> (f64, f64) {
    let [x, y, z] = direction;
    let flat = (x * x + y * y).sqrt();
    (z.atan2(flat), y.atan2(x))
}

/// By what factor the grid coarsens at the next chain step, or `None` when
/// there is nothing left to divide.
///
/// ## Why not always by two
///
/// The chain (T3c) exists for the coarse pyramid levels: a level 0 node covers
/// 312 km of Earth, and point sampling from a 1.85 km grid picks one of thirty
/// thousand pixels at random. So dividing must continue until a pixel reaches
/// the size of a coarsest-level node.
///
/// Halving does not achieve that: **10800 = 2^4 * 675**, so Earth's chain
/// would stop at 1350x675 -- 29.7 km per pixel against a 312 km node, ten
/// times too fine. The lunar grid (5760x2880 = 2^6 * 45) divides for longer
/// and would have hidden this from us: there the chain reaches where it needs
/// to on its own.
///
/// So the step is the **smallest divisor of both sides** among three primes.
/// Chain uniformity is not required: a pyramid level asks it for a grid **by
/// angle** (`cook::source_for`), not by number, so an uneven step breaks
/// nothing.
///
/// Three, not "any divisor": a larger step could overshoot the needed
/// coarseness, and there is nothing to bound the choice from below --
/// 675 = 3^3 * 5^2, and without the five Earth's chain would stop at
/// 450x225.
pub fn reduce_step(samples: usize, lines: usize) -> Option<usize> {
    // Below four rows the grid is no longer a map but four numbers.
    if lines < 8 {
        return None;
    }
    [2, 3, 5]
        .into_iter()
        .find(|k| samples.is_multiple_of(*k) && lines.is_multiple_of(*k) && lines / k >= 4)
}

/// A height grid in the simple cylindrical projection.
///
/// The fields are named after the label keys, deliberately: between the source
/// file and this struct there must be no translation to get wrong.
#[derive(Clone, Debug)]
pub struct Grid {
    /// Samples along longitude (`LINE_SAMPLES`).
    pub samples: usize,
    /// Rows along latitude (`LINES`).
    pub lines: usize,
    /// Multiplier from integer to metres (`SCALING_FACTOR`).
    pub scale_m: f64,
    /// Reference radius heights are measured from (`OFFSET`), metres.
    pub reference_m: f64,
    /// Samples per degree (`MAP_RESOLUTION`).
    pub per_degree: f64,
    /// Metres per pixel per the label (`MAP_SCALE`).
    pub metres_per_pixel: f64,
    /// Published bounds in integer units -- an oracle, not data.
    pub published: (i32, i32),
    /// The samples themselves, row by row from north to south.
    pub raw: Vec<i16>,
}

/// A label key's value -- everything after `=` to the end of the line.
pub(crate) fn label_values(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        // Label keys are upper case with underscores. Everything else is
        // prose inside `DESCRIPTION`, and must not be taken for a key.
        if key.is_empty()
            || !key
                .chars()
                .all(|c| c.is_ascii_uppercase() || c == '_' || c == '^' || c.is_ascii_digit())
        {
            continue;
        }
        // The first entry wins: the same name occurs both in `OBJECT` and in
        // an example comment below.
        out.entry(key.to_string())
            .or_insert_with(|| value.trim().to_string());
    }
    out
}

/// A number from a label field: `21008`, `0.5`, `1737400.`, `7580.84 <m/pix>`.
pub(crate) fn number(values: &HashMap<String, String>, key: &str) -> Result<f64, String> {
    let raw = values
        .get(key)
        .ok_or_else(|| format!("the label has no {key}"))?;
    let head = raw.split_whitespace().next().unwrap_or("");
    head.trim_end_matches('.')
        .parse::<f64>()
        .map_err(|e| format!("{key} = {raw:?}: {e}"))
}

impl Grid {
    /// Read a label-plus-data pair.
    ///
    /// The path points at the `.img`; the label is looked for beside it with
    /// the same stem and a `.lbl` extension. That is how they are published,
    /// and how they sit here.
    pub fn read(img: &Path) -> Result<Grid, String> {
        let lbl = img.with_extension("lbl");
        let text = std::fs::read_to_string(&lbl).map_err(|e| format!("{}: {e}", lbl.display()))?;
        let values = label_values(&text);

        let bits = number(&values, "SAMPLE_BITS")?;
        if bits != 16.0 {
            return Err(format!(
                "expected 16 bits per sample, but the label says {bits}"
            ));
        }
        let kind = values
            .get("SAMPLE_TYPE")
            .map(String::as_str)
            .unwrap_or_default();
        if kind != "LSB_INTEGER" {
            return Err(format!(
                "expected LSB_INTEGER -- no other byte order is read here, \
                 and the label says {kind:?}"
            ));
        }

        let samples = number(&values, "LINE_SAMPLES")? as usize;
        let lines = number(&values, "LINES")? as usize;

        let bytes = std::fs::read(img).map_err(|e| format!("{}: {e}", img.display()))?;
        let wanted = samples * lines * 2;
        if bytes.len() != wanted {
            return Err(format!(
                "{}: {} bytes instead of {wanted} = {samples}x{lines}x2",
                img.display(),
                bytes.len()
            ));
        }

        let raw = bytes
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();

        Ok(Grid {
            samples,
            lines,
            scale_m: number(&values, "SCALING_FACTOR")?,
            reference_m: number(&values, "OFFSET")?,
            per_degree: number(&values, "MAP_RESOLUTION")?,
            metres_per_pixel: number(&values, "MAP_SCALE")?,
            published: (
                number(&values, "MINIMUM")? as i32,
                number(&values, "MAXIMUM")? as i32,
            ),
            raw,
        })
    }

    /// The sample's integer value. Indices wrap in longitude and clamp in
    /// latitude -- exactly how the sphere itself behaves.
    pub fn at(&self, line: i64, sample: i64) -> i16 {
        let line = line.clamp(0, self.lines as i64 - 1) as usize;
        let sample = sample.rem_euclid(self.samples as i64) as usize;
        self.raw[line * self.samples + sample]
    }

    /// Sample height above the reference radius, metres.
    pub fn height_m(&self, line: i64, sample: i64) -> f64 {
        f64::from(self.at(line, sample)) * self.scale_m
    }

    /// Bounds computed from the data itself, in integer units.
    pub fn measured(&self) -> (i32, i32) {
        let mut low = i32::MAX;
        let mut high = i32::MIN;
        for &v in &self.raw {
            low = low.min(i32::from(v));
            high = high.max(i32::from(v));
        }
        (low, high)
    }

    /// Fractional sample indices for latitude and longitude, **radians**.
    pub fn index_of(&self, lat: f64, lon: f64) -> (f64, f64) {
        crate::index_of(self.per_degree, lat, lon)
    }

    /// Height at an arbitrary point, bilinear between four samples.
    pub fn sample_m(&self, lat: f64, lon: f64) -> f64 {
        crate::bilinear(self.per_degree, lat, lon, |line, sample| {
            self.height_m(line, sample)
        })
    }

    /// Height along `direction` (not necessarily unit), metres.
    pub fn sample_direction_m(&self, direction: [f64; 3]) -> f64 {
        let (lat, lon) = crate::lat_lon(direction);
        self.sample_m(lat, lon)
    }
}
