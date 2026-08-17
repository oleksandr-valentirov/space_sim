//! Patch culling: what is not worth drawing at all (ROADMAP-PLANETS.md, R3).
//!
//! PROJECT.md section 7 says it outright: **horizon culling matters more than
//! frustum** -- half the planet is always past the limb, and however the camera
//! turns, that does not change. Frustum removes what is off to the side; the
//! horizon removes what is not there in principle.
//!
//! ## Why culling comes **after** level selection rather than inside it
//!
//! The temptation is obvious: do not descend into a patch that is past the limb
//! anyway, and save on the traversal itself. The price of that saving is
//! stitching. The mask (`lod::Selection::masks`) is computed from what is in the
//! set; a neighbour dropped during traversal turns into "that side is finer",
//! the edge stays unstitched, and a crack appears exactly at the limb -- that
//! is on the silhouette, where it shows best.
//!
//! So the set is built whole, and culling only **marks** what of it to draw.
//! Almost nothing of the saving is lost: the far hemisphere is ~2R away, and
//! the error criterion gives it the coarsest levels by itself -- a handful of
//! patches that cost nothing in traversal or in the cache.
//!
//! ## The limb criterion, without a single trigonometric call in the frame
//!
//! A point `p` (a unit direction from the body centre) hides past the limb when
//! `p . u <= R_min^2 / (R * d)`, where `u` is the direction to the camera and
//! `d` the distance to it. For a patch, though, the question is not about a
//! point but about the **patch point nearest to the camera**, and the patch cone
//! answers it ([`crate::cubesphere::Cone`]): if the angle between the cone axis
//! and `u` is `beta` and the half-spread is `alpha`, then the best the patch can
//! do is `cos(beta - alpha)`. That expands into
//! `cos(beta)cos(alpha) + sin(beta)sin(alpha)`, that is four multiplications per
//! patch.
//!
//! ## The minimum radius, not the mean
//!
//! The difference is not academic: by a mean radius a mountain past the limb
//! disappears, and it is visible. The numbers here differ and get confused --
//! the asset's mean radius (`eph_body_radius`), the harmonics' reference radius
//! and a tile's lowest point; for the Moon the first two already differ by
//! **470 m** (K5e).
//!
//! **While there are no tiles, the minimum radius comes from the body.** When
//! R5 brings them, it must come **from the tile** -- min/max heights are already
//! there. Written here rather than "remembered some day":
//! [`Body::occluder_radius_m`] exists as its own field exactly for that day.
//!
//! ## Frustum after the horizon, precisely because the cheaper one goes first
//!
//! The order is not aesthetics: the horizon removes half the planet with one dot
//! product, the frustum needs a bounding sphere in camera space and four planes.
//! It is cheaper to throw away what is not there first than to ask an expensive
//! criterion about what is already gone. How much the frustum adds **on top of**
//! the horizon is its own field [`Visibility::outside_frustum`], because
//! "matters more" without a number stays a quotation.
//!
//! ## The body's rotation enters culling, and that is not a detail
//!
//! A patch cone lives in the **body's** frame, while the camera lives in the
//! world's. As long as the orientation is identity there is no difference, and
//! that is exactly why an error of this kind is not found when it is made. So
//! the direction to the camera is taken into body space here by the transposed
//! rotation matrix -- once per body, not per patch.

use crate::camera::Camera;
use crate::cubesphere::Patch;
use crate::lod::Selection;

/// An occluding body: centre, surface radius and occluder radius.
///
/// Two radii rather than one, and that is not a reserve for the future but two
/// things that already differ in meaning: the first says **where the patch
/// vertices lie**, the second **what actually blocks the view**. Today they
/// coincide; when terrain appears the second becomes the tile's lowest point,
/// and not one formula below will change because of it.
#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub centre: [f64; 3],
    pub radius_m: f64,
    pub occluder_radius_m: f64,
    /// Rotation from body space to world space -- the same one the frame turns
    /// patch origins with.
    pub rotation: [[f64; 3]; 3],
}

impl Body {
    /// A body without terrain: its own sphere is exactly what occludes.
    pub fn smooth(centre: [f64; 3], radius_m: f64, rotation: [[f64; 3]; 3]) -> Body {
        Body {
            centre,
            radius_m,
            occluder_radius_m: radius_m,
            rotation,
        }
    }

    /// A direction from the world into body space. The rotation is orthogonal,
    /// so its inverse is its transpose, with no inversion at all.
    fn in_body(&self, world: [f64; 3]) -> [f64; 3] {
        let mut out = [0.0; 3];
        for (k, value) in out.iter_mut().enumerate() {
            *value = self.rotation[0][k] * world[0]
                + self.rotation[1][k] * world[1]
                + self.rotation[2][k] * world[2];
        }
        out
    }

    /// A body point into the world.
    fn in_world(&self, local: [f64; 3]) -> [f64; 3] {
        let mut out = self.centre;
        for (k, value) in out.iter_mut().enumerate() {
            *value += self.rotation[k][0] * local[0]
                + self.rotation[k][1] * local[1]
                + self.rotation[k][2] * local[2];
        }
        out
    }
}

/// What of the set is drawn, and why the rest is not.
pub struct Visibility {
    /// Parallel to `Selection::patches`.
    pub visible: Vec<bool>,
    /// How many patches the horizon removed.
    pub past_limb: usize,
    /// How many the frustum added **on top of** the horizon -- those the
    /// horizon kept and the frame bounds removed.
    pub outside_frustum: usize,
}

