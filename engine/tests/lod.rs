//! Patch level selection holds as a mechanism, not as an impression (R2a).
//!
//! Three claims, and none of them is about beauty: bringing the camera closer
//! never lowers a level, the same frame gives the same set, and two recorded
//! numbers say what all of it costs.
//!
//! No GPU is needed here: level selection is CPU geometry, and that is exactly
//! why it is checked before anything is drawn.

use engine::camera::Camera;
use engine::cubesphere::SIDE;
use engine::frame::FOV_Y;
use engine::lod::{self, Body};

const EARTH_RADIUS_M: f64 = 6_371_000.0;
const HEIGHT_PX: f64 = 720.0;

fn earth() -> Body {
    Body::still([0.0, 0.0, 0.0], EARTH_RADIUS_M)
}

/// A camera at altitude `altitude` over a point visible from the `+X` face.
fn above(altitude: f64) -> Camera {
    let d = EARTH_RADIUS_M + altitude;
    Camera::look_at([d, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0])
}

/// Bringing the camera closer never lowers the level of any surface point.
///
/// A hundred positions, not two: the criterion is a ratio of error to
/// distance, and between two points it is monotone almost always. A dip is
/// caught precisely where the set of patches **rebuilds**, and there are
/// several such places in the range.
///
/// What is compared is not patches but **surface points**: the sets at
/// different altitudes are made of different patches, so there is nothing to
/// match item by item.
#[test]
fn coming_closer_never_lowers_the_level_of_a_point() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);
    // From 4e8 m (the distance to the Moon) down to 100 km, logarithmically.
    let far = 4.0e8_f64;
    let near = 1.0e5_f64;
    let steps = 100;

    // Three points of a face: the centre, an edge and a corner -- they move to
    // a new level at different moments, and the corner is the hardest, because
    // three faces meet there.
    let probes = [(0.5, 0.5), (0.5, 0.98), (0.02, 0.02)];
    let mut previous = [0u32; 3];
    let mut raised = 0;

    for step in 0..=steps {
        let t = f64::from(step) / f64::from(steps);
        let altitude = far * (near / far).powf(t);
        let selection = lod::select(&earth(), &above(altitude), focal, None);

        for (index, &(u, v)) in probes.iter().enumerate() {
            let level = lod::level_at(&selection, 0, u, v)
                .unwrap_or_else(|| panic!("the point ({u}, {v}) is covered by no patch"));
            assert!(
                level >= previous[index],
                "at altitude {altitude:.3e} m the point ({u}, {v}) dropped from level {} to {level}",
                previous[index]
            );
            if level > previous[index] {
                raised += 1;
            }
            previous[index] = level;
        }
    }

    // Without this the test would be green on a criterion that always returns
    // zero.
    println!("  the level rose {raised} times over 101 positions");
    assert!(
        raised >= 6,
        "the level rose only {raised} times -- the criterion reacts to nothing"
    );
}

/// The same frame gives the same set -- and in the same order.
///
/// The order here is not pedantry: the set goes into a GPU buffer, and a list
/// reshuffled every frame would mean a full buffer reupload where in fact
/// nothing changed.
#[test]
fn the_same_camera_gives_the_same_patches() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);
    for altitude in [3.0e5, 2.0e6, 4.0e8] {
        let first = lod::select(&earth(), &above(altitude), focal, None);
        let second = lod::select(&earth(), &above(altitude), focal, None);
        assert_eq!(
            first.patches, second.patches,
            "at altitude {altitude:.1e} m two calls gave different sets"
        );
    }
}

/// Two numbers that hold the debt against reality: how many patches in low
/// orbit and how many from the distance to the Moon.
///
/// The upper bound here is not an accuracy oracle but a guard: a criterion
/// without a ceiling is capable of dividing the planet into a million patches,
/// and of doing it quietly.
#[test]
fn the_count_stays_where_it_can_be_afforded() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);

    for (name, altitude) in [("low orbit", 3.0e5), ("the distance to the Moon", 4.0e8)] {
        let selection = lod::select(&earth(), &above(altitude), focal, None);
        let levels: Vec<u32> = {
            let mut l: Vec<u32> = selection.patches.iter().map(|p| p.level).collect();
            l.sort_unstable();
            l.dedup();
            l
        };
        println!(
            "  {name} ({altitude:.1e} m): {} patches, levels {levels:?}, {} vertices, the ceiling caught {}",
            selection.patches.len(),
            lod::vertex_count(&selection),
            selection.clamped
        );

        assert_eq!(
            selection.clamped, 0,
            "{name}: the level ceiling fired where it should not have"
        );
        assert!(
            selection.patches.len() <= 4096,
            "{name}: {} patches -- that is no longer a frame but a brute force",
            selection.patches.len()
        );
        // Every patch is (SIDE + 1)^2 vertices, however many of them there are.
        assert_eq!(
            lod::vertex_count(&selection),
            selection.patches.len() * (SIDE + 1) * (SIDE + 1)
        );
    }
}

