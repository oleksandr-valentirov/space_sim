//! A cube onto a sphere: three numbers and one equality (ROADMAP-PLANETS.md,
//! R1a).
//!
//! None of this needs a GPU or a window -- it is arithmetic alone, and that is
//! exactly why it is checked before anything is drawn. A crack in a shot is
//! one dark line a pixel wide, which the eye will miss; equality of vertices
//! will never miss it (rule 5 of stage R).

use engine::cubesphere::{self, grid, ratio, vertex, FACES};
use engine::sphere::EARTH_RADIUS_M;

const N: usize = 32;

/// The warp really does even out the grid -- and that is only visible next to
/// the naive projection.
///
/// One number here would mean nothing: "1.4" without "2.0" beside it does not
/// say whether that is good or bad.
#[test]
fn the_warp_makes_the_grid_more_even_than_plain_normalisation() {
    let naive = ratio(N, false, EARTH_RADIUS_M);
    let warped = ratio(N, true, EARTH_RADIUS_M);

    println!("  grid {N}x{N}: naive {naive:.4}, warped {warped:.4}");

    assert!(
        warped < naive,
        "the warp should have evened out the grid, but it came out {warped:.4} \
         against {naive:.4}"
    );
    // Not "smaller" but noticeably smaller: a warp that wins the third decimal
    // is not worth the tangent it costs at generation time.
    assert!(
        warped < 0.9 * naive,
        "the gain is too small to pay a tan for it: {warped:.4} against {naive:.4}"
    );
}

/// Every vertex lies on the sphere, not next to it.
#[test]
fn every_vertex_is_exactly_a_radius_from_the_centre() {
    let values = grid(N, true);
    let mut worst: f64 = 0.0;

    for face in 0..FACES {
        for &a in &values {
            for &b in &values {
                let p = vertex(face, a, b, EARTH_RADIUS_M);
                let r = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
                worst = worst.max((r - EARTH_RADIUS_M).abs() / EARTH_RADIUS_M);
            }
        }
    }

    println!("  largest deviation of |r|: {worst:.2e} relative");
    assert!(
        worst < 1e-15,
        "the vertices are not on the sphere: {worst:.2e}"
    );
}

/// A vertex on a shared edge of two faces is **the same bits**.
///
/// This is the step's main check, and mutations against it were run by hand --
/// with a result that corrects the plan (R1a):
///
/// - **not forcing the ends of the table** (leaving `tan(pi/4)` as it is)
///   breaks this test. That is precisely the seam the step exists for;
/// - **mirroring by a second call to `tan`** instead of by negation survives
///   this test, because `tan` in glibc on this machine turned out to be
///   bitwise odd. What catches it is the neighbouring table test, and therein
///   lies its whole value: a property that holds by accident must have a
///   guard, otherwise it will vanish on another platform -- and the boundary
///   here is bitwise;
/// - **swapping the `u` and `v` axes within one face** -- what the plan called
///   the main mutation -- breaks **nothing**. And that is not a weakness of
///   the test: a transposed face gives the same **set** of vertices, i.e. it
///   does not tear the seam at all. What it does break is not the seam but the
///   `(i, j)` correspondence between neighbouring patches -- and there are no
///   patches here yet, so catching that is for R1b/R2b, where the indices
///   acquire meaning.
///
/// All twelve edges of the cube are checked, not one.
#[test]
fn a_vertex_on_a_shared_edge_is_the_same_bits_from_both_faces() {
    let values = grid(N, true);
    let radius = EARTH_RADIUS_M;

    // Every vertex of every face, keyed by the bits of the position. Matching
    // keys mean bitwise equality of all three components.
    let mut seen: std::collections::HashMap<[u64; 3], Vec<usize>> =
        std::collections::HashMap::new();
    for face in 0..FACES {
        for &a in &values {
            for &b in &values {
                let p = vertex(face, a, b, radius);
                let key = [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()];
                let faces = seen.entry(key).or_default();
                if !faces.contains(&face) {
                    faces.push(face);
                }
            }
        }
    }

    // How many vertices exactly two faces share (edges) and how many three
    // share (corners).
    let shared_by_two = seen.values().filter(|f| f.len() == 2).count();
    let shared_by_three = seen.values().filter(|f| f.len() == 3).count();

    println!("  shared vertices: {shared_by_two} on edges, {shared_by_three} at corners");

    // Twelve edges of (N - 1) interior vertices each: the corners are counted
    // separately.
    assert_eq!(
        shared_by_two,
        12 * (N - 1),
        "not everything matched on the edges -- the seam parts where it cannot \
         be seen"
    );
    // Eight cube corners, and THREE faces meet at each, not four: this is
    // exactly where naive stitching breaks (R2b will remind us of this again).
    assert_eq!(
        shared_by_three, 8,
        "the cube corners did not meet three faces at a time"
    );

    // And no vertex belongs to more than three faces: four would mean two
    // opposite faces had merged somewhere.
    assert!(seen.values().all(|f| f.len() <= 3));
}

