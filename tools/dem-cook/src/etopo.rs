//! ETOPO 2022 reader: Earth's shape with bathymetry (stage T, step T7b).
//!
//! The third surface source alongside LOLA and LROC WAC, and its grid geometry
//! is **the same**: simple cylindrical projection, pixel-registered, 60
//! samples per degree. So registration and bilinear sampling come from
//! [`crate::index_of`] and [`crate::bilinear`] rather than being written a
//! third time.
//!
//! ## What is ours here and what is not
//!
//! The container is **GeoTIFF**, the cooker's first format with compressed
//! data. The `tiff` crate decompresses it (Deflate with a floating-point
//! predictor, 256x256 tiles), which is exactly what "what we do NOT write"
//! requires: we write neither compression nor decoders. What is ours is the
//! **interpretation of the tags**: GeoTIFF describes the mapping to the globe
//! with three arrays of numbers, and no library will say whether they mean
//! what the cooker expects.
//!
//! ## Three checks, each catching an error invisible in a picture
//!
//! 1. **registration.** `RasterType = PixelIsArea` (GeoKey 1025 = 1), i.e. a
//!    pixel covers a cell rather than sitting at a node. A half-cell shift --
//!    0.93 km -- moves the coastline exactly where the colour changes
//!    abruptly;
//! 2. **georeferencing.** `ModelPixelScale` must be 1/60 degree on both axes,
//!    and `ModelTiepoint` must put pixel `(0, 0)` at `(-180, +90)`. A product
//!    with a different corner would read without a single error and give a
//!    flipped or shifted Earth;
//! 3. **empty pixels.** `GDAL_NODATA = -99999`, handled the same way as
//!    `CORE_NULL` in WAC: **count and fail** if even one occurs. Measured over
//!    the whole product -- there are none, so there is deliberately no fill
//!    rule here: it would be a guess about data we have not seen.
//!
//! ## Why samples become integer metres
//!
//! The source is `float32` over the EGM2008 geoid, range -10,752 to +8157 m. A
//! terrain tile stores `i16` (R5c), so a scale of 1 m covers the whole range
//! with room to spare, and fractional metres are below the format's quantum
//! anyway. There is **one** rounding here, not two: the grid already sits in
//! the units the cooker will write.
//!
//! WARNING: heights are measured from the **geoid**, while the game draws a
//! sphere of radius 6,371,010 m (`reference_m`). The difference -- a geoid
//! undulation of +/-100 m and 21 km of flattening -- is not added: stage T
//! deliberately kept the sphere. The consequence stated honestly: the heights
//! are correct relative to the water surface, not relative to Earth's
//! centre.

use std::path::Path;

use tiff::decoder::{Decoder, DecodingResult, Limits};
use tiff::tags::Tag;

/// Reference radius the cooker measures Earth's heights from, metres.
///
/// The one the ephemeris asset carries (`ephemeris.h`: mean radius 6,371,010 m
/// against an equatorial 6,378,137), and that is a requirement rather than a
/// coincidence: the body in frame is drawn as a sphere of exactly this radius,
/// so any other number here would raise or sink the whole surface at once.
pub const REFERENCE_M: f64 = 6_371_010.0;

/// The "no data" value from the `GDAL_NODATA` tag.
const NODATA: f32 = -99999.0;

/// GeoTIFF tag: pixel size in coordinate system units.
const TAG_PIXEL_SCALE: Tag = Tag::Unknown(33550);
/// GeoTIFF tag: the pixel's tie to coordinates.
const TAG_TIEPOINT: Tag = Tag::Unknown(33922);
/// GeoTIFF tag: a key dictionary, among them registration and datum.
const TAG_GEO_KEYS: Tag = Tag::Unknown(34735);

