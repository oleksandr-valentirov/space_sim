//! The cooked mesh format (stage T, step T5d2).
//!
//! The same choice as with tiles (`crate::tiles`): the format lives **in the
//! engine**, because the writer and the reader of one format must be one piece
//! of code. The cooker (`tools/mesh-cook`) depends on the engine; the engine
//! knows nothing about the cooker.
//!
//! ## A mesh of unit height, with the metres alongside
//!
//! The engine keeps the ship at unit height and scales it by `height_m` every
//! frame (V2), so the cooker does the normalisation, once. Two numbers lie in
//! the file next to the geometry:
//!
//! - `height_m` -- the length of the original along `+Z` in metres. A note about
//!   the model: the game is free to scale the ship however it likes;
//! - `extent` -- the bounding-sphere radius **in units of the height**. Not
//!   derived from the first and not half a unit: the fins stick out past the
//!   hull, and a real model sticks out however it likes. `near` and the
//!   third-person camera stand on it (V2), so an error in it is a clipped hull.
//!
//! ## Positions in `f32`, and this is not the same decision as in a patch
//!
//! A planet's coordinates are large, which is why its vertices are
//! camera-relative. Here the mesh is normalised to one, so the largest number in
//! the file is of order one, and `f32` gives 1e-7 relative on those. For a 6 m
//! ship that is 0.6 um; nothing on screen is that size.
//!
//! ## Axes are not transformed anywhere
//!
//! The model is made nose along `-Y` in Blender, and the export with default
//! settings gives glTF with the nose along `+Z` -- which is already the
//! `Scene::Ship` convention. So the cooker **converts axes exactly once**, from
//! glTF into ours (`y` up against `z` up does not change: both are right-handed
//! here with the nose along `+Z`), and in this format there are no axes left at
//! all -- only numbers.

use crate::sphere::Mesh;

/// The file signature. Eight bytes, so the header reads by eye in a hex
/// dump.
pub const MAGIC: [u8; 8] = *b"SSMSH\0\0\0";

/// The format version. Grows on any layout change.
///
/// Version 2 (T9b) added paint: three `f32` per vertex after the normals, and a
/// word in the header saying whether it is there at all.
pub const VERSION: u32 = 2;

/// Signature, version, two counts, the paint flag and the model's two
/// numbers.
const HEADER_BYTES: usize = 8 + 4 + 4 + 4 + 4 + 4 + 4;

/// The model mesh together with what has to be known about it.
#[derive(Clone, Debug)]
pub struct Model {
    /// Length of the original along `+Z`, metres.
    pub height_m: f64,
    /// Bounding-sphere radius in units of the height.
    pub extent: f64,
    /// The geometry, normalised to unit height.
    pub mesh: Mesh,
    /// Base colour per vertex, **linear light**; empty means there is no paint.
    ///
    /// Per vertex rather than by material ranges: in glTF this is `COLOR_0`, and
    /// the model stays **one primitive**, that is one draw call and not a single
    /// new concept in the format. The price is known and paid -- colour
    /// discontinuities split vertices the same way normal discontinuities do.
    ///
    /// WARNING: colour only. Roughness and metallic stay constant per ship:
    /// `COLOR_0` does not carry them, and a second path for them would already
    /// be material ranges, that is a format decision, and it waits for the first
    /// detail that genuinely lacks it (the porthole glass gets by on colour for
    /// now).
    pub paint: Vec<[f32; 3]>,
}

impl Model {
    /// Normalise a mesh in metres to unit height.
    ///
    /// One piece of code for the cooker and for any other caller: two ways to
    /// divide by the length would give two different ships.
    pub fn from_metres(mesh: Mesh, paint: Vec<[f32; 3]>) -> Result<Model, String> {
        if mesh.positions.is_empty() {
            return Err("a mesh with no vertices".to_string());
        }
        if !paint.is_empty() && paint.len() != mesh.positions.len() {
            return Err(format!(
                "paint for {} vertices against {} geometry vertices",
                paint.len(),
                mesh.positions.len()
            ));
        }
        let mut low = f64::INFINITY;
        let mut high = f64::NEG_INFINITY;
        for p in &mesh.positions {
            low = low.min(p[2]);
            high = high.max(p[2]);
        }
        let height_m = high - low;
        if height_m <= 0.0 || !height_m.is_finite() {
            return Err(format!("the model has zero length along +Z: {height_m}"));
        }

        // The origin stays where the model put it: in V2 it is at the middle
        // of the hull, and the third-person camera aims exactly there.
        // Centring here would shift the ship relative to what the game flies
        // it by.
        let mut mesh = mesh;
        for p in &mut mesh.positions {
            for v in p.iter_mut() {
                *v /= height_m;
            }
        }
        let extent = mesh
            .positions
            .iter()
            .map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt())
            .fold(0.0, f64::max);