/// The criterion knows about resolution -- and that is what distance to the
/// camera cannot do.
///
/// The main substitution that could creep in here unnoticed: a selection that
/// looks only at distance passes every check above -- monotonicity,
/// determinism and count alike. What breaks it is exactly this: from the same
/// point, a taller frame has to yield more patches, because the pixel got
/// smaller.
///
/// **The set changes every other doubling, and that is arithmetic rather than
/// a flaw.** A level costs a fourfold smaller error while doubling the
/// resolution buys a twofold one -- so the accepted level lands at ~0.5 px each
/// time, the next doubling brings it to ~1.0 px and changes nothing, and the
/// one after that finally divides. Measured: 9 patches at 720, 21 at 1440 **and
/// at 2880**, 45 at 5760. Hence the strict inequality is demanded over the
/// whole range rather than between neighbours.
#[test]
fn a_taller_frame_needs_finer_patches_from_the_same_point() {
    let mut counts = Vec::new();
    for height in [720.0, 1440.0, 2880.0, 5760.0] {
        let selection = lod::select(&earth(), &above(3.0e5), lod::focal_px(FOV_Y, height), None);
        println!("  {height} px tall: {} patches", selection.patches.len());
        counts.push(selection.patches.len());
    }

    for pair in counts.windows(2) {
        assert!(
            pair[1] >= pair[0],
            "the taller frame gave fewer patches: {} against {}",
            pair[1],
            pair[0]
        );
    }
    assert!(
        counts[3] > counts[0],
        "a frame four times taller gave {} patches against {} -- the criterion \
         does not see the resolution",
        counts[3],
        counts[0]
    );
}

/// From the distance to the Moon the planet is no finer than six faces.
///
/// This is the lower side of the criterion: it has to not only add patches up
/// close but also **not add** them far away. Six patches are exactly the cube
/// faces, i.e. the selection divided nothing.
#[test]
fn from_far_away_the_planet_is_six_faces() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);
    let selection = lod::select(&earth(), &above(4.0e8), focal, None);
    assert_eq!(
        selection.patches.len(),
        6,
        "from far away {} patches were selected instead of six faces",
        selection.patches.len()
    );
}

