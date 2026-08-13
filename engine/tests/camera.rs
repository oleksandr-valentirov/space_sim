//! Camera-relative не залежить від відстані до початку координат (ROADMAP F4).
//!
//! Три твердження, кожне падає окремо:
//!
//!   1. на 1 а.о. правильний шлях рухає об'єкт плавно;
//!   2. наївний шлях там узагалі втрачає об'єкт — без цього перше було б
//!      твердженням без порівняння;
//!   3. правильний шлях дає **той самий** рух на 10³ і на 10¹¹ м. Це
//!      найсильніше формулювання: не «похибка мала», а «відстані немає
//!      в рівнянні».

use engine::camera_probe::{sweep_at, ASTRONOMICAL_UNIT};
use engine::gpu::Gpu;

const SIZE: u32 = 256;
const STEPS: u32 = 12;
/// Крок, видимий у пікселях. На міліметрі обидва шляхи дали б нуль, і тест
/// не розрізнив би нічого — 1 мм на 10 м це 0.004 пікселя.
const STEP_M: f64 = 0.1;

fn shifts(relative: bool, distance: f64) -> Option<Vec<f64>> {
    let Ok(gpu) = Gpu::new(wgpu::Instance::default(), None) else {
        eprintln!("ПРОПУЩЕНО: немає адаптера wgpu");
        return None;
    };

    let steps = sweep_at(&gpu, SIZE, relative, STEPS, STEP_M, distance).expect("замір мав пройти");

    if steps.iter().any(|s| s.visible == 0) {
        return Some(Vec::new());
    }

    Some(steps.iter().skip(1).map(|s| s.shift).collect())
}

#[test]
fn camera_relative_moves_smoothly_at_one_astronomical_unit() {
    let Some(shifts) = shifts(true, ASTRONOMICAL_UNIT) else {
        return;
    };
    assert!(
        !shifts.is_empty(),
        "об'єкт мав бути видимий у кожному кадрі"
    );

    let mean = shifts.iter().sum::<f64>() / shifts.len() as f64;
    assert!(
        mean > 1.0,
        "камера рухалась, а зображення — ні: {mean:.3} px"
    );

    for shift in &shifts {
        assert!(
            (shift - mean).abs() < mean * 0.5,
            "рух нерівний: крок {shift:.3} px проти середнього {mean:.3}"
        );
    }
}

#[test]
fn the_naive_path_loses_the_object_entirely_there() {
    let Some(shifts) = shifts(false, ASTRONOMICAL_UNIT) else {
        return;
    };
    assert!(
        shifts.is_empty(),
        "наївний шлях раптом упорався на 1 а.о. — тоді порівняння в F4 \
         нічого не доводить, і причину треба знайти"
    );
}

/// Найсильніше твердження кроку.
#[test]
fn camera_relative_behaves_the_same_eight_orders_apart() {
    let Some(near) = shifts(true, 1e3) else {
        return;
    };
    let Some(far) = shifts(true, ASTRONOMICAL_UNIT) else {
        return;
    };

    assert!(!near.is_empty() && !far.is_empty());
    assert_eq!(near.len(), far.len());

    for (a, b) in near.iter().zip(far.iter()) {
        assert!(
            (a - b).abs() < 1e-9,
            "рух на 10³ м ({a:.6}) і на 1 а.о. ({b:.6}) розійшовся — \
             відстань до початку координат не має входити в результат"
        );
    }
}