        Ok(Model {
            height_m,
            extent,
            mesh,
            paint,
        })
    }

    /// The file bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let vertices = self.mesh.positions.len();
        let stride = if self.paint.is_empty() { 24 } else { 36 };
        let mut out =
            Vec::with_capacity(HEADER_BYTES + vertices * stride + self.mesh.indices.len() * 4);
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(vertices as u32).to_le_bytes());
        out.extend_from_slice(&(self.mesh.indices.len() as u32).to_le_bytes());
        out.extend_from_slice(&u32::from(!self.paint.is_empty()).to_le_bytes());
        out.extend_from_slice(&(self.height_m as f32).to_le_bytes());
        out.extend_from_slice(&(self.extent as f32).to_le_bytes());
        for p in &self.mesh.positions {
            for v in p {
                out.extend_from_slice(&(*v as f32).to_le_bytes());
            }
        }
        for n in &self.mesh.normals {
            for v in n {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        for c in &self.paint {
            for v in c {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        for i in &self.mesh.indices {
            out.extend_from_slice(&i.to_le_bytes());
        }
        out
    }

    /// Read a file. An error is a string rather than a panic: the asset may not
    /// be on disk, and the caller is the one who must say so.
    pub fn from_bytes(bytes: &[u8]) -> Result<Model, String> {
        if bytes.len() < HEADER_BYTES {
            return Err(format!(
                "the file is shorter than the header: {} bytes",
                bytes.len()
            ));
        }
        if bytes[0..8] != MAGIC {
            return Err("wrong signature: this is not a mesh".to_string());
        }
        let word = |at: usize| {
            u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        let float = |at: usize| {
            f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        let version = word(8);
        if version != VERSION {
            return Err(format!(
                "version {version}, while the reader understands {VERSION}"
            ));
        }
        let vertices = word(12) as usize;
        let indices = word(16) as usize;
        let painted = word(20) == 1;
        let height_m = f64::from(float(24));
        let extent = f64::from(float(28));

        let stride = if painted { 36 } else { 24 };
        let need = HEADER_BYTES + vertices * stride + indices * 4;
        if bytes.len() != need {
            return Err(format!(
                "the file is {} bytes, while {vertices} vertices and {indices} indices need {need}",
                bytes.len()
            ));
        }

        let mut at = HEADER_BYTES;
        let mut positions = Vec::with_capacity(vertices);
        for _ in 0..vertices {
            let p = [float(at), float(at + 4), float(at + 8)];
            positions.push([f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]);
            at += 12;
        }
        let mut normals = Vec::with_capacity(vertices);
        for _ in 0..vertices {
            normals.push([float(at), float(at + 4), float(at + 8)]);
            at += 12;
        }
        let mut paint = Vec::new();
        if painted {
            paint.reserve(vertices);
            for _ in 0..vertices {
                paint.push([float(at), float(at + 4), float(at + 8)]);
                at += 12;
            }
        }
        let mut list = Vec::with_capacity(indices);
        for _ in 0..indices {
            let index = word(at);
            if index as usize >= vertices {
                return Err(format!("index {index} against {vertices} vertices"));
            }
            list.push(index);
            at += 4;
        }

        Ok(Model {
            height_m,
            extent,
            paint,
            mesh: Mesh {
                positions,
                normals,
                indices: list,
            },
        })
    }
}

/// The signed volume of a closed shell -- the geometry oracle.
///
/// The sum of `(a x b) . c / 6` over triangles: for a closed shell with outward
/// normals it is positive and equals the volume, and a flipped winding changes
/// the sign. An open shell gives a meaningless number -- which is exactly why
/// the oracle is checked against **a different tool** (`bmesh.calc_volume`)
/// rather than against itself.
pub fn signed_volume(mesh: &Mesh) -> f64 {
    let mut total = 0.0;
    for triangle in mesh.indices.chunks_exact(3) {
        let a = mesh.positions[triangle[0] as usize];
        let b = mesh.positions[triangle[1] as usize];
        let c = mesh.positions[triangle[2] as usize];
        total += (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
            + a[2] * (b[0] * c[1] - b[1] * c[0]))
            / 6.0;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cube of side `side`, eight vertices, normals outward along the
    /// axes.
    fn cube(side: f64) -> Mesh {
        let h = 0.5 * side;
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        for z in [-h, h] {
            for y in [-h, h] {
                for x in [-h, h] {
                    positions.push([x, y, z]);
                    let n = (x * x + y * y + z * z).sqrt();
                    normals.push([(x / n) as f32, (y / n) as f32, (z / n) as f32]);
                }
            }
        }
        // Vertices by bits (x, y, z); faces written out with outward
        // winding.
        let quad = |a: u32, b: u32, c: u32, d: u32| vec![a, b, c, a, c, d];
        let mut indices = Vec::new();
        for face in [
            quad(0, 2, 3, 1), // -z
            quad(4, 5, 7, 6), // +z
            quad(0, 1, 5, 4), // -y
            quad(2, 6, 7, 3), // +y
            quad(0, 4, 6, 2), // -x
            quad(1, 3, 7, 5), // +x
        ] {
            indices.extend(face);
        }
        Mesh {
            positions,
            normals,
            indices,
        }
    }

    /// The volume of a cube is the volume of a cube, and the sign tells the
    /// winding.
    ///
    /// An oracle for the oracle: the model is compared against Blender with it
    /// later, so an error here would pass unnoticed in both directions.
    #[test]
    fn the_signed_volume_of_a_cube_is_its_volume() {
        let mesh = cube(2.0);
        assert!((signed_volume(&mesh) - 8.0).abs() < 1e-12);

        // A flipped winding gives the same volume with a minus.
        let mut flipped = mesh.clone();
        for triangle in flipped.indices.chunks_exact_mut(3) {
            triangle.swap(1, 2);
        }
        assert!((signed_volume(&flipped) + 8.0).abs() < 1e-12);
    }

    /// Normalisation divides by the length along `+Z` and by nothing else.
    #[test]
    fn a_model_comes_out_one_unit_long() {
        let model = Model::from_metres(cube(6.0), Vec::new()).expect("a cube is a model");
        assert!((model.height_m - 6.0).abs() < 1e-12);
        let low = model
            .mesh
            .positions
            .iter()
            .fold(f64::INFINITY, |acc, p| acc.min(p[2]));
        let high = model
            .mesh
            .positions
            .iter()
            .fold(f64::NEG_INFINITY, |acc, p| acc.max(p[2]));
        assert!((high - low - 1.0).abs() < 1e-12, "length {}", high - low);

        // A cube's bounding-sphere radius is half its diagonal.
        assert!(
            (model.extent - 0.75_f64.sqrt()).abs() < 1e-12,
            "extent {}",
            model.extent
        );
        // And it is larger than half the height -- so it is not derived from
        // it.
        assert!(model.extent > 0.5);
    }

    /// The file returns exactly what was put into it.
    #[test]
    fn a_model_survives_the_round_trip() {
        let model = Model::from_metres(cube(6.0), Vec::new()).expect("a cube is a model");
        let read = Model::from_bytes(&model.to_bytes()).expect("our own file must be readable");

        assert_eq!(read.height_m as f32, model.height_m as f32);
        assert_eq!(read.extent as f32, model.extent as f32);
        assert_eq!(read.mesh.indices, model.mesh.indices);
        assert_eq!(read.mesh.normals, model.mesh.normals);
        for (a, b) in read.mesh.positions.iter().zip(&model.mesh.positions) {
            for k in 0..3 {
                assert_eq!(a[k] as f32, b[k] as f32, "a vertex moved");
            }
        }
    }

    /// A painted model also returns exactly what was put into it -- and an
    /// unpainted one stays unpainted.
    ///
    /// Two halves of one question: the file has a **word in the header** about
    /// paint, and it is what decides how many bytes per vertex to read next. An
    /// error here would not give a read error -- it would shift the indices,
    /// that is hand back a plausible mesh with scrambled geometry.
    #[test]
    fn paint_survives_the_round_trip() {
        let mesh = cube(6.0);
        let paint: Vec<[f32; 3]> = (0..mesh.positions.len())
            .map(|k| [k as f32 / 16.0, 0.25, 0.5])
            .collect();
        let model = Model::from_metres(mesh, paint.clone()).expect("a cube is a model");
        let read = Model::from_bytes(&model.to_bytes()).expect("our own file must be readable");
        assert_eq!(read.paint, paint);
        assert_eq!(read.mesh.indices, model.mesh.indices);
        assert_eq!(read.mesh.normals, model.mesh.normals);

        let plain = Model::from_metres(cube(6.0), Vec::new()).expect("a cube is a model");
        let read = Model::from_bytes(&plain.to_bytes()).expect("our own file must be readable");
        assert!(read.paint.is_empty(), "paint appeared out of nowhere");
        assert!(
            plain.to_bytes().len() < model.to_bytes().len(),
            "the unpainted file did not get shorter"
        );
    }

    /// Paint for the wrong number of vertices is an error, not a silent
    /// truncation.
    #[test]
    fn paint_of_the_wrong_length_is_an_error() {
        let message = Model::from_metres(cube(6.0), vec![[1.0, 0.0, 0.0]; 3])
            .expect_err("three colours are not enough for a cube");
        assert!(message.contains("paint"), "wrong message: {message}");
    }

    /// An alien file, an alien version and a truncated file are errors rather
    /// than garbage.
    #[test]
    fn a_wrong_file_says_what_is_wrong() {
        let model = Model::from_metres(cube(6.0), Vec::new()).expect("a cube is a model");
        let bytes = model.to_bytes();

        let message = Model::from_bytes(&bytes[..HEADER_BYTES - 1]).expect_err("a short file");
        assert!(message.contains("shorter"), "wrong message: {message}");

        let mut alien = bytes.clone();
        alien[0..8].copy_from_slice(&crate::tiles::MAGIC);
        let message = Model::from_bytes(&alien).expect_err("a tileset is not a mesh");
        assert!(message.contains("signature"), "wrong message: {message}");

        let mut future = bytes.clone();
        future[8..12].copy_from_slice(&(VERSION + 1).to_le_bytes());
        let message = Model::from_bytes(&future).expect_err("an alien version");
        assert!(message.contains("version"), "wrong message: {message}");

        let message = Model::from_bytes(&bytes[..bytes.len() - 4]).expect_err("a truncated file");
        assert!(message.contains("bytes"), "wrong message: {message}");
    }
}
