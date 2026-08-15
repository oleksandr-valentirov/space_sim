//! Reversed-Z справді дає те, заради чого його брали (ROADMAP F3).
//!
//! Тут живуть тільки **позитивні** твердження — «ближча поверхня виграла
//! кадр». У них різниця глибин справді є, драйверу нема чого доокруглювати,
//! і відповідь та сама на llvmpipe, на апаратному Vulkan і на Metal.
//!
//! Зворотні твердження — «а тут глибина не розрізняє нічого» — на GPU
//! недоказові: коли обидві поверхні пишуть однаковий біт, переможця вирішує
//! трактування нічиєї конкретним растеризатором, а не глибина. Вони
//! перевіряються арифметикою в `engine::depth` (юніт-тести того модуля), де
//! видно саме те, що стверджується. Історія цієї правки — ROADMAP F3.

use engine::depth;
use engine::depth_probe::{measure, Setup};
use engine::gpu::Gpu;

const SIZE: u32 = 128;
const NEAR: f64 = 0.1;

fn near_wins(reversed: bool, distance: f64, gap: f64) -> Option<f64> {
    let gpu = Gpu::for_tests()?;

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

/// ⚠ Найтонше місце в цьому файлі: `z_ndc` двох поверхонь тут різняться
/// рівно на **1 ULP** (перевірено арифметикою, ROADMAP F3). Різниця є, тож
/// твердження законне — але запасу немає, і якщо колись цей тест почервоніє
/// на новому залізі, першим ділом дивіться сюди, а не на рушій. Клітинка
/// лишається саме такою свідомо: 1 м на 10⁷ м — це і є край, який F3 міряв.
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
