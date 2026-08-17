//! The oracle for thinning: pixels, both ways (ROADMAP.md, N2a).
//!
//! Thinning throws data away, so the check must catch two different failures,
//! and neither shows in the vertex count alone:
//!
//! - **thinning did nothing** -- the same number of vertices, i.e. the
//!   criterion never fired, and a "same picture" test would be green;
//! - **the line changed shape** -- fewer vertices, which is exactly why a
//!   count test would be green too.
//!
//! So both claims are checked side by side on one scene: **fewer vertices and
//! the same picture**.

use engine::frame;
use engine::gpu::Gpu;
use engine::orbit::Orbit;
use engine::shot::{self, Shot};
use game::frame_view::ViewFrame;
use game::{mission, view};

const SIZE: u32 = 512;

/// How many stations. Three, not thirty: the criterion works on a polyline,
/// and the fleet is here only to put both scales in the frame -- a low orbit
/// and the halo.
const STATIONS: usize = 3;

/// How many days to fly. Enough for the station to wind hundreds of
/// revolutions on top of each other: that is the trail thinning has work on.
const DAYS: f64 = 8.0;

fn gpu() -> Option<Gpu> {
    Gpu::for_tests()
}

fn thinned(
    snapshot: &game::snapshot::WorldSnapshot,
    camera: engine::camera::Camera,
    height_px: u32,
) -> engine::scene::Scene {
    let mut cache = game::trail::Cache::new();
    let mut thinning = view::Thinning {
        cache: &mut cache,
        height_px,
    };
    view::build_thinned(snapshot, camera, &[], ViewFrame::Inertial, &mut thinning)
}

/// A fleet with leg retirement off (N3a).
///
/// Retirement thins old legs with the same chords, so with it on the frame
/// criterion gets already-thinned input and saves half as much (measured:
/// 6039 -> 3227 instead of x3 on raw). What is checked here is the
/// **criterion**, not the sum of two thinnings, so retirement is off; their
/// combination is the N3a number in ROADMAP.
fn flown() -> game::snapshot::WorldSnapshot {
    let mut world = mission::fleet(&mission::default_asset(), STATIONS).expect("the fleet builds");
    world.set_history_trimming(None);
    world.run_to_day(mission::start().t + DAYS * 86400.0, 1.0, 8);
    world.snapshot()
}

fn vertices(scene: &engine::scene::Scene) -> usize {
    scene.polylines.iter().map(|line| line.points.len()).sum()
}

/// Lit pixels of `a` with no lit pixel next to them in `b`.
///
/// **Not "differing pixels", and this is a correction of the oracle rather
/// than a weakening of it.** The criterion allows the line to shift by half a
/// pixel, and a one-pixel-wide line shifted by half a pixel changes most of
/// its pixels -- at the Earth-Moon scale, 358 changed out of 1226 lit with a
/// shape that did not change. A per-pixel comparison would be measuring
/// rasterisation, not thinning.
///
/// The claim worth checking is that **the line did not move**. A discarded
/// revolution would leave lit pixels in the full frame with nothing beside
/// them in the thinned one -- and that is what is counted below.
fn unmatched(a: &Shot, b: &Shot) -> u64 {
    let mut count = 0;
    for y in 0..a.height {
        for x in 0..a.width {
            if !is_lit(a, x, y) {
                continue;
            }
            let near = (y.saturating_sub(1)..=(y + 1).min(b.height - 1)).any(|ny| {
                (x.saturating_sub(1)..=(x + 1).min(b.width - 1)).any(|nx| is_lit(b, nx, ny))
            });
            if !near {
                count += 1;
            }
        }
    }
    count
}

fn is_lit(shot: &Shot, x: u32, y: u32) -> bool {
    let p = shot.pixel(x, y);
    [p[0], p[1], p[2]] != frame::CLEAR_BYTES
}