/// **The selection's promise, checked where the camera does not stand over a
/// face centre** (D13).
///
/// Every check above puts the camera at `[d, 0, 0]` -- exactly over the centre
/// of the `+X` face. That is the most convenient point of all, and that is
/// precisely why the flaw lived there unnoticed: `error_px` measured the
/// distance to the **four corners and the centre** of a patch, and over a face
/// centre the central node **is** the nearest point, so the answer came out
/// exact by accident. Shift the camera by twenty degrees and the nearest node
/// ends up four times further away than the surface underfoot; in the game
/// this gave **six level-0 patches at any altitude**, up to the Moon vanishing
/// from the frame below 165 km.
///
/// So the oracle here is not "how many patches" but the promise itself: **each
/// patch's error, divided by the true distance to it, is below
/// [`lod::TOLERANCE_PX`]**. True -- i.e. computed by sweeping all `(SIDE+1)^2`
/// nodes of the patch, rather than by the same approximation that is being
/// checked. The sweep is expensive; it is meant to be -- this is a reference,
/// not a frame.
///
/// The directions are taken on a golden spiral: they coincide neither with a
/// face axis, nor with a face corner, nor with each other -- i.e. exactly what
/// the `above` fixture never gave.
#[test]
fn the_tolerance_holds_wherever_the_camera_sits() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);
    // The worst case observed is the Moon: it is smaller, so a face's error is
    // smaller too, and the criterion crept up on its disappearance more
    // quietly.
    const MOON_RADIUS_M: f64 = 1_737_530.0;

    let mut worst: f64 = 0.0;
    let mut checked = 0;

    for (name, radius) in [("Earth", EARTH_RADIUS_M), ("Moon", MOON_RADIUS_M)] {
        for altitude in [5.0e3_f64, 2.0e4, 1.0e5, 1.7e5, 3.0e5, 2.0e6] {
            let body = Body::still([0.0, 0.0, 0.0], radius);
            let mut peak: f64 = 0.0;
            let mut patches = 0;

            // Thirty-two directions on a golden spiral -- evenly over the
            // sphere and without any symmetry with the cube faces.
            for k in 0..32 {
                let z = 1.0 - (2.0 * f64::from(k) + 1.0) / 32.0;
                let r = (1.0 - z * z).max(0.0).sqrt();
                let phi = f64::from(k) * std::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
                let u = [r * phi.cos(), r * phi.sin(), z];

                let d = radius + altitude;
                let eye = [u[0] * d, u[1] * d, u[2] * d];
                let camera = Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
                let selection = lod::select(&body, &camera, focal, None);
                patches = patches.max(selection.patches.len());

                for patch in &selection.patches {
                    if patch.level >= lod::MAX_LEVEL {
                        // The ceiling cuts quality deliberately, and
                        // `Selection::clamped` has already said so.
                        continue;
                    }
                    // The reference distance: a sweep over every node of the
                    // patch. More expensive than any approximation by exactly
                    // as much as it takes not to repeat the approximation
                    // being checked.
                    let mut nearest = f64::INFINITY;
                    for a in 0..=SIDE {
                        for b in 0..=SIDE {
                            let p = patch.vertex(a, b, radius);
                            let v = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
                            nearest = nearest.min((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt());
                        }
                    }
                    let px = lod::error_m(patch, radius) / nearest.max(1.0) * focal;
                    peak = peak.max(px);
                    checked += 1;
                    assert!(
                        px <= lod::TOLERANCE_PX,
                        "{name}, altitude {altitude:.1e} m, direction {u:?}: \
                         {patch:?} gives {px:.2} px of error at a tolerance of \
                         {:.1}",
                        lod::TOLERANCE_PX
                    );
                }
            }

            println!(
                "  {name}, {altitude:.1e} m: worst error {peak:.3} px, largest \
                 set {patches} patches"
            );
            worst = worst.max(peak);
        }
    }

    // A guard against a check that checks nothing: if the worst case over the
    // whole range stayed far below the tolerance, the criterion divides with
    // slack and nobody here has touched the bound.
    assert!(
        worst > lod::TOLERANCE_PX / 4.0,
        "the worst error over the whole range is {worst:.3} px -- the check \
         does not reach the tolerance"
    );
    println!("  {checked} patches checked, worst {worst:.3} px");
}

// ---------------------------------------------------------------------------
// Stitching levels (R2b)

use engine::cubesphere::{self, Patch, EDGES};
use std::collections::HashMap;

/// Neighbours in the set differ by no more than one level -- and the
/// balancing did have to be done.
///
/// The second half is mandatory: a set in which every patch is at the same
/// level passes the first check and proves nothing. So the `balanced` number
/// stands beside it -- how many patches had to be added beyond what the error
/// criterion asked for. A zero at every altitude would mean the rule is being
/// checked on material that does not violate it.
#[test]
fn no_neighbour_in_the_set_is_two_levels_away() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);
    let mut added = 0;

    for altitude in [1.0e5, 3.0e5, 1.0e6, 1.0e7, 4.0e8] {
        let selection = lod::select(&earth(), &above(altitude), focal, None);
        let leaves: std::collections::HashSet<Patch> = selection.patches.iter().copied().collect();

        for patch in &selection.patches {
            for edge in EDGES {
                let mut cell = patch.neighbour(edge).patch;
                // The leaf covering the neighbouring cell is either the cell
                // itself or an ancestor.
                let level = loop {
                    if leaves.contains(&cell) {
                        break Some(cell.level);
                    }
                    match cell.parent() {
                        Some(up) => cell = up,
                        None => break None,
                    }
                };
                // `None` means the set is finer on that side; then that side
                // measures the difference, and measures it the same way.
                if let Some(level) = level {
                    assert!(
                        patch.level - level <= 1,
                        "at altitude {altitude:.1e} m {patch:?} neighbours \
                         level {level} across {edge:?}"
                    );
                }
            }
        }

        println!(
            "  {altitude:.1e} m: {} patches, {} of them added by balancing",
            selection.patches.len(),
            selection.balanced
        );
        added += selection.balanced;
    }

    assert!(
        added > 0,
        "balancing added no patch at any altitude -- the rule is checked on \
         material that does not violate it"
    );
}

