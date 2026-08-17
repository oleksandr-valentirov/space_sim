//! The camera can turn around a body other than Earth (debt D12).
//!
//! The half of D12 that stayed open after the tilesets were wired up: the
//! Moon's terrain loads into the frame, and `engine::orbit` could only ever
//! look at the origin, which in a geocentric scene is Earth. So the Moon was a
//! few pixels and its surface could be seen only through a capture.
//!
//! What is checked here is the one thing that can go silently wrong. The
//! camera's centre and the drawn body's centre are computed by two different
//! call paths, and they must be the **same point**. In the inertial frame
//! almost any arithmetic agrees; in the rotating frame the centre goes through
//! the synodic basis as well, and a camera that skipped that step would aim
//! beside the Moon there and straight at it here -- which looks like a broken
//! scene rather than a broken camera, and would be blamed on the frame.
//!
//! No GPU: this is about where the camera points, and that is arithmetic.

use engine::orbit::Orbit;
use game::frame_view::ViewFrame;
use game::world::{EARTH, MOON};
use game::{mission, view};

/// The scene's own body centres, by id.
fn scene_centre(snapshot: &game::snapshot::WorldSnapshot, frame: ViewFrame, body: i32) -> [f64; 3] {
    let orbit = Orbit::at_altitude(mission::CAMERA_ALTITUDE_M);
    let scene = view::build_in(snapshot, orbit.camera(), frame);

    // The scene carries no ids, so the body is found by its radius -- the
    // asset's two bodies differ by a factor of four, and the test would rather
    // fail loudly here than compare the wrong pair.
    let radius = snapshot
        .bodies
        .iter()
        .find(|b| b.body == body)
        .expect("the body should be in the snapshot")
        .radius_m;

    scene
        .bodies
        .iter()
        .find(|b| (b.radius_m - radius).abs() < 1.0)
        .unwrap_or_else(|| panic!("body {body} is not in the scene"))
        .centre
}

#[test]
fn the_camera_centre_is_the_body_the_scene_drew() {
    let world = mission::world(&mission::default_asset()).expect("world");
    let snapshot = world.snapshot();

    for frame in [ViewFrame::Inertial, ViewFrame::Rotating] {
        for body in [EARTH, MOON] {
            let aimed =
                view::body_centre(&snapshot, body, frame).expect("the body is in the asset");
            let drawn = scene_centre(&snapshot, frame, body);

            assert_eq!(
                aimed, drawn,
                "body {body} in {frame:?}: the camera aims at {aimed:?} \
                 while the scene drew it at {drawn:?}"
            );
        }
    }
}

/// The two bodies are a Moon's orbit apart, in either frame.
///
/// Without this the test above passes on a `body_centre` that returns zero for
/// everything: the two paths would agree on being equally wrong. So the check
/// is the **separation**, which is the one quantity both frames must agree
/// about.
///
/// ⚠ The origin is not the same point in the two frames, and this test is
/// where that gets written down. Geocentric is the inertial frame only: the
/// synodic basis shifts to the **barycentre** (`Synodic::apply`), so Earth
/// sits about 4670 km along -x there and the Moon along +x. Assuming Earth at
/// the origin in both is the obvious mistake, and it was made while writing
/// this file.
#[test]
fn the_two_bodies_are_an_orbit_apart_in_both_frames() {
    let world = mission::world(&mission::default_asset()).expect("world");
    let snapshot = world.snapshot();

    for frame in [ViewFrame::Inertial, ViewFrame::Rotating] {
        let earth = view::body_centre(&snapshot, EARTH, frame).expect("Earth");
        let moon = view::body_centre(&snapshot, MOON, frame).expect("the Moon");

        let d = [moon[0] - earth[0], moon[1] - earth[1], moon[2] - earth[2]];
        let separation = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        println!("  {frame:?}: {separation:.4e} m apart, Earth at {earth:?}");

        assert!(
            (3.6e8..4.1e8).contains(&separation),
            "the bodies ended up {separation:.3e} m apart in {frame:?} -- not the Moon's orbit"
        );
    }

    // And the origins, stated rather than assumed.
    assert_eq!(
        view::body_centre(&snapshot, EARTH, ViewFrame::Inertial).expect("Earth"),
        [0.0, 0.0, 0.0],
        "the inertial frame is geocentric"
    );

    let earth = view::body_centre(&snapshot, EARTH, ViewFrame::Rotating).expect("Earth");
    let moon = view::body_centre(&snapshot, MOON, ViewFrame::Rotating).expect("the Moon");
    assert!(
        earth[0] < -4.0e6 && moon[0] > 3.0e8,
        "the rotating frame is barycentric, with the pair on the x axis: \
         Earth {earth:?}, Moon {moon:?}"
    );
    assert!(
        earth[1].abs() < 1.0 && earth[2].abs() < 1.0,
        "Earth should be on the rotating frame's x axis, and it is at {earth:?}"
    );
}

/// Aiming at the Moon puts the Moon dead ahead and Earth off the axis.
///
/// The claim the player actually cares about, and the one the two above cannot
/// make between them: they compare centres, this one goes through the camera.
#[test]
fn aiming_at_the_moon_puts_the_moon_in_front() {
    let world = mission::world(&mission::default_asset()).expect("world");
    let snapshot = world.snapshot();

    let moon = view::body_centre(&snapshot, MOON, ViewFrame::Inertial).expect("the Moon");
    let orbit = Orbit::around(1.7374e6, 5.0e7);
    let camera = orbit.camera_about(moon);

    let moon_local = camera.relative(moon);
    assert!(
        moon_local[0].abs() < 1.0 && moon_local[1].abs() < 1.0,
        "the Moon should be on the view axis, and it is at {moon_local:?}"
    );

    // And Earth, which the camera used to be nailed to, is now off it.
    let earth_local = camera.relative([0.0, 0.0, 0.0]);
    let aside = (f64::from(earth_local[0]).powi(2) + f64::from(earth_local[1]).powi(2)).sqrt();
    println!("  Earth sits {aside:.3e} m off the view axis");
    assert!(
        aside > 1.0e7,
        "Earth should have left the axis, and it is {aside:.3e} m off it"
    );
}
