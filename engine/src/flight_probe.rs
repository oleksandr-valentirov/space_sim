//! Проліт від поверхні до орбіти без стрибків (ROADMAP F5).
//!
//! Камера дивиться на центр сфери вздовж фіксованого радіального напрямку,
//! з відстані `R + висота`. Сфера опукла, тож кут, під яким видно її край
//! з зовнішньої точки, має точну формулу без наближень:
//!
//! ```text
//! half_angle = asin(R / (R + висота))
//! ```
//!
//! Порівняння виміряного з розрахованим — той самий інструмент, що в
//! `depth_probe` (F3, `resolvable_gap`) і `camera_probe` (F4): не «здається,
//! нормально», а число проти числа.

use crate::camera::Camera;
use crate::gpu::Gpu;
use crate::shot::Shot;
use crate::sphere::{self, Mesh};
use crate::sphere_render::{self, Params};

const FOV_Y: f64 = std::f64::consts::PI / 3.0;

/// Набагато менша за найближчий проліт (10 м): камера не має впертися в
/// near ще до того, як досягне поверхні.
const NEAR: f64 = 1.0;

const LIGHT_DIR: [f32; 3] = [0.4, 0.4, 0.82];
const COLOUR: [f32; 4] = [0.2, 0.6, 0.9, 1.0];

pub struct Sample {
    pub altitude: f64,
    pub expected_half_angle: f64,
    /// Частка пікселів кадру, яку займає сфера.
    pub coverage: f64,
    pub shot: Shot,
}

/// Малює сферу з висоти `altitude` над поверхнею, камера дивиться на центр.
pub fn measure(
    gpu: &Gpu,
    width: u32,
    height: u32,
    mesh: &Mesh,
    altitude: f64,
) -> Result<Sample, String> {
    let radius = sphere::EARTH_RADIUS_M;
    let distance = radius + altitude;

    let camera = Camera::look_at([distance, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);

    let shot = sphere_render::render(
        gpu,
        width,
        height,
        &camera,
        mesh,
        &Params {
            near: NEAR,
            light_dir: LIGHT_DIR,
            colour: COLOUR,
        },
    )?;

    let coverage = coverage_fraction(&shot);

    Ok(Sample {
        altitude,
        expected_half_angle: (radius / distance).asin(),
        coverage,
        shot,
    })
}

fn coverage_fraction(shot: &Shot) -> f64 {
    let mut lit = 0u64;
    let total = u64::from(shot.width) * u64::from(shot.height);
    for y in 0..shot.height {
        for x in 0..shot.width {
            let p = shot.pixel(x, y);
            if p[0] > 3 || p[1] > 3 || p[2] > 3 {
                lit += 1;
            }
        }
    }
    lit as f64 / total as f64
}

/// Аналітична частка кадру для диска силуету радіусом `half_angle`.
///
/// Визначена лише у двох однозначних випадках: диск цілком у кадрі, або
/// кадр цілком у диску (сфера заповнює все, аж за кутами). Проміжок, де
/// диск обрізаний краєм кадру, але не покриває кутів, — не має простої
/// формули без інтеграла кругового сегмента, і тут навмисно не рахується.
pub fn expected_coverage(half_angle: f64, aspect: f64) -> Option<f64> {
    let radius_fraction = half_angle.tan() / (FOV_Y / 2.0).tan();
    let min_extent = aspect.min(1.0);
    let diagonal = (1.0 + aspect * aspect).sqrt();

    if radius_fraction <= min_extent {
        Some(std::f64::consts::PI * radius_fraction * radius_fraction / (4.0 * aspect))
    } else if radius_fraction >= diagonal {
        Some(1.0)
    } else {
        None
    }
}

/// Висоти від 10 м до 10⁷ м, `steps` точок, рівномірно за логарифмом —
/// саме той діапазон, що в критерії F5.
pub fn altitudes(steps: u32) -> Vec<f64> {
    let lo = 10f64.log10();
    let hi = 7.0;
    (0..steps)
        .map(|i| {
            let t = f64::from(i) / f64::from(steps - 1);
            10f64.powf(lo + t * (hi - lo))
        })
        .collect()
}

/// Проганяє `altitudes(steps)` і повертає виміряні зразки — використовується
/// і для друку таблиці, і для тесту неперервності.
pub fn sweep(gpu: &Gpu, size: u32, mesh: &Mesh, steps: u32) -> Result<Vec<Sample>, String> {
    altitudes(steps)
        .into_iter()
        .map(|altitude| measure(gpu, size, size, mesh, altitude))
        .collect()
}
