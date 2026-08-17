//! The cubesphere: a cube face projected onto a sphere (ROADMAP-PLANETS.md,
//! R1a).
//!
//! ## Why not a UV sphere
//!
//! On a UV sphere all meridians converge at the poles: vertices stand shoulder
//! to shoulder there and are stretched at the equator. For F5 that did not
//! matter (scale was being checked, not quality); for a planet with LOD it does:
//! a patch's level is chosen by screen-space error, and in such a grid the error
//! differs from place to place on one shell.
//!
//! ## Why a warp, and why the tangent specifically
//!
//! The naive projection -- simply normalising a point of the cube face --
//! compresses the grid towards the face centre and stretches it towards the
//! corners. The tangential warp (`tan(u*pi/4)`) stretches the parameter so that
//! the **angular** step becomes nearly uniform. The price is a `tan` while
//! generating the grid, and rule 4 of stage R allows exactly that: the cooker
//! and patch generation are offline and CPU outside the integrator.
//!
//! How much better is not postulated but measured: [`ratio`] computes the ratio
//! of the longest edge to the shortest, and the test prints it **for both**
//! projections. One number without the other would mean nothing.
//!
//! ## The seam that is not there
//!
//! A vertex on the shared edge of two faces must match **bitwise** -- rule 5 of
//! stage R (a crack is caught by equality on the CPU, not by pixels). Here that
//! rests on three decisions, none of them cosmetic:
//!
//! 1. **One parameter table for all faces** ([`grid`]) rather than a formula
//!    each face evaluates for itself. Two symmetric formulas would give numbers
//!    differing in the last bit.
//! 2. **The table is exactly symmetric**: `w[n - k] = -w[k]` by construction,
//!    because no library guarantees `tan(-x) == -tan(x)` bitwise. Half is
//!    computed, the other half mirrored by subtraction.
//! 3. **The ends are exactly +-1.** Otherwise a vertex on an edge would have
//!    `1.0` from one face's fixed axis and
//!    `tan(pi/4) = 0.999999999999999889...` from the neighbour's moving axis --
//!    and the seam would part by exactly that epsilon times the radius: seven
//!    millimetres on Earth, that is just enough not to see by eye and to see by
//!    test.
//!
//! Then the point is assembled **in axes** (values are put into the x, y, z
//! slots), and only then normalised -- the length is computed in the unchanging
//! order `x*x + y*y + z*z`. If each face added its components in its own order,
//! the sum would round differently, and bitwise equality would vanish despite
//! all three decisions above.

/// How many faces a cube has. Not a magic six scattered through the code.
pub const FACES: usize = 6;

/// A face's axes: which `[x, y, z]` slots `u`, `v` and the fixed coordinate go
/// into, and what sign the fixed one has.
///
/// The slot order is the same for both faces of one axis (`+X` and `-X` have the
/// same arrangement, only the sign differs), and that is what makes a vertex on
/// a shared edge identical from both sides: the values land in the very same
/// slot rather than being permuted on the way.
struct Axes {
    u: usize,
    v: usize,
    w: usize,
    sign: f64,
}

const AXES: [Axes; FACES] = [
    // +X, -X: u -> y, v -> z
    Axes {
        u: 1,
        v: 2,
        w: 0,
        sign: 1.0,
    },
    Axes {
        u: 1,
        v: 2,
        w: 0,
        sign: -1.0,
    },
    // +Y, -Y: u -> x, v -> z
    Axes {
        u: 0,
        v: 2,
        w: 1,
        sign: 1.0,
    },
    Axes {
        u: 0,
        v: 2,
        w: 1,
        sign: -1.0,
    },
    // +Z, -Z: u -> x, v -> y
    Axes {
        u: 0,
        v: 1,
        w: 2,
        sign: 1.0,
    },
    Axes {
        u: 0,
        v: 1,
        w: 2,
        sign: -1.0,
    },
];