/// How many pixels are not background.
fn lit(shot: &Shot) -> u64 {
    let mut count = 0;
    for y in 0..shot.height {
        for x in 0..shot.width {
            let p = shot.pixel(x, y);
            if [p[0], p[1], p[2]] != frame::CLEAR_BYTES {
                count += 1;
            }
        }
    }
    count
}

/// Three camera scales, as the step's check requires: near Earth, the whole
/// Earth-Moon pair, the whole mission.
fn scales() -> [(&'static str, f64); 3] {
    [
        ("near Earth", 2.0e7),
        ("the Earth-Moon pair", 5.0e8),
        ("the whole mission", mission::CAMERA_ALTITUDE_M),
    ]
}

#[test]
fn thinning_drops_vertices_and_keeps_the_picture() {
    let Some(gpu) = gpu() else { return };
    let snapshot = flown();

    for (name, altitude) in scales() {
        let camera = || Orbit::at_altitude(altitude).camera();

        let full = view::build_in(&snapshot, camera(), ViewFrame::Inertial);
        let thin = thinned(&snapshot, camera(), SIZE);

        // First claim: the vertex count never grows, and at the whole-mission
        // scale it falls several fold.
        //
        // There is deliberately no "halved" threshold at every scale, and that
        // is not a weaker test but what the criterion itself measured: from
        // 2e7 m the station's orbit spans 155 pixels, and at 18 samples per
        // revolution the sagitta between neighbours is 1.2 pixels. The nodes
        // there are **needed**, and a criterion that discarded them would be
        // broken. Thinning lives at the map scale, where a revolution is
        // smaller than a pixel.
        assert!(
            vertices(&thin) <= vertices(&full),
            "{name}: {} -> {} vertices, thinning added vertices",
            vertices(&full),
            vertices(&thin)
        );
        if altitude >= mission::CAMERA_ALTITUDE_M {
            assert!(
                vertices(&thin) * 2 <= vertices(&full),
                "{name}: {} -> {} vertices, that is no thinning",
                vertices(&full),
                vertices(&thin)
            );
        }

        let full_shot = shot::take_scene(&gpu, SIZE, SIZE, &full).expect("frame");
        let thin_shot = shot::take_scene(&gpu, SIZE, SIZE, &thin).expect("frame");

        // Second: the line did not move -- both ways. One way catches lost
        // detail, the other invented detail; the tolerance is a fraction of
        // the lit pixels, because the trail covers different areas at
        // different scales.
        let lit_full = lit(&full_shot).max(1);
        let lit_thin = lit(&thin_shot).max(1);
        let lost = unmatched(&full_shot, &thin_shot);
        let gained = unmatched(&thin_shot, &full_shot);
        assert!(
            lost * 100 <= lit_full * 2,
            "{name}: {lost} lit pixels out of {lit_full} were left unmatched -- \
             thinning ate detail"
        );
        assert!(
            gained * 100 <= lit_thin * 2,
            "{name}: {gained} pixels out of {lit_thin} appeared where there were none"
        );

        // And third, without which the second could be passed by an empty
        // frame.
        assert!(
            lit_thin * 4 >= lit_full,
            "{name}: the thinned frame has {lit_thin} lit against {lit_full} -- the trail vanished"
        );
    }
}

/// The criterion is in screen space, so it **must** depend on resolution: on
/// a larger frame half a pixel is a smaller distance in metres, and more
/// vertices survive.
///
/// A test that the criterion really looks at the screen rather than at
/// metres: were the tolerance in metres, both numbers would agree.
#[test]
fn a_bigger_frame_keeps_more_vertices() {
    let snapshot = flown();
    let camera = || Orbit::at_altitude(mission::CAMERA_ALTITUDE_M).camera();

    let small = thinned(&snapshot, camera(), 360);
    let large = thinned(&snapshot, camera(), 1440);

    assert!(
        vertices(&large) > vertices(&small),
        "640x360 kept {} vertices, 2560x1440 kept {}: the criterion is not in screen space",
        vertices(&small),
        vertices(&large)
    );
}
