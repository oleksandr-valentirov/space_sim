//! Blue Marble Next Generation mosaic reader: Earth's colour (stage T, T7c).
//!
//! The fourth surface source, with the same geometry as ETOPO
//! ([`crate::etopo`]): simple cylindrical projection, pixel-registered, 60
//! samples per degree, first column at 180 west. That the grids match
//! **pixel for pixel** was the reason to take this pair of products (T7): a
//! colour node lands exactly on a height node, so a coastline cannot diverge
//! from itself.
//!
//! ## What here is someone else's and what is ours
//!
//! The `jpeg-decoder` crate parses the JPEG -- we do not write image decoders
//! (CLAUDE.md, "what we do NOT write"). Ours is what the decoder does not
//! know: how these bytes are tied to the globe and which space they are in.
//!
//! ## Space: sRGB in the file, linear in memory
//!
//! The mosaic is **sRGB**-encoded -- a picture for the eye, not a field of
//! physical quantities. The frame works in linear light (T5c), and any
//! averaging -- bilinear weights, the chain of coarser grids -- only makes
//! sense in linear: the mean of two sRGB bytes is not the colour of the
//! mixture but the colour "between them to the eye". So the reader decodes
//! once, on load, and every number here afterwards is linear.
//!
//! The cost is named: `float32` instead of a byte is 2.8 GB for level zero and
//! about 3.7 GB for the whole chain. The cooker is offline, and paying in
//! memory here is cheaper than rounding seven times in a row.
//!
//! WARNING: **this is not albedo.** The mosaic is assembled from MODIS and
//! retouched for the eye: slope shadows and traces of atmospheric correction
//! remain in it. It promises no physical reflectance the way WAC does -- which
//! is exactly why Earth's colour tile carries "surface colour" rather than
//! "reflectance".

use std::path::Path;

/// How many channels the mosaic carries.
pub const CHANNELS: usize = 3;

/// A colour grid in the simple cylindrical projection, **linear** light.
#[derive(Clone, Debug)]
pub struct Mosaic {
    /// Samples along longitude.
    pub samples: usize,
    /// Rows along latitude.
    pub lines: usize,
    /// Samples per degree.
    pub per_degree: f64,
    /// The samples themselves: `CHANNELS` in a row per pixel, row by row from
    /// north to south, each from 0 to 1 in linear light.
    pub raw: Vec<f32>,
}

impl Mosaic {
    /// Read the whole mosaic.
    pub fn read(path: &Path) -> Result<Mosaic, String> {
        let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut decoder = jpeg_decoder::Decoder::new(std::io::BufReader::new(file));
        let pixels = decoder
            .decode()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let info = decoder
            .info()
            .ok_or_else(|| format!("{}: JPEG without a header", path.display()))?;

        if info.pixel_format != jpeg_decoder::PixelFormat::RGB24 {
            return Err(format!(
                "{}: expected three eight-bit channels, found {:?}",
                path.display(),
                info.pixel_format
            ));
        }

        let (samples, lines) = (info.width as usize, info.height as usize);
        // The mosaic must cover the globe, and cover it exactly: two to one. A
        // tile product (BMNG is also distributed in eight pieces) would read
        // without an error and give an eighth of the world as the whole
        // Earth.
        if samples != 2 * lines {
            return Err(format!(
                "{}: {samples}x{lines} is not a globe, whose longitude is twice \
                 its latitude",
                path.display()
            ));
        }
        if pixels.len() != samples * lines * CHANNELS {
            return Err(format!(
                "{}: {} bytes instead of {samples}x{lines}x{CHANNELS}",
                path.display(),
                pixels.len()
            ));
        }

        let table = srgb_table();
        let raw = pixels.iter().map(|&b| table[b as usize]).collect();

        Ok(Mosaic {
            samples,
            lines,
            per_degree: samples as f64 / 360.0,
            raw,
        })
    }

    /// A grid sample, linear. Indices wrap in longitude and clamp in latitude
    /// -- exactly how the sphere itself behaves.
    pub fn at(&self, line: i64, sample: i64, channel: usize) -> f32 {
        let line = line.clamp(0, self.lines as i64 - 1) as usize;
        let sample = sample.rem_euclid(self.samples as i64) as usize;
        self.raw[(line * self.samples + sample) * CHANNELS + channel]
    }

