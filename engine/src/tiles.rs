//! Surface tiles: two formats and both readers (R5b; stage T, T2c).
//!
//! PROJECT.md section 7 forbids loading raw formats at runtime, so a cooker
//! (`tools/dem-cook`) stands between the source and the frame. The formats are
//! our own, with a version in the header, exactly like the ephemeris asset.
//!
//! There are **two** tilesets: [`Terrain`] (heights, `SSDEM`) and [`Colour`]
//! (colour, `SSCOL`). All they share is the pyramid geometry -- the free
//! functions below; why that rather than one file with two channels is said in
//! [`Colour`]. Everything further in this introduction concerns the terrain.
//!
//! ## Why the format lives here rather than in the cooker
//!
//! The writer and the reader of one format must be **one piece of code**,
//! otherwise they diverge -- not at once, but on the fourth edit. The direction
//! of the dependency is then unambiguous: the cooker already needs the
//! cubesphere geometry (`crate::cubesphere`), so `dem-cook -> engine` and not
//! the other way. The engine knows nothing about the cooker.
//!
//! ## A tile is a patch, node for node
//!
//! A tile stores height **in the very nodes** the patch grid has:
//! `(SIDE + 1)^2` values. Not a texture of arbitrary size -- exactly the nodes.
//!
//! The consequence it was done for: **terrain adds no cracks.** A vertex on the
//! shared edge of two patches is bitwise one (R2b), so the direction the cooker
//! sampled the height along is bitwise one too, so the height is one -- the same
//! number lies in two neighbouring tiles. Level stitching
//! (`cubesphere::indices`) keeps working without a single edit: it drops the odd
//! node, and the even one lies in both tiles anyway.
//!
//! ## A node carries two numbers: height and slope (stage W, Q3)
//!
//! **Three** consumers read the terrain slope -- the amplitude of the procedural
//! detail, level selection (both R7c) and the material rule (T4) -- and until
//! stage W all three computed it in the frame by a central difference over
//! neighbouring nodes. Now it lies in the node next to the height, computed once
//! by [`Terrain::build`].
//!
//! What that buys, in order of importance:
//!
//! 1. **The cube corner stops being ambiguous.** Three faces meet there, each
//!    one's stencil reaches into the other two, and the three answers differ
//!    (Q3). The cooker sees all three at once and resolves the corner **once**;
//!    one number goes into the file, so bitwise sameness is a copy rather than a
//!    coincidence of computations.
//! 2. **The frame gets cheaper.** Height and slope lie in one texel and share
//!    one window -- four `Load`s instead of twenty.
//! 3. **The halo disappears from the file.** It existed purely for the gradient.
//!
//! The price was measured before the change (W1, `--tile-probe`): `Rg16Sint`
//! costs 12,288 bytes per tile against 8192 for `R16Sint` on NVIDIA and 16,384
//! on RADV, that is x1.5 and x2. In memory that adds +40/+80 MiB for a
//! two-body scene; in array binding time, nothing -- binding was charged by
//! the **number** of textures and not by their size, which is the mechanism
//! behind D19 and the reason Y1 could close it by binding fewer.
//!
//! ## The halo remains, but **only on the input** of [`Terrain::build`] (R7b)
//!
//! The grid the cooker feeds into [`Terrain::build`] is still wider than the
//! patch by [`HALO`] node on each side: `-1` and `SIDE + 1` along each axis,
//! that is a copy of the neighbour's first row of nodes. Without it the central
//! difference at a tile boundary would have to use a clamped index, and that
//! would give the two sides of the boundary different slopes -- that is a crack
//! exactly where R2b removed it.
//!
//! WARNING: **but the halo no longer goes into the file -- into neither of the
//! two.** It was needed by the gradient, the gradient moved here, and after that
//! there is nobody to read it: height never took it (the sampling is
//! deliberately clamped to the patch grid), and colour had it only because it
//! shared a grid with the terrain. So [`STORED`] is the shape of **this
//! function's input**, and [`NODES`] the shape of both files and both textures.
//!
//! The halo corners (`(-1, -1)` and the other three) are filled with nothing
//! meaningful and read by nobody: three patches meet at a cube corner, and a
//! central difference does not ask about the diagonal.
//!
//! ## The pyramid, and where it ends
//!
//! Tiles are cooked for levels 0 to [`Terrain::levels`] - 1. There is no point
//! going deeper -- the source is finite -- and a patch of a deeper level takes
//! its height from its ancestor's tile, bilinearly. That is not an
//! approximation for cheapness: a tile **finer than a source cell** contains
//! nothing new, it only costs memory.
//!
//! ## Heights are `int16`, uncompressed
//!
//! The fork was named in advance: BC4 for heights gives visible stair-stepping
//! on gentle slopes. A tile is small (33^2 nodes at four bytes -- 4356), so
//! compressing it trades visible quality for kilobytes. Compressing colour
//! (BC7/BC6H) is a different problem and a different step.

use crate::cubesphere::{Edge, Patch, FACES, SIDE};

/// The file signature. Eight bytes, so the header reads by eye in a hex
/// dump.
pub const MAGIC: [u8; 8] = *b"SSDEM\0\0\0";

/// The format version. Grows on any layout change -- a reader of the old
/// version must say so rather than read garbage.
///
/// Version 2 added the halo (R7b): the layout differs, so a version-1 tileset
/// cannot be read even "almost correctly". Version 3 is sea level (T7f).
/// Version 4 is the baked slope and the death of the halo in the file (stage W,
/// Q3).
pub const VERSION: u32 = 4;

/// The sea level of a body that has no sea.
///
/// A sentinel rather than an `Option`: in the file it is one word, and no `i16`
/// height is ever smaller than it, so "under water" never holds. The Moon
/// carries exactly this, and the material rule works on it just as before
/// T7f.
pub const NO_SEA: f32 = f32::MIN;

/// How many nodes per tile side -- the same as the patch grid has.
pub const NODES: usize = SIDE + 1;

/// The halo width in nodes (R7b) -- **on the input** of [`Terrain::build`], not
/// in the file. One: a central difference asks about exactly the neighbour, and
/// there is nothing to pay a second one for.
pub const HALO: usize = 1;

/// How many nodes per side the grid [`Terrain::build`] accepts has -- the patch
/// together with the halo.
///
/// WARNING: this is the shape of the **height cooker's input** and nothing else:
/// the halo has not gone into the file since version 4, and the colour tileset
/// has not had it since version 3 of its own format. The shape of both files and
/// both textures is [`NODES`].
pub const STORED: usize = NODES + 2 * HALO;

/// How much slope lies in one storage unit.
///
/// An engine constant rather than a header field, and that is a decision: slope
/// is dimensionless, so its range is the same for any body -- a field would mean
/// a parameter nobody will ever change.
///
/// The number is chosen by measurement rather than by roundness
/// (`--example slope_histogram`): the largest slope in the cooked assets is
/// **0.41** for the Moon (22.3 degrees) and 0.22 for Earth, so the `i16` ceiling
/// at this unit (3.2767, that is 73 degrees) leaves an eightfold margin. It is
/// needed: a deeper pyramid measures slope on a shorter base, and crater walls
/// on it will reach 40 degrees.
///
/// The quantum meanwhile is below everything visible through it. Three
/// consumers, all three computed: the material factor shifts by 2e-4 (that is
/// 0.05 of a byte level), the detail amplitude by 0.27 m at a wavelength of
/// 5.3 km, the level criterion by 0.003 pixels from a hundred kilometres.
pub const SLOPE_UNIT: f64 = 1.0e-4;

/// How many bytes one tile takes: two bounds plus the patch grid at two `i16`
/// each.
const TILE_BYTES: usize = 4 + NODES * NODES * 4;

/// The header: signature, version, three numbers, radius, scale and sea
/// level.
const HEADER_BYTES: usize = 8 + 4 + 4 + 4 + 8 + 4 + 4;

// -- Pyramid geometry -----------------------------------------------------
//
// Free functions rather than methods, precisely because there are **two**
// tilesets: terrain ([`Terrain`]) and colour ([`Colour`]). They share exactly
// one thing -- where a patch's tile lies in the pyramid -- and that depends only
// on the number of levels. The rest differs: the sample type, the channel count,
// the pyramid depth and what is in the header.
//
// Two copies of this arithmetic would diverge not at once but on the fourth
// edit, and the divergence would look like colour shifted by a tile -- that is,
// like a cooker bug. A generic instead is forbidden here by style (CLAUDE.md),
// and is not needed: there is one parameter and it is a `u32`.

