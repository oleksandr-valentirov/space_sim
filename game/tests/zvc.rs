//! Криві нульової швидкості кажуть про систему те, що мають (ROADMAP-UI.md,
//! U6b3).
//!
//! Оракул тут — топологія, і **в обидва боки**: крива, намальована завжди
//! однаково, виглядає правильною. Тому перевіряється і те, що при високій `C`
//! ворота зачинені (два замкнені лоби, кожен навколо свого тіла), і те, що при
//! низькій вони відчинені (лоб Землі проходить далі за L1).
//!
//! Плюс твердження про сам апарат: він ніколи не опиняється в забороненій
//! області, побудованій за його ж `C` на старті. Це не тавтологія — `C` пливе
//! (U6b1 виміряв 0.007% за добу), і саме цей дрейф тут і обмежується числом.

use core_rs::{cr3bp_jacobi, cr3bp_lagrange, cr3bp_mu, Vec3d};
use game::mission;
use game::world::{EARTH, MOON};
use game::zvc;

/// `mu` фікстури — та сама пара, що в грі.
fn mass_ratio() -> f64 {
    cr3bp_mu(398_600_435_436_000.0, 4_902_800_066_000.0)
}

/// `2Ω` у точці: це `C` тіла, що стоїть на місці.
///
/// Окремої функції потенціалу на межі немає й не треба — нульова швидкість це
/// і є визначення кривої.
fn two_omega(r: Vec3d, mu: f64) -> f64 {
    cr3bp_jacobi(r, Vec3d::default(), mu)
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// При `C` вище за `C(L1)` ворота зачинені: два замкнені лоби, і між ними
/// порожньо.
#[test]
fn a_high_jacobi_constant_shuts_the_gate_around_each_body() {
    let mu = mass_ratio();
    let l1 = cr3bp_lagrange(mu, 1).expect("L1 є");
    let c1 = two_omega(l1, mu);

    // Одиничний масштаб: тут перевіряється геометрія, а не переведення в метри.
    let curves = zvc::curves(mu, c1 + 0.05, 1.0);
    assert_eq!(
        curves.len(),
        2,
        "при зачинених воротах лобів рівно два — по одному на тіло"
    );

    for (name, piece) in [("Земля", &curves[0]), ("Місяць", &curves[1])] {
        let first = piece.points[0];
        let last = piece.points[piece.points.len() - 1];
        assert!(
            distance(first, last) < 1e-6,
            "лоб «{name}» не замкнувся: {first:?} проти {last:?}"
        );
    }

    // І головне число: лоб Місяця не дотягується до L1, тобто ворота справді
    // зачинені, а не «майже».
    let moon_lobe_min_x = curves[1]
        .points
        .iter()
        .map(|p| p[0])
        .fold(f64::INFINITY, f64::min);
    println!(
        "  лоб Місяця починається з x = {moon_lobe_min_x:.4}, L1 у {:.4}",
        l1.x
    );
    assert!(
        moon_lobe_min_x > l1.x,
        "лоб Місяця дотягнувся до {moon_lobe_min_x:.4}, а L1 у {:.4}",
        l1.x
    );
}

/// При `C` нижче за `C(L1)` ворота відчинені: лоб Землі проходить далі за L1,
/// і крива рветься там, де області злилися.
#[test]
fn a_low_jacobi_constant_opens_the_gate_at_l1() {
    let mu = mass_ratio();
    let l1 = cr3bp_lagrange(mu, 1).expect("L1 є");
    let c1 = two_omega(l1, mu);

    let curves = zvc::curves(mu, c1 - 0.05, 1.0);
    assert!(
        curves.len() > 2,
        "при відчинених воротах крива рветься, а тут {} шматків",
        curves.len()
    );

    let reach = curves
        .iter()
        .flat_map(|p| p.points.iter())
        .map(|p| p[0])
        .fold(f64::NEG_INFINITY, f64::max);
    println!("  крива дотягується до x = {reach:.4}, L1 у {:.4}", l1.x);
    assert!(
        reach > l1.x,
        "крива не пройшла за L1: {reach:.4} проти {:.4}",
        l1.x
    );

    // І дно: при досить малій `C` забороненої області немає взагалі, і
    // порожній результат — це відповідь, а не збій.
    assert!(
        zvc::curves(mu, 2.5, 1.0).is_empty(),
        "при C = 2.5 забороненої області бути не може"
    );
}

/// Апарат ніколи не залітає у власну заборонену область.
///
/// `C` береться на старті — і саме тому це не тавтологія: за 42 доби прогнозу
/// вона пливе, і твердження тримається рівно настільки, наскільки малий той
/// дрейф. Якби крива малювалася від чужої `C` (скажімо, від `2Ω` барицентра
/// або з переплутаним знаком `v²`), апарат опинився б у стіні на першому ж
/// семплі.
#[test]
fn the_vessel_never_enters_the_region_its_own_constant_forbids() {
    let eph = core_rs::Ephemeris::load(&mission::default_asset()).expect("ассет");
    let mut world = mission::world(&mission::default_asset()).expect("світ");
    world.tick(16);
    let snapshot = world.snapshot();

    let c0 = snapshot.vessels[0].jacobi.expect("C рахується");
    let mut worst = f64::INFINITY;
    let mut samples = 0;

    for leg in &snapshot.vessels[0].legs {
        for sample in &leg.samples {
            let frame = eph
                .synodic_frame(EARTH, MOON, sample.state.t)
                .expect("фрейм є");
            let synodic = frame.from_inertial(&sample.state);
            // Запас: наскільки `2Ω` тут вище за `C` старту. Від'ємне означало б
            // апарат усередині стіни.
            worst = worst.min(two_omega(synodic.r, frame.mass_ratio()) - c0);
            samples += 1;
        }
    }

    println!("  {samples} семплів, найменший запас 2Ω − C₀ = {worst:.6}");
    assert!(
        worst > -0.01,
        "апарат зайшов у заборонену область на {:.6} — це вже не дрейф C",
        -worst
    );
}
