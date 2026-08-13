//! Сфера в реальному масштабі: меш і радіус Землі (ROADMAP F5).
//!
//! Кубосфера й реальний DEM — M4 (PROJECT.md §7). Тут потрібна лише
//! коректна форма й правильний масштаб, щоб перевірити те, заради чого
//! крок існує: чи тримає рушій проліт від поверхні до орбіти без розривів.

/// Середній радіус Землі (IAU 2015), метри.
pub const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// Меш сфери. Позиції — світові координати в `double`, сфера в центрі
/// координат: камера віднімається з них на CPU щокадру ([`crate::camera`],
/// ROADMAP F4), тож тут їх зберігають нескороченими. Нормалі — це напрямки,
/// а не позиції, для них double не потрібен: катастрофічного скорочення при
/// відніманні великих чисел тут просто нема чого ловити.
pub struct Mesh {
    pub positions: Vec<[f64; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

/// UV-сфера: `lat_segments` паралелей, `lon_segments` меридіанів.
///
/// Не ікосфера — простіша побудова, а рівномірність сітки для F5 не
/// критична: крок перевіряє масштаб і глибину, не якість тесселяції.
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

    // +1, бо на кожній паралелі остання вершина (j == lon_segments)
    // дублює першу — шов замикається повторенням, не індексом-обгорткою.
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
                "вершина на відстані {r}, а не {EARTH_RADIUS_M}"
            );
        }
    }

    #[test]
    fn every_index_is_in_range() {
        let mesh = generate(EARTH_RADIUS_M, 8, 16);
        let count = mesh.positions.len() as u32;
        for &i in &mesh.indices {
            assert!(i < count, "індекс {i} поза межами {count} вершин");
        }
    }
}
