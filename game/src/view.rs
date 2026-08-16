//! Snapshot to scene (ROADMAP J1, J2).
//!
//! The whole boundary between game and engine, in one direction: what the game
//! knows about the world is reduced here to what the engine must draw. Nothing
//! travels back.
//!
//! ## Why geocentric
//!
//! The sphere in the frame sits at the origin with Earth's radius
//! (`engine::frame`), so the polyline must arrive in the same frame:
//! `vessel - Earth` at each sample's instant. Not a simplification and not a
//! temporary frame -- the same anchoring as in `trajectory_render` from F6,
//! only the subtraction happens here in `double` rather than in a shader.
//!
//! The rotating frame (PROJECT.md §7 requires it as the map's default) arrives
//! with the frame service; the samples already carry the Moon's position for
//! it.
//!
//! ## History and prediction are the same legs
//!
//! The cursor divides them by colour and by nothing else: no recomputation, no
//! copying, nothing moving in storage. That is exactly what rule 5 of
//! PROJECT.md §4 means -- "time turns a computed stretch of prediction into
//! history". Here it is literally visible: only what `sample.t` is compared
//! against changes.

use engine::camera::Camera;
use engine::scene::{Body, Polyline, Scene, TerrainId, TileSet};

use crate::frame_view::{self, Synodic, ViewFrame};
use crate::snapshot::WorldSnapshot;
use crate::world::{EARTH, MOON};

// The line colours moved into `palette` (U7c) -- not for tidiness but because
// they are what defines the interface's palette: the panel's accent must be
// the prediction's colour, and living in two places the two would quietly
// diverge.
//
// The numbers did not change in the move: [0.9, 0.6, 0.2] is (229, 153, 51) in
// the same units, because the frame's target is not sRGB and a byte divides by
// 255 without gamma. The `palette` tests check that, not this comment.
use crate::palette;

/// Half-length of the cross marker as a fraction of the distance to the
/// camera.
///
/// A fraction rather than metres: a vessel is viewed both from a billion
/// metres and from close by, and the marker must stay a marker -- the same size
/// on screen.
const MARKER_FRACTION: f64 = 0.01;

/// What thinning needs to know about the window, and what it already knows.
///
/// The frame's height, and only it: `fov_y` is vertical, and
/// `engine::lod::focal_px` derives pixels per radian from exactly that. The
/// width does not enter, because the tolerance is now in metres
/// (`crate::trail`) rather than in screen pixels.
pub struct Thinning<'a> {
    pub cache: &'a mut crate::trail::Cache,
    pub height_px: u32,
}

pub fn build(snapshot: &WorldSnapshot, camera: Camera) -> Scene {
    build_with_preview(snapshot, camera, &[], ViewFrame::Inertial)
}

/// The same in a given frame (ROADMAP-UI.md, U6a2).
pub fn build_in(snapshot: &WorldSnapshot, camera: Camera, frame: ViewFrame) -> Scene {
    build_with_preview(snapshot, camera, &[], frame)
}

/// The same, but with trails thinned by the screen criterion (N2a, N2b).
///
/// Its own entry point rather than a flag on [`build_in`], and not for
/// compatibility: the step's oracle is a comparison of **two** scenes, thinned
/// against full, so both must be equally easy to build. The game calls this
/// one, the tests call both.
pub fn build_thinned(
    snapshot: &WorldSnapshot,
    camera: Camera,
    preview: &[std::sync::Arc<crate::leg::Leg>],
    frame: ViewFrame,
    thinning: &mut Thinning<'_>,
) -> Scene {
    thinning.cache.begin_frame();
    let scene = build_all(snapshot, camera, preview, frame, Some(thinning));
    thinning.cache.sweep();
    scene
}

/// The same, plus the planner's speculative line (ROADMAP J5).
///
/// The preview is drawn in its own colour and **over** the prediction rather
/// than instead of it: the player must see both lines at once -- the one they
/// will fly now, and the one they would fly under the new plan.
pub fn build_with_preview(
    snapshot: &WorldSnapshot,
    camera: Camera,
    preview: &[std::sync::Arc<crate::leg::Leg>],
    frame: ViewFrame,
) -> Scene {
    build_all(snapshot, camera, preview, frame, None)
}