/// How many tiles level `level` has.
pub fn per_level(level: u32) -> usize {
    FACES << (2 * level)
}

/// How many tiles a pyramid of `levels` levels has.
pub fn count(levels: u32) -> usize {
    (0..levels).map(per_level).sum()
}

/// A tile's ordinal number: level by level, within each face by face, within
/// each row by row.
///
/// The order is fixed and derived from the patch itself, without a table:
/// otherwise the cooker and the reader would have two ways to reach one
/// number.
pub fn index(levels: u32, patch: &Patch) -> Option<usize> {
    if patch.level >= levels {
        return None;
    }
    let before: usize = (0..patch.level).map(per_level).sum();
    let side = 1usize << patch.level;
    Some(before + (patch.face * side + patch.i as usize) * side + patch.j as usize)
}

/// The distance between neighbouring nodes of a level-`level` tile, metres.
///
/// A quarter of a great circle per face, `SIDE << level` nodes per face. This is
/// the base on which [`Terrain::build`] measures **this** level's slope: a tile
/// of a coarser level is smoothed more, and a difference in it must be divided
/// by its own step, otherwise the slope would depend on which level it was asked
/// from.
///
/// WARNING: the equiangular cubesphere warp makes the nodes uneven -- up to 1.4x
/// between the middle of a face and a corner -- and this number averages them.
/// Removed deliberately: both sides of a shared edge compute the same point, so
/// the slope there is the same regardless of the warp, and correcting for the
/// warp would make the slope map more precise than the bilinear sampling it
/// comes from.
pub fn node_step_m(reference_m: f64, level: u32) -> f64 {
    let nodes = f64::from(SIDE as u32 * (1u32 << level));
    std::f64::consts::FRAC_PI_2 * reference_m / nodes
}

/// The direction of a tile node on the unit sphere, halo included (R7b).
///
/// Inside the grid (`0..=SIDE`) this is simply a patch vertex. Outside it, it is
/// the **neighbour's** vertex, found through [`Patch::halo_node`] rather than by
/// continuing our own parameterisation: across a cube edge the face changes, and
/// with it the warp.
///
/// `None` is a halo corner, where there is no across-edge neighbour at all.
///
/// Lives here rather than in the cooker, and that is a stage-W move: since
/// version 4 the halo is the **input contract** of [`Terrain::build`], because
/// that is what computes the slope from it. The cooker remains a caller on a par
/// with the test fixtures.
pub fn node_direction(patch: &Patch, a: isize, b: isize) -> Option<[f64; 3]> {
    let edge_of = |v: isize| {
        if v < 0 {
            Some(true)
        } else if v > SIDE as isize {
            Some(false)
        } else {
            None
        }
    };

    let (edge, along) = match (edge_of(a), edge_of(b)) {
        (None, None) => return Some(patch.vertex(a as usize, b as usize, 1.0)),
        (Some(low), None) => (if low { Edge::AMin } else { Edge::AMax }, b as usize),
        (None, Some(low)) => (if low { Edge::BMin } else { Edge::BMax }, a as usize),
        // Both coordinates past the edge means a corner, not an edge.
        (Some(_), Some(_)) => return None,
    };

    let (there, na, nb) = patch.halo_node(edge, along);
    Some(there.vertex(na, nb, 1.0))
}

/// Slope -> storage units, saturating rather than wrapping.
///
/// The same policy as for heights in the cooker: a wrapped slope would turn a
/// crater wall into flat ground, and it would look plausible. Zero below --
/// slope is non-negative by construction (it is the length of a gradient).
fn quantise_slope(slope: f64) -> i16 {
    (slope / SLOPE_UNIT).round().clamp(0.0, f64::from(i16::MAX)) as i16
}

// -- The cube corner ------------------------------------------------------
//
// This is the answer to Q3, and it is what the slope moved into the asset for.
//
// **Why a central difference does not work at a corner at all.** Around a corner
// node lie **three** neighbours at one node's distance -- one on each of the
// three cube edges meeting there -- while each face's stencil has four arms. So
// two arms of every face land on one and the same node (measured by a probe at
// corner `(1,1,1)`, faces 0, 2, 4). Each face computes
// `sqrt((h_i - h_j)^2 + (h_i - h_k)^2)` with its own pivot `i`, and the three
// answers differ not through error but through the formula itself. The
// divergence between them was measured on the Moon: **39%**.
//
// So averaging the three numbers is impossible too: there is nothing to average.
//
// **What does work.** The three directions from a corner in the tangent plane
// lie at exactly 120 degrees -- the cube's threefold symmetry about a diagonal
// is exact rather than approximate -- so least squares over the three
// differences has a closed form:
//
//     g = (2 / 3L) * sum (h_i - h_0) * d_i
//
// The corner's own height cancels here (sum of d_i = 0), exactly as in an
// ordinary central difference, and for a linear field the result is **exact**.
// The difference from `h_0` is taken rather than `h_i` itself: on a constant
// field that gives an exact zero instead of the sum of three nearly opposite
// vectors.
//
// **The arm length is nominal** (`node_step_m`), not the true one. The
// cubesphere warp makes nodes uneven by up to 1.4x, and the present formula
// deliberately ignores that everywhere; taking the true length at a corner would
// make the corner more precise than the middle of a face, that is introduce a
// new non-uniformity in place of the removed one.

/// Write one number into all three faces of a cube corner -- the one they cannot
/// compute separately.
///
/// Bitwise sameness here is **by construction**: it is a copy of one `i16`, not
/// a coincidence of three computations.
fn resolve_cube_corners(
    levels: u32,
    reference_m: f64,
    scale_m: f32,
    grids: &[Vec<i16>],
    tiles: &mut [u8],
) {
    for level in 0..levels {
        let side = 1u32 << level;

        // Eight groups of three nodes. The group key is the signs of the
        // corner's own coordinates: all three components there are `1/sqrt(3)`
        // in magnitude, so only the sign tells them apart.
        let mut groups: [Vec<(Patch, usize, usize)>; 8] = Default::default();
        for face in 0..FACES {
            for (i, a) in [(0, 0), (side - 1, SIDE)] {
                for (j, b) in [(0, 0), (side - 1, SIDE)] {
                    let patch = Patch { face, level, i, j };
                    let at = patch.vertex(a, b, 1.0);
                    let key = usize::from(at[0] > 0.0)
                        | usize::from(at[1] > 0.0) << 1
                        | usize::from(at[2] > 0.0) << 2;
                    groups[key].push((patch, a, b));
                }
            }
        }

        for group in &groups {
            assert_eq!(
                group.len(),
                3,
                "exactly three faces must meet at a cube corner"
            );
            let (patch, a, b) = group[0];
            let slope = corner_slope(&patch, a, b, grids, levels, reference_m, scale_m);
            let units = quantise_slope(slope);
            for (patch, a, b) in group {
                let at = index(levels, patch).expect("the corner patch is in the pyramid");
                let cell = at * TILE_BYTES + 4 + (a * NODES + b) * 4 + 2;
                tiles[cell..cell + 2].copy_from_slice(&units.to_le_bytes());
            }
        }
    }
}

/// The slope at a corner node -- a gradient fit over three neighbours.
///
/// All three neighbours are read from **one** tile: two lie inside its face, the
/// third in the halo. That leaving across the edge along `a` and leaving across
/// the edge along `b` give the same node at a corner is exactly the degeneracy
/// that makes a central difference fail here.
fn corner_slope(
    patch: &Patch,
    a: usize,
    b: usize,
    grids: &[Vec<i16>],
    levels: u32,
    reference_m: f64,
    scale_m: f32,
) -> f64 {
    let grid = &grids[index(levels, patch).expect("the corner patch is in the pyramid")];
    let node = |a: isize, b: isize| {
        f64::from(grid[(a + HALO as isize) as usize * STORED + (b + HALO as isize) as usize])
    };
    let (a, b) = (a as isize, b as isize);

    // A step inward from the corner: forward from node 0, backward from node
    // `SIDE`. The opposite step leaves across the edge, that is into the halo.
    let inward = |v: isize| if v == 0 { 1 } else { -1 };
    let (da, db) = (inward(a), inward(b));

    let centre = patch.vertex(a as usize, b as usize, 1.0);
    let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];

    // Three neighbours: two inside the face and one across the edge. There is
    // no fourth.
    let mut gradient = [0.0f64; 3];
    for (na, nb) in [(a + da, b), (a, b + db), (a - da, b)] {
        let there = node_direction(patch, na, nb).expect("a corner neighbour is not a halo corner");
        // The tangential component of the direction to the neighbour, unit
        // length.
        let radial = dot(there, centre);
        let mut unit = [0.0; 3];
        for k in 0..3 {
            unit[k] = there[k] - radial * centre[k];
        }
        let length = dot(unit, unit).sqrt();
        let rise = node(na, nb) - node(a, b);
        for k in 0..3 {
            gradient[k] += rise * unit[k] / length;
        }
    }

    // `2/3` is the least-squares factor for three directions at 120 degrees:
    // sum of d_i d_i^T = (3/2)*I in the tangent plane.
    let step_m = node_step_m(reference_m, patch.level);
    dot(gradient, gradient).sqrt() * 2.0 * f64::from(scale_m) / (3.0 * step_m)
}

