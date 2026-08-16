//! What the thinned-trail cache rests on (ROADMAP.md, N2b).
//!
//! The cache lives off the assumption that **a sample's position in the frame
//! does not depend on the frame's time**. For the inertial frame that is
//! obvious; for the rotating one it is not: the basis there is built from the
//! Earth-Moon line, and that rotates. But it is built from **the sample's
//! own** line, taking from the frame only the constant scale and `mu`.
//!
//! If that assumption is false, the cache hands back yesterday's picture --
//! and no test of vertex counts will see it. So it is checked directly.

use engine::orbit::Orbit;
use game::frame_view::ViewFrame;
use game::{mission, trail, view};

fn camera() -> engine::camera::Camera {
    Orbit::at_altitude(mission::CAMERA_ALTITUDE_M).camera()
}

/// The vessel's history, by colour rather than "the longest polyline".
///
/// On day five the longest is the prediction, on day twenty it is already the
/// history, so choosing by length would compare two different lines.
fn history(scene: &engine::scene::Scene) -> Vec<[f64; 3]> {
    scene
        .polylines
        .iter()
        .find(|line| line.colour == game::palette::HISTORY.scene())
        .map(|line| line.points.clone())
        .unwrap_or_default()
}

/// History drawn later **bitwise** continues history drawn earlier.
///
/// Not "nearly the same": the points stay in `f64` throughout, and any
/// dependence on the frame's time would shift all of them. The run happens in
/// the rotating frame, because that is where the assumption is not obvious --
/// over fifteen days the Earth-Moon line turns by 200 degrees.
#[test]
fn the_rotating_frame_does_not_move_a_sample_that_already_happened() {
    let mut world = mission::world(&mission::default_asset()).expect("the world builds");
    let start = mission::start().t;
    // No leg retirement: it changes **the samples themselves** of old history
    // (N3a), so with it the test would ask about two different things at once.
    // That it fails under retirement is, incidentally, the proof that
    // retirement really is a one-way door.
    world.set_history_trimming(None);

    world.run_to_day(start + 5.0 * 86400.0, 1.0, 8);
    let early = view::build_in(&world.snapshot(), camera(), ViewFrame::Rotating);

    world.run_to_day(start + 20.0 * 86400.0, 1.0, 8);
    let late = view::build_in(&world.snapshot(), camera(), ViewFrame::Rotating);

    let early_trail = history(&early);
    let late_trail = history(&late);

    assert!(
        early_trail.len() >= 2 && late_trail.len() > early_trail.len(),
        "the trail did not grow: {} -> {}",
        early_trail.len(),
        late_trail.len()
    );
    for (index, point) in early_trail.iter().enumerate() {
        assert_eq!(
            *point, late_trail[index],
            "point {index} moved: a sample's position depends on the frame's \
             time, i.e. the N2b cache cannot be kept"
        );
    }
}

/// The cache holds exactly the legs the frame asked for and discards the rest.
///
/// Without discarding, the cascade after a plan edit would leave in the cache
/// legs the world no longer has (J3), and memory would grow exactly the way
/// debt D7 grows.
#[test]
fn the_cache_holds_what_the_frame_asked_for_and_nothing_else() {
    let mut world = mission::world(&mission::default_asset()).expect("the world builds");
    world.run_to_day(mission::start().t + 10.0 * 86400.0, 1.0, 8);
    let snapshot = world.snapshot();

    let legs: usize = snapshot.vessels.iter().map(|v| v.legs.len()).sum();
    assert!(legs > 1, "the check needs more than one leg");

    let mut cache = trail::Cache::new();
    let mut thinning = view::Thinning {
        cache: &mut cache,
        height_px: 720,
    };

    view::build_thinned(&snapshot, camera(), &[], ViewFrame::Inertial, &mut thinning);
    assert_eq!(thinning.cache.len(), legs);

    // A second frame with the same snapshot adds nothing: if the key were
    // unstable the count would double.
    view::build_thinned(&snapshot, camera(), &[], ViewFrame::Inertial, &mut thinning);
    assert_eq!(thinning.cache.len(), legs);

    // A frame with no vessels leaves the cache empty. An empty world rather
    // than a copy of the snapshot with a trimmed list: `WorldSnapshot`
    // deliberately does not clone, and working around that in a test would
    // mean checking the workaround.
    let empty = game::world::World::new(
        &mission::default_asset(),
        mission::config(),
        mission::start().t,
        mission::DEFAULT_WARP,
    )
    .expect("an empty world builds")
    .snapshot();
    view::build_thinned(&empty, camera(), &[], ViewFrame::Inertial, &mut thinning);
    assert!(
        thinning.cache.is_empty(),
        "{} legs the frame did not ask for remain in the cache",
        thinning.cache.len()
    );
}

/// The same snapshot twice gives the same scene -- a warm cache changes
/// nothing.
///
/// A cache oracle impossible to pass by accident: the second frame comes
/// entirely from the cache, and if it handed back anything else the difference
/// would be bitwise.
#[test]
fn a_warm_cache_draws_exactly_what_a_cold_one_did() {
    let mut world = mission::world(&mission::default_asset()).expect("the world builds");
    world.run_to_day(mission::start().t + 10.0 * 86400.0, 1.0, 8);
    let snapshot = world.snapshot();

    let mut cache = trail::Cache::new();
    let mut thinning = view::Thinning {
        cache: &mut cache,
        height_px: 720,
    };

    let cold = view::build_thinned(&snapshot, camera(), &[], ViewFrame::Rotating, &mut thinning);
    let warm = view::build_thinned(&snapshot, camera(), &[], ViewFrame::Rotating, &mut thinning);

    assert_eq!(cold.polylines.len(), warm.polylines.len());
    for (a, b) in cold.polylines.iter().zip(&warm.polylines) {
        assert_eq!(a.points, b.points, "the warm cache drew something else");
    }
}
