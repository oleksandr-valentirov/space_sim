//! Наскільки пливе константа Якобі вздовж справжньої місії (ROADMAP-UI.md,
//! U6b1).
//!
//! Це вимір **до** малювання, і від нього залежить форма віджета. `C`
//! зберігається в CR3BP — там два тіла на сталій відстані обертаються рівномірно.
//! Гра літає в повній ефемериді: відстань Земля-Місяць гуляє на десяту частину
//! за місяць, у моделі є Сонце, гармоніки, тиск світла. Тому крива нульової
//! швидкості, побудована за миттєвим `C`, — **довідка, а не межа**, і питання
//! лише в тому, наскільки вона дихає на очах.
//!
//! Число, яке звідси виходить, вирішує: жива крива, що переслідує апарат, чи
//! зріз на обраний момент.

use core_rs::{cr3bp_jacobi, State, Vec3d};
use game::mission;
use game::world::{EARTH, MOON};

/// Константа Якобі апарата в кожному семплі місії.
///
/// Кожен семпл бере фрейм **своєї миті** — інакше міряли б не дрейф `C`, а
/// власне обертання пари.
fn jacobi_along_the_mission() -> Vec<(f64, f64)> {
    let eph = std::sync::Arc::new(
        core_rs::Ephemeris::load(&mission::default_asset()).expect("ассет читається"),
    );
    let mut world = mission::world(&mission::default_asset()).expect("світ");
    world.run_to_end(1.0, 8);
    let snapshot = world.snapshot();

    let mut out = Vec::new();
    for leg in &snapshot.vessels[0].legs {
        for sample in &leg.samples {
            let t = sample.state.t;
            let frame = eph
                .synodic_frame(EARTH, MOON, t)
                .expect("фрейм будується на кожній миті місії");

            let inertial = State {
                t,
                r: Vec3d {
                    x: sample.state.r.x,
                    y: sample.state.r.y,
                    z: sample.state.r.z,
                },
                v: Vec3d {
                    x: sample.state.v.x,
                    y: sample.state.v.y,
                    z: sample.state.v.z,
                },
            };
            let synodic = frame.from_inertial(&inertial);
            out.push((t, cr3bp_jacobi(synodic.r, synodic.v, frame.mass_ratio())));
        }
    }
    out
}

/// Розмах `C` уздовж місії — одне число, яке вирішує форму віджета.
///
/// Друкується таблиця вікон, а не одне число: доба гри при warp 10⁵ — це
/// десять секунд дивлення, місяць — п'ять хвилин, і «дихає на очах» означає
/// різне на цих двох масштабах. Останній рядок (уся місія) — уже не дрейф, а
/// розпад: апарат сходить з halo-орбіти й іде туди, де синодичний фрейм пари
/// нічого не описує.
#[test]
fn how_far_the_jacobi_constant_drifts_along_the_mission() {
    let series = jacobi_along_the_mission();
    assert!(
        series.len() > 1000,
        "місія дала лише {} точок",
        series.len()
    );

    let (t0, c0) = series[0];
    let spread = |from: usize, to: usize| {
        let mut low = f64::INFINITY;
        let mut high = f64::NEG_INFINITY;
        for &(_, c) in &series[from..to] {
            low = low.min(c);
            high = high.max(c);
        }
        high - low
    };

    let whole = spread(0, series.len());

    println!("  C на старті: {c0:.6}");
    println!("  {:>10} {:>12} {:>12}", "вікно", "розмах C", "% від C");
    for days in [1.0, 7.0, 14.0, 30.0] {
        let end = series
            .iter()
            .position(|&(t, _)| t > t0 + days * 86400.0)
            .unwrap_or(series.len());
        let range = spread(0, end);
        println!(
            "  {:>8.0} діб {:>12.6} {:>11.4}%",
            days,
            range,
            range / c0 * 100.0
        );
    }
    println!(
        "  {:>8.0} діб {:>12.6} {:>11.4}%   (уся місія)",
        (series[series.len() - 1].0 - t0) / 86400.0,
        whole,
        whole / c0 * 100.0
    );

    // Твердження навмисно широкі: тут міряється фізика, а не наш код, і
    // завузький допуск перетворив би вимір на фіксацію числа. Ловиться інше —
    // що `C` взагалі рахується (не NaN, не нуль) і що вона не стала.
    assert!(c0.is_finite() && (2.0..4.0).contains(&c0), "C = {c0}");
    assert!(
        whole > 0.0,
        "константа Якобі не змінилась узагалі — це означало б, що вимір міряє \
         не те"
    );
    assert!(
        whole < c0,
        "розмах {whole:.6} завбільшки з саму константу {c0:.6} — це вже не дрейф"
    );
}