/// One node of a grid with `n` segments: `k` from 0 to `n`, the value from -1
/// to 1.
///
/// **One formula for everything** -- for a face, for a patch and for any level.
/// A patch has no right to compute its nodes its own way: neighbouring patches
/// share vertices, and two routes to the same number would give different bits.
///
/// `warped == false` gives a uniform subdivision -- needed not as a fallback but
/// as the **second number** for comparison (see [`ratio`]).
pub fn parameter(k: usize, n: usize, warped: bool) -> f64 {
    assert!(n > 0, "a grid of zero segments is not a grid");
    assert!(k <= n, "node {k} is outside a grid of {n} segments");

    // The ends are exactly +-1: `tan(pi/4)` misses by an epsilon, and it is by
    // exactly that epsilon that the seam between faces would part.
    if k == 0 {
        return -1.0;
    }
    if k == n {
        return 1.0;
    }
    // The middle of an even grid is exactly `+0.0`, not the `-0.0` the mirror
    // would leave. The sign of zero is not pedantry here: vertices are compared
    // by bits, and `(-0.0).to_bits() != (0.0).to_bits()`.
    if 2 * k == n {
        return 0.0;
    }
    // The second half is a mirror of the first, by subtraction rather than a
    // second `tan` call: the tangent's bitwise oddness is not guaranteed.
    if 2 * k > n {
        return -parameter(n - k, n, warped);
    }

    let t = 2.0 * k as f64 / n as f64 - 1.0; // -1 ... 0
    if warped {
        (t * std::f64::consts::FRAC_PI_4).tan()
    } else {
        t
    }
}

/// A face's whole parameter table: `n + 1` values from -1 to 1 inclusive.
pub fn grid(n: usize, warped: bool) -> Vec<f64> {
    (0..=n).map(|k| parameter(k, n, warped)).collect()
}

/// A vertex on the sphere from a face and two values out of [`grid`].
///
/// `a` and `b` are already-made coordinates on the cube face (that is, already
/// warped if the grid is warped): the warp lives in the table rather than here,
/// because otherwise it would be applied twice for the same vertex from two
/// neighbouring faces.
pub fn vertex(face: usize, a: f64, b: f64, radius: f64) -> [f64; 3] {
    let axes = &AXES[face];

    let mut cube = [0.0; 3];
    cube[axes.u] = a;
    cube[axes.v] = b;
    cube[axes.w] = axes.sign;

    // The order of the terms is fixed and does not depend on the face -- see
    // the module introduction.
    let length = (cube[0] * cube[0] + cube[1] * cube[1] + cube[2] * cube[2]).sqrt();
    let scale = radius / length;
    [cube[0] * scale, cube[1] * scale, cube[2] * scale]
}

/// Where a direction points: the face and the position on its grid.
///
/// `s` and `t` are in the **grid** parameter, that is in the units [`parameter`]
/// works in: both from 0 to 1, and node `k` of a grid with `n` segments sits
/// exactly at `k/n`. Multiply by `Patch::face_nodes(level)` and you get the
/// fractional node number on the face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Location {
    pub face: usize,
    pub s: f64,
    pub t: f64,
}

/// The inverse of [`vertex`]: from a direction to a face and a place on it.
///
/// Needed where the question goes **from a point in space to an asset** rather
/// than the other way: planetshine asks what the reflectance under the ship is
/// (T6), and the forward mapping cannot answer that -- it can only enumerate
/// nodes.
///
/// The vector's length does not matter: the direction determines the face and
/// the place on it. The warp is undone analytically -- `parameter` puts
/// `tan(t*pi/4)`, so back goes `atan(a)*4/pi`, and that is an exact inverse
/// rather than a search.
///
/// WARNING: **on a cube edge the answer is ambiguous by construction**, and that
/// is not a flaw: a point on a shared edge belongs to both faces, while the
/// vertex there is bitwise one (see the module introduction). The face with the
/// largest coordinate by absolute value is taken, the first one on a tie -- so
/// the answer is stable, but the neighbouring face would give the same point.
pub fn locate(direction: [f64; 3]) -> Location {
    // The fixed axis is the largest by absolute value: it is the one that
    // hits the face.
    let mut w = 0;
    for k in 1..3 {
        if direction[k].abs() > direction[w].abs() {
            w = k;
        }
    }
    let face = 2 * w + usize::from(direction[w] < 0.0);
    let axes = &AXES[face];

    // The cube is scaled so the fixed coordinate becomes `sign`; the sign of
    // `direction[w]` matches the face's sign, so we divide by the modulus.
    let scale = 1.0 / direction[w].abs();
    let unwarp = |v: f64| (v * scale).atan() * (4.0 / std::f64::consts::PI);
    Location {
        face,
        s: 0.5 * (unwarp(direction[axes.u]) + 1.0),
        t: 0.5 * (unwarp(direction[axes.v]) + 1.0),
    }
}