/// The patch whose tile covers this patch: itself or the nearest ancestor in the
/// pyramid.
///
/// Along with it -- how many times coarser the tile is, that is by how much the
/// local coordinates must be divided.
pub fn covering(levels: u32, patch: &Patch) -> (Patch, u32) {
    let mut it = *patch;
    while it.level >= levels {
        it = it.parent().expect("level 0 is always in the pyramid");
    }
    (it, patch.level - it.level)
}

/// Where a patch looks inside the tile covering it (R7a).
///
/// Three numbers: the tile's index in the pyramid, the patch's offset inside it
/// **in nodes**, and the step -- how many ancestor nodes fall on one patch node.
/// At `deeper == 0` that is `(0, 0)` and `1.0`.
///
/// The shader does the same sampling, and two independent computations of the
/// same window would diverge -- not at once but on the fourth edit. Now it is one
/// formula, and the test compares the GPU against it rather than against a second
/// copy of it.
pub fn window(levels: u32, patch: &Patch) -> (usize, [f64; 2], f64) {
    let (tile, deeper) = covering(levels, patch);
    let at = index(levels, &tile).expect("covering has already lowered the level");

    // `SIDE` ancestor nodes across `2^deeper` children -- so the step is
    // fractional.
    let step = 1.0 / f64::from(1u32 << deeper);
    let offset = |index: u32| f64::from(index % (1 << deeper)) * SIDE as f64 * step;
    (at, [offset(patch.i), offset(patch.j)], step)
}

/// One body's terrain -- a pyramid of tiles over cubesphere patches.
#[derive(Clone, Debug)]
pub struct Terrain {
    /// How many pyramid levels, from 0 inclusive.
    pub levels: u32,
    /// The reference radius heights are measured from, metres.
    pub reference_m: f64,
    /// How many metres are in one storage unit.
    pub scale_m: f32,
    /// Sea level in storage units, or [`NO_SEA`] for a dry body (T7f).
    ///
    /// WARNING: it lives here rather than in `Scene::Body`, and the reason is not
    /// convenience: sea level is expressed in **this DEM's datum**, that is it
    /// only means anything together with `reference_m` and `scale_m`. A scene
    /// field would allow a state where the game says "zero" while the asset
    /// measures heights from a different zero -- and the sea would stand a
    /// kilometre above or below the mosaic's coastline, silently.
    ///
    /// Why it is needed at all: the material rule highlights slope, and under
    /// water what is visible is not the slope of the sea floor but the surface of
    /// the sea. Measured on `assets/earth.dem` -- the floor is **steeper** than
    /// the land (median 0.0071 against 0.0030, 90% 0.0333 against 0.0201), so
    /// without this field the rule draws mid-ocean ridges on water.
    pub sea_units: f32,
    /// The tiles in a row in canonical order -- see [`Terrain::index`].
    tiles: Vec<u8>,
}

impl Terrain {
    /// How many tiles level `level` has.
    fn per_level(level: u32) -> usize {
        crate::tiles::per_level(level)
    }

    /// How many tiles a pyramid of `levels` levels has.
    pub fn count(levels: u32) -> usize {
        crate::tiles::count(levels)
    }

    /// The tile's ordinal number in the pyramid.
    pub fn index(&self, patch: &Patch) -> Option<usize> {
        crate::tiles::index(self.levels, patch)
    }

    /// The patch whose tile covers this patch: itself or the nearest
    /// ancestor.
    pub fn covering(&self, patch: &Patch) -> (Patch, u32) {
        crate::tiles::covering(self.levels, patch)
    }

    /// A tile's height bounds in storage units: lowest and highest.
    ///
    /// This is what R3a expected from tiles: the occluder radius is measured
    /// from the **lowest** point rather than from the body's mean radius.
    ///
    /// The halo is not included: the bounds describe **this** tile, not a band
    /// around it. A neighbour's hollow would lower the body's occluder radius in
    /// a place where there is none.
    pub fn bounds(&self, index: usize) -> (i16, i16) {
        let at = index * TILE_BYTES;
        (
            i16::from_le_bytes([self.tiles[at], self.tiles[at + 1]]),
            i16::from_le_bytes([self.tiles[at + 2], self.tiles[at + 3]]),
        )
    }

    /// A tile node's sample: channel 0 is height, channel 1 slope, both in
    /// storage units ([`Terrain::scale_m`] and [`SLOPE_UNIT`]).
    ///
    /// The indices are patch coordinates, `0..=SIDE`, and no others: since
    /// version 4 there is no halo in the file (see the module introduction).
    fn channel(&self, index: usize, a: i32, b: i32, channel: usize) -> i16 {
        let check = |v: i32| {
            assert!(
                (0..NODES as i32).contains(&v),
                "node {v} is outside the tile"
            );
            v as usize
        };
        let at = index * TILE_BYTES + 4 + (check(a) * NODES + check(b)) * 4 + channel * 2;
        i16::from_le_bytes([self.tiles[at], self.tiles[at + 1]])
    }

    /// A tile node's height in storage units.
    pub fn node(&self, index: usize, a: i32, b: i32) -> i16 {
        self.channel(index, a, b, 0)
    }

    /// The slope at a tile node in [`SLOPE_UNIT`] units -- what
    /// [`Terrain::build`] baked.
    pub fn slope_node(&self, index: usize, a: i32, b: i32) -> i16 {
        self.channel(index, a, b, 1)
    }

    /// One tile's raw bytes -- what will go into a texture (R5c).
    pub fn tile_bytes(&self, index: usize) -> &[u8] {
        let at = index * TILE_BYTES;
        &self.tiles[at + 4..at + TILE_BYTES]
    }

    /// Where a patch looks inside the tile covering it (R7a).
    pub fn window(&self, patch: &Patch) -> (usize, [f64; 2], f64) {
        crate::tiles::window(self.levels, patch)
    }

    /// A bilinear sample of a channel at node `(a, b)` of the given patch, in
    /// storage units.
    ///
    /// If the patch is deeper than the pyramid, the sample is taken from the
    /// ancestor's tile: the patch node lies between the coarser tile's nodes.
    /// Shared between height and slope on purpose -- they have one window, and
    /// two computations of it would diverge.
    fn sample(&self, patch: &Patch, a: usize, b: usize, channel: usize) -> f64 {
        let (index, origin, step) = self.window(patch);
        if step == 1.0 {
            return f64::from(self.channel(index, a as i32, b as i32, channel));
        }

        let x = origin[0] + a as f64 * step;
        let y = origin[1] + b as f64 * step;

        let (x0, y0) = (x.floor(), y.floor());
        let (tx, ty) = (x - x0, y - y0);
        let (x0, y0) = (x0 as usize, y0 as usize);
        let get = |dx: usize, dy: usize| {
            f64::from(self.channel(
                index,
                (x0 + dx).min(SIDE) as i32,
                (y0 + dy).min(SIDE) as i32,
                channel,
            ))
        };
        let top = get(0, 0) * (1.0 - ty) + get(0, 1) * ty;
        let bottom = get(1, 0) * (1.0 - ty) + get(1, 1) * ty;
        top * (1.0 - tx) + bottom * tx
    }

    /// The height at node `(a, b)` of the given patch, metres.
    pub fn height_m(&self, patch: &Patch, a: usize, b: usize) -> f64 {
        self.sample(patch, a, b, 0) * f64::from(self.scale_m)
    }

