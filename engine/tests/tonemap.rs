//! The compression pass in the frame (stage T, step T5c3).
//!
//! The oracle is an **exact byte**, not the look of a highlight, and the
//! fixture is built so that nothing else enters it.
//!
//! The source of values above one here is `Body::colour` rather than the hull
//! material, and that is a decision. The GGX specular peak is no good for such
//! a check: at grazing angles it legitimately reaches the hundreds, while the
//! curve approaches one asymptotically, so everything above ~57 lands in the
//! last byte anyway -- i.e. "clipping" is always there and proves nothing. A
//! smooth body lit exactly from the camera's side, by contrast, gives
//! **exactly** its own colour at the centre of the disc:
//! `0.05 + 0.95*1 = 1`.
//!
//! So the byte at the centre of the frame is `srgb(compress(colour))`, and
//! there is not one free number in that equation.

use engine::camera::Camera;
use engine::gpu::Gpu;
use engine::scene::{Body, Scene, TileSet};
use engine::{shot, sphere, srgb, tonemap};

const SIZE: u32 = 128;

/// A smooth body lit exactly from the camera's side.
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

/// A brightness above one reaches the frame as the byte the curve gives.
///
/// Four values: one below the knee (where the curve is obliged to be the
/// identity), one slightly above, and two far above -- of exactly the order a
/// hull highlight gives. Without the compression pass the last three would
/// give an identical `255`.
#[test]
fn a_body_brighter_than_one_lands_on_the_byte_the_curve_predicts() {
    let Some(gpu) = Gpu::for_tests() else { return };

    let mut seen = Vec::new();
    for value in [0.5f32, 0.95, 1.5, 2.0, 3.7] {
        let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene([value; 4])).expect("frame");
        let got = shot.pixel(SIZE / 2, SIZE / 2)[0];
        let expected = srgb::linear_to_byte(tonemap::compress(f64::from(value)));
        println!(
            "  {value} -> curve {:.4} -> expected byte {expected}, in frame \
             {got}",
            tonemap::compress(f64::from(value))
        );
        assert!(
            got.abs_diff(expected) <= 1,
            "brightness {value} should have given byte {expected}, and the \
             frame gave {got}"
        );
        seen.push(got);
    }

    // And the three brightnesses above one have to stay **distinct**: that is
    // what the pass adds. Without it this would read 255, 255, 255.
    assert!(
        seen[1] < seen[2] && seen[2] < seen[3] && seen[3] < seen[4],
        "the brightnesses above the knee merged: {seen:?}"
    );
    assert!(
        seen[4] < 255,
        "the brightest one clipped anyway at {}",
        seen[4]
    );
}

/// Below the knee the pass changes **nothing**, and that is checked by the
/// frame.
///
/// Every stage-T oracle that measures bytes rests on this: the Moon's
/// reflectance (T5b), the material rule (T4b), the colour tiles (T3b). If the
/// curve touched the dark values, every one of them would have to be
/// remeasured.
#[test]
fn below_the_knee_the_pass_is_invisible() {
    let Some(gpu) = Gpu::for_tests() else { return };

    for value in [0.02f32, 0.1, 0.35, 0.79] {
        let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene([value; 4])).expect("frame");
        let got = shot.pixel(SIZE / 2, SIZE / 2)[0];
        let expected = srgb::linear_to_byte(f64::from(value));
        println!("  {value} -> byte {expected}, in frame {got}");
        assert!(
            got.abs_diff(expected) <= 1,
            "brightness {value} below the knee gave {got} instead of \
             {expected}"
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
    let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene_one).expect("frame");
    let at_one = shot.pixel(SIZE / 2, SIZE / 2)[0];
    let expected = srgb::linear_to_byte(f64::from(colour));
    println!("  exposure 1.0 -> byte {at_one}, expected {expected}");
    assert!(
        at_one.abs_diff(expected) <= 1,
        "the default exposure moved the frame: {at_one} instead of {expected}"
    );

    let mut scene_two = scene([colour; 4]);
    scene_two.exposure = 2.0;
    let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene_two).expect("frame");
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

/// The curve is written down twice -- in Rust and in the shader -- and has to
/// match.
#[test]
fn the_shader_carries_the_same_knee() {
    let source = include_str!("../shaders/tonemap.slang");
    let wanted = format!("static const float KNEE = {};", tonemap::KNEE);
    assert!(
        source.contains(&wanted),
        "shaders/tonemap.slang has no line \"{wanted}\""
    );
}