/// The shared body of both entry points: `thin` decides whether trails are
/// thinned.
///
/// `None` means "hand back every sample", which is exactly how all the code
/// before N2a saw the scene -- so this is not "an optimisation turned off" but
/// the reference the thinned one is measured against.
fn build_all(
    snapshot: &WorldSnapshot,
    camera: Camera,
    preview: &[std::sync::Arc<crate::leg::Leg>],
    frame: ViewFrame,
    mut thin: Option<&mut Thinning<'_>>,
) -> Scene {
    let mut scene = Scene::new(camera);

    // The "now" basis, for bodies and markers. Trajectory points take their
    // own instant's basis, which is exactly why it is computed beside the
    // sample rather than here.
    //
    // No basis means no rotating frame: if the asset has no Moon, or it stands
    // at one point with Earth, the scene stays inertial. That is not silently
    // ignoring the choice: an inertial frame is the correct answer to "there
    // is no pair of bodies", while a NaN at every vertex is not.
    let now = match frame {
        ViewFrame::Inertial => None,
        ViewFrame::Rotating => synodic_now(snapshot),
    };
    let moon_now = moon_local(snapshot).unwrap_or([0.0; 3]);

    // The bodies get the same Earth subtraction as the polylines, for the same
    // reason: the frame is geocentric (see the module intro). Earth ends up
    // exactly at the origin, the Moon where it is relative to it at that
    // instant, and in the rotating frame both stand still.
    if let Some(earth) = snapshot.bodies.iter().find(|b| b.body == EARTH) {
        for body in &snapshot.bodies {
            // There is no way to draw a body without a size: a zero radius
            // means "the asset does not say", not "a dot".
            if body.radius_m <= 0.0 {
                continue;
            }
            let centre = [
                body.position[0] - earth.position[0],
                body.position[1] - earth.position[1],
                body.position[2] - earth.position[2],
            ];
            scene.bodies.push(Body {
                centre: match now {
                    Some(s) => s.apply(centre, moon_now),
                    None => centre,
                },
                radius_m: body.radius_m,
                // A body's rotation about its own centre does not depend on
                // the choice of origin -- but it does depend on the choice of
                // **axes**, and in the rotating frame the axes differ.
                orientation: match now {
                    Some(s) => frame_view::compose(s.rotation(), body.orientation),
                    None => body.orientation,
                },
                // Smooth by default; `attach_terrain` enables terrain after
                // construction, because the frame issues the tile handle
                // rather than the snapshot
                // (D12).
                tiles: TileSet::Smooth,
                // Colour is a property of the body too (T1), and flat for now:
                // colour tiles arrive in T3. Choosing by body index is the same
                // route as the air below, and for the same reason: the asset
                // has no colour, and inventing it in the engine would not be
                // allowed at all.
                colour: body_colour(body.body),
                // Air is a property of the body (S1). Earth has it, the rest
                // of the asset does not: the Moon has no atmosphere, and `None`
                // here means exactly that rather than "not done yet".
                air: if body.body == EARTH {
                    Some(engine::scene::Atmosphere::EARTH.with_surface(body.radius_m))
                } else {
                    None
                },
            });
        }

        // The direction to the light comes from where everything else in the
        // frame comes from: the ephemeris, at that same instant (debt D16,
        // step V5). Geocentric, because the whole scene is; in the rotating
        // frame it is rotated by the same
        // basis as the bodies -- but **only rotated**: a direction has neither
        // origin nor scale, so `apply`, with its shift to the barycentre and
        // division by the Earth-Moon distance, would be wrong here.
        if let Some(sun) = snapshot.sun {
            let d = [
                sun[0] - earth.position[0],
                sun[1] - earth.position[1],
                sun[2] - earth.position[2],
            ];
            let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if length > 0.0 {
                let unit = [d[0] / length, d[1] / length, d[2] / length];
                scene.sun = match now {
                    Some(s) => s.direction(unit),
                    None => unit,
                };
            }
        }
    }

    // The zero-velocity curve exists only in the rotating frame, and that is
    // not an implementation limit: it lives in the synodic system's plane and
    // in an inertial frame would rotate along with the pair, showing a wall
    // where a moment ago there was none.
    if now.is_some() {
        if let (Some(mu), Some(c)) = (mass_ratio(snapshot), current_jacobi(snapshot)) {
            scene
                .polylines
                .extend(crate::zvc::curves(mu, c, frame_view::SYNODIC_SCALE_M));
        }
    }

    for vessel in &snapshot.vessels {
        let mut history: Vec<[f64; 3]> = Vec::new();
        let mut future: Vec<[f64; 3]> = Vec::new();

        // Every point takes **its own instant's** basis -- that is the whole
        // point of the rotating frame: the "now" basis would give a merely
        // rotated inertial trajectory. The sample's time sorts the point into
        // history or prediction, and after thinning it arrives with the point
        // -- there are no indices there any more.
        let place = |t: f64, point: [f64; 3], history: &mut Vec<_>, future: &mut Vec<_>| {
            if t <= snapshot.t {
                history.push(point);
            } else {
                // The prediction's first point repeats history's last,
                // otherwise there would be a gap between the two polylines one
                // integrator step wide -- i.e. hours of flight.
                if future.is_empty() {
                    if let Some(&last) = history.last() {
                        future.push(last);
                    }
                }
                future.push(point);
            }
        };

        match thin.as_deref_mut() {
            None => {
                for leg in &vessel.legs {
                    let normals = plane_normals(&leg.samples);
                    for (index, sample) in leg.samples.iter().enumerate() {
                        let point = match now {
                            Some(s) => match sample_frame(sample, normals[index], &s) {
                                Some(turned) => turned,
                                // A sample's basis cannot be degenerate if the
                                // "now" one exists -- but a silent NaN would
                                // cost more than this branch.
                                None => continue,
                            },
                            None => geocentric(sample),
                        };
                        place(sample.state.t, point, &mut history, &mut future);
                    }
                }
            }
            Some(thinning) => {
                let focal =
                    engine::lod::focal_px(engine::frame::FOV_Y, f64::from(thinning.height_px));
                for leg in &vessel.legs {
                    // The points arrive already transformed and thinned: all
                    // the frame does is sort them into two polylines.
                    for &(t, point) in thinning.cache.points(
                        leg,
                        frame,
                        now.as_ref(),
                        &scene.camera,
                        focal,
                        crate::thin::TOLERANCE_PX,
                    ) {
                        place(t, point, &mut history, &mut future);
                    }
                }
            }
        }

        push_line(&mut scene, history, palette::HISTORY.scene());
        push_line(&mut scene, future, palette::PREDICTION.scene());

        // Where the vessel is now. The position is interpolated (from the
        // snapshot) while Earth comes from the nearest sample: over one
        // integrator step it moves by fractions of a percent of the frame's
        // scale, and finding it more precisely would mean a fourth ephemeris
        // call per frame for something invisible.
        if let Some(earth) = earth_near(vessel, snapshot.t) {
            let position = [
                vessel.state.r.x - earth[0],
                vessel.state.r.y - earth[1],
                vessel.state.r.z - earth[2],
            ];
            // A marker is "now", so its basis is the current one.
            let position = match now {
                Some(s) => s.apply(position, moon_now),
                None => position,
            };
            push_marker(&mut scene, position);
        }
    }

    let mut speculative = Vec::new();
    for leg in preview {
        let normals = plane_normals(&leg.samples);
        for (index, sample) in leg.samples.iter().enumerate() {
            match now {
                Some(s) => {
                    if let Some(turned) = sample_frame(sample, normals[index], &s) {
                        speculative.push(turned);
                    }
                }
                None => speculative.push(geocentric(sample)),
            }
        }
    }
    // The preview is not thinned: the planner rebuilds it every time, so a
    // per-leg cache would have nothing to rest on here (ROADMAP.md, N2b).
    push_line(&mut scene, speculative, palette::PREVIEW.scene());

    scene
}

