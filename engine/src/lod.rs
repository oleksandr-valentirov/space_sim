//! Patch level selection by screen-space error (ROADMAP-PLANETS.md, R2a).
//!
//! ## Why not distance to the camera
//!
//! Distance knows nothing about the field of view, the resolution or the body's
//! radius: the same distance to Earth and to an asteroid are different angles,
//! and the same angle at 1280 and at 3840 pixels is a different number of
//! pixels. So the criterion here is the **patch's geometric error projected onto
//! the screen**, and distance enters it as one factor rather than replacing it.
//!
//! ## What exactly is measured as the error
//!
//! A patch is a grid of flat quads stretched over a sphere. It departs from the
//! sphere most in the middle of a cell, and the size of that departure is the
//! **sagitta**: `radius - |midpoint of the chord|` on a unit sphere. There is no
//! trigonometry here and none is needed (`sqrt` is enough), and that is not
//! asceticism -- `cos(theta/2)` would have to be computed from an angle we do
//! not have, instead of from a midpoint we already do.
//!
//! **The worst cell of a patch is the one closer to the face's centre line.**
//! That is a property of the projection: on face `+Z` the point `(a, b, 1)`
//! after normalisation moves at a rate of `sqrt(b^2 + 1)/(a^2 + b^2 + 1)` in
//! `a`, and as `|b|` grows the denominator grows faster than the numerator. So
//! the walk goes along the patch node nearest the face centre rather than over
//! all `SIDE^2` cells: 64 computations per patch instead of 1024, and it is the
//! same answer, not an approximation. Verified by brute force in a test --
//! otherwise it would be an argument nobody can refute.
//!
//! ## The ceiling is named by a number
//!
//! [`MAX_LEVEL`] exists because the criterion has no floor of its own: a camera
//! touching the surface would demand infinite subdivision. A ceiling cuts
//! quality **silently**, so [`select`] returns reaching it as a separate field
//! rather than hiding it in the chosen set.

use crate::camera::Camera;
use crate::cubesphere::{EdgeMask, Patch, EDGES, FACES, SIDE};
use crate::tiles::Terrain;
use std::collections::HashSet;

/// How many pixels of geometric error are tolerated.
///
/// One pixel rather than "smooth enough": an error smaller than a pixel cannot
/// move a single rasterised fragment, that is the threshold past which finer
/// patches do not change the frame at all.
pub const TOLERANCE_PX: f64 = 1.0;

/// The floor of subdivision. On Earth that is a cell of ~79 m and a sagitta of
/// ~1.2e-4 m -- below a pixel from any distance the camera is allowed at all.
///
/// The number is not a reserve for the future: terrain (R5) arrives as tiles and
/// has a grid of its own, so subdividing a smooth sphere deeper has nothing to
/// show.
pub const MAX_LEVEL: u32 = 12;

/// What was selected this frame.
pub struct Selection {
    /// Patches in traversal order -- from face 0 onwards, children by `(i, j)`.
    ///
    /// The order is fixed by construction (the recursion is deterministic), and
    /// that is not cosmetic: the patch set goes into a GPU buffer, and a buffer
    /// that reshuffles the same things every frame cannot be compared with the
    /// previous frame.
    pub patches: Vec<Patch>,
    /// Each patch's edges across which the neighbour is **coarser** -- an array
    /// parallel to [`Self::patches`].
    ///
    /// Parallel rather than a field in `Patch`: a patch is topology (where it is
    /// on the cubesphere), while a mask is a property of the set it ended up in.
    /// The same patch in two sets has different masks and stays the same
    /// patch.
    pub masks: Vec<EdgeMask>,
    /// How many patches hit [`MAX_LEVEL`] instead of satisfying the criterion.
    ///
    /// A field of its own, because a ceiling that cuts quality quietly is the
    /// same mistake as no ceiling: it will be visible on screen while the cause
    /// has to be hunted in the code.
    pub clamped: usize,
    /// How many patches level balancing added -- beyond what the error
    /// criterion asked for.
    ///
    /// This is the price of the rule "neighbours differ by no more than one
    /// level", and it should not be paid blind: if the number ever exceeds the
    /// selection itself, the criterion is tearing neighbours apart too
    /// sharply.
    pub balanced: usize,
}