/// The parameter table is a bitwise mirror of itself, and the ends are exact.
///
/// A separate check, because it is a precondition of the equality above and
/// can be broken without touching anything else: it is enough to compute the
/// second half by a second call to `tan` instead of by mirroring.
#[test]
fn the_parameter_table_is_a_mirror_of_itself() {
    for n in [2, 8, 32, 33] {
        let values = grid(n, true);
        assert_eq!(values[0], -1.0, "the left end is not exact at n = {n}");
        assert_eq!(values[n], 1.0, "the right end is not exact at n = {n}");

        for k in 0..=n {
            // Zero is handled separately, and not out of pedantry: negating
            // zero flips the sign, so at the exact midpoint bitwise equality
            // with the mirror is impossible by definition. The requirement on
            // it is stronger instead -- exactly `+0.0`, because that is the
            // very bit the vertices are compared by.
            if values[k] == 0.0 {
                assert_eq!(
                    values[k].to_bits(),
                    0.0_f64.to_bits(),
                    "the midpoint at n = {n} is a \"minus zero\""
                );
                continue;
            }
            assert_eq!(
                values[k].to_bits(),
                (-values[n - k]).to_bits(),
                "the table is asymmetric at {k} for n = {n}"
            );
        }
    }

    // An even grid must have an exact zero midpoint; an odd one has no middle
    // node at all.
    assert_eq!(grid(8, true)[4], 0.0);
}

// ---------------------------------------------------------------------------
// The patch: origin in f64, vertices in f32 (R1b)

use engine::camera::Camera;
use engine::cubesphere::{Patch, SIDE};

/// A vertex is off by a constant fraction of the **patch size**, not of the
/// distance.
///
/// This is rule 2 of stage R, and it is checked as a law rather than as a
/// single number: at levels 0, 5 and 10 a patch differs a millionfold in size,
/// and the fraction has to stay the same. One measurement at one level would
/// pass on an implementation where the offset is taken from someone else's
/// origin.
#[test]
fn a_vertex_is_off_by_a_fraction_of_the_patch_not_of_the_distance() {
    // `f32` gives 24 bits of mantissa, i.e. 6e-8 relative -- the same number
    // as in reversed-Z (F3). The factor of 2 is because the offset from the
    // patch centre is rounded once more on subtraction.
    const TOLERANCE: f64 = 2.0 * 6e-8;

    for level in [0, 5, 10] {
        let patch = Patch {
            face: 4,
            level,
            i: (1 << level) / 3,
            j: (1 << level) / 2,
        };
        let mesh = patch.mesh(EARTH_RADIUS_M);

        // The patch size is the length of its diagonal; the error is compared
        // against that.
        let corner = patch.vertex(0, 0, EARTH_RADIUS_M);
        let opposite = patch.vertex(SIDE, SIDE, EARTH_RADIUS_M);
        let size = ((corner[0] - opposite[0]).powi(2)
            + (corner[1] - opposite[1]).powi(2)
            + (corner[2] - opposite[2]).powi(2))
        .sqrt();

        let mut worst: f64 = 0.0;
        for a in 0..=SIDE {
            for b in 0..=SIDE {
                let exact = patch.vertex(a, b, EARTH_RADIUS_M);
                let offset = mesh.offsets[a * (SIDE + 1) + b];
                let rebuilt = [
                    mesh.origin[0] + f64::from(offset[0]),
                    mesh.origin[1] + f64::from(offset[1]),
                    mesh.origin[2] + f64::from(offset[2]),
                ];
                let error = ((rebuilt[0] - exact[0]).powi(2)
                    + (rebuilt[1] - exact[1]).powi(2)
                    + (rebuilt[2] - exact[2]).powi(2))
                .sqrt();
                worst = worst.max(error);
            }
        }

        println!(
            "  level {level:2}: patch {size:.3e} m, worst error {worst:.3e} m \
             = {:.2e} of the size",
            worst / size
        );
        assert!(
            worst <= TOLERANCE * size,
            "level {level}: {worst:.3e} m on a patch of {size:.3e} m -- that is \
             {:.2e} of the size, and it should have been no more than \
             {TOLERANCE:.0e}",
            worst / size
        );
    }
}