/// The ratio of the longest edge of a face's grid to the shortest.
///
/// A measure of non-uniformity, and the only one that means anything here:
/// absolute lengths depend on the radius, this ratio does not. Computed along
/// both grid directions, because the warp acts on both.
pub fn ratio(n: usize, warped: bool, radius: f64) -> f64 {
    let values = grid(n, warped);
    let mut shortest = f64::INFINITY;
    let mut longest: f64 = 0.0;

    let distance = |a: [f64; 3], b: [f64; 3]| {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    };

    // One face -- the other five are the same up to a permutation of axes.
    for i in 0..=n {
        for j in 0..=n {
            let here = vertex(0, values[i], values[j], radius);
            if i < n {
                let d = distance(here, vertex(0, values[i + 1], values[j], radius));
                shortest = shortest.min(d);
                longest = longest.max(d);
            }
            if j < n {
                let d = distance(here, vertex(0, values[i], values[j + 1], radius));
                shortest = shortest.min(d);
                longest = longest.max(d);
            }
        }
    }

    longest / shortest
}

/// How many segments per patch side. So there are `SIDE + 1` vertices each way.
///
/// Thirty-two is the same number the warp was measured on (R1a), and it also
/// gives 1089 vertices per patch: enough for a patch to be worth its own draw,
/// and few enough for there to be thousands of them. No ceiling follows from it
/// -- it is a grid parameter, not an architectural one.
pub const SIDE: usize = 32;

/// A patch: a quarter, a sixteenth, ... of a cube face, depending on the level.
///
/// **The unit of everything** (rule 1 of stage R): the position lives in `f64`
/// here, the vertices in `f32` relative to it, and level selection, culling,
/// GPU upload and the DEM tile all hang off the patch too.
///
/// A patch does not invent vertices of its own: the nodes come from the same
/// [`parameter`] as for a whole face, only on a finer grid. So a vertex on the
/// boundary of two patches is **the same bit** from both sides, and likewise at
/// a level boundary: the grid of level `L + 1` contains the grid of level `L`
/// exactly in its even nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Patch {
    pub face: usize,
    /// How many times the face has been halved: 0 is the whole face, 1 a
    /// quarter, ...
    pub level: u32,
    /// The patch's indices on the face, both from 0 to `2^level - 1`.
    pub i: u32,
    pub j: u32,
}

/// A patch edge, named by the coordinate that is constant on it.
///
/// The naming is deliberately the same as in [`Patch::vertex`]: edge `AMin` is
/// the vertices `(0, b)`, `BMax` the vertices `(a, SIDE)`. There is no "left" or
/// "top" edge here: a cubesphere has no top, it has `a` and `b`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    AMin,
    AMax,
    BMin,
    BMax,
}

/// All four edges in a fixed order -- so that traversal does not depend on the
/// call site.
pub const EDGES: [Edge; 4] = [Edge::AMin, Edge::AMax, Edge::BMin, Edge::BMax];

/// The edges across which the neighbour is **coarser**, as [`Edge::bit`] bits.
///
/// Sixteen values, and that is not "for now": there are exactly that many
/// combinations of four edges, they are known in advance, and that is why all
/// index sets can be built before the first frame rather than worked out during
/// debugging.
pub type EdgeMask = u8;

impl Edge {
    pub fn bit(self) -> EdgeMask {
        match self {
            Edge::AMin => 1,
            Edge::AMax => 2,
            Edge::BMin => 4,
            Edge::BMax => 8,
        }
    }

    /// The opposite edge of the same patch.
    fn opposite(self) -> Edge {
        match self {
            Edge::AMin => Edge::AMax,
            Edge::AMax => Edge::AMin,
            Edge::BMin => Edge::BMax,
            Edge::BMax => Edge::BMin,
        }
    }

    fn along_a(self) -> bool {
        matches!(self, Edge::AMin | Edge::AMax)
    }

    fn positive(self) -> bool {
        matches!(self, Edge::AMax | Edge::BMax)
    }
}

/// The neighbour across an edge: the patch itself and **its** edge that touches
/// us.
///
/// The second field is not a convenience: across a cube edge "my `AMax`" becomes
/// the neighbour's `BMin` or `BMax`, and that cannot be guessed from the asking
/// side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Neighbour {
    pub patch: Patch,
    pub edge: Edge,
}