/// Pixels per radian at the centre of the frame.
///
/// Height rather than width: `fov_y` sets the vertical field of view, and the
/// horizontal one is derived from it through the aspect ratio.
pub fn focal_px(fov_y: f64, height_px: f64) -> f64 {
    height_px / 2.0 / (fov_y / 2.0).tan()
}

/// A patch's geometric error in metres -- the largest sagitta of its grid.
///
/// Independent of the camera: a property of the patch and the body's radius.
pub fn error_m(patch: &Patch, radius: f64) -> f64 {
    // The patch node nearest the face's centre line -- the cells are largest
    // there (see the module introduction). Face nodes are numbered 0 to `n`, the
    // centre being `n / 2`.
    let n = Patch::face_nodes(patch.level);
    let centre = n / 2;
    let local = |index: u32| -> usize {
        let first = index as usize * SIDE;
        centre.clamp(first, first + SIDE) - first
    };

    let a_at = local(patch.i);
    let b_at = local(patch.j);

    let mut worst: f64 = 0.0;
    for k in 0..SIDE {
        // A unit sphere: the radius is a factor at the end, and then this
        // quantity works for any body without a second walk.
        worst = worst.max(sagitta(patch, k, b_at, k + 1, b_at));
        worst = worst.max(sagitta(patch, a_at, k, a_at, k + 1));
    }
    worst * radius
}

/// A patch's longest cell in metres -- the chord between neighbouring nodes.
///
/// The same walk as in [`error_m`] and for the same reason: the largest cells
/// lie near the face's centre line.
pub fn cell_m(patch: &Patch, radius: f64) -> f64 {
    let n = Patch::face_nodes(patch.level);
    let centre = n / 2;
    let local = |index: u32| -> usize {
        let first = index as usize * SIDE;
        centre.clamp(first, first + SIDE) - first
    };
    let a_at = local(patch.i);
    let b_at = local(patch.j);

    let mut worst: f64 = 0.0;
    for k in 0..SIDE {
        worst = worst.max(chord(patch, k, b_at, k + 1, b_at));
        worst = worst.max(chord(patch, a_at, k, a_at, k + 1));
    }
    worst * radius
}

