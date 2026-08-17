//! The scene the game hands the engine really does reach the pixels
//! (ROADMAP J1).
//!
//! The tests in `trajectory.rs` prove the numbers are right; these prove they
//! get into the frame. Without the second the first is worth nothing: an
//! empty scene and a correct one both give a "green test" if nobody looks at
//! the pixels.
//!
//! The oracle here is not analytic and cannot be: the shape of a halo orbit
//! in perspective has no short formula. So the claims checked are ones that
//! break under real bugs -- that the line is there, that it disappears with
//! the trajectory, and that the camera moves it.

use engine::frame;
use engine::gpu::Gpu;
use engine::orbit::Orbit;
use engine::shot::{self, Shot};
use game::{mission, view};

const SIZE: u32 = 256;

fn gpu() -> Option<Gpu> {
    // The engine's shared helper: it also decides whether skipping is allowed
    // (`SPACE_SIM_REQUIRE_GPU`, U6c) and prints the adapter name to the log.
    Gpu::for_tests()
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

/// A computed forecast shows in the frame, and an uncomputed one does not.
///
/// The difference between the two frames is the proof: if the first drew
/// something else (the planet, say), both numbers would be equally non-zero.
#[test]
fn the_prediction_appears_in_the_frame_and_only_when_it_exists() {
    let Some(gpu) = gpu() else { return };

    let camera = || Orbit::at_altitude(mission::CAMERA_ALTITUDE_M).camera();
    let mut world = mission::world(&mission::default_asset()).expect("the world builds");

    // Nothing computed yet: only the planet is in the frame, and from a
    // billion metres it takes a few pixels.
    let empty = shot::take_scene(&gpu, SIZE, SIZE, &view::build(&world.snapshot(), camera()))
        .expect("frame");
    let empty_lit = lit(&empty);

    world.run_to_end(1.0, 8);
    let full = shot::take_scene(&gpu, SIZE, SIZE, &view::build(&world.snapshot(), camera()))
        .expect("frame");
    let full_lit = lit(&full);

    assert!(
        empty_lit < 100,
        "an empty forecast drew {empty_lit} pixels -- that is no longer just the planet"
    );
    assert!(
        full_lit > empty_lit + 500,
        "the forecast added only {} pixels ({full_lit} against {empty_lit})",
        full_lit - empty_lit
    );

    // No PNG is written here on purpose: `cargo test` runs the binary from the
    // crate directory, so the file would land in `game/build/` rather than
    // where anyone looks. Screenshots come from `cargo run -p game -- --shot`.
}

/// The camera moves the prediction the way it moves the planet.
///
/// The cheapest check that the polyline goes down the same camera-relative
/// path as the sphere's vertices: were it projected separately (with its own
/// offset, as in `trajectory_render`), rotating the camera would not touch it.
#[test]
fn the_camera_moves_the_prediction_too() {
    let Some(gpu) = gpu() else { return };

    let mut world = mission::world(&mission::default_asset()).expect("the world builds");
    world.run_to_end(1.0, 8);
    let snapshot = world.snapshot();

    let mut orbit = Orbit::at_altitude(mission::CAMERA_ALTITUDE_M);
    let before =
        shot::take_scene(&gpu, SIZE, SIZE, &view::build(&snapshot, orbit.camera())).expect("frame");

    // A quarter turn: the orbit lies in a plane, and from the side it is bound
    // to look different.
    orbit.drag(300.0, 0.0);
    let after =
        shot::take_scene(&gpu, SIZE, SIZE, &view::build(&snapshot, orbit.camera())).expect("frame");

    let differing = (0..SIZE)
        .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
        .filter(|&(x, y)| before.pixel(x, y) != after.pixel(x, y))
        .count();

    assert!(
        differing > 200,
        "rotating the camera changed only {differing} pixels -- the polyline does \
         not listen to it"
    );
}

// ---------------------------------------------------------------------------
// Bodies in the scene (ROADMAP-PLANETS.md, R1c)

/// The scene carries bodies as **data**: centre, size, rotation.
///
/// The oracle is not pixels (R1c draws nothing new yet) but three claims
/// about numbers, each catching its own bug:
///
/// 1. Earth exactly at the origin -- the frame is geocentric, and if the
///    subtraction were not from it, Earth would drift by 1.5e11 m;
/// 2. the Moon at 3.6-4.1e8 m from it -- i.e. it really is the Moon and not a
///    barycentric position somebody forgot to convert;
/// 3. Earth is rotated, and its rotation changes with time -- otherwise an
///    identity would arrive in the scene and nobody would notice until the
///    planet got terrain.
#[test]
fn the_scene_carries_the_bodies_as_data() {
    use game::world::{EARTH, MOON};

    let mut world = mission::world(&mission::default_asset()).expect("world");
    let orbit = Orbit::at_altitude(mission::CAMERA_ALTITUDE_M);

    let scene = view::build(&world.snapshot(), orbit.camera());
    assert_eq!(
        scene.bodies.len(),
        2,
        "the fixture has two bodies with a radius"
    );

    let earth = scene.bodies[0];
    let moon = scene.bodies[1];

    // 1. Earth is the origin of the frame.
    assert_eq!(earth.centre, [0.0, 0.0, 0.0]);
    assert!(
        (earth.radius_m - 6.371e6).abs() < 1.0e4,
        "Earth's radius from the asset: {}",
        earth.radius_m
    );

    // 2. The Moon is at the Moon's distance.
    let distance =
        (moon.centre[0].powi(2) + moon.centre[1].powi(2) + moon.centre[2].powi(2)).sqrt();
    println!(
        "  the Moon at {:.4e} m, radius {:.4e} m",
        distance, moon.radius_m
    );
    assert!(
        (3.6e8..4.1e8).contains(&distance),
        "the Moon ended up at {distance:.3e} m -- that is not the Moon's orbit"
    );
    assert!(
        (moon.radius_m - 1.7374e6).abs() < 1.0e4,
        "the Moon's radius from the asset: {}",
        moon.radius_m
    );

    // 3. The rotation exists, is unit length and changes with time.
    let length = |q: [f64; 4]| (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    assert!((length(earth.orientation) - 1.0).abs() < 1e-9);
    assert_ne!(
        earth.orientation,
        [1.0, 0.0, 0.0, 0.0],
        "Earth arrived unrotated -- the orientation was lost somewhere"
    );

    // Six hours later the rotation is different, and it is Earth's: over the
    // same time the Moon turns noticeably less (a day against a month).
    // Compute the forecast first, or the cursor runs into the horizon and goes
    // nowhere.
    world.tick(64);
    let want = world.snapshot().t + 6.0 * 3600.0;
    while world.snapshot().t < want {
        world.step(6.0 * 3600.0 / mission::DEFAULT_WARP, 64);
    }
    let later = view::build(&world.snapshot(), orbit.camera());
    assert_ne!(
        later.bodies[0].orientation, earth.orientation,
        "Earth did not turn in six hours"
    );

    let turned = |a: [f64; 4], b: [f64; 4]| {
        // The angle between two quaternions: 2*acos|<a, b>|.
        let d = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]).abs();
        2.0 * d.clamp(-1.0, 1.0).acos()
    };
    let earth_turn = turned(earth.orientation, later.bodies[0].orientation);
    let moon_turn = turned(moon.orientation, later.bodies[1].orientation);
    println!(
        "  in 6 h: Earth by {:.3} deg, the Moon by {:.3} deg",
        earth_turn.to_degrees(),
        moon_turn.to_degrees()
    );
    assert!(
        earth_turn > moon_turn * 10.0,
        "Earth turned by {:.3} deg and the Moon by {:.3} deg -- over six hours the \
         difference should be tens of times",
        earth_turn.to_degrees(),
        moon_turn.to_degrees()
    );

    // Body indices stayed in the game rather than travelling into the engine:
    // `Body` knows nothing about them, which is why this line is a reminder
    // rather than a check.
    assert_eq!([EARTH, MOON], [3, 4]);
}

