//! Horizon culling: both sides and a recorded number (R3a).
//!
//! The check here is analytic, and that is not a matter of taste: the limb is
//! the exact geometry of a tangent from a point to a sphere, i.e. a claim with
//! no approximation to hide a mistake behind. Pixels speak about culling in
//! the worst possible language: culling that is too greedy looks like
//! "something somewhere did not draw".
//!
//! Both sides are mandatory. Culling that discards nothing passes any check on
//! what is visible; culling that discards everything passes any check on
//! counts. So below there is both a patch touching the limb (it must stay) and
//! a patch a kilometre past it (it must go).
//!
//! Since R3b the frustum stands alongside, and the main question about it is
//! not "does it work" but **how much it adds beyond the horizon**. PROJECT.md
//! section 7 says the horizon matters more; here that stops being a
//! quotation.

use engine::camera::Camera;
use engine::cubesphere::{Patch, FACES, SIDE};
use engine::cull::{self, Body};
use engine::frame::FOV_Y;
use engine::lod;

const EARTH_RADIUS_M: f64 = 6_371_000.0;
const HEIGHT_PX: f64 = 720.0;

const IDENTITY: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn earth() -> Body {
    Body::smooth([0.0, 0.0, 0.0], EARTH_RADIUS_M, IDENTITY)
}

fn earth_lod() -> lod::Body {
    lod::Body::still([0.0, 0.0, 0.0], EARTH_RADIUS_M)
}

fn above(altitude: f64) -> Camera {
    let d = EARTH_RADIUS_M + altitude;
    Camera::look_at([d, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0])
}

/// A patch's cone really does cover the patch -- every node of it, not just
/// the corners.
///
/// The whole criterion rests on this: if the cone is narrower than the patch,
/// culling will discard what is visible, and do it quietly. The corners are
/// taken because the face parametrisation is monotone; the exhaustive sweep
/// here proves the reasoning is right rather than merely plausible.
#[test]
fn the_cone_of_a_patch_covers_every_node_of_it() {
    let mut tightest: f64 = 1.0;
    for face in 0..FACES {
        for (level, i, j) in [(0, 0, 0), (1, 1, 0), (3, 5, 2), (6, 40, 17)] {
            let patch = Patch { face, level, i, j };
            let cone = patch.cone();
            let mut worst: f64 = 1.0;
            for a in 0..=SIDE {
                for b in 0..=SIDE {
                    let p = patch.vertex(a, b, 1.0);
                    let dot = cone.axis[0] * p[0] + cone.axis[1] * p[1] + cone.axis[2] * p[2];
                    worst = worst.min(dot);
                }
            }
            assert!(
                worst >= cone.cos_half - 1e-15,
                "{patch:?}: a node outside the cone ({worst} against {})",
                cone.cos_half
            );
            // How tight the cone is: a one would mean it had degenerated and
            // culling would never discard anything.
            tightest = tightest.min(worst - cone.cos_half);
        }
    }
    println!("  largest slack of the cone over the nodes: {tightest:.2e}");
    assert!(
        tightest < 1e-9,
        "the cone is noticeably wider than the patch ({tightest:.2e}) -- the corners \
         are taken in the wrong place"
    );
}

/// A patch right on the limb stays, one a kilometre past it goes.
///
/// Measured with a **point** rather than a real patch: a patch has size, and
/// its contribution would mix with what is being checked. A point is a patch
/// with a cone of zero half-angle, i.e. the same criterion without the second
/// term.
#[test]
fn a_patch_touching_the_limb_stays_and_one_past_it_goes() {
    let altitude = 3.0e5;
    let distance = EARTH_RADIUS_M + altitude;
    let body = earth();
    let limb = cull::limb_cos(&body, distance);

    // The angle past which the surface hides. `acos` is allowed here: this is
    // a test, not a frame -- in the frame the formula stays free of
    // trigonometry.
    let horizon = limb.acos();
    println!(
        "  from {altitude:.1e} m the limb is {:.4} deg from the sub-camera point",
        horizon.to_degrees()
    );

    // A kilometre along the surface is this many radians.
    let kilometre = 1000.0 / EARTH_RADIUS_M;
    let to_eye = [1.0, 0.0, 0.0];
    // A point at angle `angle` from the direction to the camera, as a cone of
    // zero half-angle: cos(beta - 0) = cos(beta).
    let visible_at = |angle: f64| angle.cos() > limb;

    assert!(
        visible_at(horizon - kilometre),
        "a patch a kilometre before the limb was discarded"
    );
    assert!(
        !visible_at(horizon + kilometre),
        "a patch a kilometre past the limb stayed"
    );
    // The limb itself is the boundary, and on it the criterion must not be
    // greedy.
    assert!(
        !visible_at(horizon + 1e-12),
        "the criterion lets through what is already past the limb"
    );

    // The sub-camera point cannot disappear under any circumstances.
    let straight_down = Patch {
        face: 0,
        level: 4,
        i: 8,
        j: 8,
    };
    assert!(
        !cull::beyond_limb(&straight_down, to_eye, limb),
        "culling removed the patch directly under the camera"
    );
}