    /// The surface slope at a patch node -- dimensionless, metres per metre
    /// (R7c).
    ///
    /// This is what R7c takes the **amplitude** of the procedural detail from:
    /// the noise continues the character of the terrain rather than lying over it
    /// as an even carpet. On flat ground the slope is small and there is almost
    /// no detail; on a crater wall the opposite. Level selection (R7c) and the
    /// material rule (T4) read it too.
    ///
    /// Since stage W this is a **read** rather than a computation: the slope is
    /// baked into the node ([`Terrain::build`]), so on the shared edge of two
    /// patches it is bitwise one -- and at a cube corner too, where three faces
    /// would otherwise give three different numbers.
    ///
    /// WARNING: a patch deeper than the pyramid now **interpolates the slope**
    /// rather than differentiating an interpolated height. At tile nodes both
    /// routes give the same number (see `build`), between nodes a different one,
    /// and both are continuous; the first was chosen because it costs the same
    /// sample as the height.
    pub fn slope_at(&self, patch: &Patch, a: usize, b: usize) -> f64 {
        self.sample(patch, a, b, 1) * SLOPE_UNIT
    }

    /// The distance between neighbouring nodes of the pyramid's finest level,
    /// metres.
    ///
    /// It is also the wavelength of the coarsest octave of the procedural detail
    /// ([`crate::detail`]): the detail begins exactly where the data ends.
    ///
    /// WARNING: it has stopped being the base for the slope: since stage W every
    /// level measures slope on **its own** step ([`node_step_m`]), and that is
    /// why the baked number does not depend on which level it was asked from.
    pub fn step_m(&self) -> f64 {
        node_step_m(self.reference_m, self.levels.saturating_sub(1))
    }

    /// The lowest point of the whole terrain, metres above the reference radius.
    ///
    /// Level 0 covers the body entirely, so there is no need to walk the whole
    /// pyramid.
    pub fn lowest_m(&self) -> f64 {
        let mut low = i16::MAX;
        for index in 0..Terrain::per_level(0) {
            low = low.min(self.bounds(index).0);
        }
        f64::from(low) * f64::from(self.scale_m)
    }

    /// Assemble a set from ready tiles -- the cooker's path.
    ///
    /// The tiles are supplied in canonical order and **with the halo**:
    /// [`STORED`]x[`STORED`] heights, row by row, from node `-HALO`.
    ///
    /// ## The slope is computed here rather than in the cooker, and that is a
    /// decision
    ///
    /// The invariant "the slope at a shared node is one number" belongs to the
    /// **format**, not to whoever fills it in: the writer already receives the
    /// halo and sees the whole pyramid, so both height cookers (the Moon from
    /// LOLA, Earth from ETOPO) get the invariant for free, without a single line
    /// of their own. A cooker that computed the slope itself would be a second
    /// place where this arithmetic lives -- and they would diverge not at once
    /// but on the fourth edit.
    ///
    /// ## The base of the difference is **its own** level's step
    ///
    /// A level-`L` tile is smoothed by its own level, and a difference in it must
    /// be divided by its own [`node_step_m`]. Then the baked number does not
    /// depend on which level the patch asked from: two neighbouring patches
    /// covered by tiles of different levels (balancing allows a one-level
    /// difference) get a slope of the same magnitude rather than one twice as
    /// large.
    ///
    /// WARNING: **this is bitwise the same number the frame computed before stage
    /// W.** The formula then took the difference at the pyramid's finest step and
    /// converted it into its own tile's coordinates; at a whole node a bilinear
    /// sample at `+-delta` gives exactly `delta*(h[x+1] - h[x-1])`, and the
    /// factors cancel -- what remains is a central difference at the tile's own
    /// step. So the move changes not the numbers but who computes them.
    ///
    /// The halo is needed for exactly this: without it a difference at a node on
    /// a tile boundary would hit a clamped index and give the two sides of a
    /// shared edge different slopes (R7b). The halo does not go into the file.
    pub fn build(
        levels: u32,
        reference_m: f64,
        scale_m: f32,
        sea_units: f32,
        grids: &[Vec<i16>],
    ) -> Terrain {
        assert_eq!(
            grids.len(),
            Terrain::count(levels),
            "not as many tiles as a pyramid of {levels} levels must have"
        );
        let mut tiles = Vec::with_capacity(grids.len() * TILE_BYTES);
        let mut at = 0usize;
        for level in 0..levels {
            // Height storage units -> slope, at this level's own step.
            let rise = f64::from(scale_m) / (2.0 * node_step_m(reference_m, level));
            for _ in 0..Terrain::per_level(level) {
                let grid = &grids[at];
                at += 1;
                assert_eq!(grid.len(), STORED * STORED, "the tile has the wrong shape");
                let node = |a: isize, b: isize| {
                    grid[(a + HALO as isize) as usize * STORED + (b + HALO as isize) as usize]
                };

                // The bounds are over the patch grid, without the halo (see
                // `bounds`), and so are computed in the same pass as the samples
                // themselves.
                let mut low = i16::MAX;
                let mut high = i16::MIN;
                let mut body = Vec::with_capacity(NODES * NODES * 2);
                for a in 0..NODES as isize {
                    for b in 0..NODES as isize {
                        let height = node(a, b);
                        low = low.min(height);
                        high = high.max(height);

                        // The difference is taken in `f64` already: two
                        // saturated `i16`s of opposite signs do not fit in an
                        // `i16`, and saturation in the source is a thing that
                        // really happens (`quantise`).
                        let du = (f64::from(node(a + 1, b)) - f64::from(node(a - 1, b))) * rise;
                        let dv = (f64::from(node(a, b + 1)) - f64::from(node(a, b - 1))) * rise;
                        body.push(height);
                        body.push(quantise_slope((du * du + dv * dv).sqrt()));
                    }
                }

                tiles.extend_from_slice(&low.to_le_bytes());
                tiles.extend_from_slice(&high.to_le_bytes());
                for value in body {
                    tiles.extend_from_slice(&value.to_le_bytes());
                }
            }
        }

        resolve_cube_corners(levels, reference_m, scale_m, grids, &mut tiles);

        Terrain {
            levels,
            reference_m,
            scale_m,
            sea_units,
            tiles,
        }
    }

    /// The file bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_BYTES + self.tiles.len());
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(NODES as u32).to_le_bytes());
        out.extend_from_slice(&self.levels.to_le_bytes());
        out.extend_from_slice(&self.reference_m.to_le_bytes());
        out.extend_from_slice(&self.scale_m.to_le_bytes());
        out.extend_from_slice(&self.sea_units.to_le_bytes());
        out.extend_from_slice(&self.tiles);
        out
    }

    /// Parse the file bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Terrain, String> {
        if bytes.len() < HEADER_BYTES {
            return Err(format!("{} bytes is not even a header", bytes.len()));
        }
        if bytes[..8] != MAGIC {
            return Err("wrong signature: this is not a tileset".to_string());
        }
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let version = word(8);
        if version != VERSION {
            return Err(format!(
                "format version {version}, while this engine reads {VERSION}"
            ));
        }
        let nodes = word(12) as usize;
        if nodes != NODES {
            return Err(format!(
                "a tile of {nodes} nodes against a patch grid of {NODES} -- they do not match"
            ));
        }
        let levels = word(16);
        let reference_m = f64::from_le_bytes(bytes[20..28].try_into().unwrap());
        let scale_m = f32::from_le_bytes(bytes[28..32].try_into().unwrap());
        let sea_units = f32::from_le_bytes(bytes[32..36].try_into().unwrap());

        let tiles = bytes[HEADER_BYTES..].to_vec();
        let wanted = Terrain::count(levels) * TILE_BYTES;
        if tiles.len() != wanted {
            return Err(format!(
                "{} bytes of tiles instead of {wanted} for {levels} levels",
                tiles.len()
            ));
        }

        Ok(Terrain {
            levels,
            reference_m,
            scale_m,
            sea_units,
            tiles,
        })
    }
}

/// The colour tileset's signature. A separate file, not a second channel in the
/// terrain.
pub const COLOUR_MAGIC: [u8; 8] = *b"SSCOL\0\0\0";

/// The colour format's version -- its own, independent of the terrain's.
///
/// Version 2 added the sample space (`srgb`): without it a one-byte "colour"
/// field would mean different things for the Moon and for Earth, and the only
/// way to tell which would be the channel count -- that is, a guess (T7e).
/// Version 3 threw out the halo (stage W, W4).
pub const COLOUR_VERSION: u32 = 3;

/// The colour header: signature, version, nodes, levels, channels, scale,
/// space.
const COLOUR_HEADER_BYTES: usize = 8 + 4 + 4 + 4 + 4 + 4 + 4;