/// A camera 10 m away and a camera 4e8 m away see the same patch.
///
/// The second half of R1b, and without it the first is worth nothing: the
/// error can be small relative to the patch and still be eaten by the camera
/// subtraction if that subtraction is done in the wrong place. Here it is done
/// the way the GPU will do it: `camera.relative(origin)` **once per patch**,
/// plus the rotated vertex offset -- against `camera.relative(exact)` **per
/// vertex**, i.e. against what F4 did.
///
/// What is measured is an **angle**, not metres, and that is not presentation
/// of the result. The absolute divergence is obliged to grow with distance:
/// `f32` holds a constant relative precision, so at 4e8 m its step is 32 m and
/// both paths round to that same grid alike. The question is not whether the
/// metres grew but whether the **silhouette** moved: the divergence divided by
/// the distance to the vertex itself is precisely the angle by which the
/// vertex will shift on screen.
///
/// So the claim is strong in the right form: not "the error is small" but "the
/// angle is the same at both distances" -- distance is not in the equation.
#[test]
fn the_patch_looks_the_same_from_ten_metres_and_from_the_moon() {
    let patch = Patch {
        face: 0,
        level: 8,
        i: 100,
        j: 137,
    };
    let mesh = patch.mesh(EARTH_RADIUS_M);

    // The camera looks at the patch centre from two distances along its
    // normal.
    let direction = [
        mesh.origin[0] / EARTH_RADIUS_M,
        mesh.origin[1] / EARTH_RADIUS_M,
        mesh.origin[2] / EARTH_RADIUS_M,
    ];
    let mut angles = Vec::new();

    for distance in [10.0, 4.05e8] {
        let eye = [
            mesh.origin[0] + direction[0] * distance,
            mesh.origin[1] + direction[1] * distance,
            mesh.origin[2] + direction[2] * distance,
        ];
        let camera = Camera::look_at(eye, mesh.origin, [0.0, 0.0, 1.0]);

        // The patch path: the camera is subtracted from the patch origin, the
        // offset is added already in `f32`. This is exactly what the vertex
        // shader will do.
        let base = camera.relative(mesh.origin);

        let mut worst_angle: f64 = 0.0;
        let mut worst_metres: f64 = 0.0;
        for a in 0..=SIDE {
            for b in 0..=SIDE {
                let offset = mesh.offsets[a * (SIDE + 1) + b];
                // The offset lives in world axes, so `rotate` takes it into
                // camera space -- exactly what the vertex shader will do with
                // the view matrix.
                let turned = camera.rotate([
                    f64::from(offset[0]),
                    f64::from(offset[1]),
                    f64::from(offset[2]),
                ]);
                let by_patch = [
                    base[0] + turned[0],
                    base[1] + turned[1],
                    base[2] + turned[2],
                ];
                // The F4 path: camera-relative per vertex, from full `f64`.
                let by_vertex = camera.relative(patch.vertex(a, b, EARTH_RADIUS_M));

                let gap = ((f64::from(by_patch[0]) - f64::from(by_vertex[0])).powi(2)
                    + (f64::from(by_patch[1]) - f64::from(by_vertex[1])).powi(2)
                    + (f64::from(by_patch[2]) - f64::from(by_vertex[2])).powi(2))
                .sqrt();
                // The distance to the vertex itself, not to the patch centre:
                // up close they differ by orders of magnitude, and it is the
                // far vertices that give the largest metres.
                let range = (f64::from(by_vertex[0]).powi(2)
                    + f64::from(by_vertex[1]).powi(2)
                    + f64::from(by_vertex[2]).powi(2))
                .sqrt();

                worst_metres = worst_metres.max(gap);
                worst_angle = worst_angle.max(gap / range);
            }
        }

        // Pixels -- so the number means something without translation: 1280
        // pixels over a 60 deg field of view, i.e. a radian is ~1223 pixels.
        println!(
            "  camera at {distance:.3e} m: {worst_metres:.3e} m, angle \
             {worst_angle:.2e} rad = {:.1e} pixels",
            worst_angle * 1223.0
        );
        angles.push(worst_angle);
    }

    let (near, far) = (angles[0], angles[1]);
    assert!(
        near < 1e-6 && far < 1e-6,
        "the silhouette moves: {near:.2e} rad up close, {far:.2e} rad far away"
    );
    // The main claim: distance is not in the equation. The factor of 10 is
    // slack for the camera's own rounding, not for growth with distance.
    assert!(
        far <= near.max(1e-12) * 10.0,
        "the angle is larger far away than up close ({far:.2e} against \
         {near:.2e}) -- so distance is in the equation after all"
    );
}