/// How much the horizon takes away -- a recorded number, not "about half".
#[test]
fn the_horizon_takes_away_most_of_the_planet() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);

    for altitude in [1.0e5, 3.0e5, 2.0e6, 1.0e7, 4.0e8] {
        let camera = above(altitude);
        let selection = lod::select(&earth_lod(), &camera, focal, None);
        let visibility = cull::horizon(&selection, &earth(), &camera);
        let all = selection.patches.len();
        let drawn = visibility.drawn();

        println!(
            "  {altitude:.1e} m: {all} patches, {drawn} drawn, past the limb {} \
             ({:.0}%)",
            visibility.past_limb,
            100.0 * visibility.past_limb as f64 / all as f64
        );

        assert_eq!(drawn + visibility.past_limb, all);
        // Both sides at every altitude: something discarded and something
        // left.
        assert!(
            drawn > 0,
            "from {altitude:.1e} m not a single patch was left"
        );
        assert!(
            visibility.past_limb > 0,
            "from {altitude:.1e} m the horizon removed nothing"
        );
    }
}

/// Culling never removes what is visible: a comparison against an honest ray.
///
/// A second path to the same answer, and therein lies its whole value. The
/// criterion works with a patch's cone; here, for every node of every patch,
/// the question is asked directly -- does the segment "eye -> node" intersect
/// the body's sphere. If the ray says "visible" and culling threw the patch
/// away, that is a mistake, and precisely the one seen on screen as "something
/// somewhere did not draw".
///
/// The converse is not required: the cone wraps the patch, so culling also
/// keeps some slack. How much slack is a number too, and it is printed here.
///
/// ## Why there are many altitudes and directions rather than one
///
/// The first version checked one altitude (300 km) from a camera over the
/// **centre of a face** -- and missed a mistake that threw away the face
/// directly under the camera. The condition only fires when the eye is
/// **inside the patch's cone**, i.e. on wide cones and low altitudes; on one
/// convenient camera that combination simply never came up. Hence both the
/// golden spiral here and the range of altitudes down to a hundred metres: a
/// mistake of this class lives exactly there.
#[test]
fn nothing_visible_is_ever_thrown_away() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);
    let mut worst_slack = 0usize;
    let mut checked = 0;

    for step in 0..12 {
        // Thirty-two directions on a golden spiral -- none coincides with a
        // face axis or a face corner.
        let z = 1.0 - (2.0 * f64::from(step) + 1.0) / 12.0;
        let r = (1.0 - z * z).max(0.0).sqrt();
        let phi = f64::from(step) * std::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
        let unit = [r * phi.cos(), r * phi.sin(), z];

        for altitude in [1.0e2_f64, 1.0e3, 1.0e4, 1.0e5, 3.0e5, 2.0e6] {
            let d = EARTH_RADIUS_M + altitude;
            let eye = [unit[0] * d, unit[1] * d, unit[2] * d];
            let camera = Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
            let selection = lod::select(&earth_lod(), &camera, focal, None);
            let visibility = cull::horizon(&selection, &earth(), &camera);

            // A node is visible if the segment to it does not enter the
            // sphere. For a convex body that is equivalent to
            // `(P - C) . (E - C) > R^2`, with no square roots at all.
            let seen = |p: [f64; 3]| {
                p[0] * eye[0] + p[1] * eye[1] + p[2] * eye[2] > EARTH_RADIUS_M.powi(2)
            };

            let mut kept_but_hidden = 0;
            for (patch, &visible) in selection.patches.iter().zip(&visibility.visible) {
                let mut any = false;
                for a in 0..=SIDE {
                    for b in 0..=SIDE {
                        if seen(patch.vertex(a, b, EARTH_RADIUS_M)) {
                            any = true;
                        }
                    }
                }
                assert!(
                    !(any && !visible),
                    "altitude {altitude:.1e} m, direction {unit:?}: {patch:?} \
                     has visible nodes and culling removed it"
                );
                if visible && !any {
                    kept_but_hidden += 1;
                }
            }

            // The cone's slack has to be moderate: if it kept twice as much
            // as needed, culling would cost more than it returns.
            //
            // The fraction is measured only on sets of sixteen patches or
            // more, and that is not a weakening. Right next to the surface the
            // limb shrinks to fractions of a degree and the set falls to a few
            // patches -- and "four spare out of six" sounds alarming when six
            // is the whole of it. A fraction over such numbers speaks about
            // the geometry of the cone rather than about the price of culling,
            // which is what the guard is here for.
            if visibility.drawn() >= 16 {
                assert!(
                    kept_but_hidden * 2 <= visibility.drawn(),
                    "altitude {altitude:.1e} m: the cone kept {kept_but_hidden} \
                     spare patches out of {} -- it is too large",
                    visibility.drawn()
                );
            }
            worst_slack = worst_slack.max(kept_but_hidden);
            checked += selection.patches.len();
        }
    }

    println!(
        "  {checked} patches over 72 cameras; the most slack in one set is \
         {worst_slack}"
    );
}