/// Surface colour -- the same tile pyramid as the terrain (stage T, T2c).
///
/// ## Why a separate file rather than a second channel in [`Terrain`]
///
/// The fork ("one file with two channels or two tilesets") was closed by the T2a
/// measurement, and closed with numbers rather than taste: heights and colour
/// have **different pyramid depths** (5 against 6, because the sources have
/// different resolutions) and **different sample types** (`i16` against `u8`). A
/// shared file would have to either equalise the depths -- that is, inflate the
/// heights fourfold with nothing -- or carry holes. What stays shared is the
/// pyramid geometry, and it really is shared: the free functions at the top of
/// this file.
///
/// **Their versions are separate too, and grow independently.** The signature
/// differs (`SSCOL` against `SSDEM`), so the files cannot be confused, while a
/// shared version would mean a change in one discards the other's cooked files
/// -- for a change they do not contain. Today that is visible in the numbers:
/// the terrain is at version 4, the colour at 3.
///
/// ## Why one channel, and what happens with Earth
///
/// The LROC WAC mosaic is monochrome (643 nm), so the Moon has one channel --
/// that is the source, not an economy. Earth (T7) will need four: `Rgba8Unorm`
/// is the narrowest texture format that has colour, because **a three-byte
/// format does not exist in wgpu or in Vulkan** without extensions. So the
/// channel count is a header field, and **the file holds exactly what will go
/// into the texture**: a conversion at load time is a place where bytes can go
/// astray, and the terrain deliberately has none (R5c).
///
/// ## Why the samples are integers while the source is real
///
/// The source carries reflectance in `f32`, and storing it that way would mean a
/// tile four times larger for a dynamic range the surface does not have.
/// Measured over the whole mosaic: median 0.044, ninety percent below 0.076,
/// 99.9% below 0.197, and the tail to 0.599 is 0.09% of pixels. So 256 levels
/// over the range `0 ... scale` give a step of 0.00098 at `scale = 0.25`, that
/// is **23 gradations across the sea-continent contrast** (0.021 against 0.044).
/// The tail saturates to white, and that is more honest than spending 96% of the
/// scale on 0.09% of pixels.
#[derive(Clone, Debug)]
pub struct Colour {
    /// How many pyramid levels, from 0 inclusive.
    pub levels: u32,
    /// How many bytes per node: 1 for the Moon, 4 for Earth.
    pub channels: u32,
    /// What a sample of 255 equals -- reflectance, dimensionless.
    pub scale: f32,
    /// Whether the samples are encoded in sRGB rather than linearly.
    ///
    /// The Moon: no -- WAC carries physical reflectance, and the byte holds it
    /// directly. Earth: yes -- BMNG is a picture for the eye, and a linear value
    /// in a byte would give the ocean (0.0015) zero, that is a black hole instead
    /// of water.
    ///
    /// A field rather than a convention "four channels means sRGB": both
    /// consumers must know the space explicitly -- the texture chooses between
    /// `Rgba8Unorm` and `Rgba8UnormSrgb`, and [`Colour::reflectance`] decodes on
    /// its own, so everything the CPU reads stays linear regardless of the
    /// body.
    pub srgb: bool,
    /// The tiles in a row in canonical order -- see [`index`].
    tiles: Vec<u8>,
}

impl Colour {
    /// How many bytes one tile takes.
    pub fn tile_len(channels: u32) -> usize {
        NODES * NODES * channels as usize
    }

    /// Assemble a set from ready tiles -- the cooker's path.
    ///
    /// The tiles are supplied in canonical order: [`NODES`]x[`NODES`] nodes, row
    /// by row, at `channels` bytes per node.
    ///
    /// WARNING: there is no halo here and there never needed to be one (version
    /// 3, W4). It lived in the colour only because both tilesets shared one grid
    /// with the terrain, and the terrain needed it for the gradient. Nobody read
    /// it even there: a bilinear sample at the very edge of a patch gives it a
    /// weight of exactly zero.
    pub fn build(levels: u32, channels: u32, scale: f32, srgb: bool, grids: &[Vec<u8>]) -> Colour {
        assert!(
            channels == 1 || channels == 4,
            "channels are 1 or 4, not {channels}"
        );
        assert_eq!(
            grids.len(),
            count(levels),
            "not as many tiles as a pyramid of {levels} levels must have"
        );
        let tile_len = Colour::tile_len(channels);
        let mut tiles = Vec::with_capacity(grids.len() * tile_len);
        for grid in grids {
            assert_eq!(grid.len(), tile_len, "the tile has the wrong shape");
            tiles.extend_from_slice(grid);
        }
        Colour {
            levels,
            channels,
            scale,
            srgb,
            tiles,
        }
    }

    /// One tile's raw bytes -- exactly what will go into a texture.
    pub fn tile_bytes(&self, index: usize) -> &[u8] {
        let len = Colour::tile_len(self.channels);
        &self.tiles[index * len..(index + 1) * len]
    }

    /// A tile node's sample in storage units.
    ///
    /// The indices are patch coordinates, `0..=SIDE`, as for the terrain.
    pub fn node(&self, index: usize, a: i32, b: i32, channel: u32) -> u8 {
        assert!(
            channel < self.channels,
            "channel {channel} is outside the tile"
        );
        let check = |v: i32| {
            assert!(
                (0..NODES as i32).contains(&v),
                "node {v} is outside the tile"
            );
            v as usize
        };
        let at = index * Colour::tile_len(self.channels)
            + (check(a) * NODES + check(b)) * self.channels as usize
            + channel as usize;
        self.tiles[at]
    }

    /// A node's reflectance -- the same as [`Colour::node`], in the source's
    /// units and **always linear**.
    ///
    /// If the tileset is encoded in sRGB ([`Colour::srgb`]), decoding happens
    /// here: the CPU consumer (planetshine, T6) asks about light rather than
    /// about a byte, and the difference between them over a dark ocean is
    /// twentyfold.
    pub fn reflectance(&self, index: usize, a: i32, b: i32, channel: u32) -> f64 {
        let unit = f64::from(self.node(index, a, b, channel)) / 255.0;
        let linear = if self.srgb {
            if unit <= 0.04045 {
                unit / 12.92
            } else {
                ((unit + 0.055) / 1.055).powf(2.4)
            }
        } else {
            unit
        };
        linear * f64::from(self.scale)
    }

    /// The body's mean reflectance per channel, linear (T7h).
    ///
    /// One number per planet is exactly what the multiple-scattering table
    /// requires: it is built once per body rather than per point, so there is
    /// nowhere to ask it about local albedo.
    ///
    /// Taken from the **coarsest** pyramid level, and for the same reason as in
    /// [`Colour::under`]: the source here is the whole visible hemisphere, and a
    /// finer level would promise a precision the problem does not have.
    ///
    /// WARNING: **the mean is unweighted, that is over nodes rather than over
    /// area.** The equiangular cubesphere stretches a cell towards a face corner
    /// by about a factor of two, so corners weigh more than they should.
    /// Correcting that with a metric weight is not worth it here: the oracle for
    /// the number is the mean of the **mosaic itself** in a latitude-longitude
    /// grid, computed by the cooker by an entirely different route, and it also
    /// says how large the bias really is.
    pub fn mean(&self) -> [f64; 3] {
        let mut sum = [0.0; 3];
        let mut count = 0.0;
        for index in 0..per_level(0) {
            for a in 0..NODES as i32 {
                for b in 0..NODES as i32 {
                    for (channel, total) in sum.iter_mut().enumerate() {
                        // A single-channel tileset (the Moon) is grey: the same
                        // sample into all three channels.
                        let read = if self.channels == 1 {
                            0
                        } else {
                            channel as u32
                        };
                        *total += self.reflectance(index, a, b, read);
                    }
                    count += 1.0;
                }
            }
        }
        sum.map(|total| total / count)
    }

    /// Reflectance in direction `direction` from the body's centre (T6c).
    ///
    /// The direction is in **body space**: the caller removes the planet's
    /// rotation, because the tileset is nailed to the surface rather than to the
    /// world.
    ///
    /// ## Why the coarsest level rather than the finest
    ///
    /// The question this sampling exists for is "what light does the planet shine
    /// on the ship with". The source there is **a disc hundreds of kilometres
    /// across**, not a point: from 100 km above the Moon the visible cap is over
    /// 800 km wide. The pyramid's finest level would answer at a resolution of
    /// 1.3 km -- that is, promise a precision the problem itself does not have,
    /// and the shine would flicker with every crater under the ship.
    ///
    /// Level zero, meanwhile, is **averaged by construction**: its nodes are
    /// gathered by a chain of twice-coarser grids (T3c) rather than decimated. A
    /// node there is 85 km on the Moon, that is exactly the scale at which a sea
    /// differs from a continent, and that is the number step T6 measures.
    pub fn under(&self, direction: [f64; 3], channel: u32) -> f64 {
        let at = crate::cubesphere::locate(direction);
        let tile = index(
            self.levels,
            &Patch {
                face: at.face,
                level: 0,
                i: 0,
                j: 0,
            },
        )
        .expect("level zero is in every pyramid");
        let node = |v: f64| (v * SIDE as f64).round().clamp(0.0, SIDE as f64) as i32;
        self.reflectance(tile, node(at.s), node(at.t), channel)
    }