/// What the reader takes from the header before touching any pixels.
///
/// Its own type for the same reason as [`crate::albedo::Header`]: git holds
/// exactly the header (`data/etopo/etopo_2022_60s_surface.lbl`, 32 KiB) while
/// the 466 MB product itself does not (Q5). So parsing must be callable
/// **without the data**, or there would be nothing to check it with.
#[derive(Clone, Debug)]
pub struct Header {
    /// Samples along longitude.
    pub samples: usize,
    /// Rows along latitude.
    pub lines: usize,
    /// Samples per degree -- the inverse of `ModelPixelScale`.
    pub per_degree: f64,
    /// Where the corner of pixel `(0, 0)` is tied: longitude and latitude,
    /// degrees.
    pub corner_deg: (f64, f64),
}

impl Header {
    /// Read the header, from the product and from the label in git alike.
    ///
    /// Works on both because `Decoder::new` reads only the IFD: the pixels
    /// stay untouched until asked for. That is exactly why the label can be
    /// the head of the file rather than a separate description.
    pub fn read(path: &Path) -> Result<Header, String> {
        let mut decoder = open(path)?;
        Header::from_decoder(&mut decoder)
    }

    fn from_decoder(
        decoder: &mut Decoder<std::io::BufReader<std::fs::File>>,
    ) -> Result<Header, String> {
        let (width, height) = decoder
            .dimensions()
            .map_err(|e| format!("GeoTIFF dimensions: {e}"))?;

        let scale = doubles(decoder, TAG_PIXEL_SCALE, "ModelPixelScale")?;
        if scale.len() < 2 {
            return Err(format!(
                "ModelPixelScale has {} numbers instead of 3",
                scale.len()
            ));
        }
        // The step in longitude and latitude must be equal: the grid is square
        // in degrees, and a single `per_degree` instead of two rests on that.
        if (scale[0] - scale[1]).abs() > 1e-12 {
            return Err(format!(
                "ModelPixelScale is not square: {} against {} degrees",
                scale[0], scale[1]
            ));
        }
        if scale[0] <= 0.0 {
            return Err(format!("ModelPixelScale = {} degrees", scale[0]));
        }

        let tie = doubles(decoder, TAG_TIEPOINT, "ModelTiepoint")?;
        if tie.len() < 6 {
            return Err(format!(
                "ModelTiepoint has {} numbers instead of 6",
                tie.len()
            ));
        }
        // The first triple is the pixel, the second its coordinates. The
        // cooker can read only a tie to the raster corner; any other means a
        // different product.
        if tie[0] != 0.0 || tie[1] != 0.0 {
            return Err(format!(
                "ModelTiepoint ties pixel ({}, {}), not the raster corner",
                tie[0], tie[1]
            ));
        }

        // Registration: 1 is PixelIsArea, 2 is PixelIsPoint. The difference is
        // half a pixel, i.e. 0.93 km at the equator, and no picture shows it.
        let keys = shorts(decoder, TAG_GEO_KEYS, "GeoKeyDirectory")?;
        match geo_key(&keys, 1025) {
            Some(1) => {}
            Some(other) => {
                return Err(format!(
                    "RasterType = {other}: the grid is node-registered, but \
                     the reader assumes pixel-registered"
                ))
            }
            None => return Err("GeoKeyDirectory has no RasterType".to_string()),
        }

        Ok(Header {
            samples: width as usize,
            lines: height as usize,
            per_degree: 1.0 / scale[0],
            corner_deg: (tie[3], tie[4]),
        })
    }

    /// Whether the grid is tied where the cooker expects: the world's
    /// north-west corner.
    ///
    /// A product starting elsewhere would read without a single error and give
    /// a shifted Earth -- an error visible only next to a coastline.
    pub fn covers_globe(&self) -> bool {
        let span_lon = self.samples as f64 / self.per_degree;
        let span_lat = self.lines as f64 / self.per_degree;
        (self.corner_deg.0 + 180.0).abs() < 1e-9
            && (self.corner_deg.1 - 90.0).abs() < 1e-9
            && (span_lon - 360.0).abs() < 1e-9
            && (span_lat - 180.0).abs() < 1e-9
    }
}

/// Earth's height grid in the simple cylindrical projection, integer metres.
#[derive(Clone, Debug)]
pub struct Relief {
    /// Samples along longitude.
    pub samples: usize,
    /// Rows along latitude.
    pub lines: usize,
    /// Samples per degree.
    pub per_degree: f64,
    /// The samples themselves, row by row from north to south, metres.
    pub raw: Vec<i16>,
}

