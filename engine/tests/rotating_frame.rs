//! The number decision U6a1 stands on (ROADMAP-UI.md).
//!
//! The rotating-frame transform is computed on the CPU in `f64`, not in the
//! vertex shader, and the reason is not taste but a discrepancy in metres. That
//! is what is checked here: if one day someone returns to the `f32` path, this
//! number should stand in front of them rather than be forgotten in the
//! commits.
//!
//! The test needs neither a GPU nor a window: both paths are arithmetic over
//! the same halo-orbit fixture.

use engine::rotating_probe::{cost, error_px, precision};
use engine::trajectory;

/// The `f32` path loses hundreds of metres, and at a close view that shows.
#[test]
fn the_float_path_loses_enough_metres_to_show() {
    let samples = trajectory::load();
    let p = precision(&samples);

    println!(
        "  f32 path: worst {:.1} m (sample {}), {:.1} m on average",
        p.worst_m, p.worst_sample, p.mean_m
    );

    // The bounds are wide deliberately: this is not a constant we pick but a
    // property of `f32` at 4e8 m. The test catches not "the number moved by a
    // percent" but "the path suddenly became exact" or "became catastrophic".
    assert!(
        (50.0..500.0).contains(&p.worst_m),
        "a worst discrepancy of {:.1} m is no longer the order of magnitude \
         U6a1 was decided on",
        p.worst_m
    );
    assert!(
        p.mean_m > 10.0,
        "a mean discrepancy of {:.1} m is too small to justify the decision",
        p.mean_m
    );

    // At a view 10 km wide the error is an order of magnitude above a pixel; at
    // a view of the whole Earth-Moon system it is not visible at all. Both
    // sides, because one number without the other would read as a verdict on
    // all `f32` in the renderer.
    let close = error_px(p.worst_m, 1.0e4);
    let far = error_px(p.worst_m, 1.0e9);
    println!("  at 10 km of frame: {close:.1} px, at 1e6 km: {far:.3} px");
    assert!(close > 10.0, "at a close view the error is {close:.1} px");
    assert!(far < 0.1, "at a far view the error is {far:.3} px");
}

/// Both passes run to the end and give numbers one can compare.
///
/// **There is no clock in this test, deliberately, and that is a fix.** At
/// first `with transform > without transform` stood here -- a statement that
/// fell over on windows-mingw in CI: in debug both passes cost ~272 ns instead
/// of 2.69 and 10.56 in release, because there the time is set by the overhead
/// of a debug build rather than by the arithmetic, and the noise swapped the
/// numbers round (-0.7 ns).
///
/// The lesson is wider than this test: **a wall-clock comparison is never an
/// oracle in the gate**. The number from U6a1 lives where it means something --
/// in `--rotating-probe`, release, on one machine in one run. What stays here
/// is what does not depend on the machine: both passes execute, both return
/// finite positive numbers, and neither is optimised into nothing (`cost`
/// guards that by checking the buffer length).
#[test]
fn both_passes_run_to_the_end_and_return_numbers() {
    let samples = trajectory::load();
    let c = cost(&samples, 50);

    println!(
        "  camera-relative {:.2} ns, with transform {:.2} ns ({:+.0}%) -- \
         numbers of this machine and this profile, not an oracle",
        c.camera_ns,
        c.camera_and_frame_ns,
        c.overhead()
    );

    assert!(c.points > 1000, "the fixture gave only {} points", c.points);
    for (name, value) in [
        ("camera-relative", c.camera_ns),
        ("with transform", c.camera_and_frame_ns),
    ] {
        assert!(
            value.is_finite() && value > 0.0,
            "{name}: {value} -- the pass did not execute"
        );
    }
}
