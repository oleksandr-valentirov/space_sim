//! Reversed-Z справді дає те, заради чого його брали (ROADMAP F3).
//!
//! Три різні твердження, і кожне падає окремо:
//!
//!   1. reversed-Z розрізняє метр на 10⁷ м;
//!   2. звичайна проєкція там уже не розрізняє **нічого** — без цього перше
//!      було б твердженням без порівняння;
//!   3. на 10⁸ м не розрізняє і reversed-Z, і це не вада, а межа float32.
//!
//! Третє тут не «відомий баг», а зафіксована межа: саме через неї PROJECT.md
//! §7 вимагає багатопрохідного рендера по діапазонах.

use engine::depth;
use engine::depth_probe::{measure, Setup};
use engine::gpu::Gpu;

const SIZE: u32 = 128;
const NEAR: f64 = 0.1;

fn near_wins(reversed: bool, distance: f64, gap: f64) -> Option<f64> {
    let Ok(gpu) = Gpu::new(wgpu::Instance::default(), None) else {
        eprintln!("ПРОПУЩЕНО: немає адаптера wgpu");
        return None;
    };

    let measured = measure(
        &gpu,
        SIZE,
        SIZE,
        &Setup {
            reversed,
            near: NEAR,
            distance,
            gap,
        },
    )
    .expect("замір мав пройти");

    Some(measured.near_wins)
}

#[test]
fn reversed_z_resolves_a_metre_at_ten_million() {
    let Some(share) = near_wins(true, 1e7, 1.0) else {
        return;
    };
    assert_eq!(
        share, 1.0,
        "ближча поверхня мала виграти весь кадр, виграла {share}"
    );
}

#[test]
fn a_conventional_projection_loses_the_same_case_entirely() {
    let Some(share) = near_wins(false, 1e7, 1.0) else {
        return;
    };
    assert!(
        share < 0.01,
        "звичайна проєкція раптом упоралася ({share}) — тоді порівняння \
         в F3 нічого не доводить, і причину треба знайти"
    );
}

/// Межа, а не помилка.
#[test]
fn even_reversed_z_cannot_resolve_a_metre_at_a_hundred_million() {
    let Some(share) = near_wins(true, 1e8, 1.0) else {
        return;
    };

    assert!(
        depth::resolvable_gap(1e8) > 1.0,
        "оцінка каже, що метр на 10⁸ м мав би бути роздільним — тоді \
         арифметика в depth.rs розійшлася з дійсністю"
    );
    assert!(
        share < 1.0,
        "метр на 10⁸ м раптом роздільний ({share}). Це добра новина, але \
         вона суперечить розрахунку — перевірте формат глибини"
    );
}

/// А зазор, більший за межу, — роздільний і там.
#[test]
fn a_gap_above_the_limit_resolves_at_a_hundred_million() {
    let limit = depth::resolvable_gap(1e8);
    let Some(share) = near_wins(true, 1e8, limit * 10.0) else {
        return;
    };
    assert_eq!(
        share,
        1.0,
        "зазор {} м удесятеро більший за межу мав розрізнитися",
        limit * 10.0
    );
}
