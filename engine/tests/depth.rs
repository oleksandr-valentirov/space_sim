//! Reversed-Z really does deliver what it was taken for (ROADMAP F3).
//!
//! Only **positive** statements live here -- "the nearer surface won the
//! frame". In those the depth difference really exists, the driver has nothing
//! to round off, and the answer is the same on llvmpipe, on hardware Vulkan and
//! on Metal.
//!
//! The converse statements -- "and here depth resolves nothing" -- are not
//! provable on a GPU: when both surfaces write the same bit, the winner is
//! decided by how a particular rasteriser treats a tie, not by depth. Those are
//! checked by arithmetic in `engine::depth` (that module's unit tests), where
//! exactly what is asserted is visible. The history of this fix is ROADMAP F3.

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
    .expect("the measurement should have gone through");

    Some(measured.near_wins)
}

/// WARNING: the finest place in this file -- the `z_ndc` of the two surfaces
/// differ here by exactly **1 ULP** (checked by arithmetic, ROADMAP F3). The
/// difference exists, so the statement is legitimate -- but there is no margin,
/// and if one day this test goes red on new hardware, look here first, not at
/// the engine. The cell stays exactly like this deliberately: 1 m at 1e7 m is
/// the very edge F3 measured.
#[test]
fn reversed_z_resolves_a_metre_at_ten_million() {
    let Some(share) = near_wins(true, 1e7, 1.0) else {
        return;
    };
    assert_eq!(
        share, 1.0,
        "the nearer surface should have won the whole frame, it won {share}"
    );
}

/// And a gap above the limit is resolvable there too.
#[test]
fn a_gap_above_the_limit_resolves_at_a_hundred_million() {
    let limit = depth::resolvable_gap(1e8);
    let Some(share) = near_wins(true, 1e8, limit * 10.0) else {
        return;
    };
    assert_eq!(
        share,
        1.0,
        "a gap of {} m, tenfold above the limit, should have resolved",
        limit * 10.0
    );
}

// ---------------------------------------------------------------------------
// Four depth ranges (R4b)

/// Composing by passes is no worse than a single pass -- on the same pair of
/// surfaces.
///
/// A positive statement, like the rest of the file. What it does **not** prove
/// is said alongside by a number: ranges do not make depth any sharper at all
/// (`engine::depth::tests::a_finite_range_is_no_sharper_than_an_infinite_one`
/// -- 4.0 m at 1e8 m, the same in all three variants). So the pair taken here
/// is a **resolvable** one (a gap of 1e4 m at 1e8 m against a limit of 4 m),
/// and exactly what the passes are responsible for is checked: back-to-front,
/// clearing depth between passes, and clip planes that divide the scene without
/// a seam.
///
/// Both passes contain both surfaces -- they are separated by the planes, not
/// by a hand in the test, exactly as in `frame::Frame::plan`.
#[test]
fn splitting_the_scene_into_ranges_keeps_the_nearer_surface_in_front() {
    let Some(gpu) = Gpu::for_tests() else { return };

    const DISTANCE: f64 = 1.0e8;
    const GAP: f64 = 1.0e4;
    const FOV_Y: f64 = std::f64::consts::PI / 3.0;

    let quad = |distance: f64, colour: [f32; 4], projection| engine::depth_probe::Params {
        projection,
        colour,
        // Twice the half-screen at this distance -- covers the whole frame.
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
            .expect("the frame should have been drawn");

    // The same frame in a single pass -- so the number has something to be
    // compared against.
    let one = depth::reversed_infinite(FOV_Y, 1.0, NEAR);
    let together = [
        quad(DISTANCE, far_colour, one),
        quad(DISTANCE - GAP, near_colour, one),
    ];
    let single = engine::depth_probe::render_ranges(&gpu, SIZE, SIZE, true, &[&together])
        .expect("the frame should have been drawn");

    println!(
        "  {DISTANCE:.0e} m, gap {GAP:.0e} m: one pass {:.3}, two \
         ranges {:.3}",
        single.near_wins, split.near_wins
    );
    assert!(
        single.near_wins > 0.99,
        "a single pass failed on a resolvable pair: {:.3}",
        single.near_wins
    );
    assert!(
        split.near_wins > 0.99,
        "splitting into ranges lost the nearer surface: {:.3}",
        split.near_wins
    );
}