    /// The file bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(COLOUR_HEADER_BYTES + self.tiles.len());
        out.extend_from_slice(&COLOUR_MAGIC);
        out.extend_from_slice(&COLOUR_VERSION.to_le_bytes());
        out.extend_from_slice(&(NODES as u32).to_le_bytes());
        out.extend_from_slice(&self.levels.to_le_bytes());
        out.extend_from_slice(&self.channels.to_le_bytes());
        out.extend_from_slice(&self.scale.to_le_bytes());
        out.extend_from_slice(&u32::from(self.srgb).to_le_bytes());
        out.extend_from_slice(&self.tiles);
        out
    }

    /// Parse the file bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Colour, String> {
        if bytes.len() < COLOUR_HEADER_BYTES {
            return Err(format!("{} bytes is not even a header", bytes.len()));
        }
        if bytes[..8] != COLOUR_MAGIC {
            return Err("wrong signature: this is not a colour tileset".to_string());
        }
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let version = word(8);
        if version != COLOUR_VERSION {
            return Err(format!(
                "colour format version {version}, while this engine reads {COLOUR_VERSION}"
            ));
        }
        let nodes = word(12) as usize;
        if nodes != NODES {
            return Err(format!(
                "a tile of {nodes} nodes against a patch grid of {NODES} -- they do not match"
            ));
        }
        let levels = word(16);
        let channels = word(20);
        if channels != 1 && channels != 4 {
            return Err(format!(
                "{channels} channels -- there is no texture of that format in wgpu \
                 or in Vulkan"
            ));
        }
        let scale = f32::from_le_bytes(bytes[24..28].try_into().unwrap());
        let srgb = match word(28) {
            0 => false,
            1 => true,
            other => return Err(format!("sample space {other}: it is 0 or 1")),
        };

        let tiles = bytes[COLOUR_HEADER_BYTES..].to_vec();
        let wanted = count(levels) * Colour::tile_len(channels);
        if tiles.len() != wanted {
            return Err(format!(
                "{} bytes of tiles instead of {wanted} for {levels} levels at {channels} channels",
                tiles.len()
            ));
        }

        Ok(Colour {
            levels,
            channels,
            scale,
            srgb,
            tiles,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pyramid in which a node's height is a known function of its position on
    /// the face.
    ///
    /// Linear in both coordinates on purpose: bilinear sampling reproduces a
    /// linear function **exactly**, so any divergence in the test below is an
    /// addressing error rather than interpolation error. An oracle that confuses
    /// the two is not an oracle.
    fn ramp(levels: u32) -> Terrain {
        let mut grids = Vec::with_capacity(Terrain::count(levels));
        for level in 0..levels {
            let side = 1u32 << level;
            // The face does not enter the height: the ramp is the same on all
            // six, and that is why it catches an addressing error rather than
            // masking it behind a face.
            for _face in 0..FACES {
                for i in 0..side {
                    for j in 0..side {
                        // The node in face fractions is a quantity that is the
                        // same at every level. That is what makes the pyramid
                        // consistent: tiles of level 0 and level 2 at the same
                        // point of a face carry the same number, as they must in
                        // a real pyramid.
                        //
                        // The multipliers are chosen so the values stay integral
                        // at every level's nodes: otherwise rounding to `i16`
                        // would itself create the divergence the test looks for.
                        //
                        // The halo here simply continues the same straight line.
                        // For a fixture that is honest: `height_m` never reads
                        // nodes `-1` and `SIDE + 1` at all, and equality with a
                        // neighbour is checked by the cooker on real data, not by
                        // this ramp.
                        let span = (SIDE << level) as f64;
                        let mut grid = Vec::with_capacity(STORED * STORED);
                        for a in 0..STORED {
                            for b in 0..STORED {
                                let a = a as isize - HALO as isize;
                                let b = b as isize - HALO as isize;
                                let x = (i as isize * SIDE as isize + a) as f64 / span;
                                let y = (j as isize * SIDE as isize + b) as f64 / span;
                                grid.push((2048.0 * x + 4096.0 * y) as i16);
                            }
                        }
                        grids.push(grid);
                    }
                }
            }
        }
        Terrain::build(levels, 1_000_000.0, 1.0, NO_SEA, &grids)
    }

    /// The slope of a ramp is the ramp, and that number is known in advance.
    ///
    /// The fixture is linear in face fractions: `2048*x + 4096*y` units. So the
    /// slope is analytic -- `sqrt(2048^2 + 4096^2)` units per face fraction,
    /// divided by a quarter of a great circle -- and depends neither on the
    /// level, nor on the node, nor on whether the patch is deeper than the
    /// pyramid.
    ///
    /// That is exactly why this is an oracle rather than an observation: **nodes
    /// on the patch edge are computed through the halo** (the one arriving in
    /// [`Terrain::build`]), and any shift in its addressing would spoil precisely
    /// those while leaving the middle correct. So nodes are taken from both edges
    /// and from the middle.
    ///
    /// WARNING: **compared against the quantised analytic value, not against a
    /// tolerance.** The slope now lies in the file as an integer
    /// ([`SLOPE_UNIT`]), so "almost the same number" does not happen here at all:
    /// either exactly what the analytic value would give was baked, or the
    /// addressing is wrong. A tolerance would hide a half-quantum error, and the
    /// quantum here is 3.4% of the fixture's own slope.
    #[test]
    fn the_slope_of_a_ramp_is_the_ramp() {
        const LEVELS: u32 = 3;
        let terrain = ramp(LEVELS);

        // Units per face fraction -> metres per metre.
        let expected = (2048.0f64.powi(2) + 4096.0f64.powi(2)).sqrt() * f64::from(terrain.scale_m)
            / (std::f64::consts::FRAC_PI_2 * terrain.reference_m);
        let want = f64::from(quantise_slope(expected)) * SLOPE_UNIT;

        let mut checked = 0;
        let mut skipped = 0;
        for level in [0, 1, 2, 3, 5] {
            let side = 1u32 << level;
            for (face, i, j) in [(0, 0, 0), (3, side / 2, side - 1), (5, side - 1, 0)] {
                let patch = Patch { face, level, i, j };
                for (a, b) in [(0, 0), (0, SIDE), (SIDE, 0), (SIDE, SIDE), (SIDE / 2, 7)] {
                    // WARNING: this fixture has no right to check cube corner
                    // nodes, and that is not a weakening but a limit of the
                    // fixture itself: the ramp is linear in the **face's**
                    // parameters, and at a corner there are three different ones
                    // -- so there is no smooth field there at all and no "right
                    // answer" either. The corner is checked by `tilt`, which is
                    // linear in 3D.
                    let span = SIDE << level;
                    let (u, v) = (i as usize * SIDE + a, j as usize * SIDE + b);
                    if (u == 0 || u == span) && (v == 0 || v == span) {
                        skipped += 1;
                        continue;
                    }
                    let got = terrain.slope_at(&patch, a, b);
                    assert_eq!(
                        got.to_bits(),
                        want.to_bits(),
                        "{patch:?} node ({a}, {b}): slope {got:.6e} instead of \
                         {want:.6e} -- the halo or the difference base addresses the wrong place"
                    );
                    checked += 1;
                }
            }
        }

        println!(
            "  ramp slope {expected:.6e} -> {want:.6e} after quantisation; \
             {checked} nodes bitwise identical, {skipped} cube corners skipped"
        );
        assert!(skipped > 0, "no corner came up -- the fixture has shrunk");
    }

    // -- The cube corner: a fixture linear in 3D ---------------------------
    //
    // WARNING: **the ramp above is no good as a corner oracle, and that is not a
    // detail.** It is linear in the **face's** parameters, and at a corner three
    // faces meet with three different parameterisations -- there the ramp is not
    // a smooth field at all, so it has no "right answer" for a corner.
    //
    // Hence a second fixture: the height is linear in **three-dimensional
    // space**, `h = A*(d.k)`. Such a field is the same from any face by
    // construction, and its slope on the sphere is known analytically --
    // `A*scale/R * sin(theta)`, where theta is the angle between the direction
    // and `k`. The oracle exists at every point of the body, a corner
    // included.

    /// The fixture's axis -- deliberately neither along an axis nor along a cube
    /// diagonal.
    ///
    /// A symmetric direction would give an accidentally correct answer at a
    /// corner -- the same class of trap as a camera above a face centre (D13,
    /// D14).
    const TILT: [f64; 3] = [
        0.267_261_241_912_424_4,
        0.534_522_483_824_848_8,
        0.801_783_725_737_273_2,
    ];

    /// The fixture's amplitude in storage units. Large on purpose: the slope
    /// must come out at hundreds of quanta, otherwise quantisation would eat the
    /// measurement itself.
    const TILT_UNITS: f64 = 30_000.0;

    /// A pyramid whose height is a linear function of the direction.
    fn tilt(levels: u32) -> Terrain {
        let mut grids = Vec::with_capacity(Terrain::count(levels));
        for level in 0..levels {
            let side = 1u32 << level;
            for face in 0..FACES {
                for i in 0..side {
                    for j in 0..side {
                        let patch = Patch { face, level, i, j };
                        let mut grid = Vec::with_capacity(STORED * STORED);
                        for a in 0..STORED as isize {
                            for b in 0..STORED as isize {
                                let (a, b) = (a - HALO as isize, b - HALO as isize);
                                // Nobody needs a halo corner -- zero, as in the
                                // cooker.
                                let value = match node_direction(&patch, a, b) {
                                    Some(d) => {
                                        let dot = d[0] * TILT[0] + d[1] * TILT[1] + d[2] * TILT[2];
                                        (TILT_UNITS * dot).round()
                                    }
                                    None => 0.0,
                                };
                                grid.push(value as i16);
                            }
                        }
                        grids.push(grid);
                    }
                }
            }
        }
        Terrain::build(levels, MOON_RADIUS_M, 1.0, NO_SEA, &grids)
    }

    /// The fixture body's radius -- the Moon, so the numbers are
    /// recognisable.
    const MOON_RADIUS_M: f64 = 1_737_400.0;

    /// The fixture's analytic slope in direction `d`.
    fn tilt_slope(terrain: &Terrain, d: [f64; 3]) -> f64 {
        let dot = d[0] * TILT[0] + d[1] * TILT[1] + d[2] * TILT[2];
        let mut tangential = 0.0;
        for k in 0..3 {
            let component = TILT[k] - dot * d[k];
            tangential += component * component;
        }
        TILT_UNITS * f64::from(terrain.scale_m) * tangential.sqrt() / terrain.reference_m
    }

    /// **A cube corner is no worse than the middle of a face** -- and that is
    /// the whole answer to Q3.
    ///
    /// Two errors are compared against one analytic value: at the eight corner
    /// nodes and at the ordinary nodes beside them, where the formula asks nobody
    /// about a third face. If the corner is computed by a fit over three
    /// neighbours, both errors are of the same order -- that is the sphere's
    /// discretisation error, common to all nodes. If instead the corner is left
    /// to one face's central difference, its error jumps several-fold, because
    /// two arms of the stencil there land on one node.
    #[test]
    fn the_cube_corner_is_no_worse_than_the_middle_of_a_face() {
        const LEVELS: u32 = 3;
        let terrain = tilt(LEVELS);

        let mut worst_corner: f64 = 0.0;
        let mut worst_plain: f64 = 0.0;
        let mut corners = 0;
        for level in 0..LEVELS {
            let side = 1u32 << level;
            for face in 0..FACES {
                for (i, a) in [(0, 0usize), (side - 1, SIDE)] {
                    for (j, b) in [(0, 0usize), (side - 1, SIDE)] {
                        let patch = Patch { face, level, i, j };
                        let error = |a: usize, b: usize| {
                            let want = tilt_slope(&terrain, patch.vertex(a, b, 1.0));
                            (terrain.slope_at(&patch, a, b) - want).abs() / want
                        };
                        worst_corner = worst_corner.max(error(a, b));
                        // The neighbouring node along each axis: it is on the
                        // same cube edge, but the stencil no longer reaches the
                        // corner.
                        let step = |v: usize| if v == 0 { 2 } else { SIDE - 2 };
                        worst_plain = worst_plain.max(error(step(a), b));
                        worst_plain = worst_plain.max(error(a, step(b)));
                        corners += 1;
                    }
                }
            }
        }

        println!(
            "  {corners} corner nodes: worst relative error at a corner \
             {:.2}%, at an ordinary node beside it {:.2}%",
            worst_corner * 100.0,
            worst_plain * 100.0
        );
        assert!(
            worst_corner < 2.0 * worst_plain.max(1e-3),
            "the corner deviated by {:.2}% against {:.2}% at its neighbour -- \
             the fit over three neighbours does not work",
            worst_corner * 100.0,
            worst_plain * 100.0
        );
    }

    /// **A corner node carries bitwise one number in all three faces.**
    ///
    /// This is what Q3 demanded, and here it is a copy rather than a coincidence
    /// of computations: the corner is computed once per group of three and then
    /// written into all three tiles. The fixture is arbitrary -- what matters is
    /// not its shape but that all three faces agree.
    #[test]
    fn all_three_faces_agree_on_the_cube_corner() {
        const LEVELS: u32 = 3;
        let terrain = tilt(LEVELS);

        // The grouping is the same as in `resolve_cube_corners`, but derived
        // independently -- from the node's direction rather than from the
        // traversal order.
        let mut checked = 0;
        for level in 0..LEVELS {
            let side = 1u32 << level;
            let mut seen: std::collections::HashMap<usize, (i16, Patch)> = Default::default();
            for face in 0..FACES {
                for (i, a) in [(0, 0usize), (side - 1, SIDE)] {
                    for (j, b) in [(0, 0usize), (side - 1, SIDE)] {
                        let patch = Patch { face, level, i, j };
                        let at = patch.vertex(a, b, 1.0);
                        let key = usize::from(at[0] > 0.0)
                            | usize::from(at[1] > 0.0) << 1
                            | usize::from(at[2] > 0.0) << 2;
                        let index = terrain
                            .index(&patch)
                            .expect("the corner patch is in the pyramid");
                        let units = terrain.slope_node(index, a as i32, b as i32);
                        if let Some((first, whose)) = seen.insert(key, (units, patch)) {
                            assert_eq!(
                                units, first,
                                "corner {key} at level {level}: {patch:?} says {units}, \
                                 while {whose:?} says {first}"
                            );
                            checked += 1;
                        }
                    }
                }
            }
        }
        println!("  {checked} cross-face comparisons at corners -- all bitwise identical");
        assert_eq!(
            checked,
            2 * 8 * LEVELS as usize,
            "not every corner was checked"
        );
    }

    /// A patch deeper than the pyramid gives **exactly** the ancestor's height
    /// at an ancestor node.
    ///
    /// This is the simplest claim about addressing, and it catches the crudest
    /// error: a window shifted the wrong way gives a neighbouring node here
    /// rather than the same one.
    #[test]
    fn a_node_that_coincides_with_the_parent_reads_the_parents_height() {
        let levels = 3;
        let terrain = ramp(levels);

        // Level 3 with three pyramid levels (0..2) is exactly one deeper.
        let deep = Patch {
            face: 2,
            level: levels,
            i: 5,
            j: 3,
        };
        let (parent, deeper) = terrain.covering(&deep);
        assert_eq!(
            deeper, 1,
            "the ancestor should have been exactly one level up"
        );

        // A patch's node (0,0) always coincides with an ancestor node.
        let from_deep = terrain.height_m(&deep, 0, 0);
        let (index, origin, step) = terrain.window(&deep);
        assert_eq!(step, 0.5, "the step one level deeper is half a node");

        let (x, y) = (origin[0] as i32, origin[1] as i32);
        let direct = f64::from(terrain.node(index, x, y)) * f64::from(terrain.scale_m);
        assert_eq!(
            from_deep, direct,
            "node (0,0) of the deep patch was read from the wrong place in the \
             ancestor (ancestor {parent:?}, window {origin:?})"
        );
    }

    /// A shared point of two neighbouring deep patches gives the same height.
    ///
    /// This is the requirement "terrain adds no cracks", carried onto levels
    /// deeper than the data. **Both** cases are checked, and the second matters
    /// more:
    ///
    /// 1. neighbours inside one ancestor -- here the window arithmetic itself
    ///    does the work;
    /// 2. neighbours on opposite sides of an ancestor boundary -- here the
    ///    windows differ, the tiles differ, and the heights agree only because
    ///    the node on the shared edge lies in both tiles (R5b). An off-by-one in
    ///    the offset breaks exactly this case and leaves the first green.
    #[test]
    fn neighbouring_deep_patches_agree_on_their_shared_edge() {
        let levels = 3;
        let terrain = ramp(levels);
        let level = levels + 1; // two levels deeper than the pyramid

        let at = |i: u32, j: u32| Patch {
            face: 1,
            level,
            i,
            j,
        };

        // `i` grows with coordinate `a`, so the edge between (i, j) and
        // (i+1, j) is node a = SIDE on the left and a = 0 on the right.
        let pairs = [
            // Both inside one ancestor tile: 4 children per ancestor at
            // deeper = 2, so 0..3 is one ancestor.
            (0u32, 1u32),
            // Across an ancestor boundary: 3 and 4 belong to different
            // tiles.
            (3, 4),
        ];

        for (left, right) in pairs {
            let (l_index, _, _) = terrain.window(&at(left, 0));
            let (r_index, _, _) = terrain.window(&at(right, 0));
            let across_parents = l_index != r_index;

            for b in [0usize, 7, SIDE / 2, SIDE] {
                let from_left = terrain.height_m(&at(left, 0), SIDE, b);
                let from_right = terrain.height_m(&at(right, 0), 0, b);
                assert_eq!(
                    from_left, from_right,
                    "patches {left} and {right} diverged on their shared edge at \
                     node b = {b} (different ancestors: {across_parents})"
                );
            }
        }
    }

    /// The window of a patch that has its own tile is the identity.
    ///
    /// A guard against a regression in the other direction: if `window` starts
    /// shifting what must not be shifted, the terrain slips at every ordinary
    /// level at once rather than only at deep ones.
    #[test]
    fn a_patch_with_its_own_tile_reads_itself_unchanged() {
        let terrain = ramp(3);
        for level in 0..3 {
            let patch = Patch {
                face: 4,
                level,
                i: 0,
                j: 0,
            };
            let (index, origin, step) = terrain.window(&patch);
            assert_eq!(origin, [0.0, 0.0], "the own tile shifted");
            assert_eq!(step, 1.0, "the own tile changed its step");
            assert_eq!(index, terrain.index(&patch).expect("the tile exists"));
        }
    }

    /// A colour pyramid in which every node carries a number of its own.
    ///
    /// The value is derived from all three coordinates -- the tile, `a` and `b`
    /// -- so any addressing error (swapped axes, a forgotten halo, a shift by a
    /// tile) gives a different byte rather than a different shade.
    fn speckle(levels: u32, channels: u32) -> Colour {
        let mut grids = Vec::with_capacity(count(levels));
        for index in 0..count(levels) {
            let mut grid = Vec::with_capacity(Colour::tile_len(channels));
            for a in 0..NODES {
                for b in 0..NODES {
                    for c in 0..channels as usize {
                        grid.push((index * 7 + a * 13 + b * 31 + c * 61) as u8);
                    }
                }
            }
            grids.push(grid);
        }
        Colour::build(levels, channels, 0.25, false, &grids)
    }

    /// A colour file returns exactly the nodes that were put into it -- all
    /// channels included.
    #[test]
    fn a_colour_tileset_survives_the_round_trip() {
        for channels in [1u32, 4] {
            let built = speckle(2, channels);
            let read =
                Colour::from_bytes(&built.to_bytes()).expect("our own file must be readable");
            assert_eq!(read.levels, 2);
            assert_eq!(read.channels, channels);
            assert_eq!(read.scale, 0.25);

            for index in 0..count(2) {
                for a in [0, 1, SIDE as i32 / 2, SIDE as i32] {
                    for b in [0, 1, SIDE as i32 / 2, SIDE as i32] {
                        for c in 0..channels {
                            assert_eq!(
                                read.node(index, a, b, c),
                                built.node(index, a, b, c),
                                "tile {index}, node ({a}, {b}), channel {c}"
                            );
                        }
                    }
                }
                assert_eq!(read.tile_bytes(index), built.tile_bytes(index));
            }
        }
    }

    /// One tileset does not read as the other, and says so.
    ///
    /// This is the half of the "separate files" decision that is easy to lose:
    /// the signatures differ precisely so that a mixed-up file gives an error
    /// rather than a plausible pyramid of the wrong numbers.
    #[test]
    fn the_two_tilesets_refuse_to_be_each_other() {
        let terrain = ramp(2);
        let colour = speckle(2, 1);

        let message = Colour::from_bytes(&terrain.to_bytes()).expect_err("terrain is not colour");
        assert!(message.contains("signature"), "wrong message: {message}");
        let message = Terrain::from_bytes(&colour.to_bytes()).expect_err("colour is not terrain");
        assert!(message.contains("signature"), "wrong message: {message}");
    }

    /// Different pyramid depths are not a coincidence but what the files were
    /// separated for.
    ///
    /// The same patch reads **its own** tile in the deeper pyramid and an
    /// ancestor's tile in the shallower one; the windows differ, and that is
    /// exactly why a shared file would have to equalise the depths. The numbers
    /// here are the Moon assets' own: 5 height levels against 6 colour ones
    /// (T2a).
    #[test]
    fn the_shared_geometry_lets_the_depths_differ() {
        let patch = Patch {
            face: 2,
            level: 5,
            i: 9,
            j: 17,
        };
        let (colour_index, colour_origin, colour_step) = window(6, &patch);
        let (_, height_origin, height_step) = window(5, &patch);

        assert_eq!(colour_step, 1.0, "a level-5 patch has its own colour tile");
        assert_eq!(colour_origin, [0.0, 0.0]);
        assert_eq!(colour_index, index(6, &patch).expect("the tile exists"));

        assert_eq!(
            height_step, 0.5,
            "the same patch takes its height from an ancestor"
        );
        assert_eq!(height_origin, [16.0, 16.0], "and looks into its half");
    }

    /// A sample by direction lands on the face and the node that are visible.
    ///
    /// The fixture is a **per-face marker**: every node of a level-zero tile
    /// carries its face's number, so the answer names a face unambiguously rather
    /// than "something similar". The directions are taken along the axes, where
    /// the correct face is known without computation.
    #[test]
    fn a_direction_reads_the_face_it_points_at() {
        let levels = 2;
        let mut grids = Vec::with_capacity(count(levels));
        for tile in 0..count(levels) {
            // Level-zero tiles come first, one per face.
            let mark = if tile < FACES { 40 * tile + 10 } else { 0 };
            grids.push(vec![mark as u8; Colour::tile_len(1)]);
        }
        let colour = Colour::build(levels, 1, 1.0, false, &grids);

        let axis = [
            ([1.0, 0.0, 0.0], 0),
            ([-1.0, 0.0, 0.0], 1),
            ([0.0, 1.0, 0.0], 2),
            ([0.0, -1.0, 0.0], 3),
            ([0.0, 0.0, 1.0], 4),
            ([0.0, 0.0, -1.0], 5),
        ];
        for (direction, face) in axis {
            let expected = f64::from(40 * face as u8 + 10) / 255.0;
            let got = colour.under(direction, 0);
            assert!(
                (got - expected).abs() < 1e-12,
                "direction {direction:?} read {got} instead of face {face} ({expected})"
            );
        }
    }

    /// Inside one face the sample distinguishes places rather than returning one
    /// number.
    ///
    /// This is what `under` exists for: over a dark place the shine must differ
    /// from over a light one. The fixture is a ramp along node `a` within one
    /// face, and two directions at its opposite edges must give the ramp's
    /// ends.
    #[test]
    fn inside_one_face_the_sample_moves_with_the_direction() {
        let levels = 1;
        let mut grid = vec![0u8; Colour::tile_len(1)];
        for a in 0..NODES {
            for b in 0..NODES {
                grid[a * NODES + b] = (a * 255 / (NODES - 1)) as u8;
            }
        }
        let mut grids = vec![vec![0u8; Colour::tile_len(1)]; count(levels)];
        grids[4] = grid; // face +Z
        let colour = Colour::build(levels, 1, 1.0, false, &grids);

        // Face +Z: `u -> x`, `v -> y`. So the ramp runs along `x`.
        let low = colour.under([-0.9, 0.0, 1.0], 0);
        let high = colour.under([0.9, 0.0, 1.0], 0);
        let middle = colour.under([0.0, 0.0, 1.0], 0);
        println!("  ramp: {low:.3} ... {middle:.3} ... {high:.3}");
        assert!(low < middle && middle < high, "the ramp does not read");
        assert!(
            high - low > 0.5,
            "the ramp's span slipped: {low} ... {high}"
        );
    }
}