/// Neighbouring patches share their vertices **bitwise**, across a level
/// boundary too.
///
/// This is where swapping the axes within a face finally becomes visible (R1a
/// says so outright): patch indices have meaning, and the neighbour along `i`
/// has to match along the edge rather than "somewhere in the same set".
#[test]
fn neighbouring_patches_share_their_edge_bit_for_bit() {
    let radius = EARTH_RADIUS_M;

    // Neighbours on one face.
    let left = Patch {
        face: 2,
        level: 3,
        i: 3,
        j: 5,
    };
    let right = Patch {
        face: 2,
        level: 3,
        i: 4,
        j: 5,
    };
    for b in 0..=SIDE {
        let a = left.vertex(SIDE, b, radius);
        let c = right.vertex(0, b, radius);
        for k in 0..3 {
            assert_eq!(
                a[k].to_bits(),
                c[k].to_bits(),
                "the edge between neighbours parted at node {b}, component {k}"
            );
        }
    }

    // A level boundary: a level-3 patch and the four level-4 patches in its
    // place. The level-4 grid contains the level-3 one at its even nodes --
    // and that is no coincidence but what the stitching in R2b will rest on.
    let coarse = Patch {
        face: 2,
        level: 3,
        i: 3,
        j: 5,
    };
    let fine = Patch {
        face: 2,
        level: 4,
        i: 6,
        j: 10,
    };
    for a in 0..=SIDE / 2 {
        for b in 0..=SIDE / 2 {
            let from_coarse = coarse.vertex(a, b, radius);
            let from_fine = fine.vertex(2 * a, 2 * b, radius);
            for k in 0..3 {
                assert_eq!(
                    from_coarse[k].to_bits(),
                    from_fine[k].to_bits(),
                    "levels 3 and 4 parted at node ({a}, {b}), component {k}"
                );
            }
        }
    }
}

/// The patch mesh is closed: indices within bounds, exactly as many triangles
/// as cells, and no vertex lost.
#[test]
fn the_patch_mesh_is_closed() {
    let mesh = Patch {
        face: 5,
        level: 2,
        i: 1,
        j: 2,
    }
    .mesh(EARTH_RADIUS_M);
    let vertices = (SIDE + 1) * (SIDE + 1);
    let indices = cubesphere::indices(0);

    assert_eq!(mesh.offsets.len(), vertices);
    assert_eq!(mesh.normals.len(), vertices);
    assert_eq!(indices.len(), SIDE * SIDE * 6);
    assert!(indices.iter().all(|&i| (i as usize) < vertices));

    let mut used = vec![false; vertices];
    for &i in &indices {
        used[i as usize] = true;
    }
    assert!(
        used.iter().all(|&u| u),
        "there are vertices no triangle draws"
    );

    // A normal is a unit direction, not a position.
    for n in &mesh.normals {
        let length =
            (f64::from(n[0]).powi(2) + f64::from(n[1]).powi(2) + f64::from(n[2]).powi(2)).sqrt();
        assert!((length - 1.0).abs() < 1e-6, "a normal of length {length}");
    }
}

// ---------------------------------------------------------------------------
// The cubesphere in the frame instead of the UV sphere (R1d)