/// A cone around a patch on the **unit** sphere: an axis from the body centre
/// and a half-spread given by its cosine and sine.
#[derive(Clone, Copy, Debug)]
pub struct Cone {
    pub axis: [f64; 3],
    pub cos_half: f64,
    pub sin_half: f64,
}

/// A patch's grid: the origin in `f64`, the vertices as `f32` offsets from
/// it.
pub struct PatchMesh {
    /// The patch centre in the body's world coordinates.
    ///
    /// The centre rather than a corner: offsets from it are half as long, and
    /// `f32` has a constant **relative** error, so an offset half as long is a
    /// vertex twice as precise.
    pub origin: [f64; 3],
    /// Vertices as `position - origin`. A row is a constant `i`.
    pub offsets: Vec<[f32; 3]>,
    /// Normals are directions, so `f32` here is not an approximation but the
    /// quantity itself.
    pub normals: Vec<[f32; 3]>,
}

impl Patch {
    /// How many nodes per face side at this level.
    pub fn face_nodes(level: u32) -> usize {
        SIDE << level
    }

    /// This patch's node `(a, b)` in the whole face's coordinates.
    fn node(&self, a: usize, b: usize) -> (usize, usize) {
        (self.i as usize * SIDE + a, self.j as usize * SIDE + b)
    }

    /// The patch covering this one -- that is, one level coarser.
    ///
    /// Level zero has no parent: a cube face is nested in nothing.
    pub fn parent(&self) -> Option<Patch> {
        if self.level == 0 {
            return None;
        }
        Some(Patch {
            face: self.face,
            level: self.level - 1,
            i: self.i / 2,
            j: self.j / 2,
        })
    }

    /// The four children in a fixed order -- the same one as in the
    /// level-selection traversal.
    pub fn children(&self) -> [Patch; 4] {
        [(0, 0), (0, 1), (1, 0), (1, 1)].map(|(di, dj)| Patch {
            face: self.face,
            level: self.level + 1,
            i: self.i * 2 + di,
            j: self.j * 2 + dj,
        })
    }

    /// The same-level neighbour across a given edge.
    ///
    /// ## Why there is no twenty-four-row table here
    ///
    /// A cube edge is usually described by a "face, edge -> face, edge,
    /// direction" table, and it always turns out to have been written twice.
    /// There is none here, because it is **derived** from [`AXES`], that is from
    /// the same source the vertices are computed with. They have nowhere to
    /// diverge.
    ///
    /// The argument in full: leaving the face through `a == +1` means the
    /// coordinate in slot `u` equals `+1`. So the neighbouring face is the one
    /// whose very same slot is **fixed** and with the same sign. On it the
    /// former `v` stays the same number (in [`AXES`] `u` and `v` never change
    /// sign -- only the fixed axis has a sign), while the former fixed axis
    /// becomes moving and pins the patch to the far edge.
    ///
    /// Hence the main consequence that saves stitching: **the index along a
    /// shared edge is never reversed**. Vertex `k` from one side is vertex `k`
    /// from the other, with the same bits.
    pub fn neighbour(&self, edge: Edge) -> Neighbour {
        let side = 1i64 << self.level;
        let (i, j) = (i64::from(self.i), i64::from(self.j));
        let (di, dj) = match edge {
            Edge::AMin => (-1, 0),
            Edge::AMax => (1, 0),
            Edge::BMin => (0, -1),
            Edge::BMax => (0, 1),
        };
        let (ni, nj) = (i + di, j + dj);
        if (0..side).contains(&ni) && (0..side).contains(&nj) {
            return Neighbour {
                patch: Patch {
                    face: self.face,
                    level: self.level,
                    i: ni as u32,
                    j: nj as u32,
                },
                edge: edge.opposite(),
            };
        }

        let axes = &AXES[self.face];
        // The slot that hits +-1, and the slot that stays moving.
        let exit = if edge.along_a() { axes.u } else { axes.v };
        let keep = if edge.along_a() { axes.v } else { axes.u };
        let along = if edge.along_a() { j } else { i };
        let sign = if edge.positive() { 1.0 } else { -1.0 };

        let far = (0..FACES)
            .find(|&f| AXES[f].w == exit && AXES[f].sign == sign)
            .expect("a cube has a face of each sign on every axis");
        // The former fixed axis became moving: `+1` is the far edge of the
        // grid.
        let outer = axes.sign > 0.0;
        let pinned = if outer { side - 1 } else { 0 };

        let (i, j, edge) = if AXES[far].u == keep {
            // The moving index went into `a`, so the shared edge runs along
            // `b`.
            (along, pinned, if outer { Edge::BMax } else { Edge::BMin })
        } else {
            (pinned, along, if outer { Edge::AMax } else { Edge::AMin })
        };

        Neighbour {
            patch: Patch {
                face: far,
                level: self.level,
                i: i as u32,
                j: j as u32,
            },
            edge,
        }
    }