impl Relief {
    /// Read the whole product.
    pub fn read(path: &Path) -> Result<Relief, String> {
        let mut decoder = open(path)?;
        let header = Header::from_decoder(&mut decoder)?;
        if !header.covers_globe() {
            return Err(format!(
                "a {}x{} grid with corner {:?} does not cover the globe",
                header.samples, header.lines, header.corner_deg
            ));
        }

        // The crate's limits are sized for pictures on a screen; here there
        // are 933 MB of samples, a normal source size rather than a sign of a
        // broken file. The cooker is offline and has the memory.
        let image = decoder
            .read_image()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let DecodingResult::F32(values) = image else {
            return Err("expected float32 -- no other sample type is read here".to_string());
        };
        if values.len() != header.samples * header.lines {
            return Err(format!(
                "{} samples instead of {}x{}",
                values.len(),
                header.samples,
                header.lines
            ));
        }

        // There is deliberately no fill rule (see the module intro): a product
        // with holes is a different product and must not be cooked silently.
        let empty = values.iter().filter(|&&v| v == NODATA).count();
        if empty > 0 {
            return Err(format!(
                "{empty} samples = {NODATA} (GDAL_NODATA); the cooker has no \
                 fill rule"
            ));
        }

        let raw = values
            .iter()
            .map(|&v| quantise(f64::from(v)))
            .collect::<Vec<i16>>();
        drop(values);

        Ok(Relief {
            samples: header.samples,
            lines: header.lines,
            per_degree: header.per_degree,
            raw,
        })
    }

    /// A grid sample. Indices wrap in longitude and clamp in latitude --
    /// exactly how the sphere itself behaves.
    pub fn at(&self, line: i64, sample: i64) -> i16 {
        let line = line.clamp(0, self.lines as i64 - 1) as usize;
        let sample = sample.rem_euclid(self.samples as i64) as usize;
        self.raw[line * self.samples + sample]
    }

    /// Sample height above the reference sphere, metres.
    pub fn height_m(&self, line: i64, sample: i64) -> f64 {
        f64::from(self.at(line, sample))
    }

    /// Height at an arbitrary point, bilinear between four samples.
    ///
    /// WARNING: **the first column here is 180 west, not the prime meridian.**
    /// In LOLA and WAC the grid starts at 0 (which `index_of` assumes), in
    /// ETOPO at -180, and `ModelTiepoint` is what says so. So the longitude
    /// arrives in the shared registration shifted by pi: without the shift the
    /// map would read without a single error and stand half a globe off -- an
    /// error that looks like a correct Earth until you check where the ocean
    /// is.
    pub fn sample_m(&self, lat: f64, lon: f64) -> f64 {
        crate::bilinear(
            self.per_degree,
            lat,
            lon + std::f64::consts::PI,
            |line, sample| self.height_m(line, sample),
        )
    }

    /// Height along `direction` (not necessarily unit), metres.
    pub fn sample_direction_m(&self, direction: [f64; 3]) -> f64 {
        let (lat, lon) = crate::lat_lon(direction);
        self.sample_m(lat, lon)
    }

    /// Bounds computed from the data itself, metres.
    pub fn measured(&self) -> (i16, i16) {
        let mut low = i16::MAX;
        let mut high = i16::MIN;
        for &v in &self.raw {
            low = low.min(v);
            high = high.max(v);
        }
        (low, high)
    }