impl Visibility {
    pub fn drawn(&self) -> usize {
        self.visible.iter().filter(|&&v| v).count()
    }
}

/// The cosine of the angle past which a surface point hides behind the limb.
///
/// Above one means nothing is visible: the camera is inside the body. Below -1
/// means everything is visible, which happens when the occluder is smaller than
/// the surface.
pub fn limb_cos(body: &Body, distance_m: f64) -> f64 {
    let r = body.occluder_radius_m;
    r * r / (body.radius_m * distance_m.max(1.0))
}

/// Whether the patch is hidden past the limb.
///
/// `to_eye` is the unit direction from the body centre to the camera.
pub fn beyond_limb(patch: &Patch, to_eye: [f64; 3], limb_cos: f64) -> bool {
    let cone = patch.cone();
    let cos_beta = (cone.axis[0] * to_eye[0] + cone.axis[1] * to_eye[1] + cone.axis[2] * to_eye[2])
        .clamp(-1.0, 1.0);
    // **The eye inside the cone is a special case, and it is exactly where
    // this once broke.** The formula below gives `cos(beta - alpha)`, and cosine
    // is even, so at `beta < alpha` it returns `cos(alpha - beta)` -- the cosine
    // of the angle to the **edge** of the cap rather than to its nearest point.
    // The nearest point then lies exactly under the eye, that is at angle zero,
    // and the right answer is one.
    //
    // The price of the mistake was not cosmetic: a level-zero face has
    // `alpha = 54.7 deg`, so `cos(alpha - beta) <= R/d` held for everything
    // closer than `1.7 R` -- and culling threw away the face **the camera stands
    // over**. In the frame this looked like a body cut off along a cube-face
    // edge, and it appeared in bands, because the threshold creeps with
    // altitude. The comment in this very place claimed the opposite ("the
    // formula works at beta < alpha too"), which is why the bug lived from
    // R3a.
    if cos_beta >= cone.cos_half {
        return false;
    }
    let sin_beta = (1.0 - cos_beta * cos_beta).max(0.0).sqrt();
    // cos(beta - alpha) -- the best the patch can do when the eye is outside
    // the cap.
    let best = cos_beta * cone.cos_half + sin_beta * cone.sin_half;
    best <= limb_cos
}

/// Horizon culling for the whole set.
pub fn horizon(selection: &Selection, body: &Body, camera: &Camera) -> Visibility {
    let eye = camera.position();
    let d = [
        eye[0] - body.centre[0],
        eye[1] - body.centre[1],
        eye[2] - body.centre[2],
    ];
    let distance = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let to_eye = if distance > 0.0 {
        [d[0] / distance, d[1] / distance, d[2] / distance]
    } else {
        [0.0, 0.0, 1.0]
    };
    let limb = limb_cos(body, distance);

    let to_eye = body.in_body(to_eye);

    let mut out = Visibility {
        visible: Vec::with_capacity(selection.patches.len()),
        past_limb: 0,
        outside_frustum: 0,
    };
    for patch in &selection.patches {
        let hidden = beyond_limb(patch, to_eye, limb);
        if hidden {
            out.past_limb += 1;
        }
        out.visible.push(!hidden);
    }
    out
}

/// The patch bounding sphere in world coordinates.
///
/// The centre is the surface point on the cone axis, the radius is the chord to
/// the farthest node: `R*|p - axis| = R*sqrt(2 - 2cos(alpha))`. Not a generous
/// upper estimate but the exact bound for the same cone the horizon uses -- and
/// that is why the two criteria cannot disagree about what a patch is.
fn bounding_sphere(patch: &Patch, body: &Body) -> ([f64; 3], f64) {
    let cone = patch.cone();
    let centre = body.in_world([
        cone.axis[0] * body.radius_m,
        cone.axis[1] * body.radius_m,
        cone.axis[2] * body.radius_m,
    ]);
    let radius = body.radius_m * (2.0 - 2.0 * cone.cos_half).max(0.0).sqrt();
    (centre, radius)
}

/// Frustum culling -- **on top of** the finished horizon culling.
///
/// Four side planes and no near or far one: the far plane is infinite
/// (reversed-Z, F3), and the near one is orders of magnitude smaller than any
/// patch. A patch behind the camera is rejected by those same four planes -- all
/// at once, because behind the camera they converge.
pub fn frustum(
    visibility: &mut Visibility,
    selection: &Selection,
    body: &Body,
    camera: &Camera,
    fov_y: f64,
    aspect: f64,
) {
    let t = (fov_y / 2.0).tan();
    let (tx, ty) = (aspect * t, t);
    // Normalising the planes, so the comparison is against a radius in metres
    // rather than a quantity with a hidden scale.
    let (nx, ny) = ((1.0 + tx * tx).sqrt(), (1.0 + ty * ty).sqrt());

    for (patch, visible) in selection.patches.iter().zip(visibility.visible.iter_mut()) {
        if !*visible {
            continue;
        }
        let (centre, radius) = bounding_sphere(patch, body);
        let p = camera.relative64(centre);
        // The camera looks along -z, so `z` is negative in front, and
        // "outside" for each plane is a positive distance larger than the
        // radius.
        let outside = (p[0] + tx * p[2]) / nx > radius
            || (-p[0] + tx * p[2]) / nx > radius
            || (p[1] + ty * p[2]) / ny > radius
            || (-p[1] + ty * p[2]) / ny > radius;
        if outside {
            *visible = false;
            visibility.outside_frustum += 1;
        }
    }
}