/// The edge length of one cell on a unit sphere.
fn chord(patch: &Patch, a0: usize, b0: usize, a1: usize, b1: usize) -> f64 {
    let p = patch.vertex(a0, b0, 1.0);
    let q = patch.vertex(a1, b1, 1.0);
    let d = [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// The sagitta of one cell on a unit sphere.
fn sagitta(patch: &Patch, a0: usize, b0: usize, a1: usize, b1: usize) -> f64 {
    let p = patch.vertex(a0, b0, 1.0);
    let q = patch.vertex(a1, b1, 1.0);
    let mid = [
        (p[0] + q[0]) / 2.0,
        (p[1] + q[1]) / 2.0,
        (p[2] + q[2]) / 2.0,
    ];
    let length = (mid[0] * mid[0] + mid[1] * mid[1] + mid[2] * mid[2]).sqrt();
    1.0 - length
}

/// A patch's error in pixels for this camera.
///
/// ## The distance is measured to the patch's cone, not to five of its nodes
///
/// The temptation to take the minimum over four corners and the centre looks
/// harmless while the camera stands **exactly above the face centre**: there the
/// central node is the nearest point and the answer is accidentally exact. Move
/// the camera twenty degrees aside and the nearest node ends up four times
/// farther than the surface underfoot: above the Moon from 170 km the sampling
/// said 758 km, the error fell from 1.9 pixels to 0.43, and the face was not
/// subdivided **at all** (D13). The worst part is that every level-selection
/// test placed the camera exactly above the face centre, so the flaw was
/// invisible by construction of the fixture.
///
/// So the distance is taken to the **spherical cap the patch is inscribed in**
/// -- [`Patch::cone`], the same cone `cull::beyond_limb` asks about the limb
/// with. The cap is larger than the patch, so the distance to it is a lower
/// estimate and the error an upper one: level selection can only err towards
/// excess subdivision.
///
/// Two cases, and the second is not an optimisation:
/// - **the eye above the cap** (`cos(beta) >= cos(alpha)`) -- the nearest point
///   is directly beneath it, `d - R`. The difference of two close numbers
///   directly, without the law of cosines, which at an altitude of 1 m would
///   lose twelve significant digits to cancellation;
/// - **the eye to the side** -- the nearest point is on the cap's edge, at angle
///   `beta - alpha`, and `cos(beta - alpha)` expands into the same four
///   multiplications as at the limb.
pub fn error_px(
    patch: &Patch,
    body: &Body,
    eye_in_body: [f64; 3],
    focal_px: f64,
    relief_slope: f64,
) -> f64 {
    let e = eye_in_body;
    let d = (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt();
    let r = body.radius_m;

    let cone = patch.cone();
    // The camera at the body's very centre -- a case the game does not allow,
    // but the number here must be finite: division by zero would give NaN, and a
    // NaN in a comparison would quietly pick the coarsest level.
    let cos_beta = if d > 0.0 {
        ((cone.axis[0] * e[0] + cone.axis[1] * e[1] + cone.axis[2] * e[2]) / d).clamp(-1.0, 1.0)
    } else {
        -1.0
    };

    let nearest = if cos_beta >= cone.cos_half {
        d - r
    } else {
        let sin_beta = (1.0 - cos_beta * cos_beta).max(0.0).sqrt();
        let cos_gap = cos_beta * cone.cos_half + sin_beta * cone.sin_half;
        (d * d + r * r - 2.0 * d * r * cos_gap).max(0.0).sqrt()
    };

    // **Two independent errors, and the sphere is only one of them** (R7c).
    //
    // The sagitta says how far a flat cell departs from the **sphere**, and up
    // close it is negligible: a sphere is locally flat. Measured -- a kilometre
    // above the Moon the criterion stops at a cell of 2665 m, that is 1662
    // pixels wide. Neither the procedural detail nor the DEM itself fits in such
    // a grid, its node there being 5330 m. So a criterion that looks only at the
    // sphere silently forbids terrain as such.
    //
    // The second error is **terrain**: a flat cell on a slope `s` departs from
    // the surface by an amount of order `s * L`. It falls with level
    // **linearly**, while the sagitta falls quadratically, so up close it is the
    // one that decides, and it is what drives subdivision to the levels where
    // detail is visible.
    //
    // The maximum rather than the sum: the sources are independent, the larger
    // of the two sets the level, and a sum would only double the answer where
    // they coincide.
    let sphere = error_m(patch, r);
    let relief = if relief_slope > 0.0 {
        relief_slope * cell_m(patch, r)
    } else {
        0.0
    };
    sphere.max(relief) / nearest.max(1.0) * focal_px
}

/// The body a level is selected for.
///
/// A struct of its own rather than [`crate::scene::Body`]: level selection needs
/// neither a tile set nor a quaternion -- it needs what the distance is computed
/// from.
///
/// ## The rotation is here after all, and it was not here once
///
/// At first it seems rotation does not affect level selection: a sphere is the
/// same from every side, and the **quality** of the set really does not change.
/// Something else changes -- **which patch** gets which level. A patch lives in
/// body space; when the body is rotated it stands in the world not where its own
/// coordinate puts it, and a fine region would be left where the camera is no
/// longer looking. This becomes noticeable with terrain (R5), but it was always
/// wrong.
///
/// Instead of rotating every vertex, the **eye** is taken into body space --
/// once per body instead of a thousand times per patch. The rotation is
/// orthogonal, so its inverse is its transpose.
#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub centre: [f64; 3],
    pub radius_m: f64,
    /// Rotation from body space into world space.
    pub rotation: [[f64; 3]; 3],
    /// Selection does not descend below this level.
    ///
    /// Not the same as [`MAX_LEVEL`]: that ceiling is about the criterion's
    /// arithmetic, this one about data. There is no point subdividing a tiled
    /// body deeper than its tile pyramid (R5c), and the difference must be
    /// visible in the type rather than hidden in the frame.
    pub max_level: u32,
}

impl Body {
    /// A body that does not rotate.
    pub fn still(centre: [f64; 3], radius_m: f64) -> Body {
        Body {
            centre,
            radius_m,
            rotation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            max_level: MAX_LEVEL,
        }
    }

    /// The camera position in body space: subtract the centre first, then
    /// rotate back.
    pub fn eye_in_body(&self, eye: [f64; 3]) -> [f64; 3] {
        let d = [
            eye[0] - self.centre[0],
            eye[1] - self.centre[1],
            eye[2] - self.centre[2],
        ];
        let mut out = [0.0; 3];
        for (k, value) in out.iter_mut().enumerate() {
            *value = self.rotation[0][k] * d[0]
                + self.rotation[1][k] * d[1]
                + self.rotation[2][k] * d[2];
        }
        out
    }
}

/// The patch set for this camera and this body -- selected, balanced, stitched.
///
/// Three actions rather than one, and there is deliberately no split point
/// between them from outside: an unbalanced set is good for nothing but cracks,
/// and masks without balancing would have to be computed for a two-level
/// difference, which stitching cannot do. Whoever asks for a set asks for a set
/// that draws.
pub fn select(body: &Body, camera: &Camera, focal_px: f64, terrain: Option<&Terrain>) -> Selection {
    let mut out = Selection {
        patches: Vec::new(),
        masks: Vec::new(),
        clamped: 0,
        balanced: 0,
    };
    let eye = body.eye_in_body(camera.position());
    for face in 0..FACES {
        subdivide(
            Patch {
                face,
                level: 0,
                i: 0,
                j: 0,
            },
            body,
            eye,
            focal_px,
            terrain,
            &mut out,
        );
    }
    balance(&mut out);
    out.masks = stitching(&out.patches);
    out
}

/// Bring the set up to the rule "neighbours differ by no more than one level".
///
/// ## Why one level exactly, and why that is enough
///
/// Stitching (`cubesphere::indices`) can drop every second node of an edge --
/// that is exactly one level of difference. A difference of two would mean
/// keeping every fourth node of our edge, and there would be not sixteen index
/// sets but countless ones. So the rule is not about a pretty mesh but about how
/// many distinct index sets exist in advance.
///
/// Balancing **only refines**: a coarser neighbour is subdivided until the
/// difference falls to one. Coarsening the fine side is not allowed -- the error
/// criterion asked for that level, and quietly giving it back would be lying
/// about quality.
fn balance(selection: &mut Selection) {
    let before = selection.patches.len();
    let mut leaves: HashSet<Patch> = selection.patches.iter().copied().collect();
    // The queue holds patches whose neighbours have not been checked yet. The
    // children of a just-subdivided patch land here too: the subdivision may
    // have made them too fine for THEIR own neighbours, and the wave is entitled
    // to keep going.
    let mut queue: Vec<Patch> = selection.patches.clone();

    while let Some(patch) = queue.pop() {
        // The patch may have been subdivided while it waited in the queue.
        if patch.level < 2 || !leaves.contains(&patch) {
            continue;
        }
        for edge in EDGES {
            // **A loop, not a single check.** One subdivision reduces the
            // difference by one level, and the difference can be larger: with
            // the sagitta criterion it changes smoothly, so a three-level
            // difference on neighbouring patches simply never happened -- while
            // with terrain (R7c) the slope between neighbours jumps, and it
            // appeared in the very first frame. The subdivided side then stayed
            // two levels coarser, `stitching` saw a difference of 2 and tripped
            // the `debug_assert`. The children of the subdivided patch do queue
            // themselves, but that does not help them: their neighbour is FINER
            // than they are, so from their side all is well, and the side that
            // started must be the one to ask.
            loop {
                let cell = patch.neighbour(edge).patch;
                let Some(coarse) = covering(&leaves, cell) else {
                    // The other side is finer than us -- its business, not
                    // ours.
                    break;
                };
                if patch.level - coarse.level < 2 {
                    break;
                }
                leaves.remove(&coarse);
                for child in coarse.children() {
                    leaves.insert(child);
                    queue.push(child);
                }
            }
        }
    }

    // The order is restored by traversal rather than by sorting: it must be the
    // same one `subdivide` produces, otherwise the GPU buffer reshuffles the
    // same things every frame and stops being comparable with the previous
    // frame.
    selection.patches.clear();
    for face in 0..FACES {
        collect(
            Patch {
                face,
                level: 0,
                i: 0,
                j: 0,
            },
            &leaves,
            &mut selection.patches,
        );
    }
    selection.balanced = selection.patches.len() - before;
}

/// The set's leaf covering this cell: the cell itself or one of its ancestors.
///
/// `None` means there is nothing to cover it -- that is, the set on that side is
/// **finer** than the cell, and the question must be asked from the other
/// side.
fn covering(leaves: &HashSet<Patch>, cell: Patch) -> Option<Patch> {
    let mut cell = cell;
    loop {
        if leaves.contains(&cell) {
            return Some(cell);
        }
        cell = cell.parent()?;
    }
}

fn collect(patch: Patch, leaves: &HashSet<Patch>, out: &mut Vec<Patch>) {
    if leaves.contains(&patch) {
        out.push(patch);
        return;
    }
    assert!(
        patch.level < MAX_LEVEL,
        "the set does not cover the face: {patch:?} has no leaf at all"
    );
    for child in patch.children() {
        collect(child, leaves, out);
    }
}

/// Each patch's stitching mask: the edges across which the neighbour is
/// coarser.
fn stitching(patches: &[Patch]) -> Vec<EdgeMask> {
    let leaves: HashSet<Patch> = patches.iter().copied().collect();
    patches
        .iter()
        .map(|patch| {
            let mut mask = 0;
            for edge in EDGES {
                let cell = patch.neighbour(edge).patch;
                if let Some(other) = covering(&leaves, cell) {
                    if other.level < patch.level {
                        debug_assert_eq!(
                            other.level + 1,
                            patch.level,
                            "balancing missed a level difference"
                        );
                        mask |= edge.bit();
                    }
                }
            }
            mask
        })
        .collect()
}

fn subdivide(
    patch: Patch,
    body: &Body,
    eye: [f64; 3],
    focal: f64,
    terrain: Option<&Terrain>,
    out: &mut Selection,
) {
    // The slope is **local**, not one number per body: flat ground must not pay
    // vertices for the fact that there are mountains somewhere on the body.
    // Taken at the patch centre -- one sample per patch, and the same on both
    // sides of a shared edge, because `slope_at` is bitwise identical there
    // (R7c).
    let relief_slope = match terrain {
        Some(terrain) => terrain.slope_at(&patch, SIDE / 2, SIDE / 2),
        None => 0.0,
    };
    if error_px(&patch, body, eye, focal, relief_slope) <= TOLERANCE_PX {
        out.patches.push(patch);
        return;
    }
    if patch.level >= body.max_level.min(MAX_LEVEL) {
        out.patches.push(patch);
        out.clamped += 1;
        return;
    }

    for (di, dj) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        subdivide(
            Patch {
                face: patch.face,
                level: patch.level + 1,
                i: patch.i * 2 + di,
                j: patch.j * 2 + dj,
            },
            body,
            eye,
            focal,
            terrain,
            &mut *out,
        );
    }
}

/// The level of the patch covering node `(u, v)` of face `face` in the selected
/// set.
///
/// Needed for monotonicity: sets at two camera positions consist of different
/// patches, so there is nothing to compare piece by piece. What can be compared
/// is a **surface point** -- which level covered it here and which there.
pub fn level_at(selection: &Selection, face: usize, u: f64, v: f64) -> Option<u32> {
    selection
        .patches
        .iter()
        .find(|p| {
            if p.face != face {
                return false;
            }
            let side = f64::from(1u32 << p.level);
            let (i, j) = (f64::from(p.i), f64::from(p.j));
            u >= i / side && u <= (i + 1.0) / side && v >= j / side && v <= (j + 1.0) / side
        })
        .map(|p| p.level)
}

/// The vertex count a set costs.
///
/// The one figure everything else is compared against: a patch is `(SIDE + 1)^2`
/// vertices regardless of level.
pub fn vertex_count(selection: &Selection) -> usize {
    selection.patches.len() * (SIDE + 1) * (SIDE + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cheap walk gives the same answer as brute force over all cells.
    ///
    /// This is the check of the argument in the module introduction: if the
    /// worst cell did not lie on the face's centre line, brute force would find
    /// more.
    #[test]
    fn the_cheap_walk_finds_the_worst_cell() {
        for patch in [
            Patch {
                face: 0,
                level: 0,
                i: 0,
                j: 0,
            },
            Patch {
                face: 3,
                level: 2,
                i: 0,
                j: 0,
            },
            Patch {
                face: 5,
                level: 2,
                i: 3,
                j: 1,
            },
            Patch {
                face: 1,
                level: 4,
                i: 15,
                j: 15,
            },
        ] {
            // Both bounds inclusive: the worst cell likes to lie on the patch
            // edge nearest the face centre, and `0..SIDE` in both loops skips
            // exactly that edge. The first version of this brute force did skip
            // it -- and "the cheap walk found more than brute force" was what
            // pointed at it.
            let mut brute: f64 = 0.0;
            for a in 0..SIDE {
                for b in 0..=SIDE {
                    brute = brute.max(sagitta(&patch, a, b, a + 1, b));
                }
            }
            for a in 0..=SIDE {
                for b in 0..SIDE {
                    brute = brute.max(sagitta(&patch, a, b, a, b + 1));
                }
            }
            let cheap = error_m(&patch, 1.0);
            println!(
                "  {:?} level {}: walk {:.6e}, brute force {:.6e}",
                patch.face, patch.level, cheap, brute
            );
            assert!(
                (cheap - brute).abs() <= brute * 1e-12,
                "the walk gave {cheap:.6e}, brute force {brute:.6e}"
            );
        }
    }

    /// The error falls fourfold per level -- otherwise the criterion would make
    /// no sense.
    ///
    /// The sagitta is proportional to the square of the step, and the step
    /// halves, so a factor of 4 is not an observation but what must be. A
    /// deviation would mean the warp spoiled the grid more deeply than R1a
    /// thinks.
    ///
    /// **The patches are taken near the face centre rather than at a corner, and
    /// that is not a detail.** A corner patch lies in a different place on the
    /// face at every level (its worst cell slides from the face centre towards
    /// the edge), so the factor there grows -- 4.00, 4.03, 4.11 -- and grows
    /// legitimately. The same piece of surface must be compared, otherwise what
    /// is measured is the warp rather than the subdivision.
    #[test]
    fn the_error_falls_fourfold_with_every_level() {
        const RADIUS: f64 = 6_371_000.0;
        let mut previous = None;
        for level in 0..8 {
            // The patch whose first node is exactly the face centre: the cells
            // are largest there, and at every level they are in the same
            // place.
            let index = (1u32 << level) / 2;
            let patch = Patch {
                face: 0,
                level,
                i: index,
                j: index,
            };
            let e = error_m(&patch, RADIUS);
            if let Some(before) = previous {
                let factor: f64 = before / e;
                println!("  level {level}: {e:.4} m, factor {factor:.3}");
                assert!(
                    (3.9..4.1).contains(&factor),
                    "level {level} changed the error by a factor of {factor:.3}"
                );
            } else {
                println!("  level {level}: {e:.4} m");
            }
            previous = Some(e);
        }
    }
}
