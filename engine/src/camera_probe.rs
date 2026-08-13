//! Camera-relative проти наївного шляху (ROADMAP F4).
//!
//! PROJECT.md §7, рішення 1: світові координати НІКОЛИ не потрапляють у
//! `float`. Трансформації рахуються в `double` на CPU відносно камери, у
//! шейдер їде вже `float` у камерному просторі.
//!
//! Цей модуль міряє, що саме це дає. Обидва шляхи закінчуються однаково —
//! чотирикутник у камерному просторі, той самий шейдер, той самий пайплайн.
//! Різниця тільки в арифметиці на CPU:
//!
//!   camera-relative   `(світ_об'єкта − світ_камери) as f32`
//!   наївний           `світ_об'єкта as f32 − світ_камери as f32`
//!
//! Другий губить усе на відніманні близьких великих чисел. На 1 а.о.
//! (1.496·10¹¹ м) ULP у `f32` — приблизно **16 км**, тож обидві координати
//! спершу лягають на 16-кілометрову ґратку, а вже потім віднімаються.
//! Об'єкт за десять метрів від камери опиняється або точно на ній, або за
//! кілометри.
//!
//! Міряється зсув центру ваги об'єкта в пікселях між кадрами, поки камера
//! рухається міліметровими кроками. Правильний шлях дає плавний субпіксельний
//! рух; наївний — нерухомість, а тоді стрибок, тобто саме тремтіння.

use crate::depth;
use crate::depth_probe::{render_quads, Params};
use crate::gpu::Gpu;

/// Відстань від світового початку — та сама, що в критерії F4.
pub const ASTRONOMICAL_UNIT: f64 = 1.495_978_707e11;

/// Скільки метрів між камерою й об'єктом. Десять метрів — масштаб локальної
/// сцени корабля з PROJECT.md §7.
const RANGE: f64 = 10.0;

/// Півширина об'єкта. Метр на десяти метрах — помітна, але не на весь екран,
/// інакше центр ваги нічого не показував би.
const HALF_SIZE: f64 = 1.0;

const FOV_Y: f64 = std::f64::consts::PI / 3.0;
const NEAR: f64 = 0.1;

pub struct Step {
    /// Зсув центру ваги проти попереднього кадру, пікселі.
    pub shift: f64,
    /// Скільки пікселів зайняв об'єкт. Нуль означає, що його не видно.
    pub visible: u64,
}

/// Проганяє камеру `steps` кроками по `step_m` метрів і повертає, як рухався
/// об'єкт у кадрі.
pub fn sweep(
    gpu: &Gpu,
    size: u32,
    relative: bool,
    steps: u32,
    step_m: f64,
) -> Result<Vec<Step>, String> {
    sweep_at(gpu, size, relative, steps, step_m, ASTRONOMICAL_UNIT)
}

/// Те саме, але з довільною відстанню до світового початку.
///
/// Існує, бо на 1 а.о. наївний шлях не тремтить, а **зникає**: ULP у 16 км
/// проти сцени в десять метрів не лишає від неї нічого. Тремтіння видно там,
/// де ULP порівнянний із розміром об'єкта, і знайти цю відстань — окреме
/// питання, на яке відповідає розгортка.
pub fn sweep_at(
    gpu: &Gpu,
    size: u32,
    relative: bool,
    steps: u32,
    step_m: f64,
    origin_distance: f64,
) -> Result<Vec<Step>, String> {
    let projection = depth::reversed_infinite(FOV_Y, 1.0, NEAR);

    // Обидва — далеко від початку координат. Саме це й ламає наївний шлях:
    // не велика відстань між ними, а велика відстань до нуля.
    let object_world = [origin_distance, 0.0, 0.0];

    let mut out = Vec::new();
    let mut previous: Option<(f64, f64)> = None;

    for i in 0..steps {
        let camera_world = [
            origin_distance + RANGE,
            f64::from(i) * step_m,
            f64::from(i) * step_m,
        ];

        let view = if relative {
            // Віднімання у double, і аж потім звуження. Різниця мала, тож у
            // f32 вона представлена з повною точністю.
            [
                (object_world[0] - camera_world[0]) as f32,
                (object_world[1] - camera_world[1]) as f32,
                (object_world[2] - camera_world[2]) as f32,
            ]
        } else {
            // Звуження, і аж потім віднімання. Обидва доданки лягають на
            // ґратку з кроком ~16 км, і те, що лишається, — шум цієї ґратки.
            [
                object_world[0] as f32 - camera_world[0] as f32,
                object_world[1] as f32 - camera_world[1] as f32,
                object_world[2] as f32 - camera_world[2] as f32,
            ]
        };

        // Камера дивиться вздовж -z, а різниця вище лежить уздовж +x.
        // Перекладаємо: віддалення від камери стає -z.
        let centre = [view[1], view[2], -view[0].abs()];

        let params = Params {
            projection,
            colour: [0.2, 0.9, 0.3, 1.0],
            placement: [centre[0], centre[1], centre[2], HALF_SIZE as f32],
        };

        let measured = render_quads(gpu, size, size, true, &[params])?;
        let centroid = centroid(&measured.shot);

        let shift = match (previous, centroid) {
            (Some((px, py)), Some((cx, cy))) => ((cx - px).powi(2) + (cy - py).powi(2)).sqrt(),
            _ => 0.0,
        };

        if centroid.is_some() {
            previous = centroid;
        }

        out.push(Step {
            shift,
            visible: visible_pixels(&measured.shot),
        });
    }

    Ok(out)
}

fn visible_pixels(shot: &crate::shot::Shot) -> u64 {
    let mut count = 0;
    for y in 0..shot.height {
        for x in 0..shot.width {
            if shot.pixel(x, y)[1] > 40 {
                count += 1;
            }
        }
    }
    count
}

/// Центр ваги видимих пікселів. `None`, якщо об'єкта в кадрі немає.
fn centroid(shot: &crate::shot::Shot) -> Option<(f64, f64)> {
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut count = 0u64;

    for y in 0..shot.height {
        for x in 0..shot.width {
            if shot.pixel(x, y)[1] > 40 {
                sum_x += f64::from(x);
                sum_y += f64::from(y);
                count += 1;
            }
        }
    }

    if count == 0 {
        None
    } else {
        Some((sum_x / count as f64, sum_y / count as f64))
    }
}
