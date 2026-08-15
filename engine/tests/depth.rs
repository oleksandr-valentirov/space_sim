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

// ---------------------------------------------------------------------------
// Чотири діапазони глибини (R4b)

/// Композиція проходами не гірша за один прохід — на тій самій парі поверхонь.
///
/// Позитивне твердження, як і решта файлу. Що воно **не** доводить, сказано
/// поруч числом: діапазони не роблять глибину роздільнішою взагалі
/// (`engine::depth::tests::a_finite_range_is_no_sharper_than_an_infinite_one`
/// — 4.0 м на 10⁸ м однаково в усіх трьох варіантах). Тому пара тут узята
/// **роздільна** (зазор 10⁴ м на 10⁸ м проти межі 4 м), і перевіряється рівно
/// те, за що проходи відповідають: back-to-front, очищення глибини між
/// проходами й площини відсікання, що ділять сцену без шва.
///
/// Обидва проходи містять обидві поверхні — розділяють їх площини, а не рука
/// в тесті, рівно як у `frame::Frame::plan`.
#[test]
fn splitting_the_scene_into_ranges_keeps_the_nearer_surface_in_front() {
    let Some(gpu) = Gpu::for_tests() else { return };

    const DISTANCE: f64 = 1.0e8;
    const GAP: f64 = 1.0e4;
    const FOV_Y: f64 = std::f64::consts::PI / 3.0;

    let quad = |distance: f64, colour: [f32; 4], projection| engine::depth_probe::Params {
        projection,
        colour,
        // Удвічі більше за півекран на цій відстані — накриває кадр цілком.
        placement: [
            0.0,
            0.0,
            -distance as f32,
            (2.0 * distance * (FOV_Y / 2.0).tan()) as f32,
        ],
    };
    let far_colour = [0.9, 0.1, 0.1, 1.0];
    let near_colour = [0.1, 0.9, 0.1, 1.0];

    let boundary = DISTANCE - GAP / 2.0;
    let outer = depth::reversed_infinite(FOV_Y, 1.0, boundary);
    let inner = depth::reversed_finite(FOV_Y, 1.0, boundary / 1.0e4, boundary);
    let far_range = [
        quad(DISTANCE, far_colour, outer),
        quad(DISTANCE - GAP, near_colour, outer),
    ];
    let near_range = [
        quad(DISTANCE, far_colour, inner),
        quad(DISTANCE - GAP, near_colour, inner),
    ];

    let split =
        engine::depth_probe::render_ranges(&gpu, SIZE, SIZE, true, &[&far_range, &near_range])
            .expect("кадр мав намалюватися");

    // Той самий кадр одним проходом — щоб число мало з чим порівнятись.
    let one = depth::reversed_infinite(FOV_Y, 1.0, NEAR);
    let together = [
        quad(DISTANCE, far_colour, one),
        quad(DISTANCE - GAP, near_colour, one),
    ];
    let single = engine::depth_probe::render_ranges(&gpu, SIZE, SIZE, true, &[&together])
        .expect("кадр мав намалюватися");

    println!(
        "  {DISTANCE:.0e} м, зазор {GAP:.0e} м: один прохід {:.3}, два \
         діапазони {:.3}",
        single.near_wins, split.near_wins
    );
    assert!(
        single.near_wins > 0.99,
        "один прохід не впорався з роздільною парою: {:.3}",
        single.near_wins
    );
    assert!(
        split.near_wins > 0.99,
        "поділ на діапазони загубив ближчу поверхню: {:.3}",
        split.near_wins
    );
}
