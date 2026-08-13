//! Сфера в реальному масштабі: проліт від поверхні до орбіти без розривів
//! (ROADMAP F5).
//!
//! Три твердження:
//!
//!   1. близько до поверхні сфера займає весь кадр — вона опукла й велика,
//!      інакше десь загубилась камера-relative арифметика;
//!   2. на орбіті (10⁷ м) виміряний силует збігається з точною формулою
//!      `asin(R/(R+висота))` — без цього перше нічого не доводить;
//!   3. між ними покриття кадру НЕ зростає — стрибок уверх і був би тим
//!      самим розривом, який критерій F5 забороняє.

use engine::flight_probe::{expected_coverage, sweep};
use engine::gpu::Gpu;
use engine::sphere;

const SIZE: u32 = 256;
const STEPS: u32 = 15;

fn samples() -> Option<Vec<engine::flight_probe::Sample>> {
    let Ok(gpu) = Gpu::new(wgpu::Instance::default(), None) else {
        eprintln!("ПРОПУЩЕНО: немає адаптера wgpu");
        return None;
    };

    // Низька роздільність меша: тест перевіряє масштаб і глибину, не якість
    // тесселяції — і швидший headless-прогін.
    let mesh = sphere::generate(sphere::EARTH_RADIUS_M, 32, 64);
    Some(sweep(&gpu, SIZE, &mesh, STEPS).expect("проліт мав пройти"))
}

#[test]
fn the_sphere_fills_the_frame_ten_metres_up() {
    let Some(samples) = samples() else {
        return;
    };
    let first = &samples[0];
    assert!(
        first.coverage > 0.99,
        "на висоті {} м сфера мала заповнити весь кадр, зайняла {}",
        first.altitude,
        first.coverage
    );
}

#[test]
fn the_silhouette_matches_the_analytic_disc_at_orbit() {
    let Some(samples) = samples() else {
        return;
    };
    let last = samples.last().unwrap();

    let expected = expected_coverage(last.expected_half_angle, 1.0)
        .expect("на 10⁷ м диск мав уміститись у кадр");

    assert!(
        (last.coverage - expected).abs() < 0.02,
        "виміряне покриття {:.4} проти аналітичного {:.4} на висоті {:.0e} м",
        last.coverage,
        expected,
        last.altitude
    );
}

/// Найсильніше твердження кроку: саме воно й перевіряє «без розривів».
#[test]
fn coverage_never_grows_as_altitude_increases() {
    let Some(samples) = samples() else {
        return;
    };

    for pair in samples.windows(2) {
        let [a, b] = pair else { unreachable!() };
        assert!(
            b.coverage <= a.coverage + 1e-9,
            "покриття зросло з висотою: {:.4} м -> {:.4} м дало {:.4} -> {:.4}",
            a.altitude,
            b.altitude,
            a.coverage,
            b.coverage
        );
    }
}