/// A sample's position relative to Earth **at that same instant** -- what
/// either of the two frames starts from.
pub fn geocentric(sample: &crate::leg::Sample) -> [f64; 3] {
    [
        sample.state.r.x - sample.earth[0],
        sample.state.r.y - sample.earth[1],
        sample.state.r.z - sample.earth[2],
    ]
}

/// A sample's point in the synodic frame of its own instant, at `now`'s
/// scale.
pub fn sample_frame(
    sample: &crate::leg::Sample,
    normal: [f64; 3],
    now: &Synodic,
) -> Option<[f64; 3]> {
    let d = [
        sample.moon[0] - sample.earth[0],
        sample.moon[1] - sample.earth[1],
        sample.moon[2] - sample.earth[2],
    ];
    let basis = now.with_line(d, normal)?;
    Some(basis.apply(geocentric(sample), d))
}

/// Normals of the instantaneous Earth-Moon plane over one leg's samples.
///
/// Public for the sake of a test comparing the **transform** against the
/// engine's formula (`engine::trajectory::rotating_position`, itself compared
/// against the C oracle): if the test computed the normals its own way it
/// would compare two different planes and blame the discrepancy on them.
///
/// A central difference rather than the Moon's analytic velocity: the sample
/// does not have it and will not -- 104 bytes per sample is already debt D7,
/// and adding 24 more for appearance is not worth it. F6 measured that at a
/// step of about 2.7 h a central difference gives a 3.5e-7 discrepancy against
/// the C oracle; at a leg's ends the difference is one-sided.
pub fn plane_normals(samples: &[crate::leg::Sample]) -> Vec<[f64; 3]> {
    let line = |s: &crate::leg::Sample| {
        [
            s.moon[0] - s.earth[0],
            s.moon[1] - s.earth[1],
            s.moon[2] - s.earth[2],
        ]
    };
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };

    (0..samples.len())
        .map(|i| {
            let before = line(&samples[i.saturating_sub(1)]);
            let after = line(&samples[(i + 1).min(samples.len() - 1)]);
            let rate = [
                after[0] - before[0],
                after[1] - before[1],
                after[2] - before[2],
            ];
            cross(line(&samples[i]), rate)
        })
        .collect()
}