    /// The node lying **one step past the edge** of the patch across a given
    /// edge -- in the neighbour (R7b).
    ///
    /// Returns the neighbouring patch and its node; that node's direction is
    /// taken from the neighbour's [`Self::vertex`] rather than from continuing
    /// our own parameterisation. The difference is not cosmetic: across a cube
    /// edge the face changes, and with it the tangential warp, so `a = -1` in
    /// our own coordinates would point at a place on the sphere where the
    /// expected point is not.
    ///
    /// `along` is the index along the shared edge, from 0 to `SIDE`. It is **not
    /// reversed**: node `k` on our side is node `k` on theirs, and that is a
    /// property of `AXES`, proved in R2b rather than a coincidence.
    ///
    /// ## Patch corners are not covered here
    ///
    /// `(-1, -1)` and the other three are not "past an edge" but "past a
    /// corner", and there is no across-edge neighbour for them at all: at a cube
    /// corner **three** patches meet, not four. The gradient at a node does not
    /// need them (a central difference asks about `+-1` along each axis
    /// separately), so the function does not know about them.
    pub fn halo_node(&self, edge: Edge, along: usize) -> (Patch, usize, usize) {
        let neighbour = self.neighbour(edge);
        // A step inward from the edge by which the neighbour touches us. When
        // that edge runs along `b`, the neighbour's moving index is `a`, and
        // `along` goes into it.
        let (a, b) = match neighbour.edge {
            Edge::AMin => (1, along),
            Edge::AMax => (SIDE - 1, along),
            Edge::BMin => (along, 1),
            Edge::BMax => (along, SIDE - 1),
        };
        (neighbour.patch, a, b)
    }

    /// A patch vertex in world coordinates -- full `f64`, no offsets.
    ///
    /// This is the same formula [`Self::mesh`] uses for the origin, and that is
    /// exactly why the check "`origin` + offset ~ direct computation" means
    /// something: the right-hand side takes no part in the left.
    pub fn vertex(&self, a: usize, b: usize, radius: f64) -> [f64; 3] {
        let n = Self::face_nodes(self.level);
        let (u, v) = self.node(a, b);
        vertex(
            self.face,
            parameter(u, n, true),
            parameter(v, n, true),
            radius,
        )
    }

    /// The cone the patch fits into entirely: the axis and the apex angle.
    ///
    /// Needed by culling (R3): "is the whole patch past the limb" is a question
    /// about the patch point nearest the camera, and a cone answers it with one
    /// dot product instead of walking a thousand vertices.
    ///
    /// The angle is taken over the patch's **four corners**, and that is not an
    /// approximation for cheapness: the face parameterisation is monotonic, so
    /// the patch point farthest from the centre is exactly a corner. The claim
    /// is verified by walking all nodes (`tests/cull.rs`) rather than left as an
    /// argument, and the walk shows a margin of exactly zero: a corner is not
    /// "somewhere near" the bound, it is the bound.
    ///
    /// The cosine and sine of the half-spread are returned rather than the angle
    /// itself: further on they enter
    /// `cos(beta - alpha) = cos(beta)cos(alpha) + sin(beta)sin(alpha)`, and no
    /// trigonometry is left in the frame at all.
    pub fn cone(&self) -> Cone {
        let axis = self.vertex(SIDE / 2, SIDE / 2, 1.0);
        let mut cos_half: f64 = 1.0;
        for (a, b) in [(0, 0), (0, SIDE), (SIDE, 0), (SIDE, SIDE)] {
            let corner = self.vertex(a, b, 1.0);
            let dot = axis[0] * corner[0] + axis[1] * corner[1] + axis[2] * corner[2];
            cos_half = cos_half.min(dot);
        }
        let cos_half = cos_half.clamp(-1.0, 1.0);
        Cone {
            axis,
            cos_half,
            sin_half: (1.0 - cos_half * cos_half).max(0.0).sqrt(),
        }
    }