// ---------------------------------------------------------------------------
// Bodies in pixels (ROADMAP-PLANETS.md, R1e)

/// An eye equidistant from Earth and the Moon.
///
/// A point on the perpendicular bisector of the Earth-Moon segment: the
/// distance to both is equal **by construction**. Then the apparent sizes are
/// in exactly the ratio of the radii, with no correction for range --
/// otherwise one would have to prove how much the range ate of the
/// difference, and that is fitting rather than an oracle.
fn eye_beside(earth: [f64; 3], moon: [f64; 3], distance: f64) -> [f64; 3] {
    let line = sub(moon, earth);
    let mid = [
        earth[0] + line[0] / 2.0,
        earth[1] + line[1] / 2.0,
        earth[2] + line[2] / 2.0,
    ];
    // Away from the line of the bodies -- anywhere, as long as it is
    // perpendicular.
    let away = unit(cross(line, [0.0, 0.0, 1.0]));
    [
        mid[0] + away[0] * distance,
        mid[1] + away[1] * distance,
        mid[2] + away[2] * distance,
    ]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn length(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn unit(v: [f64; 3]) -> [f64; 3] {
    let n = length(v);
    [v[0] / n, v[1] / n, v[2] / n]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The radius of a body's disc in pixels when it is **in the centre** of the
/// frame.
///
/// `asin(R/d)` is the exact silhouette half-angle of a convex sphere (F5),
/// then a tangent through half the field of view: the same arithmetic as in
/// the projection matrix.
///
/// Only in the centre: off axis a sphere projects to an ellipse (the further
/// out, the more noticeably so), and the circular formula is simply not about
/// that figure. That is why sizes are measured with three frames rather than
/// one.
fn disc_radius_px(radius_m: f64, distance_m: f64, height: u32) -> f64 {
    let half_angle = (radius_m / distance_m).asin();
    half_angle.tan() / (frame::FOV_Y / 2.0).tan() * f64::from(height) / 2.0
}

fn is_lit(shot: &Shot, x: u32, y: u32) -> bool {
    let p = shot.pixel(x, y);
    [p[0], p[1], p[2]] != frame::CLEAR_BYTES
}

/// How many lit pixels lie in a square around a point, and where their centre
/// of mass is.
fn blob(shot: &Shot, centre: [f32; 2], half: f64) -> (u64, [f64; 2]) {
    let mut count = 0u64;
    let mut sum = [0.0f64; 2];
    for y in 0..shot.height {
        for x in 0..shot.width {
            if !is_lit(shot, x, y) {
                continue;
            }
            if (f64::from(x) - f64::from(centre[0])).abs() > half
                || (f64::from(y) - f64::from(centre[1])).abs() > half
            {
                continue;
            }
            count += 1;
            sum[0] += f64::from(x) + 0.5;
            sum[1] += f64::from(y) + 0.5;
        }
    }
    let middle = if count == 0 {
        [0.0, 0.0]
    } else {
        [sum[0] / count as f64, sum[1] / count as f64]
    };
    (count, middle)
}

/// Both bodies from the snapshot are in their places and at their size.
///
/// This is what R1c left open: the scene already carried two bodies while the
/// frame kept drawing one sphere of Earth's radius at the origin.
///
/// Three frames, and that is not extravagance. The eye is the same in all --
/// on the perpendicular bisector, from where the bodies are equidistant. The
/// first frame looks between them and answers "where are they": the centre of
/// mass of the disc against the projection of the body's centre, plus empty
/// sky around. The second and third look at each body alone and answer "what
/// size are they": exactly in the centre of the frame the silhouette is a
/// circle with an exact formula for its radius. Off centre the same
/// silhouette is an ellipse (measured: 35x26 pixels at 41 degrees off axis),
/// and a circular oracle would be measuring the wrong thing.
#[test]
fn both_bodies_land_where_they_are_and_at_the_size_they_are() {
    let Some(gpu) = gpu() else { return };

    const WIDTH: u32 = 2048;
    const HEIGHT: u32 = 1024;
    /// Far enough for both bodies to fit in the frame with neither touching an
    /// edge.
    const DISTANCE_M: f64 = 2.2e8;

    let world = mission::world(&mission::default_asset()).expect("world");
    let snapshot = world.snapshot();

    // Positions come from the same scene the frame will see: otherwise two
    // different instants would be compared.
    let probe = view::build(&snapshot, Orbit::at_altitude(1.0e6).camera());
    let (earth, moon) = (probe.bodies[0], probe.bodies[1]);

    let eye = eye_beside(earth.centre, moon.centre, DISTANCE_M);
    let d_earth = length(sub(earth.centre, eye));
    let d_moon = length(sub(moon.centre, eye));
    assert!(
        (d_earth - d_moon).abs() / d_earth < 1e-12,
        "the eye is not equidistant: {d_earth:.6e} against {d_moon:.6e}"
    );

    // Looking between the bodies, with "up" perpendicular to their line, so
    // that the line lies horizontally and each body gets half the frame.
    let line = sub(moon.centre, earth.centre);
    let mid = [
        earth.centre[0] + line[0] / 2.0,
        earth.centre[1] + line[1] / 2.0,
        earth.centre[2] + line[2] / 2.0,
    ];
    let up = unit(cross(sub(mid, eye), line));
    let together = view::build(&snapshot, engine::camera::Camera::look_at(eye, mid, up));
    let taken = shot::take_scene(&gpu, WIDTH, HEIGHT, &together).expect("frame");

    // Where they are. The disc radius is needed only as a window size here,
    // not as an oracle: off centre the silhouette is elliptical, which is why
    // the window is taken twice the circle.
    let mut windows = Vec::new();
    for (name, body) in [("Earth", earth), ("the Moon", moon)] {
        let distance = length(sub(body.centre, eye));
        let centre = together
            .camera
            .to_screen(frame::FOV_Y, WIDTH, HEIGHT, body.centre)
            .expect("the body is in front of the camera");
        let half = 3.0 * disc_radius_px(body.radius_m, distance, HEIGHT) + 8.0;

        let (count, middle) = blob(&taken, centre, half);
        println!(
            "  {name}: centre of mass ({:.1}, {:.1}) against the projection \
             ({:.1}, {:.1}), {count} pixels",
            middle[0], middle[1], centre[0], centre[1]
        );
        assert!(count > 0, "{name}: not a single body pixel in the frame");
        assert!(
            (middle[0] - f64::from(centre[0])).hypot(middle[1] - f64::from(centre[1])) < 1.5,
            "{name} is drawn somewhere other than its projection"
        );
        windows.push((centre, half));
    }

    // Outside the two windows there is empty sky: in a scene with no forecast
    // there is nothing else to draw, and neither silhouette spilled out of its
    // window.
    let mut outside = 0u64;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            if !is_lit(&taken, x, y) {
                continue;
            }
            let inside = windows.iter().any(|(c, half)| {
                (f64::from(x) - f64::from(c[0])).abs() <= *half
                    && (f64::from(y) - f64::from(c[1])).abs() <= *half
            });
            if !inside {
                outside += 1;
            }
        }
    }
    assert_eq!(outside, 0, "{outside} pixels are lit outside the bodies");

    // What size they are. The same eye, looking straight at the body -- and
    // the silhouette becomes a circle, for which the formula is exact.
    let mut drawn = Vec::new();
    for (name, body) in [("Earth", earth), ("the Moon", moon)] {
        let distance = length(sub(body.centre, eye));
        let scene = view::build(
            &snapshot,
            engine::camera::Camera::look_at(eye, body.centre, up),
        );
        let shot = shot::take_scene(&gpu, WIDTH, HEIGHT, &scene).expect("frame");

        let expected = disc_radius_px(body.radius_m, distance, HEIGHT);
        let centre = [WIDTH as f32 / 2.0, HEIGHT as f32 / 2.0];
        let (count, _) = blob(&shot, centre, 2.0 * expected + 8.0);

        // The radius from the area rather than the extent: the area gathers
        // the whole disc, so edge discretisation enters it under a square root
        // rather than at full height.
        let measured = (count as f64 / std::f64::consts::PI).sqrt();
        println!(
            "  {name} in the centre of the frame: radius {measured:.2} against {expected:.2} px"
        );
        assert!(
            (measured - expected).abs() < 1.0,
            "{name}: a disc of radius {measured:.2} px instead of {expected:.2} px"
        );
        drawn.push((measured, body.radius_m));
    }

    // And the main number of the step: the sizes are in the ratio of the
    // radii. The distance dropped out of it -- it is the same, which is what
    // the eye's position is for.
    let ratio = drawn[0].0 / drawn[1].0;
    let real = drawn[0].1 / drawn[1].1;
    println!("  sizes: {ratio:.3} against {real:.3} by radii");
    assert!(
        (ratio - real).abs() / real < 0.05,
        "the discs are in the ratio {ratio:.3} while the radii are {real:.3}"
    );
}

/// A rotated body looks the same -- and that is not an empty claim.
///
/// A smooth sphere **cannot** show its rotation: both the silhouette and the
/// normal at every point map onto themselves. So what is checked is the one
/// thing that can be checked here, and it is worth checking: that the
/// rotation is applied **equally** to the geometry and to the normals. Apply
/// it to one of them and the frame changes at once: the patches part company
/// or the lighting slides.
///
/// Measured rather than declared. A quarter turn about x changes 2382 pixels
/// by one unit of brightness (the other diagonal of the mesh triangles) and
/// 36 pixels noticeably -- **all 36 lie within 0.1 pixel of the silhouette
/// edge**, where the cubesphere mesh really is not symmetric under rotation.
/// The control beside it: shifting the centre by one radius changes 144414
/// pixels noticeably, four thousand times more.
///
/// Seeing the rotation **with the eye** becomes possible from R5, when bodies
/// get a surface; until then the oracle for orientation is the numbers of R1c
/// (pole, RA of the prime meridian, rotation rate) rather than pixels.
#[test]
fn turning_a_smooth_sphere_moves_only_the_edge_of_its_silhouette() {
    let Some(gpu) = gpu() else { return };

    const SIDE: u32 = 512;
    const ALTITUDE_M: f64 = 1.0e7;
    /// A difference larger than this is no longer interpolation rounding.
    const NOTABLE: i32 = 4;

    let world = mission::world(&mission::default_asset()).expect("world");
    let snapshot = world.snapshot();

    let scene = |orientation: Option<[f64; 4]>, shift: f64| {
        let mut scene = view::build(&snapshot, Orbit::at_altitude(ALTITUDE_M).camera());
        if let Some(q) = orientation {
            scene.bodies[0].orientation = compose(q, scene.bodies[0].orientation);
        }
        scene.bodies[0].centre[1] += shift;
        scene
    };

    let base = scene(None, 0.0);
    let earth = base.bodies[0];
    let radius_px = disc_radius_px(
        earth.radius_m,
        length(sub(earth.centre, base.camera.position())),
        SIDE,
    );
    let taken = shot::take_scene(&gpu, SIDE, SIDE, &base).expect("frame");

    // Notable differences, together with how far they are from the silhouette
    // edge.
    let notable = |other: &Shot| {
        let mut count = 0u64;
        let mut furthest = 0.0f64;
        for y in 0..SIDE {
            for x in 0..SIDE {
                let (a, b) = (taken.pixel(x, y), other.pixel(x, y));
                let difference = (0..3)
                    .map(|k| (i32::from(a[k]) - i32::from(b[k])).abs())
                    .max()
                    .expect("three channels");
                if difference <= NOTABLE {
                    continue;
                }
                count += 1;
                let dx = f64::from(x) + 0.5 - f64::from(SIDE) / 2.0;
                let dy = f64::from(y) + 0.5 - f64::from(SIDE) / 2.0;
                furthest = furthest.max((dx.hypot(dy) - radius_px).abs());
            }
        }
        (count, furthest)
    };

    // A quarter turn about three axes rather than one: a swapped quaternion
    // component would coincide with itself on a lucky axis.
    let half = std::f64::consts::FRAC_PI_4;
    for (name, axis) in [
        ("x", [1.0, 0.0, 0.0]),
        ("y", [0.0, 1.0, 0.0]),
        ("z", [0.0, 0.0, 1.0]),
    ] {
        let turn = [
            half.cos(),
            half.sin() * axis[0],
            half.sin() * axis[1],
            half.sin() * axis[2],
        ];
        let turned = shot::take_scene(&gpu, SIDE, SIDE, &scene(Some(turn), 0.0)).expect("frame");

        let (count, furthest) = notable(&turned);
        println!(
            "  a 90 deg turn about {name}: {count} notable pixels, the furthest \
             {furthest:.2} px from the silhouette edge"
        );
        assert!(
            count < 100,
            "the turn about {name} changed {count} pixels noticeably -- geometry \
             and normals went different ways"
        );
        assert!(
            furthest < 1.0,
            "the turn about {name} changed a pixel {furthest:.2} px from the \
             silhouette edge -- that is no longer the edge"
        );
    }

    // The control: the same comparison under a shift of one radius. Without it
    // "nothing changed" would only mean the comparison is blind.
    let shifted = shot::take_scene(&gpu, SIDE, SIDE, &scene(None, earth.radius_m)).expect("frame");
    let (count, _) = notable(&shifted);
    println!("  a shift of one radius: {count} notable pixels");
    assert!(
        count > 50_000,
        "a shift by a radius changed only {count} pixels -- the comparison sees nothing"
    );
}

/// The quaternion product `[w, x, y, z]`: `b` first, then `a`.
fn compose(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [aw, ax, ay, az] = a;
    let [bw, bx, by, bz] = b;
    [
        aw * bw - ax * bx - ay * by - az * bz,
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
    ]
}

// ---------------------------------------------------------------------------
// The rotating frame (ROADMAP-UI.md, U6a2)

/// The synodic coordinates from `view` are those the engine's formula gives,
/// and that formula is checked against C.
///
/// The oracle here is not "the loop looks closed", which is why it is worth
/// something. `engine::trajectory::rotating_position` was compared with
/// `frame_from_inertial` (C, `core/frame.h`) over 1345 fixture samples with a
/// divergence of 3.48e-7 (F6); if the game's transform agrees with it over
/// the whole live trajectory, it agrees with the core too -- transitively,
/// without a second fixture.
///
/// There is exactly one deliberate difference: the engine returns
/// dimensionless CR3BP units (divided by the `L` of **their own** instant),
/// while the game multiplies them by the present Earth-Moon distance. So the
/// Moon of every sample lands where the Moon is now -- and that is what keeps
/// the picture still while `L` wanders between 3.63 and 4.06e8 m.
#[test]
fn the_rotating_frame_agrees_with_the_formula_checked_against_c() {
    use game::frame_view::ViewFrame;

    let mut world = mission::world(&mission::default_asset()).expect("world");
    world.tick(16);
    let snapshot = world.snapshot();

    let camera = || Orbit::at_altitude(mission::CAMERA_ALTITUDE_M).camera();
    let scene = view::build_in(&snapshot, camera(), ViewFrame::Rotating);

    // The same constant scale the game multiplies the dimensionless
    // coordinates by.
    let scale = game::frame_view::SYNODIC_SCALE_M;

    // What the game should have given: the engine's formula on the same
    // samples and the same normals, times the scale.
    let vessel = &snapshot.vessels[0];
    let mut expected: Vec<[f64; 3]> = Vec::new();
    for leg in &vessel.legs {
        let normals = view::plane_normals(&leg.samples);
        for (index, sample) in leg.samples.iter().enumerate() {
            if sample.state.t <= snapshot.t {
                continue;
            }
            // The engine expects a **unit** normal (`fill_axes` normalises
            // it), while `plane_normals` returns `d x d_dot` as is -- the game
            // normalises inside the basis itself. Two contracts for the same
            // quantity, and a silent mismatch here would give 1e23 m of
            // difference.
            let n = normals[index];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            let p = engine::trajectory::rotating_position(
                [sample.state.r.x, sample.state.r.y, sample.state.r.z],
                sample.earth,
                sample.moon,
                [n[0] / len, n[1] / len, n[2] / len],
            );
            expected.push([p[0] * scale, p[1] * scale, p[2] * scale]);
        }
    }
    assert!(
        expected.len() > 500,
        "the forecast is too short: {}",
        expected.len()
    );

    // The forecast is the longest polyline; there is no history at the start
    // of the mission.
    let drawn = scene
        .polylines
        .iter()
        .max_by_key(|p| p.points.len())
        .expect("the scene has polylines")
        .points
        .clone();
    assert_eq!(
        drawn.len(),
        expected.len(),
        "the point counts differ -- different polylines are being compared"
    );

    let mut worst = 0.0f64;
    for (a, b) in drawn.iter().zip(expected.iter()) {
        let e = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        worst = worst.max(e);
    }
    println!(
        "  {} points, worst divergence from the engine's formula {worst:.3e} m",
        drawn.len()
    );

    // A metre in 4e8 is 2.5e-9 relative, i.e. the level of the formula itself
    // rather than a transform error. The game takes `mu` from the asset, the
    // engine has it as a constant, and at this level they should agree too.
    assert!(
        worst < 1.0,
        "a divergence of {worst:.3e} m is a different formula, not different arithmetic"
    );

    // The second number the map is switched on for: in the synodic frame the
    // same trajectory takes three times less room, because the pair's rotation
    // has been taken out of it.
    let spread = |points: &[[f64; 3]]| {
        let mut worst: f64 = 0.0;
        for p in points {
            for q in points.iter().step_by(points.len() / 32 + 1) {
                let d =
                    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
                worst = worst.max(d);
            }
        }
        worst
    };
    let inertial = view::build_in(&snapshot, camera(), ViewFrame::Inertial);
    let inertial_points = inertial
        .polylines
        .iter()
        .max_by_key(|p| p.points.len())
        .expect("the scene has polylines")
        .points
        .clone();
    let (a, b) = (spread(&inertial_points), spread(&drawn));
    println!("  spread: inertial {a:.4e} m, synodic {b:.4e} m");
    assert!(
        b < 0.5 * a,
        "a synodic spread of {b:.3e} against an inertial {a:.3e} -- the frame took \
         nothing out"
    );
}

/// The pair sits where it belongs in the synodic frame.
///
/// Earth at `-mu*L` from the origin, the Moon at `(1 - mu)*L`, both on the x
/// axis -- that is the definition of the frame, and it also checks that the
/// bodies went through **the same** transform as the polylines. A body left
/// in inertial coordinates would hang apart from the trajectory around it.
#[test]
fn the_pair_sits_on_the_axis_in_the_rotating_frame() {
    use game::frame_view::ViewFrame;

    let world = mission::world(&mission::default_asset()).expect("world");
    let snapshot = world.snapshot();
    let camera = Orbit::at_altitude(mission::CAMERA_ALTITUDE_M).camera();

    let scene = view::build_in(&snapshot, camera, ViewFrame::Rotating);
    let (earth, moon) = (scene.bodies[0], scene.bodies[1]);

    // The scale is the present Earth-Moon distance, i.e. exactly what the
    // frame normalises everything else by.
    let l = moon.centre[0] - earth.centre[0];
    let mu = -earth.centre[0] / l;
    println!(
        "  Earth {:?}, the Moon {:?}, L = {l:.4e} m, mu = {mu:.9}",
        earth.centre, moon.centre
    );

    assert!(
        (3.6e8..4.1e8).contains(&l),
        "a distance of {l:.3e} m between the bodies -- that is not the Moon's orbit"
    );
    assert!(
        (mu - 0.0121505856).abs() < 1e-6,
        "the barycentre sits at mu = {mu:.9}, but should at 0.01215",
    );
    for (name, body) in [("Earth", earth), ("the Moon", moon)] {
        assert!(
            body.centre[1].abs() < 1.0 && body.centre[2].abs() < 1.0,
            "{name} left the x axis: {:?}",
            body.centre
        );
    }
}

/// In the synodic frame the Moon stands still; in the inertial one it leaves
/// over three days.
///
/// This is the property the map switches to a rotating system for, checked in
/// pixels rather than numbers: the camera is aimed at the Moon at instant A
/// and does not move, while the world lives three days. In the synodic frame
/// frame B matches frame A; in the inertial one the Moon covers about 36
/// degrees of orbit over the same time -- a quarter of a billion metres --
/// and leaves a 5.8e7 m wide field of view entirely.
///
/// The polylines are removed from the scene on purpose: they move in both
/// frames (the vessel flies, the forecast grows), and without that the frame
/// would be measuring two things at once.
#[test]
fn the_moon_stands_still_in_the_rotating_frame_and_leaves_the_inertial_one() {
    use game::frame_view::ViewFrame;
    use game::world::{EARTH, MOON};

    let Some(gpu) = gpu() else { return };

    const SIDE: u32 = 512;
    /// Where to watch the Moon from: 5e7 m gives a disc of about 30 pixels.
    const DISTANCE_M: f64 = 5.0e7;
    const DAYS: f64 = 3.0;

    let mut world = mission::world(&mission::default_asset()).expect("world");
    world.tick(16);
    let before = world.snapshot();

    // Each frame gets its own camera, aimed where the Moon is at instant A in
    // that very frame. There can be no shared camera here: the coordinates
    // differ.
    let aim = |frame: ViewFrame| -> engine::camera::Camera {
        let scene = view::build_in(&before, Orbit::at_altitude(1.0e9).camera(), frame);
        let moon = scene.bodies[1].centre;
        // To the side of the Earth-Moon line, so that Earth does not get into
        // the frame.
        let side = [-moon[1], moon[0], 0.0];
        let n = (side[0] * side[0] + side[1] * side[1]).sqrt();
        let eye = [
            moon[0] + side[0] / n * DISTANCE_M,
            moon[1] + side[1] / n * DISTANCE_M,
            moon[2],
        ];
        engine::camera::Camera::look_at(eye, moon, [0.0, 0.0, 1.0])
    };

    // Three days of world time. The forecast is computed first, or the cursor
    // runs into the horizon and goes nowhere.
    let want = before.t + DAYS * 86400.0;
    while world.snapshot().t < want {
        world.step(DAYS * 86400.0 / mission::DEFAULT_WARP, 64);
    }
    let after = world.snapshot();

    // Over that time the Moon really did travel what it should -- otherwise
    // "vanished from the frame" would prove nothing.
    let moon_at = |snapshot: &game::snapshot::WorldSnapshot| {
        let body = |index: i32| {
            snapshot
                .bodies
                .iter()
                .find(|b| b.body == index)
                .expect("the body is in the snapshot")
        };
        let (earth, moon) = (body(EARTH), body(MOON));
        [
            moon.position[0] - earth.position[0],
            moon.position[1] - earth.position[1],
            moon.position[2] - earth.position[2],
        ]
    };
    let (a, b) = (moon_at(&before), moon_at(&after));
    let travelled = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    println!("  over {DAYS} days the Moon travelled {travelled:.3e} m");
    assert!(
        travelled > 1.0e8,
        "the Moon travelled only {travelled:.3e} m -- the world did not move"
    );

    for frame in [ViewFrame::Rotating, ViewFrame::Inertial] {
        let camera = aim(frame);
        let shoot = |snapshot: &game::snapshot::WorldSnapshot, camera| {
            let mut scene = view::build_in(snapshot, camera, frame);
            // Bodies only: polylines move in any frame.
            scene.polylines.clear();
            shot::take_scene(&gpu, SIDE, SIDE, &scene).expect("frame")
        };

        let first = shoot(&before, camera);
        let second = shoot(&after, aim(frame));

        // The **silhouette** is compared rather than the colour, and that is a
        // decision of V5: the direction to the star now comes from the
        // ephemeris, so over three days the Moon's terminator crawls across
        // the disc. A shadow that moved is right; a disc that moved is not,
        // and the oracle may not confuse them. Before V5 comparing pixels did
        // exactly that: 686 changed pixels on a motionless disc.
        let drawn = |shot: &Shot, x, y| {
            let p = shot.pixel(x, y);
            [p[0], p[1], p[2]] != frame::CLEAR_BYTES
        };
        let differing = (0..SIDE)
            .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
            .filter(|&(x, y)| drawn(&first, x, y) != drawn(&second, x, y))
            .count();
        let (lit_first, lit_second) = (lit(&first), lit(&second));

        println!("  {frame:?}: disc {lit_first} -> {lit_second} pixels, {differing} differing");
        // The Moon's disc from 5e7 m is 15 pixels of radius, i.e. about 730
        // pixels of area. Less would mean the camera is looking elsewhere.
        assert!(
            lit_first > 500,
            "{frame:?}: only {lit_first} pixels in the first frame -- the Moon is not visible"
        );

        match frame {
            // Still: the same disc on the same pixels. The tolerance is the
            // silhouette edge, the same one R1e measured (36 pixels per turn).
            ViewFrame::Rotating => assert!(
                differing < 100,
                "in the synodic frame {differing} pixels changed over three days -- \
                 the Moon does not stand still"
            ),
            // Gone: in a frame aimed at yesterday's place only sky is left.
            ViewFrame::Inertial => {
                assert!(
                    lit_second == 0,
                    "inertially the Moon left {lit_second} pixels in the frame -- \
                     it should have gone entirely"
                );
                // Every pixel that was disc changed -- not "many" but the whole
                // Moon.
                assert!(
                    differing as u64 >= lit_first,
                    "inertially {differing} pixels of {lit_first} changed -- the disc \
                     did not vanish entirely"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Terrain in the game (D12)

/// The game attaches terrain to the body it was loaded for -- and only to it.
///
/// The second half is the main one. An `attach_terrain` that set `Loaded` on
/// every body would pass a "the Moon has terrain" check and spoil Earth, for
/// which there is no DEM in the repository at all: it would be drawn with
/// lunar heights. So both bodies are checked, not one.
#[test]
fn the_game_attaches_terrain_to_the_moon_and_leaves_the_earth_smooth() {
    use engine::scene::{TerrainId, TileSet};
    use game::world::{EARTH, MOON};

    let mut world = mission::world(&mission::default_asset()).expect("world");
    world.tick(8);
    let snapshot = world.snapshot();

    let camera = || Orbit::at_altitude(mission::CAMERA_ALTITUDE_M).camera();
    let mut scene = view::build(&snapshot, camera());

    // Before the call all of them are smooth, i.e. what the game drew before
    // D12.
    assert!(
        scene.bodies.iter().all(|b| b.tiles == TileSet::Smooth),
        "the scene arrived with terrain before it was switched on"
    );

    // The handle is made up on purpose: what is checked is who got it rather
    // than what is inside, and a real one would need a GPU with bindless.
    let id = TerrainId(0);
    view::attach_terrain(&mut scene, &snapshot, MOON, id);

    // The order of bodies in the scene is the snapshot's, minus those with no
    // radius.
    let with_radius: Vec<i32> = snapshot
        .bodies
        .iter()
        .filter(|b| b.radius_m > 0.0)
        .map(|b| b.body)
        .collect();
    assert_eq!(
        with_radius.len(),
        scene.bodies.len(),
        "the body ordering rule diverged between build and attach_terrain"
    );

    for (body, drawn) in with_radius.iter().zip(&scene.bodies) {
        let expected = if *body == MOON {
            TileSet::Loaded(id)
        } else {
            TileSet::Smooth
        };
        assert_eq!(drawn.tiles, expected, "body {body} got the wrong tile set");
    }

    // And separately, that Earth really was in the frame. Without this line
    // "Earth is smooth" would pass for a scene with no Earth in it.
    assert!(
        with_radius.contains(&EARTH) && with_radius.contains(&MOON),
        "both bodies should have been in the scene, but there are {with_radius:?}"
    );
}

/// A body that is not in the scene does not break the call.
///
/// A scene without the Moon is a legitimate state (an asset without it, a
/// body with no radius), and `attach_terrain` must survive it: terrain is
/// decoration, not an invariant.
#[test]
fn attaching_terrain_to_a_body_that_is_not_there_does_nothing() {
    use engine::scene::TerrainId;

    let mut world = mission::world(&mission::default_asset()).expect("world");
    world.tick(8);
    let snapshot = world.snapshot();

    let camera = Orbit::at_altitude(mission::CAMERA_ALTITUDE_M).camera();
    let mut scene = view::build(&snapshot, camera);
    let before: Vec<_> = scene.bodies.iter().map(|b| b.tiles).collect();

    // 99 -- there is no body with that index in the asset.
    view::attach_terrain(&mut scene, &snapshot, 99, TerrainId(0));

    let after: Vec<_> = scene.bodies.iter().map(|b| b.tiles).collect();
    assert_eq!(
        before, after,
        "an unknown body changed someone else's tiles"
    );
}

/// Earth carries air and the Moon does not (ROADMAP-ATMOSPHERE.md, S1).
///
/// A body without an atmosphere is not "not done yet" but a fact about the
/// Moon, and the engine has the right to save on it. The test catches exactly
/// the bug that is easy to make with the first caller: hanging air on every
/// body in the asset.
#[test]
fn the_earth_carries_air_and_the_moon_does_not() {
    let mut world = mission::world(&mission::default_asset()).expect("the world builds");
    world.run_to_day(mission::start().t + 2.0 * 86400.0, 1.0, 8);
    let snapshot = world.snapshot();
    let scene = view::build(&snapshot, Orbit::at_altitude(1.0e9).camera());

    // The order of bodies in the scene is the snapshot's, skipping those with
    // no radius (`view::attach_terrain` relies on the same rule).
    let mut with_air = 0;
    for body in &scene.bodies {
        if body.air.is_some() {
            with_air += 1;
        }
    }
    assert_eq!(
        with_air,
        1,
        "exactly one body of {} should have air",
        scene.bodies.len()
    );

    let earth = scene
        .bodies
        .iter()
        .find(|b| b.air.is_some())
        .expect("just counted");
    let air = earth.air.expect("just checked");
    // The upper boundary stands above the radius **from the asset**, not above
    // a constant.
    assert!(
        (air.thickness_m(earth.radius_m) - engine::scene::Atmosphere::EARTH_THICKNESS_M).abs()
            < 1.0,
        "a layer of {} m above a radius of {}",
        air.thickness_m(earth.radius_m),
        earth.radius_m
    );
}

// ---------------------------------------------------------------------------
// Light from the ephemeris (debt D16, step V5)

/// The direction to the star comes from the ephemeris and changes with the
/// mission date.
///
/// Two claims, each closing its half of debt D16:
///
/// - **where from.** The direction must match what the snapshot itself gives:
///   the unit vector from Earth to the Sun. A constant baked into the engine
///   would match only by accident;
/// - **when.** Over a hundred days of mission Earth covers almost a third of
///   its orbit, and the direction must follow. That is what "the time of day
///   in the game depends on the date" means, and it is what was missing
///   before the step.
///
/// A hundred days rather than half a year is the asset's limit: the fixture
/// covers 120 days (`core/cook/cook_fixture.c`), over which the Sun moves 98
/// degrees, so the dot product falls below 0.2. Half a year would give -1,
/// but outside the asset.
#[test]
fn the_sun_comes_from_the_ephemeris_and_moves_with_the_date() {
    use game::world::EARTH;

    let geocentric_sun = |snapshot: &game::snapshot::WorldSnapshot| {
        let earth = snapshot
            .bodies
            .iter()
            .find(|b| b.body == EARTH)
            .expect("Earth is in the snapshot");
        let sun = snapshot.sun.expect("the asset has the Sun");
        let d = [
            sun[0] - earth.position[0],
            sun[1] - earth.position[1],
            sun[2] - earth.position[2],
        ];
        let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        [d[0] / n, d[1] / n, d[2] / n]
    };

    let mut world = mission::world(&mission::default_asset()).expect("world");
    world.tick(16);
    let before = world.snapshot();
    let first = view::build(&before, Orbit::at_altitude(1.0e9).camera());

    // The scene is inertial, so its direction is exactly the geocentric one.
    let want = geocentric_sun(&before);
    for k in 0..3 {
        assert!(
            (first.sun[k] - want[k]).abs() < 1.0e-12,
            "the scene's light {:?} does not come from the ephemeris ({want:?})",
            first.sun
        );
    }

    let days = 100.0;
    world.run_to_day(before.t + days * 86400.0, mission::DEFAULT_WARP, 64);
    let after = world.snapshot();
    let second = view::build(&after, Orbit::at_altitude(1.0e9).camera());

    let moved: f64 = (0..3).map(|k| first.sun[k] * second.sun[k]).sum();
    assert!(
        moved < 0.5,
        "over {days} days the direction to the Sun hardly changed: cos = {moved}"
    );
}