/// The cubesphere's silhouette matches the UV sphere's -- by mask, not by
/// colour.
///
/// The oracle here is **a different path to the same picture**:
/// `sphere_render` (F5) draws a UV sphere with the same camera, projection and
/// near plane, while `Frame` now draws patches. What is compared is not pixels
/// but the "planet / sky" mask: the colours are **obliged** to differ -- the
/// cubesphere's normals are different, and the sRGB conversion is already
/// recorded as a separate decision (rule 7 of stage R).
///
/// The tolerance is a fraction of the frame, and it is taken from geometry
/// rather than from thin air: both meshes approximate a circle with 32
/// segments per 90 deg, so each has its own silhouette to within fractions of
/// a pixel, and the divergence accumulates along the whole rim of the disc.
#[test]
fn the_cubesphere_draws_the_same_silhouette_as_the_uv_sphere() {
    use engine::frame;
    use engine::gpu::Gpu;
    use engine::shot::{self, Shot};
    use engine::{sphere, sphere_render};

    const WIDTH: u32 = 1280;
    const HEIGHT: u32 = 720;

    let Some(gpu) = Gpu::for_tests() else {
        return;
    };

    let camera = frame::default_camera();
    let near = frame::DEFAULT_ALTITUDE_M / 10.0;

    // The old path: a UV sphere, camera-relative per vertex.
    let mesh = sphere::generate(sphere::EARTH_RADIUS_M, 64, 128);
    let old = sphere_render::render(
        &gpu,
        WIDTH,
        HEIGHT,
        &camera,
        &mesh,
        &sphere_render::Params {
            near,
            light_dir: [0.4, 0.4, 0.82],
            colour: [0.2, 0.6, 0.9, 1.0],
        },
    )
    .expect("the UV sphere should have drawn");

    // The new path: patches, camera-relative once per patch. The same `Frame`
    // that goes to the window -- otherwise the wrong frame would be checked.
    let new = shot::take_scene(
        &gpu,
        WIDTH,
        HEIGHT,
        &frame::default_scene(frame::default_camera()),
    )
    .expect("the cubesphere should have drawn");

    // The two paths have different backgrounds -- `sphere_render` clears to
    // black, `Frame` to its own sky colour -- so the mask is taken from each
    // one's own background.
    let planet = |s: &Shot, x: u32, y: u32, sky: [u8; 3]| {
        let p = s.pixel(x, y);
        [p[0], p[1], p[2]] != sky
    };
    const BLACK: [u8; 3] = [0, 0, 0];

    let mut differ = 0u64;
    let mut lit = 0u64;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let in_old = planet(&old, x, y, BLACK);
            let in_new = planet(&new, x, y, frame::CLEAR_BYTES);
            if in_old {
                lit += 1;
            }
            if in_old != in_new {
                differ += 1;
            }
        }
    }

    let edge = 2.0 * std::f64::consts::PI * (lit as f64 / std::f64::consts::PI).sqrt();
    println!(
        "  silhouette: {lit} pixels, divergence {differ} = {:.2} pixels per \
         rim pixel (rim ~ {edge:.0})",
        differ as f64 / edge
    );

    assert!(
        lit > 0,
        "the old sphere did not draw -- there is nothing to check against"
    );
    // No more than a pixel along the rim (R1d), and the rim is computed from
    // the disc's area rather than guessed: 2*pi*r with r = sqrt(area/pi).
    assert!(
        (differ as f64) <= edge,
        "the silhouette moved by {differ} pixels with a rim of {edge:.0} -- \
         that is not a tolerance, that is an axis or a winding order"
    );
}

// ---------------------------------------------------------------------------
// Neighbourhood and stitching (R2b)

use engine::cubesphere::{Edge, EDGES};

/// Neighbourhood is symmetric: whoever is my neighbour has me as a neighbour,
/// across the same edge.
///
/// **All** level-2 patches on all six faces are checked, not a sample: a
/// mistake in translating indices across a cube edge sits in exactly one case
/// out of twenty-four, and a sample will not see it.
#[test]
fn being_a_neighbour_is_mutual() {
    const LEVEL: u32 = 2;
    let side = 1u32 << LEVEL;

    for face in 0..FACES {
        for i in 0..side {
            for j in 0..side {
                let patch = Patch {
                    face,
                    level: LEVEL,
                    i,
                    j,
                };
                for edge in EDGES {
                    let there = patch.neighbour(edge);
                    let back = there.patch.neighbour(there.edge);
                    assert_eq!(
                        back.patch, patch,
                        "{patch:?} through {edge:?} landed in {:?}, and does not \
                         come back from there",
                        there.patch
                    );
                    assert_eq!(
                        back.edge, edge,
                        "{patch:?} through {edge:?}: a different edge came back"
                    );
                }
            }
        }
    }
}

