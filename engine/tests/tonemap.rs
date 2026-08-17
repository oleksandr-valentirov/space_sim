//! Прохід стиснення в кадрі (етап T, крок T5c3).
//!
//! Оракул — **точний байт**, а не вигляд відблиску, і фікстура зроблена так,
//! щоб інше в нього не входило.
//!
//! Джерело значень понад одиницю тут не матеріал корпусу, а `Body::colour`,
//! і це рішення. Дзеркальний пік GGX для такої перевірки не годиться:
//! при дотичних кутах він законно доходить до сотень, а крива прямує до
//! одиниці асимптотично, тож у останній байт усе одно потрапляє все, що
//! більше за ~57 — тобто «зрізання» там є завжди й нічого не доводить.
//! Гладке тіло, освітлене точно з боку камери, натомість дає в центрі диска
//! **рівно** свій колір: `0.05 + 0.95·1 = 1`.
//!
//! Отже байт у центрі кадру — це `srgb(compress(colour))`, і в цьому рівнянні
//! немає жодного вільного числа.

use engine::camera::Camera;
use engine::gpu::Gpu;
use engine::scene::{Body, Scene, TileSet};
use engine::{shot, sphere, srgb, tonemap};

const SIZE: u32 = 128;

/// Гладке тіло, освітлене точно з боку камери.
fn scene(colour: [f32; 4]) -> Scene {
    let distance = sphere::EARTH_RADIUS_M * 3.0;
    let eye = [distance, 0.0, 0.0];
    let mut scene = Scene::new(Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]));
    scene.sun = [1.0, 0.0, 0.0];
    scene.bodies.push(Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: sphere::EARTH_RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: TileSet::Smooth,
        colour,
        air: None,
    });
    scene
}

/// Яскравість понад одиницю доходить у кадр тим байтом, який дає крива.
///
/// Чотири значення: одне нижче коліна (там крива зобов'язана бути тотожністю),
/// одне трохи вище, і два далеко вище — рівно того порядку, який дає відблиск
/// корпусу. Без проходу стиснення три останні дали б однаковий `255`.
#[test]
fn a_body_brighter_than_one_lands_on_the_byte_the_curve_predicts() {
    let Some(gpu) = Gpu::for_tests() else { return };

    let mut seen = Vec::new();
    for value in [0.5f32, 0.95, 1.5, 2.0, 3.7] {
        let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene([value; 4])).expect("кадр");
        let got = shot.pixel(SIZE / 2, SIZE / 2)[0];
        let expected = srgb::linear_to_byte(tonemap::compress(f64::from(value)));
        println!(
            "  {value} → крива {:.4} → чекали байт {expected}, у кадрі {got}",
            tonemap::compress(f64::from(value))
        );
        assert!(
            got.abs_diff(expected) <= 1,
            "яскравість {value} мала дати байт {expected}, а кадр дав {got}"
        );
        seen.push(got);
    }

    // І три яскравості понад одиницю мусять лишитися **різними**: саме це
    // прохід і додає. Без нього тут було б 255, 255, 255.
    assert!(
        seen[1] < seen[2] && seen[2] < seen[3] && seen[3] < seen[4],
        "яскравості понад коліном злиплися: {seen:?}"
    );
    assert!(
        seen[4] < 255,
        "найяскравіша все одно зрізалася в {}",
        seen[4]
    );
}

/// Нижче коліна прохід не змінює **нічого**, і це перевіряється кадром.
///
/// На цьому стоять усі оракули етапу T, які міряють байти: відбивна здатність
/// Місяця (T5b), правило матеріалу (T4b), кольорові тайли (T3b). Якби крива
/// чіпала темні значення, кожен з них довелося б переміряти.
#[test]
fn below_the_knee_the_pass_is_invisible() {
    let Some(gpu) = Gpu::for_tests() else { return };

    for value in [0.02f32, 0.1, 0.35, 0.79] {
        let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene([value; 4])).expect("кадр");
        let got = shot.pixel(SIZE / 2, SIZE / 2)[0];
        let expected = srgb::linear_to_byte(f64::from(value));
        println!("  {value} → байт {expected}, у кадрі {got}");
        assert!(
            got.abs_diff(expected) <= 1,
            "яскравість {value} нижче коліна дала {got} замість {expected}"
        );
    }
}

/// The exposure reaches the frame, and at its default it changes nothing.
///
/// The GPU half of step Z1. The unit tests next to the curve prove the
/// arithmetic; this proves the number actually arrives at the shader through
/// the uniform, which is the part that can fail silently -- a binding that
/// never gets written reads as zero, and a frame that went black would look
/// exactly like a frame that went dark on purpose.
///
/// Both halves in one test on purpose: the default and a raised exposure share
/// one fixture, and the pair is the claim. One alone proves nothing -- an
/// exposure the shader ignores entirely would pass the first half.
#[test]
fn the_exposure_reaches_the_shader_and_one_changes_nothing() {
    let Some(gpu) = Gpu::for_tests() else { return };

    // 0.35 is below the knee, so at exposure one the byte is the colour
    // itself, and at exposure two it is `compress(0.7)` -- still below the
    // knee, so still the identity. That keeps the expectation free of the
    // curve for both halves.
    let colour = 0.35f32;

    let mut scene_one = scene([colour; 4]);
    scene_one.exposure = 1.0;
    let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene_one).expect("кадр");
    let at_one = shot.pixel(SIZE / 2, SIZE / 2)[0];
    let expected = srgb::linear_to_byte(f64::from(colour));
    println!("  exposure 1.0 -> byte {at_one}, expected {expected}");
    assert!(
        at_one.abs_diff(expected) <= 1,
        "the default exposure moved the frame: {at_one} instead of {expected}"
    );

    let mut scene_two = scene([colour; 4]);
    scene_two.exposure = 2.0;
    let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene_two).expect("кадр");
    let at_two = shot.pixel(SIZE / 2, SIZE / 2)[0];
    let doubled = srgb::linear_to_byte(tonemap::expose(f64::from(colour), 2.0));
    println!("  exposure 2.0 -> byte {at_two}, expected {doubled}");
    assert!(
        at_two.abs_diff(doubled) <= 1,
        "exposure 2.0 gave {at_two} instead of {doubled}"
    );
    assert!(
        at_two > at_one,
        "twice the exposure did not brighten the frame: {at_two} after {at_one}"
    );
}

/// Крива записана двічі — у Rust і в шейдері — і мусить збігатися.
#[test]
fn the_shader_carries_the_same_knee() {
    let source = include_str!("../shaders/tonemap.slang");
    let wanted = format!("static const float KNEE = {};", tonemap::KNEE);
    assert!(
        source.contains(&wanted),
        "у shaders/tonemap.slang немає рядка «{wanted}»"
    );
}
