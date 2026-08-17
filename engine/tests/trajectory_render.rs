//! Both trajectory frames really do draw something on screen (ROADMAP F6).
//!
//! Not "by eye": the first version of this renderer silently produced an empty
//! frame for the rotating frame specifically -- the pipeline built, `draw` ran
//! without a single error or warning from wgpu, and the only symptom was a
//! black PNG. The cause was never fully established (`trajectory.slang`, the
//! comment above the two entry points), and this test is the surety against a
//! regression of that same class: since it catches an empty frame, the shader
//! can be changed without eyeballing the shots by hand every time.

use engine::gpu::Gpu;
use engine::trajectory;
use engine::trajectory_render::{geocentric_framing, render, rotating_framing, Params};

const SIZE: u32 = 256;

fn lit_pixels(shot: &engine::shot::Shot) -> u64 {
    let mut count = 0;
    for y in 0..shot.height {
        for x in 0..shot.width {
            let p = shot.pixel(x, y);
            if p[0] > 5 || p[1] > 5 || p[2] > 5 {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn both_frames_draw_visible_pixels() {
    let Some(gpu) = Gpu::for_tests() else {
        return;
    };

    let samples = trajectory::load();

    let geocentric = render(
        &gpu,
        SIZE,
        SIZE,
        &samples,
        &Params {
            rotating: false,
            framing: geocentric_framing(&samples),
            colour: [0.9, 0.6, 0.2, 1.0],
        },
    )
    .expect("the geocentric render should have run");

    let rotating = render(
        &gpu,
        SIZE,
        SIZE,
        &samples,
        &Params {
            rotating: true,
            framing: rotating_framing(&samples),
            colour: [0.3, 0.8, 0.9, 1.0],
        },
    )
    .expect("the rotating render should have run");

    assert!(
        lit_pixels(&geocentric) > 100,
        "the geocentric frame is nearly empty: {} pixels",
        lit_pixels(&geocentric)
    );
    assert!(
        lit_pixels(&rotating) > 100,
        "the rotating frame is nearly empty: {} pixels -- that is exactly how \
         the two-entry-point bug looked before it was fixed",
        lit_pixels(&rotating)
    );
}
