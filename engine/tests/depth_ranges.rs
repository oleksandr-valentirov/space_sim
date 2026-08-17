//! Two depth ranges in a real scene for the first time (stage V, step V3).
//!
//! Before this step `plan` always counted one pass: the engine-probe scene has
//! a span of 22.7, i.e. less than one depth buffer (F3: seven orders). A frame
//! with a hull metres away and a planet millions of metres away is the first
//! with two of them, and the first check of what Q2 kept the ranges in the
//! design for.
//!
//! What is checked here is what only a GPU can show: whether the hull is
//! whole, and whether there is a seam where the ranges meet. The arithmetic of
//! the boundary itself is in the `engine::frame` unit tests
//! (`the_range_boundary_never_falls_inside_the_hull`).

use engine::camera::Camera;
use engine::gpu::Gpu;
use engine::scene::{Body, Scene, Ship, TileSet};
use engine::shot::Shot;
use engine::{frame, ship, shot, sphere};

const SIZE: u32 = 256;
const FOV_Y: f64 = std::f64::consts::PI / 3.0;
const ASPECT: f64 = 1.0;

/// The camera's altitude above the ground in the [`ship_over_the_ground`]
/// scene, metres.
const EYE_ALTITUDE_M: f64 = 1000.0;

/// How many metres from the camera to the ship in that same scene, across and
/// down.
const GROUND_RANGE_M: f64 = 12.0;
const GROUND_DROP_M: f64 = 4.0;

fn gpu() -> Option<Gpu> {
    Gpu::for_tests()
}

fn earth() -> Body {
    Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: sphere::EARTH_RADIUS_M,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: TileSet::Smooth,
        colour: frame::COLOUR,
        // No air, deliberately: the background stays the clear colour, so any
        // hole in the frame shows up as a hole. A sky would cover it over.
        air: None,
    }
}

fn hull(centre: [f64; 3], orientation: [f64; 4]) -> Ship {
    Ship {
        centre,
        orientation,
        height_m: ship::DEFAULT_HEIGHT_M,
        extent_m: 0.5 * ship::DEFAULT_HEIGHT_M,
        colour: [0.72, 0.74, 0.78, 1.0],
        roughness: engine::ship::HULL_ROUGHNESS,
        metallic: engine::ship::HULL_METALLIC,
    }
}

fn drawn(shot: &Shot, x: u32, y: u32) -> bool {
    let p = shot.pixel(x, y);
    [p[0], p[1], p[2]] != frame::CLEAR_BYTES
}