/// The pair's mass fraction from the asset -- the definition of the system
/// both the curve and the Lagrange points live in. `/core` computes it, not
/// Rust (U6b2).
fn mass_ratio(snapshot: &WorldSnapshot) -> Option<f64> {
    let earth = snapshot.bodies.iter().find(|b| b.body == EARTH)?;
    let moon = snapshot.bodies.iter().find(|b| b.body == MOON)?;
    Some(core_rs::cr3bp_mu(earth.mu, moon.mu))
}

/// The vessel's `C`, the one the curve is drawn for.
///
/// The first vessel rather than all of them: there is one curve per frame, and
/// ten translucent curves over each other would say nothing to anyone. The
/// game has one vessel for now; when there are more, the curve will belong to
/// the **selected** one -- that is an interface choice, and there is nothing
/// here to make it from in advance.
fn current_jacobi(snapshot: &WorldSnapshot) -> Option<f64> {
    snapshot.vessels.first()?.jacobi
}

/// The Moon relative to Earth at the snapshot's instant.
fn moon_local(snapshot: &WorldSnapshot) -> Option<[f64; 3]> {
    let earth = snapshot.bodies.iter().find(|b| b.body == EARTH)?;
    let moon = snapshot.bodies.iter().find(|b| b.body == MOON)?;
    Some([
        moon.position[0] - earth.position[0],
        moon.position[1] - earth.position[1],
        moon.position[2] - earth.position[2],
    ])
}

/// The synodic basis at the snapshot's instant -- the one bodies and markers
/// stand in.
///
/// Here the normal comes from **velocities** (`d x d_dot`) rather than from a
/// difference of samples: the snapshot has them and has no neighbouring
/// instant. Both routes give the same plane -- that is what makes the frame
/// whole.
fn synodic_now(snapshot: &WorldSnapshot) -> Option<Synodic> {
    let earth = snapshot.bodies.iter().find(|b| b.body == EARTH)?;
    let moon = snapshot.bodies.iter().find(|b| b.body == MOON)?;

    let d = [
        moon.position[0] - earth.position[0],
        moon.position[1] - earth.position[1],
        moon.position[2] - earth.position[2],
    ];
    let rate = [
        moon.velocity[0] - earth.velocity[0],
        moon.velocity[1] - earth.velocity[1],
        moon.velocity[2] - earth.velocity[2],
    ];
    let normal = [
        d[1] * rate[2] - d[2] * rate[1],
        d[2] * rate[0] - d[0] * rate[2],
        d[0] * rate[1] - d[1] * rate[0],
    ];

    let total = earth.mu + moon.mu;
    if total <= 0.0 {
        return None;
    }
    // The scale is constant (`SYNODIC_SCALE_M`) rather than the current
    // distance: it is that constancy which holds the Moon still between
    // frames.
    Synodic::new(d, normal, frame_view::SYNODIC_SCALE_M, moon.mu / total)
}