    /// Land fraction (`h >= 0`) over the sphere, weighted by `cos(latitude)`.
    ///
    /// The reader's principal oracle, which is exactly why it lives here and
    /// not in a test: one number catches the whole class of geometry errors.
    /// The true fraction is 29.2%; a half-pixel shift, reversed row order or a
    /// misparsed predictor move it by percent, and a broken grid by tens.
    ///
    /// Weighted by `cos(latitude)` rather than counting pixels: in a
    /// cylindrical projection a polar row covers hundreds of times less area
    /// than an equatorial one, and without the weight Antarctica would count
    /// as much as Africa.
    pub fn land_fraction(&self) -> f64 {
        let degrees = std::f64::consts::PI / 180.0;
        let mut land = 0.0;
        let mut total = 0.0;
        for line in 0..self.lines {
            let lat = 90.0 - (line as f64 + 0.5) * 180.0 / self.lines as f64;
            let weight = (lat * degrees).cos();
            let row = &self.raw[line * self.samples..(line + 1) * self.samples];
            let above = row.iter().filter(|&&v| v >= 0).count();
            land += weight * above as f64;
            total += weight * self.samples as f64;
        }
        land / total
    }

    /// The angle one grid pixel covers, radians.
    ///
    /// The same as [`crate::albedo::Albedo::pixel_rad`], and needed for the
    /// same thing -- choosing a grid for a pyramid level (T3c,
    /// `cook::source_for`).
    pub fn pixel_rad(&self) -> f64 {
        std::f64::consts::PI / (180.0 * self.per_degree)
    }

    /// The same grid, coarser by one chain step: each sample is a block mean.
    ///
    /// WARNING: **heights need the chain too, unlike the Moon.** There the
    /// source (7.6 km) is coarser than the deepest level's node (5.3 km), so
    /// point sampling is honest at every level. Here the source is five times
    /// finer than a node, and thirty thousand times at level 0, so without
    /// averaging Earth's distant silhouette would be made of random source
    /// pixels: now a trench, now a mountain.
    ///
    /// The mean is taken in `f64` and rounded once -- otherwise seven chain
    /// steps would contribute seven roundings of half a metre each.
    pub fn reduced(&self) -> Option<Relief> {
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
                raw.push(quantise(sum / (step * step) as f64));
            }
        }
        Some(Relief {
            samples,
            lines,
            per_degree: self.per_degree / step as f64,
            raw,
        })
    }

    /// The chain of grids, each coarser than the last; the zeroth is this one
    /// (T3c).
    pub fn chain(&self) -> Vec<Relief> {
        let mut out = vec![self.clone()];
        while let Some(next) = out.last().expect("the chain is not empty").reduced() {
            out.push(next);
        }
        out
    }
}

/// Floating-point metres to integer metres, saturating rather than wrapping.
///
/// Wrapping here would be the worst possible: a 40 km trench would become a
/// mountain, and it would look plausible. The product's measured range
/// (-10,752 to +8157) does not approach the limit, so saturation is a
/// safeguard against a different product rather than a working path.
fn quantise(metres: f64) -> i16 {
    metres
        .round()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

fn open(path: &Path) -> Result<Decoder<std::io::BufReader<std::fs::File>>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let decoder = Decoder::new(std::io::BufReader::new(file))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(decoder.with_limits(Limits::unlimited()))
}

fn doubles(
    decoder: &mut Decoder<std::io::BufReader<std::fs::File>>,
    tag: Tag,
    name: &str,
) -> Result<Vec<f64>, String> {
    decoder
        .get_tag_f64_vec(tag)
        .map_err(|e| format!("tag {name}: {e}"))
}

fn shorts(
    decoder: &mut Decoder<std::io::BufReader<std::fs::File>>,
    tag: Tag,
    name: &str,
) -> Result<Vec<u16>, String> {
    decoder
        .get_tag_u16_vec(tag)
        .map_err(|e| format!("tag {name}: {e}"))
}

/// A key's value from the `GeoKeyDirectory`.
///
/// The directory is an array of `u16` in fours: four header numbers, then four
/// per key (number, where the value lives, how much of it, the value itself).
/// Only keys held in the directory itself (`location == 0`) matter here; the
/// rest point at other tags, and the cooker reads none of those.
fn geo_key(keys: &[u16], wanted: u16) -> Option<u16> {
    if keys.len() < 4 {
        return None;
    }
    let count = keys[3] as usize;
    for k in 0..count {
        let at = 4 + k * 4;
        if at + 3 >= keys.len() {
            break;
        }
        if keys[at] == wanted && keys[at + 1] == 0 {
            return Some(keys[at + 3]);
        }
    }
    None
}