/// The rectangle bounding every drawn pixel.
fn lit_bounds(shot: &Shot) -> Option<(u32, u32, u32, u32)> {
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in 0..shot.height {
        for x in 0..shot.width {
            if !drawn(shot, x, y) {
                continue;
            }
            bounds = Some(match bounds {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        }
    }
    bounds
}

/// The silhouette of the same mesh computed on the CPU -- the V2 oracle,
/// unchanged except in one thing: the ship here is not at the origin, so the
/// mesh has to be moved to where it stands. The rotation is the identity, and
/// that is a choice of the scene rather than a simplification of the oracle.
fn projected_bounds(camera: &Camera, height_m: f64, centre: [f64; 3]) -> (f64, f64, f64, f64) {
    let mesh = ship::generate(height_m);
    let mut bounds = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in &mesh.positions {
        let world = [p[0] + centre[0], p[1] + centre[1], p[2] + centre[2]];
        let screen = camera
            .to_screen(FOV_Y, SIZE, SIZE, world)
            .expect("a vertex behind the camera -- wrong scene");
        bounds.0 = bounds.0.min(f64::from(screen[0]));
        bounds.1 = bounds.1.min(f64::from(screen[1]));
        bounds.2 = bounds.2.max(f64::from(screen[0]));
        bounds.3 = bounds.3.max(f64::from(screen[1]));
    }
    bounds
}

/// The ship in orbit, the planet **behind the camera**.
///
/// From 400 km the Earth's disc spans 70.2 deg from the nadir, so it falls
/// entirely out of a frame aimed at the zenith for any field of view up to
/// 109 deg. There are still two passes at that: `far_for` measures the span of
/// the scene, not of what is visible -- the planet stays in the scene and pulls
/// the far boundary out to 1.3e7 m.
///
/// So the frame holds exactly the ship, and its silhouette can be compared
/// against the projection by the same oracle as in V2 -- but now with two
/// ranges instead of one.
fn ship_against_space() -> Scene {
    let radius = sphere::EARTH_RADIUS_M + 400_000.0;
    let eye = [radius, 0.0, 0.0];
    let centre = [radius + 15.0, 0.0, 0.0];
    let camera = Camera::look_at(eye, centre, [0.0, 0.0, 1.0]);

    let mut scene = Scene::new(camera);
    scene.bodies.push(earth());
    scene.ships.push(hull(centre, [1.0, 0.0, 0.0, 0.0]));
    scene
}

/// The local basis of the ground scene: `up` away from the planet's centre,
/// `east` and `north` across it.
///
/// The direction is oblique deliberately: a fixture standing exactly over the
/// centre of a cube face has already hidden two mistakes in a row (D13, D14).
fn ground_basis() -> ([f64; 3], [f64; 3], [f64; 3]) {
    let unit = |v: [f64; 3]| {
        let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / n, v[1] / n, v[2] / n]
    };
    let up = unit([0.37, -0.51, 0.77]);
    let east = unit([-up[1], up[0], 0.0]);
    let north = [
        up[1] * east[2] - up[2] * east[1],
        up[2] * east[0] - up[0] * east[2],
        up[0] * east[1] - up[1] * east[0],
    ];
    (up, east, north)
}

/// The ship beside the camera, both a kilometre above the ground.
///
/// This is the third-person view in flight, and the scene is needed precisely
/// because in it **the range boundary cuts the visible surface in the middle
/// of the frame**: near = 0.96 m (a tenth of the distance to the hull),
/// far = 1.27e7 m, so the boundary stands at 3.51 km while the horizon from a
/// kilometre up is at 113 km.
///
/// ## Why a kilometre and not ten metres -- and why that is not taste
///
/// The camera's altitude **cancels**, and therein lies the whole thing. `near`
/// is a tenth of the altitude, `far` is roughly the planet's diameter, so for a
/// camera **without a ship** the boundary always comes out as
/// `sqrt(2R*h/10)`, i.e. exactly `horizon/3.16`. Ground at that distance sits
/// at 1.58 of the horizon's dip angle -- and the dip itself at ten metres is
/// 1.77 mrad against a pixel of 4.09 mrad. So the whole strip of the second
/// range lies **within a quarter of a pixel** below the horizon, and nothing
/// can be checked there: measured by breaking it, planes pushed apart
/// fourfold left the frame without a single hole.
///
/// A ship twelve metres away breaks that relation: `near` is now its distance
/// rather than the altitude, so the boundary stays at three and a half
/// kilometres while the horizon goes out to a hundred and thirteen. Ground at
/// the boundary sits at 16 deg and the horizon at 1 deg, with sixty rows of
/// frame between them.
///
/// The ship is dropped four metres below the camera deliberately: that way the
/// whole hull lies **below** the horizon line. A ship poking up into the sky
/// gives legitimate background pixels under its nose in its own columns, and
/// the seam oracle would have to be made more complicated than what it
/// checks.
fn ship_over_the_ground() -> Scene {
    let radius = sphere::EARTH_RADIUS_M;
    let (up, east, _) = ground_basis();

    let at = |altitude: f64, east_m: f64| {
        let r = radius + altitude;
        [
            up[0] * r + east[0] * east_m,
            up[1] * r + east[1] * east_m,
            up[2] * r + east[2] * east_m,
        ]
    };

    let eye = at(EYE_ALTITUDE_M, 0.0);
    let centre = at(EYE_ALTITUDE_M - GROUND_DROP_M, GROUND_RANGE_M);
    let camera = Camera::look_at(eye, centre, up);

    let mut scene = Scene::new(camera);
    scene.bodies.push(earth());
    scene.ships.push(hull(centre, [1.0, 0.0, 0.0, 0.0]));
    scene
}

/// Both scenes really do ask for two ranges -- otherwise the rest of the file
/// is checking a single-pass frame and saying nothing about it.
///
/// The second claim is about the ground scene, and it is what makes the seam
/// check non-empty: **the boundary has to land between the camera and the
/// horizon**. A sphere's horizon is an exact formula, `sqrt(2Rh)`, with no
/// approximations, so this too is a number of the scene rather than taste. If
/// the boundary lands further out there will be no seam, for the simple reason
/// that the two ranges meet nowhere in the visible frame.
#[test]
fn both_scenes_ask_for_two_depth_ranges() {
    let orbit = frame::Frame::depth_ranges(&ship_against_space(), ASPECT);
    assert_eq!(orbit.len(), 2, "the ship in orbit: {orbit:?}");

    let ground = frame::Frame::depth_ranges(&ship_over_the_ground(), ASPECT);
    assert_eq!(ground.len(), 2, "the ship over the ground: {ground:?}");

    let horizon = (2.0 * sphere::EARTH_RADIUS_M * EYE_ALTITUDE_M).sqrt();
    assert!(
        ground[1] < horizon,
        "the boundary at {} m lies beyond the horizon ({horizon} m) -- there is \
         no ground from the second range in frame, and no place for a seam to \
         appear",
        ground[1]
    );
}

/// The second range does not clip the hull: the silhouette is the same as
/// without it.
///
/// The oracle is the mesh projected through `Camera::to_screen`, i.e. exactly
/// the one V2 measured the ship with in a single-pass frame. The tolerance is
/// asymmetric for the same reason: a rasteriser can lose a spike, but it
/// cannot draw outside the geometry.
#[test]
fn two_ranges_do_not_clip_the_hull() {
    let Some(gpu) = gpu() else {
        return;
    };

    let scene = ship_against_space();
    let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene).expect("the frame with the ship");
    let (x0, y0, x1, y1) = lit_bounds(&shot).expect("the frame is empty -- there is no ship");
    let expected = projected_bounds(&scene.camera, ship::DEFAULT_HEIGHT_M, scene.ships[0].centre);

    let inside = |what: &str, drawn: f64, want: f64, sign: f64| {
        let over = sign * (drawn - want);
        assert!(
            over <= 1.0,
            "{what}: the frame went past the projection by {over} px ({drawn} \
             against {want})"
        );
        assert!(
            over >= -2.5,
            "{what}: the frame fell short of the projection by {} px ({drawn} \
             against {want})",
            -over
        );
    };
    inside("left", f64::from(x0), expected.0, -1.0);
    inside("top", f64::from(y0), expected.1, -1.0);
    inside("right", f64::from(x1), expected.2, 1.0);
    inside("bottom", f64::from(y1), expected.3, 1.0);
}