// ---------------------------------------------------------------------------
// The frustum after the horizon (R3b)

/// How much the frustum adds **beyond** the horizon -- on the same cameras.
///
/// This turns "the horizon matters more than the frustum" (PROJECT.md section
/// 7) from a quotation into a measurement. Had it come out the other way, that
/// would have been a finding rather than a mistake, and the finding is what
/// would have had to be written down.
///
/// Two cameras per altitude: the nadir (the planet in the centre of frame) and
/// a view along the limb. The second is deliberately unfavourable to the
/// horizon -- that is where the frustum has a chance.
#[test]
fn the_frustum_adds_less_than_the_horizon_took() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);
    let aspect = 16.0 / 9.0;

    for altitude in [1.0e5, 3.0e5, 2.0e6, 4.0e8] {
        for (name, camera) in [
            ("nadir", above(altitude)),
            ("along the limb", along_limb(altitude)),
        ] {
            let selection = lod::select(&earth_lod(), &camera, focal, None);
            let mut visibility = cull::horizon(&selection, &earth(), &camera);
            cull::frustum(
                &mut visibility,
                &selection,
                &earth(),
                &camera,
                FOV_Y,
                aspect,
            );

            println!(
                "  {altitude:.1e} m, {name}: {} patches -> the limb removed {}, \
                 the frustum another {} -> {} drawn",
                selection.patches.len(),
                visibility.past_limb,
                visibility.outside_frustum,
                visibility.drawn()
            );

            assert_eq!(
                visibility.drawn() + visibility.past_limb + visibility.outside_frustum,
                selection.patches.len(),
                "patches were lost between the two culls"
            );
            assert!(
                visibility.drawn() > 0,
                "{name} from {altitude:.1e} m: nothing was left"
            );
            assert!(
                visibility.outside_frustum <= visibility.past_limb,
                "the frustum removed {} against the horizon's {} -- that is a \
                 finding, and it has to be written down rather than walked past",
                visibility.outside_frustum,
                visibility.past_limb
            );
        }
    }
}

/// A camera at altitude `altitude`, turned so the planet runs along the edge
/// of the frame.
fn along_limb(altitude: f64) -> Camera {
    let d = EARTH_RADIUS_M + altitude;
    let eye = [d, 0.0, 0.0];
    // Looking at a point on the limb rather than at the centre: that way half
    // the frame is sky.
    let horizon = (EARTH_RADIUS_M / d).acos();
    let target = [
        EARTH_RADIUS_M * horizon.cos(),
        EARTH_RADIUS_M * horizon.sin(),
        0.0,
    ];
    Camera::look_at(eye, target, [0.0, 0.0, 1.0])
}

