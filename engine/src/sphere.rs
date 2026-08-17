//! A sphere at real scale: the mesh and Earth's radius (ROADMAP F5).
//!
//! The cubesphere and a real DEM are M4 (PROJECT.md §7). All that is needed
//! here is a correct shape at the right scale, to check what the step exists
//! for: whether the engine holds a flight from the surface to orbit without
//! breaks.

/// Earth's mean radius (IAU 2015), metres.
pub const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// A sphere mesh. Positions are world coordinates in `double` with the sphere
/// at the origin: the camera is subtracted from them on the CPU every frame
/// ([`crate::camera`], ROADMAP F4), so they are stored unreduced here.
/// Normals are directions rather than positions and need no double: there is
/// no catastrophic cancellation of large numbers to catch.
#[derive(Clone, Debug)]
pub struct Mesh {
    pub positions: Vec<[f64; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

/// A UV sphere: `lat_segments` parallels, `lon_segments` meridians.
///
/// Not an icosphere -- a simpler construction, and grid uniformity is not
/// critical for F5: the step checks scale and depth, not tessellation
/// quality.
pub fn generate(radius: f64, lat_segments: u32, lon_segments: u32) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();

    for i in 0..=lat_segments {
        let v = f64::from(i) / f64::from(lat_segments);
        let theta = v * std::f64::consts::PI;
        let (sin_theta, cos_theta) = (theta.sin(), theta.cos());

        for j in 0..=lon_segments {
            let u = f64::from(j) / f64::from(lon_segments);
            let phi = u * 2.0 * std::f64::consts::PI;
            let (sin_phi, cos_phi) = (phi.sin(), phi.cos());

            let direction = [sin_theta * cos_phi, sin_theta * sin_phi, cos_theta];
            positions.push([
                direction[0] * radius,
                direction[1] * radius,
                direction[2] * radius,
            ]);
            normals.push([
                direction[0] as f32,
                direction[1] as f32,
                direction[2] as f32,
            ]);
        }
    }

    // +1, because on every parallel the last vertex (j == lon_segments)
    // duplicates the first -- the seam closes by repetition rather than by a
    // wrapping index.
    let stride = lon_segments + 1;
    let mut indices = Vec::new();
    for i in 0..lat_segments {
        for j in 0..lon_segments {
            let a = i * stride + j;
            let b = a + stride;

            indices.push(a);
            indices.push(b);
            indices.push(a + 1);

            indices.push(a + 1);
            indices.push(b);
            indices.push(b + 1);
        }
    }

    Mesh {
        positions,
        normals,
        indices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_vertex_sits_on_the_sphere() {
        let mesh = generate(EARTH_RADIUS_M, 8, 16);
        for p in &mesh.positions {
            let r = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
            assert!(
                (r - EARTH_RADIUS_M).abs() < 1e-6,
                "a vertex at distance {r} rather than {EARTH_RADIUS_M}"
            );
        }
    }

    #[test]
    fn every_index_is_in_range() {
        let mesh = generate(EARTH_RADIUS_M, 8, 16);
        let count = mesh.positions.len() as u32;
        for &i in &mesh.indices {
            assert!(i < count, "index {i} is outside the {count} vertices");
        }
    }
}