/// The horizon row in every column of the frame -- **from geometry, not from
/// the frame**.
///
/// The tangent points of a sphere from an eye at radius `r` lie on a circle,
/// and it is expressed exactly: `E.T = R^2` for `|T| = R`, whence
/// `T(phi) = (R^2/r)*up + R*sqrt(1 - R^2/r^2)*w(phi)`. Projected through
/// `Camera::to_screen`, that set is the horizon line.
///
/// The frame must not be asked about it, and that is this check's main lesson:
/// a hole **right on the horizon** merely shifts the "first drawn pixel" down,
/// and an oracle that takes the horizon from the frame does not see it at all.
/// Measured by breaking it: planes pushed apart by an order of magnitude take
/// away six rows of ground while the check "there are no holes below the first
/// drawn pixel" stays green.
fn horizon_rows(camera: &Camera, altitude_m: f64) -> Vec<Option<f64>> {
    let radius = sphere::EARTH_RADIUS_M;
    let r = radius + altitude_m;
    let (up, east, north) = ground_basis();

    let along_up = radius * radius / r;
    let across = radius * (1.0 - (radius / r) * (radius / r)).sqrt();

    let mut rows: Vec<Option<f64>> = vec![None; SIZE as usize];
    // More steps than columns, and with a large margin: the horizon crosses the
    // frame at an angle, so a sparse sampling would leave columns without an
    // answer.
    let steps = 20_000;
    for k in 0..steps {
        let phi = 2.0 * std::f64::consts::PI * f64::from(k) / f64::from(steps);
        let w = [
            phi.cos() * east[0] + phi.sin() * north[0],
            phi.cos() * east[1] + phi.sin() * north[1],
            phi.cos() * east[2] + phi.sin() * north[2],
        ];
        let point = [
            along_up * up[0] + across * w[0],
            along_up * up[1] + across * w[1],
            along_up * up[2] + across * w[2],
        ];
        let Some(screen) = camera.to_screen(FOV_Y, SIZE, SIZE, point) else {
            continue;
        };
        let x = f64::from(screen[0]);
        if !(0.0..f64::from(SIZE)).contains(&x) {
            continue;
        }
        let slot = &mut rows[x as usize];
        let y = f64::from(screen[1]);
        *slot = Some(slot.map_or(y, |old: f64| old.min(y)));
    }
    rows
}

/// Where the ranges meet, the surface does not tear -- and it reaches all the
/// way to the horizon.
///
/// Two claims, and the second matters more than the first:
///
/// - **the ground begins at the horizon**, not wherever it happened to. That
///   is exactly where a seam at the range boundary would land: in this scene
///   the boundary stands at 3.57 km and the horizon at 11.3 km, so the strip
///   of the second range is thin and lies right up against the horizon;
/// - **below the horizon there is not one hole.**
///
/// The tolerance is a pixel: the horizon lands between rows, and the
/// rasteriser paints the one whose centre is covered.
#[test]
fn the_surface_has_no_seam_where_the_ranges_meet() {
    let Some(gpu) = gpu() else {
        return;
    };

    let scene = ship_over_the_ground();
    let shot = shot::take_scene(&gpu, SIZE, SIZE, &scene).expect("the frame with ground");
    let horizon = horizon_rows(&scene.camera, EYE_ALTITUDE_M);

    let mut checked = 0;
    for x in 0..shot.width {
        let Some(row) = horizon[x as usize] else {
            continue;
        };
        // Columns where the horizon does not fit in the frame assert nothing.
        if !(1.0..f64::from(shot.height) - 1.0).contains(&row) {
            continue;
        }
        checked += 1;

        let first = (row.ceil() as u32 + 1).min(shot.height - 1);
        for y in first..shot.height {
            assert!(
                drawn(&shot, x, y),
                "column {x}: below the horizon (row {row}) there is a hole in \
                 row {y}"
            );
        }
        // And conversely: there is no ground above the horizon, otherwise
        // "there are no holes" would be satisfied by everything being drawn.
        let above = row.floor() as u32;
        assert!(
            above < 2 || !drawn(&shot, x, above - 2),
            "column {x}: above the horizon (row {row}) something is drawn"
        );
    }

    assert!(
        checked > SIZE / 2,
        "the horizon was checked in only {checked} columns out of {SIZE} -- \
         wrong scene"
    );
}