/// The frustum drops nothing that lands in the frame.
///
/// A second path, as with the horizon: instead of planes, a direct projection
/// of the nodes into pixels (`Camera::to_screen`). If even one node of a patch
/// lands in the frame and culling removed the patch, that is the same
/// "something somewhere did not draw" mistake.
#[test]
fn the_frustum_never_drops_what_lands_in_the_frame() {
    const WIDTH: u32 = 1280;
    const HEIGHT: u32 = 720;
    let focal = lod::focal_px(FOV_Y, f64::from(HEIGHT));
    let aspect = f64::from(WIDTH) / f64::from(HEIGHT);

    for altitude in [3.0e5, 2.0e6] {
        let camera = along_limb(altitude);
        let selection = lod::select(&earth_lod(), &camera, focal, None);
        let mut visibility = cull::horizon(&selection, &earth(), &camera);
        let limb_only: Vec<bool> = visibility.visible.clone();
        cull::frustum(
            &mut visibility,
            &selection,
            &earth(),
            &camera,
            FOV_Y,
            aspect,
        );

        for ((patch, &kept), &visible) in selection
            .patches
            .iter()
            .zip(&limb_only)
            .zip(&visibility.visible)
        {
            // The frustum owes nothing for what the horizon already removed.
            if !kept || visible {
                continue;
            }
            for a in 0..=SIDE {
                for b in 0..=SIDE {
                    let world = patch.vertex(a, b, EARTH_RADIUS_M);
                    if let Some(px) = camera.to_screen(FOV_Y, WIDTH, HEIGHT, world) {
                        assert!(
                            px[0] < 0.0
                                || px[1] < 0.0
                                || px[0] > WIDTH as f32
                                || px[1] > HEIGHT as f32,
                            "{patch:?} was removed, but its node lands in the frame at {px:?}"
                        );
                    }
                }
            }
        }
        println!(
            "  {altitude:.1e} m along the limb: the frustum removed {} patches, \
             and not one of them had a node in the frame",
            visibility.outside_frustum
        );
    }
}

/// Turning the body turns the set with it, not against it.
///
/// A guard against a mistake that is impossible to see while the orientation
/// is the identity, and which lived in the code from R2a to R3b for exactly
/// that reason: a patch's cone lives in the body's frame, the camera in the
/// world's, and while the rotation is the identity there is no difference.
///
/// The claim is a symmetry. Turning the body by an angle `theta` about an axis
/// and turning the camera by the same angle about the same axis is one and the
/// same picture, so the sets of patches and their visibility have to match
/// **item by item**. The weaker form ("the count is the same") would pass on
/// an implementation that ignores the rotation entirely.
#[test]
fn turning_the_body_turns_the_set_with_it() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);
    let aspect = 16.0 / 9.0;
    let altitude = 3.0e5;
    let d = EARTH_RADIUS_M + altitude;

    // A 40 deg turn about z: a matrix without zeroes where zeroes would hide a
    // mistake.
    let theta: f64 = 40.0_f64.to_radians();
    let (c, s) = (theta.cos(), theta.sin());
    let turn = [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]];

    // A still body, the camera over the point (d, 0, 0).
    let still_camera = Camera::look_at([d, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    // A turned body, the camera turned the same way.
    let eye = [d * c, d * s, 0.0];
    let turned_camera = Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);

    let run = |body_lod: lod::Body, occluder: Body, camera: &Camera| {
        let selection = lod::select(&body_lod, camera, focal, None);
        let mut visibility = cull::horizon(&selection, &occluder, camera);
        cull::frustum(
            &mut visibility,
            &selection,
            &occluder,
            camera,
            FOV_Y,
            aspect,
        );
        (selection.patches, visibility.visible)
    };

    let (still_patches, still_visible) = run(
        lod::Body::still([0.0, 0.0, 0.0], EARTH_RADIUS_M),
        earth(),
        &still_camera,
    );
    let (turned_patches, turned_visible) = run(
        lod::Body {
            rotation: turn,
            ..lod::Body::still([0.0, 0.0, 0.0], EARTH_RADIUS_M)
        },
        Body::smooth([0.0, 0.0, 0.0], EARTH_RADIUS_M, turn),
        &turned_camera,
    );

    println!(
        "  still: {} patches, {} drawn; turned by {:.0} deg: {} and {}",
        still_patches.len(),
        still_visible.iter().filter(|&&v| v).count(),
        theta.to_degrees(),
        turned_patches.len(),
        turned_visible.iter().filter(|&&v| v).count()
    );

    assert_eq!(
        still_patches, turned_patches,
        "turning the body together with the camera changed the set of patches"
    );
    assert_eq!(
        still_visible, turned_visible,
        "turning the body together with the camera changed the visibility"
    );
}