    /// The patch grid -- what will go to the GPU.
    pub fn mesh(&self, radius: f64) -> PatchMesh {
        // The origin is the patch's central vertex. `SIDE` is even, so it
        // really exists rather than being interpolated.
        let origin = self.vertex(SIDE / 2, SIDE / 2, radius);

        let mut offsets = Vec::with_capacity((SIDE + 1) * (SIDE + 1));
        let mut normals = Vec::with_capacity((SIDE + 1) * (SIDE + 1));
        for a in 0..=SIDE {
            for b in 0..=SIDE {
                let p = self.vertex(a, b, radius);
                offsets.push([
                    (p[0] - origin[0]) as f32,
                    (p[1] - origin[1]) as f32,
                    (p[2] - origin[2]) as f32,
                ]);
                // A sphere's normal is the direction from the centre, that is
                // the position divided by the radius. A division rather than a
                // normalisation: the length is already known.
                normals.push([
                    (p[0] / radius) as f32,
                    (p[1] / radius) as f32,
                    (p[2] / radius) as f32,
                ]);
            }
        }

        PatchMesh {
            origin,
            offsets,
            normals,
        }
    }
}

/// How many index sets exist -- one per combination of four edges.
pub const MASKS: usize = 16;

/// A patch's indices, stitched to coarser neighbours by an edge mask.
///
/// ## One substitution instead of sixteen triangulations
///
/// On an edge whose neighbour is coarser, our grid has twice as many nodes as
/// theirs: they provide only the **even** ones. The classic way is to cut a
/// strip near such an edge and replace it with a fan, separately for each of the
/// sixteen combinations, with a special case at every patch corner.
///
/// Here the same thing is done by one index substitution: an odd node on a
/// stitched edge **does not exist**, and wherever it is mentioned the
/// neighbouring even one stands instead. The consequences, each worth naming:
///
/// - **the edge becomes exactly the neighbour's chord.** Our polyline of nodes
///   `0, 0, 2, 2, 4...` is geometrically the segments `0->2->4`, that is exactly
///   the segments the coarser patch draws, between the same vertices. Bitwise
///   the same: the grid of level `L` lies in the even nodes of the grid of level
///   `L + 1` (R1a);
/// - **the triangulation stays one formula.** Cells near the edge each produce
///   one degenerate triangle, which the rasteriser discards, and two real ones
///   -- together the same fan that would otherwise have to be written out by
///   hand;
/// - **patch corners are not a special case.** A node can lie on two stitched
///   edges at once, but then it is a corner node, and `SIDE` is even -- so both
///   its indices are even and the substitution does not fire at all. An odd node
///   lies strictly inside its own edge and never reaches the second. Two
///   substitutions never argue -- by parity rather than by agreement.
///
/// The price is `SIDE / 2` degenerate triangles per stitched edge out of 2048,
/// that is a percent in the worst case.
pub fn indices(mask: EdgeMask) -> Vec<u32> {
    // Not a runtime check but the condition for the method to exist at all:
    // with an odd side the corners would be odd nodes, and the substitution
    // would start eating the patch corner.
    const _: () = assert!(SIDE.is_multiple_of(2));
    let stride = (SIDE + 1) as u32;

    // Node -> index in the patch grid, substituting odd nodes on stitched
    // edges.
    let node = |a: usize, b: usize| -> u32 {
        let odd_on_b_edge = a % 2 == 1
            && ((b == 0 && mask & Edge::BMin.bit() != 0)
                || (b == SIDE && mask & Edge::BMax.bit() != 0));
        let odd_on_a_edge = b % 2 == 1
            && ((a == 0 && mask & Edge::AMin.bit() != 0)
                || (a == SIDE && mask & Edge::AMax.bit() != 0));
        let a = if odd_on_b_edge { a - 1 } else { a };
        let b = if odd_on_a_edge { b - 1 } else { b };
        a as u32 * stride + b as u32
    };

    let mut indices = Vec::with_capacity(SIDE * SIDE * 6);
    for a in 0..SIDE {
        for b in 0..SIDE {
            let v00 = node(a, b);
            let v01 = node(a, b + 1);
            let v10 = node(a + 1, b);
            let v11 = node(a + 1, b + 1);
            indices.extend_from_slice(&[v00, v10, v01, v01, v10, v11]);
        }
    }
    indices
}