    /// Colour at an arbitrary point, bilinear between four samples.
    ///
    /// WARNING: the longitude is shifted by pi for the same reason as in
    /// [`crate::etopo`]: the grid starts at -180 while the shared registration
    /// counts from zero.
    pub fn sample(&self, lat: f64, lon: f64) -> [f64; CHANNELS] {
        let mut out = [0.0; CHANNELS];
        for (channel, value) in out.iter_mut().enumerate() {
            *value = crate::bilinear(
                self.per_degree,
                lat,
                lon + std::f64::consts::PI,
                |line, sample| f64::from(self.at(line, sample, channel)),
            );
        }
        out
    }

    /// Colour along `direction` (not necessarily unit).
    pub fn sample_direction(&self, direction: [f64; 3]) -> [f64; CHANNELS] {
        let (lat, lon) = crate::lat_lon(direction);
        self.sample(lat, lon)
    }

    /// Angular size of a pixel, radians.
    pub fn pixel_rad(&self) -> f64 {
        std::f64::consts::PI / 180.0 / self.per_degree
    }

    /// The same mosaic, coarser by one chain step: each sample is a block
    /// mean.
    ///
    /// A mean of **linear** values rather than sRGB bytes -- which was the
    /// reason to decode once on read.
    pub fn reduced(&self) -> Option<Mosaic> {
        let step = crate::reduce_step(self.samples, self.lines)?;
        let (samples, lines) = (self.samples / step, self.lines / step);
        let mut raw = Vec::with_capacity(samples * lines * CHANNELS);
        for line in 0..lines {
            for sample in 0..samples {
                for channel in 0..CHANNELS {
                    let mut sum = 0.0f64;
                    for dl in 0..step {
                        for ds in 0..step {
                            let l = step * line + dl;
                            let s = step * sample + ds;
                            sum += f64::from(self.raw[(l * self.samples + s) * CHANNELS + channel]);
                        }
                    }
                    raw.push((sum / (step * step) as f64) as f32);
                }
            }
        }
        Some(Mosaic {
            samples,
            lines,
            per_degree: self.per_degree / step as f64,
            raw,
        })
    }

    /// The chain of grids, each coarser than the last; the zeroth is this one.
    ///
    /// The same thing for the same reason as
    /// [`crate::albedo::Albedo::chain`]: without it a coarse pyramid level
    /// would point-sample from a thousand pixels and give blotchy noise
    /// instead of a map (T3c). How much each step coarsens is decided by
    /// [`crate::reduce_step`] -- and not always by two: Earth's grid halves
    /// only four times.
    pub fn chain(&self) -> Vec<Mosaic> {
        let mut out = vec![self.clone()];
        while let Some(next) = out.last().expect("the chain is not empty").reduced() {
            out.push(next);
        }
        out
    }

    /// Mean colour over the whole mosaic, weighted by `cos(latitude)`.
    ///
    /// Not decoration: the sky albedo (S1) takes one colour per body, and this
    /// number is its estimate until the frame has a better one.
    pub fn mean(&self) -> [f64; CHANNELS] {
        let degrees = std::f64::consts::PI / 180.0;
        let mut sum = [0.0; CHANNELS];
        let mut total = 0.0;
        for line in 0..self.lines {
            let lat = 90.0 - (line as f64 + 0.5) * 180.0 / self.lines as f64;
            let weight = (lat * degrees).cos();
            let row =
                &self.raw[line * self.samples * CHANNELS..(line + 1) * self.samples * CHANNELS];
            for pixel in row.chunks_exact(CHANNELS) {
                for (channel, value) in pixel.iter().enumerate() {
                    sum[channel] += weight * f64::from(*value);
                }
            }
            total += weight * self.samples as f64;
        }
        sum.map(|s| s / total)
    }
}

/// sRGB byte to linear light, via a 256-entry table.
///
/// A table rather than a formula per pixel: there are exactly 256 inputs and
/// 233 million pixels. The formula itself is standard sRGB with a linear
/// segment at the bottom; the `x^2.2` approximation fails precisely in the
/// dark, i.e. in the ocean, which occupies two thirds of the frame.
fn srgb_table() -> [f32; 256] {
    let mut table = [0.0f32; 256];
    for (index, value) in table.iter_mut().enumerate() {
        let x = index as f64 / 255.0;
        *value = if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        } as f32;
    }
    table
}

/// Linear light to an sRGB byte. The inverse of [`srgb_table`].
///
/// The cooker needs it: a tile stores a **byte**, and storing a linear value
/// there would spend the whole scale on light tones -- the ocean at 0.0015
/// linear would get zero. So the tile holds sRGB again, and the GPU decodes it
/// on sampling, for free (`Rgba8UnormSrgb`).
pub fn to_srgb(linear: f64) -> u8 {
    let x = linear.clamp(0.0, 1.0);
    let encoded = if x <= 0.003_130_8 {
        12.92 * x
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}