fn push_line(scene: &mut Scene, points: Vec<[f64; 3]>, colour: [f32; 4]) {
    // A polyline of one vertex is not a polyline. The engine would skip such a
    // case itself, but an empty `Polyline` in the scene would make a reader
    // guess why it is there.
    if points.len() >= 2 {
        scene.polylines.push(Polyline { points, colour });
    }
}

/// Earth's position in the sample nearest to `t`.
fn earth_near(vessel: &crate::snapshot::VesselSnapshot, t: f64) -> Option<[f64; 3]> {
    let mut best: Option<(f64, [f64; 3])> = None;

    for leg in &vessel.legs {
        for sample in &leg.samples {
            let gap = (sample.state.t - t).abs();
            if best.is_none_or(|(was, _)| gap < was) {
                best = Some((gap, sample.earth));
            }
        }
    }

    best.map(|(_, earth)| earth)
}

/// A body's colour while the asset has none (T1).
///
/// The Moon grey, Earth blue -- precisely because they are, and precisely as
/// far as a flat colour can say so. The numbers here are temporary by
/// construction: in T3 colour arrives as tiles, and this field remains the
/// colour of a body **without** tiles.
///
/// An unknown body gets grey rather than black: a black planet in frame reads
/// as a hole in the sky, i.e. as a render bug rather than as "the asset did
/// not say".
fn body_colour(body: i32) -> [f32; 4] {
    match body {
        EARTH => [0.2, 0.6, 0.9, 1.0],
        MOON => [0.55, 0.54, 0.52, 1.0],
        _ => [0.5, 0.5, 0.5, 1.0],
    }
}

/// A cross of three segments at a point.
///
/// Three polylines rather than a point: `PointList` would give one invisible
/// pixel, and the engine has no marker primitive of its own and need not gain
/// one for this.
fn push_marker(scene: &mut Scene, position: [f64; 3]) {
    let camera = scene.camera.position();
    let distance = {
        let d = [
            position[0] - camera[0],
            position[1] - camera[1],
            position[2] - camera[2],
        ];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    };
    let arm = distance * MARKER_FRACTION;

    for axis in 0..3 {
        let mut a = position;
        let mut b = position;
        a[axis] -= arm;
        b[axis] += arm;
        scene.polylines.push(Polyline {
            points: vec![a, b],
            colour: palette::VESSEL.scene(),
        });
    }
}

/// Enables terrain on the body it was loaded for (D12).
///
/// ## Why its own call rather than a `build` parameter
///
/// The **frame** issues a `TerrainId` (`Frame::load_terrain`, R5c), while
/// `view::build` knows nothing of the frame and should not: it turns a
/// snapshot into a scene, and that is a pure function of game state. Threading
/// a handle it does not understand through it would make twenty existing calls
/// longer for the sake of something that concerns two.
///
/// So terrain is what is **added to a finished scene** by whoever knows what
/// was loaded. The wording is honest about the future too: a body may have
/// terrain on one machine and not on another if the adapter gave no bindless
/// (`Frame::load_terrain` refuses there), and the scene does not become a
/// different scene because of it.
///
/// ## How the body is found in the scene
///
/// `engine::scene::Body` carries no identifier -- the engine does not need one,
/// and letting it know about `EARTH`/`MOON` would teach the engine the game.
/// So the index is computed **by the same rule `build` placed the bodies
/// with**: the order of `snapshot.bodies`, skipping those without a radius.
/// One rule for two functions, which is exactly why it is written here rather
/// than guessed.
///
/// Stays silent if the body is not in the scene: a snapshot without the Moon
/// is a legitimate state, not grounds to panic.
pub fn attach_terrain(scene: &mut Scene, snapshot: &WorldSnapshot, body: i32, terrain: TerrainId) {
    let mut index = 0;
    for candidate in &snapshot.bodies {
        if candidate.radius_m <= 0.0 {
            continue;
        }
        if candidate.body == body {
            if index < scene.bodies.len() {
                scene.bodies[index].tiles = TileSet::Loaded(terrain);
            }
            return;
        }
        index += 1;
    }
}