/// **The step's main check: the surface is closed.**
///
/// A crack is a hole, and a hole is a triangle edge without a pair. On a
/// closed surface every unoriented edge belongs to exactly two triangles, and
/// this claim knows nothing about levels, faces or masks: it catches an
/// unstitched level junction, a mixed-up face, and a cube corner where
/// **three** patches meet instead of four.
///
/// Vertices are compared **by bits**, not with a tolerance. A tolerance here
/// would mean that a micrometre crack is not a crack, and it very much is: a
/// break in a pixel appears not from the size of the gap but from the
/// background showing through it.
///
/// Degenerate triangles (two of the three nodes coincide) are discarded: they
/// are exactly what the stitching removes an odd node with, the rasteriser
/// does not draw them, and in an edge count they would be noise.
#[test]
fn the_stitched_surface_has_no_edge_without_a_pair() {
    let focal = lod::focal_px(FOV_Y, HEIGHT_PX);

    for altitude in [1.0e5, 3.0e5, 2.0e6] {
        let selection = lod::select(&earth(), &above(altitude), focal, None);

        // Position -> index. Bitwise equality becomes equality of indices, and
        // from there the edges are counted with integers.
        let mut ids: HashMap<[u64; 3], u32> = HashMap::new();
        let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
        let mut used: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let mut degenerate = 0;
        let mut triangles = 0;

        for (patch, &mask) in selection.patches.iter().zip(&selection.masks) {
            // The unit sphere: a radius factor adds nothing to the topology.
            let nodes: Vec<u32> = {
                let mut v = Vec::with_capacity((SIDE + 1) * (SIDE + 1));
                for a in 0..=SIDE {
                    for b in 0..=SIDE {
                        let p = patch.vertex(a, b, 1.0);
                        let key = [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()];
                        let next = ids.len() as u32;
                        v.push(*ids.entry(key).or_insert(next));
                    }
                }
                v
            };

            for tri in cubesphere::indices(mask).chunks(3) {
                let t = [
                    nodes[tri[0] as usize],
                    nodes[tri[1] as usize],
                    nodes[tri[2] as usize],
                ];
                if t[0] == t[1] || t[1] == t[2] || t[2] == t[0] {
                    degenerate += 1;
                    continue;
                }
                triangles += 1;
                used.extend(t);
                for (x, y) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                    *edges.entry((x.min(y), x.max(y))).or_default() += 1;
                }
            }
        }

        let lonely: Vec<_> = edges.iter().filter(|(_, &n)| n != 2).collect();
        println!(
            "  {altitude:.1e} m: {} patches, {triangles} triangles, \
             {degenerate} degenerate, {} edges, {} without a pair",
            selection.patches.len(),
            edges.len(),
            lonely.len()
        );
        assert!(
            lonely.is_empty(),
            "at altitude {altitude:.1e} m {} edges belong to other than two \
             triangles -- that is a crack",
            lonely.len()
        );

        // The Euler characteristic of a sphere: V - E + F = 2. Without it,
        // closedness would also hold for a surface glued to itself inside out.
        //
        // The vertices counted are the **used** ones, not all: the odd node of
        // a stitched edge stays in the grid (the indices address it in full)
        // but belongs to no triangle -- and in the topology it does not exist.
        // The difference here is not cosmetic: it equals the number of
        // degenerate triangles exactly, and it is what shows that the
        // stitching removed precisely what it meant to.
        assert_eq!(
            ids.len() - used.len(),
            degenerate,
            "the discarded nodes and the degenerate triangles should be equal in number"
        );
        let v = used.len() as i64;
        let e = edges.len() as i64;
        let f = triangles as i64;
        assert_eq!(
            v - e + f,
            2,
            "at altitude {altitude:.1e} m V - E + F = {}, and a sphere gives 2",
            v - e + f
        );
    }
}