/// The vertices of a shared edge match **bitwise**, and node `k` is node `k`.
///
/// The step's main check (rule 5 of stage R). The weaker form -- "the set of
/// vertices matches" -- is no good here: a transposed face would give the same
/// set and part ways in the correspondence of indices, and the stitching rests
/// on precisely that. The cube corners, where **three** faces meet, fall into
/// the check along with everything else: they are the patches in which two of
/// the four edges lead to different faces.
#[test]
fn a_shared_edge_matches_node_by_node_not_just_as_a_set() {
    const LEVEL: u32 = 2;
    let side = 1u32 << LEVEL;
    let radius = EARTH_RADIUS_M;

    // The vertices of a patch edge in order of increasing shared index.
    let along = |patch: &Patch, edge: Edge, k: usize| match edge {
        Edge::AMin => patch.vertex(0, k, radius),
        Edge::AMax => patch.vertex(SIDE, k, radius),
        Edge::BMin => patch.vertex(k, 0, radius),
        Edge::BMax => patch.vertex(k, SIDE, radius),
    };

    let mut across_faces = 0;
    for face in 0..FACES {
        for i in 0..side {
            for j in 0..side {
                let patch = Patch {
                    face,
                    level: LEVEL,
                    i,
                    j,
                };
                for edge in EDGES {
                    let there = patch.neighbour(edge);
                    if there.patch.face != face {
                        across_faces += 1;
                    }
                    for k in 0..=SIDE {
                        let mine = along(&patch, edge, k);
                        let theirs = along(&there.patch, there.edge, k);
                        for c in 0..3 {
                            assert_eq!(
                                mine[c].to_bits(),
                                theirs[c].to_bits(),
                                "{patch:?} / {edge:?}: node {k} parted from \
                                 {:?} / {:?} in component {c}",
                                there.patch,
                                there.edge
                            );
                        }
                    }
                }
            }
        }
    }

    // Twenty-four cube edges (four per face), `side` patches on each, and
    // every edge counted from both sides.
    assert_eq!(
        across_faces,
        FACES * 4 * side as usize,
        "the number of neighbourhoods crossing cube edges is not the number \
         there are"
    );
}

/// **A halo node sits exactly one step past the edge** (R7b).
///
/// The halo catches what terrain cannot do without it: the gradient at a node
/// on a tile boundary needs a neighbour from another tile, and a clamped index
/// would give different amplitudes on the two sides of the boundary -- i.e. a
/// crack exactly where R2b removed one.
///
/// The check has to be **independent of the formula itself**, otherwise it
/// merely repeats it. Hence two different oracles:
///
/// 1. **Inside a face -- exact arithmetic.** Node `-1` of patch `(i, j)` is
///    node `SIDE - 1` of patch `(i - 1, j)`, and that is visible straight from
///    the numbering, without any `neighbour`. Bitwise equality of vertices.
/// 2. **Across a cube edge -- geometry.** There is no shared numbering there,
///    but there is a claim the formula cannot fake: three points -- the halo,
///    the edge node and our first interior node -- run **consecutively**, i.e.
///    the step from the halo to the edge is close to the step from the edge
///    inward. The warp makes them unequal, but not different by a factor of
///    two.
#[test]
fn a_halo_node_sits_one_step_past_the_edge() {
    use engine::cubesphere::{Patch, EDGES, SIDE};

    const LEVEL: u32 = 2;
    let side = 1u32 << LEVEL;
    let radius = EARTH_RADIUS_M;

    let mut same_face = 0;
    let mut across = 0;
    let mut worst_ratio: f64 = 1.0;

    for face in 0..FACES {
        for i in 0..side {
            for j in 0..side {
                let patch = Patch {
                    face,
                    level: LEVEL,
                    i,
                    j,
                };
                for edge in EDGES {
                    let (there, ha, hb) = patch.halo_node(edge, SIDE / 3);
                    let halo = there.vertex(ha, hb, radius);

                    // Our three points across the edge: the halo, the edge
                    // itself, the first interior node.
                    let k = SIDE / 3;
                    let (edge_node, inner) = match edge {
                        Edge::AMin => (patch.vertex(0, k, radius), patch.vertex(1, k, radius)),
                        Edge::AMax => (
                            patch.vertex(SIDE, k, radius),
                            patch.vertex(SIDE - 1, k, radius),
                        ),
                        Edge::BMin => (patch.vertex(k, 0, radius), patch.vertex(k, 1, radius)),
                        Edge::BMax => (
                            patch.vertex(k, SIDE, radius),
                            patch.vertex(k, SIDE - 1, radius),
                        ),
                    };

                    if there.face == face {
                        // The same side of the face: the neighbour is one
                        // patch over, and the node needed follows straight
                        // from the numbering.
                        let (di, dj) = match edge {
                            Edge::AMin => (-1i64, 0i64),
                            Edge::AMax => (1, 0),
                            Edge::BMin => (0, -1),
                            Edge::BMax => (0, 1),
                        };
                        let plain = Patch {
                            face,
                            level: LEVEL,
                            i: (i64::from(i) + di) as u32,
                            j: (i64::from(j) + dj) as u32,
                        };
                        let (pa, pb) = match edge {
                            Edge::AMin => (SIDE - 1, k),
                            Edge::AMax => (1, k),
                            Edge::BMin => (k, SIDE - 1),
                            Edge::BMax => (k, 1),
                        };
                        let expected = plain.vertex(pa, pb, radius);
                        for c in 0..3 {
                            assert_eq!(
                                halo[c].to_bits(),
                                expected[c].to_bits(),
                                "{patch:?} / {edge:?}: the halo did not match \
                                 node {plain:?} ({pa}, {pb}) in component {c}"
                            );
                        }
                        same_face += 1;
                    } else {
                        across += 1;
                    }

                    let length = |p: [f64; 3], q: [f64; 3]| {
                        let d = [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
                        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
                    };
                    let out = length(halo, edge_node);
                    let inward = length(inner, edge_node);
                    let ratio = (out / inward).max(inward / out);
                    worst_ratio = worst_ratio.max(ratio);
                    assert!(
                        ratio < 1.5,
                        "{patch:?} / {edge:?}: a step of {out:.1} m outward \
                         against {inward:.1} m inward -- the halo is not on the \
                         neighbouring node"
                    );
                }
            }
        }
    }

    println!(
        "  halos inside a face {same_face}, across a cube edge {across}, \
         worst step ratio {worst_ratio:.4}"
    );
    assert_eq!(
        across,
        FACES * 4 * side as usize,
        "the number of halos crossing cube edges is not the number there are"
    );
}

/// A stitched edge keeps exactly its even nodes -- and exactly on the edges in
/// the mask.
///
/// Two halves, and without the second the first is worth nothing: a set that
/// throws away the odd nodes **everywhere** would pass the "there are none on
/// a stitched edge" check and ruin every interior seam.
#[test]
fn a_stitched_edge_keeps_only_its_even_nodes() {
    let stride = (SIDE + 1) as u32;
    let node = |a: usize, b: usize| (a * (SIDE + 1) + b) as u32;

    for mask in 0..16u8 {
        let indices = engine::cubesphere::indices(mask);
        assert_eq!(indices.len(), SIDE * SIDE * 6);

        let used: std::collections::HashSet<u32> = indices.iter().copied().collect();
        for (edge, along) in [
            (Edge::AMin, 0),
            (Edge::AMax, 1),
            (Edge::BMin, 2),
            (Edge::BMax, 3),
        ] {
            let stitched = mask & edge.bit() != 0;
            for k in 1..SIDE {
                let index = match along {
                    0 => node(0, k),
                    1 => node(SIDE, k),
                    2 => node(k, 0),
                    _ => node(k, SIDE),
                };
                let odd = k % 2 == 1;
                let expected = !(stitched && odd);
                assert_eq!(
                    used.contains(&index),
                    expected,
                    "mask {mask:04b}, {edge:?}, node {k}: expected {expected}, \
                     but the node {}",
                    if used.contains(&index) {
                        "is drawn"
                    } else {
                        "is gone"
                    }
                );
            }
        }

        // Interior nodes stay in place under any mask.
        for a in 1..SIDE {
            for b in 1..SIDE {
                assert!(
                    used.contains(&node(a, b)),
                    "mask {mask:04b} lost the interior node ({a}, {b})"
                );
            }
        }
        assert!(indices.iter().all(|&i| i < stride * stride));
    }
}

// ---------------------------------------------------------------------------
// The inverse mapping: direction -> face and node (stage T, step T6b)

use engine::cubesphere::locate;

/// A node found by the inverse mapping is the node it started from.
///
/// The oracle here is direct: `Patch::vertex` gives the point of a node,
/// `locate` has to return an **integer** index of that same node. The
/// integrality is the claim: an error of a quarter of a node would go
/// unnoticed by a comparison of "roughly there", while on an asset it would
/// mean the wrong texel.
///
/// WARNING: **all six faces and asymmetric nodes are checked.** A symmetric
/// point here hides exactly what the level-selection fixture hid (D13, D14):
/// swapped `u` and `v`, a mirrored axis, a half-node shift -- on the diagonal
/// and at the centre of a face, all of these give the right answer.
#[test]
fn a_node_found_by_direction_is_the_node_it_started_from() {
    for level in [0, 3] {
        let n = Patch::face_nodes(level);
        for face in 0..FACES {
            let patch = Patch {
                face,
                level,
                i: 0,
                j: 0,
            };
            // Asymmetric nodes: different along both axes, none at the centre
            // or on the diagonal. The face edges are deliberately excluded --
            // there the answer is ambiguous by construction (see `locate`),
            // and there is a separate claim about them below.
            for (a, b) in [(1, 7), (7, 1), (SIDE - 3, 2), (SIDE - 1, SIDE - 5)] {
                let direction = patch.vertex(a, b, 1.0);
                let found = locate(direction);
                let (u, v) = (found.s * n as f64, found.t * n as f64);
                assert_eq!(found.face, face, "the face moved at node ({a}, {b})");
                assert!(
                    (u - a as f64).abs() < 1e-9 && (v - b as f64).abs() < 1e-9,
                    "level {level}, face {face}, node ({a}, {b}) was found as \
                     ({u:.6}, {v:.6})"
                );
            }
        }
    }
}

/// Between nodes the inverse is exact too -- and that is already about the
/// warp itself.
///
/// The nodes check the parameter table rather than `atan`: at them `s` and `t`
/// are multiples of the grid step, so a crude inverse (one without the warp at
/// all, say) would agree at the ends. Here the directions are arbitrary -- one
/// per octant, plus one nearly along an axis -- and the round trip has to
/// reproduce the direction itself.
#[test]
fn a_direction_between_nodes_comes_back_as_itself() {
    let directions: [[f64; 3]; 6] = [
        [0.31, 0.72, -0.62],
        [-0.83, 0.14, 0.54],
        [0.07, -0.95, 0.31],
        [-0.22, -0.41, -0.88],
        [0.999, 0.03, -0.02],
        [0.05, 0.02, -0.998],
    ];
    for d in directions {
        let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let unit = [d[0] / length, d[1] / length, d[2] / length];
        let found = locate(unit);
        // Back the same way the grid is built: grid parameter -> warp -> face
        // -> sphere.
        let warp = |x: f64| ((2.0 * x - 1.0) * std::f64::consts::FRAC_PI_4).tan();
        let back = vertex(found.face, warp(found.s), warp(found.t), 1.0);
        let off = ((back[0] - unit[0]).powi(2)
            + (back[1] - unit[1]).powi(2)
            + (back[2] - unit[2]).powi(2))
        .sqrt();
        assert!(
            off < 1e-12,
            "direction {unit:?} came back as {back:?} ({off})"
        );
        assert!(
            (0.0..=1.0).contains(&found.s) && (0.0..=1.0).contains(&found.t),
            "parameters outside the face: {found:?}"
        );
    }
}

/// On a cube edge the face is a choice, but the point is not.
///
/// A node on a shared edge belongs to two faces, and at a corner to three, so
/// asking "which face is correct" is meaningless: both are. The claim that
/// does have meaning is different -- whichever face `locate` picks, it gives
/// back the same point of the sphere. Had the choice of face and the
/// recomputation of coordinates parted ways, a quarter of the sphere would
/// move here.
#[test]
fn on_a_cube_edge_the_face_is_a_choice_but_the_point_is_not() {
    let warp = |x: f64| ((2.0 * x - 1.0) * std::f64::consts::FRAC_PI_4).tan();
    for face in 0..FACES {
        let patch = Patch {
            face,
            level: 0,
            i: 0,
            j: 0,
        };
        // The corners of the face and the midpoints of its edges -- everything
        // that lies on a cube edge.
        for (a, b) in [
            (0, 0),
            (0, SIDE),
            (SIDE, 0),
            (SIDE, SIDE),
            (0, SIDE / 2),
            (SIDE / 2, SIDE),
        ] {
            let unit = patch.vertex(a, b, 1.0);
            let found = locate(unit);
            let back = vertex(found.face, warp(found.s), warp(found.t), 1.0);
            let off = ((back[0] - unit[0]).powi(2)
                + (back[1] - unit[1]).powi(2)
                + (back[2] - unit[2]).powi(2))
            .sqrt();
            assert!(
                off < 1e-12,
                "face {face}, node ({a}, {b}): {unit:?} -> face {} -> {back:?}",
                found.face
            );
        }
    }
}
