//! The cooker: same input, same byte, and the same number by two paths (R5b).
//!
//! Two claims, neither about a tile looking nice.
//!
//! **The first is stability.** An asset that differs every time breaks
//! everything resting on it: hash comparison, the build cache, `git diff`.
//! Here it is checked by two consecutive runs rather than postulated.
//!
//! **The second is the K5e shape of oracle: two paths, one number.** The
//! height in the tile and the height read from the source by latitude and
//! longitude must agree. The paths really are different: the cooker goes
//! through `Patch::vertex` and `sample_direction_m`, the test through an
//! explicit translation of a direction into degrees and `sample_m`. An error
//! in the cubesphere would shift the first and leave the second alone.

use dem_cook::cook::build;
use dem_cook::Grid;
use engine::cubesphere::{Patch, FACES, SIDE};
use engine::tiles::{self, Terrain};
use std::path::Path;

const LEVELS: u32 = 3;

fn grid() -> Grid {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/lola/ldem_4.img");
    Grid::read(&path).expect("the LOLA grid should have read")
}

/// A cheap stable byte hash -- FNV-1a. No cryptography needed here: the
/// question is not "was it forged" but "is it the same".
fn digest(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

/// Two cooker runs give byte-for-byte the same.
#[test]
fn cooking_twice_gives_the_same_bytes() {
    let grid = grid();
    let first = build(&grid, LEVELS).to_bytes();
    let second = build(&grid, LEVELS).to_bytes();

    println!(
        "  {} levels, {} tiles, {} bytes, hash {:016x}",
        LEVELS,
        Terrain::count(LEVELS),
        first.len(),
        digest(&first)
    );
    assert_eq!(
        digest(&first),
        digest(&second),
        "the two runs gave different bytes"
    );
    assert_eq!(first, second);
}

/// The file reads back into what was put into it.
#[test]
fn the_file_survives_a_round_trip() {
    let grid = grid();
    let terrain = build(&grid, LEVELS);
    let back = Terrain::from_bytes(&terrain.to_bytes()).expect("the file should have read");

    assert_eq!(back.levels, terrain.levels);
    assert_eq!(back.scale_m, terrain.scale_m);
    assert_eq!(back.reference_m, terrain.reference_m);
    assert_eq!(back.to_bytes(), terrain.to_bytes());

    // A foreign version must announce itself rather than read as garbage.
    let mut broken = terrain.to_bytes();
    broken[8] = 99;
    assert!(
        Terrain::from_bytes(&broken).is_err(),
        "a foreign version was accepted"
    );
    broken[0] = b'X';
    assert!(
        Terrain::from_bytes(&broken).is_err(),
        "a foreign signature was accepted"
    );
}

/// The tile's height and the source's height: one number, two paths.
#[test]
fn the_tile_agrees_with_the_source_read_another_way() {
    let grid = grid();
    let terrain = build(&grid, LEVELS);

    let mut worst: f64 = 0.0;
    let mut checked = 0;
    for level in 0..LEVELS {
        let side = 1u32 << level;
        for face in 0..FACES {
            for i in 0..side {
                for j in 0..side {
                    let patch = Patch { face, level, i, j };
                    // The tile's centre and three of its corners -- where an
                    // error in the face axes would show most.
                    for (a, b) in [(SIDE / 2, SIDE / 2), (0, 0), (0, SIDE), (SIDE, 0)] {
                        let d = patch.vertex(a, b, 1.0);
                        // The other path: direction -> degrees -> `sample_m`.
                        let lat = d[2].atan2((d[0] * d[0] + d[1] * d[1]).sqrt());
                        let lon = d[1].atan2(d[0]);
                        let from_source = grid.sample_m(lat, lon);
                        let from_tile = terrain.height_m(&patch, a, b);
                        worst = worst.max((from_tile - from_source).abs());
                        checked += 1;
                    }
                }
            }
        }
    }

    println!("  {checked} points; largest discrepancy {worst:.4} m");
    // Half a storage quantum is all that is allowed: rounding to 0.5 m and
    // nothing beyond it.
    assert!(
        worst <= f64::from(terrain.scale_m) / 2.0 + 1e-9,
        "the tile diverged from the source by {worst:.4} m"
    );
}

/// A patch deeper than the pyramid takes its height from an ancestor -- and at
/// the tile's edge that is the ancestor's own height.
///
/// This is what the absence of cracks in the terrain rests on: neighbouring
/// patches at a deep level can live in **different** ancestor tiles, and on a
/// shared edge they must give the same number.
#[test]
fn a_patch_deeper_than_the_pyramid_reads_its_ancestor() {
    let grid = grid();
    let terrain = build(&grid, LEVELS);

    // A pair of neighbours at a level deeper than the pyramid, living in
    // different ancestor tiles: `i = 1` and `i = 2` at `LEVELS = 3` are
    // children of different level-2 patches.
    let deep = LEVELS + 1;
    let left = Patch {
        face: 2,
        level: deep,
        i: (1 << deep) / 2 - 1,
        j: 3,
    };
    let right = Patch {
        face: 2,
        level: deep,
        i: (1 << deep) / 2,
        j: 3,
    };
    assert_ne!(
        terrain.covering(&left).0,
        terrain.covering(&right).0,
        "the neighbours should land in different ancestor tiles, or the test \
         catches nothing"
    );

    let mut worst: f64 = 0.0;
    for b in 0..=SIDE {
        let a = terrain.height_m(&left, SIDE, b);
        let c = terrain.height_m(&right, 0, b);
        worst = worst.max((a - c).abs());
    }
    println!("  shared edge of two tiles: discrepancy {worst:.6} m");
    assert_eq!(worst, 0.0, "the terrain diverged at the tile boundary");
}

/// **A tile's halo really is the neighbour's node, and exactly where it is
/// expected**
/// (R7b).
///
/// What this check pins and what it does not. The geometry -- "a halo node
/// sits one step past the edge" -- is proved separately and **independently of
/// the formula** by
/// `engine::tests::cubesphere::a_halo_node_sits_one_step_past_the_edge`
/// (bitwise inside a face, by a step ratio across a cube edge). What is proved
/// here is the second half: that the cooker put that number **in the grid cell**
/// the central difference will take it from, and that it equals bitwise what
/// the neighbour stores as an ordinary node.
///
/// WARNING: **it looks at `Terrain::build`'s input rather than at the
/// tileset**, and since stage W there is no other way: the file no longer
/// holds a halo (version 4). The check did not weaken from that but
/// strengthened -- it is now about the place the halo is actually used rather
/// than about its copy on disk.
///
/// Two halves, and without the second the first is worth nothing:
///
/// 1. **Equality with the neighbour.** Copy and original are the same number.
///    They should not diverge by construction (one direction), so a difference
///    would mean a layout shift rather than error.
/// 2. **The halo does not equal the edge.** That is the implementation the
///    step was made against: a clamped index would give a copy of the outermost
///    row at the tile boundary, would sail through the first half of the check
///    -- and would give the two sides of the boundary different gradients.
#[test]
fn the_halo_holds_the_neighbours_own_node() {
    use engine::cubesphere::{Edge, EDGES};

    use engine::tiles::{HALO, STORED};

    let grid = grid();
    let grids = dem_cook::cook::height_grids(&grid, LEVELS);
    // A halo grid node by patch coordinates: `-1` and `SIDE + 1` are legal.
    let node = |tile: usize, a: i32, b: i32| {
        grids[tile][(a + HALO as i32) as usize * STORED + (b + HALO as i32) as usize]
    };

    let mut compared = 0;
    let mut same_as_edge = 0;
    for level in 0..LEVELS {
        let side = 1u32 << level;
        for face in 0..FACES {
            for i in 0..side {
                for j in 0..side {
                    let patch = Patch { face, level, i, j };
                    let here = tiles::index(LEVELS, &patch).expect("level in the pyramid");
                    for edge in EDGES {
                        for along in 0..=SIDE {
                            let (there, na, nb) = patch.halo_node(edge, along);
                            let theirs = node(
                                tiles::index(LEVELS, &there).expect("neighbour in the same pyramid"),
                                na as i32,
                                nb as i32,
                            );

                            // Our halo cell and our outermost node side by
                            // side.
                            let (side, k) = (SIDE as i32, along as i32);
                            let (ha, hb, ea, eb) = match edge {
                                Edge::AMin => (-1, k, 0, k),
                                Edge::AMax => (side + 1, k, side, k),
                                Edge::BMin => (k, -1, k, 0),
                                Edge::BMax => (k, side + 1, k, side),
                            };
                            let mine = node(here, ha, hb);
                            assert_eq!(
                                mine, theirs,
                                "{patch:?} / {edge:?}: halo ({ha}, {hb}) gives \
                                 {mine}, while neighbour {there:?} at node \
                                 ({na}, {nb}) gives {theirs}"
                            );
                            if mine == node(here, ea, eb) {
                                same_as_edge += 1;
                            }
                            compared += 1;
                        }
                    }
                }
            }
        }
    }

    let flat = same_as_edge as f64 / compared as f64;
    println!(
        "  compared {compared} halo nodes; matching the edge {same_as_edge} \
         ({:.1}%)",
        flat * 100.0
    );
    assert!(
        flat < 0.5,
        "half the halo equals the outermost row ({:.1}%) -- that is a clamped \
         index, not a neighbour",
        flat * 100.0
    );
}

/// **The slope on a shared edge is bitwise one number on both sides** (R7c; W3).
///
/// This is the main condition under which procedural detail may exist at all.
/// The noise amplitude follows the slope; if the slope differed at a shared
/// node, the detail would tear the surface exactly where R2b removed the crack
/// -- and it would look like a crack in the geometry rather than an amplitude
/// error.
///
/// Why this comes out bitwise rather than "nearly": the slope sits in the node
/// as an integer (stage W), and in two neighbouring tiles on a shared edge it
/// is the same number -- the central difference on both sides takes the same
/// four heights. Our halo `(-1, k)` is the neighbour's node `(SIDE - 1, k)`,
/// our node `(1, k)` is its halo, and `(0, k +/- 1)` lie on the edge itself and
/// are shared. Across a cube edge the axes may swap and change sign -- which is
/// exactly why the gradient's **length** is taken: addition commutes and the
/// square eats the sign.
///
/// WARNING: **there are no exceptions any more, and that is the whole point of
/// step W3.** Before it the test had a `tainted` predicate skipping a band
/// around the eight cube corners: there one face's stencil reaches into a
/// second and the neighbouring face's into a third, and the three answers
/// differed by 39% (Q3). Now `Terrain::build` resolves the corner once for all
/// three faces, so **everything** must agree.
///
/// Two cases are checked, and the second matters more: patches **deeper than
/// the pyramid**, the ones actually seen up close, when the detail means
/// something.
#[test]
fn the_slope_is_one_number_from_both_sides_of_an_edge() {
    use engine::cubesphere::{Edge, EDGES};

    let grid = grid();
    let terrain = build(&grid, LEVELS);

    // The edge node from the side of whoever looks across it.
    let node = |edge: Edge, k: usize| match edge {
        Edge::AMin => (0, k),
        Edge::AMax => (SIDE, k),
        Edge::BMin => (k, 0),
        Edge::BMax => (k, SIDE),
    };

    let mut compared = 0;
    let mut across_faces = 0;
    let mut at_corners = 0;
    for level in [LEVELS - 1, LEVELS + 1] {
        let side = 1u32 << level;
        for face in 0..FACES {
            // Face corners and one cell inside: where cube edges meet, an
            // error is most likely.
            for (i, j) in [(0, 0), (side - 1, side - 1), (0, side - 1), (1, 1)] {
                let patch = Patch { face, level, i, j };
                for edge in EDGES {
                    let there = patch.neighbour(edge);
                    if there.patch.face != face {
                        across_faces += 1;
                    }
                    for k in [0, 1, SIDE / 3, SIDE / 2, SIDE - 1, SIDE] {
                        let (ma, mb) = node(edge, k);
                        let (ta, tb) = node(there.edge, k);
                        let mine = terrain.slope_at(&patch, ma, mb);
                        let theirs = terrain.slope_at(&there.patch, ta, tb);

                        // How many of the checked nodes are cube corners
                        // themselves. Without this counter the test could pass
                        // without touching one, i.e. saying nothing about W3.
                        let span = (SIDE << level) as u32;
                        let (u, v) = (
                            patch.i * SIDE as u32 + ma as u32,
                            patch.j * SIDE as u32 + mb as u32,
                        );
                        if (u == 0 || u == span) && (v == 0 || v == span) {
                            at_corners += 1;
                        }

                        assert_eq!(
                            mine.to_bits(),
                            theirs.to_bits(),
                            "{patch:?} / {edge:?} node {k}: slope {mine:.9e} \
                             against {theirs:.9e} in {:?}",
                            there.patch
                        );
                        compared += 1;
                    }
                }
            }
        }
    }

    println!(
        "  {compared} edge nodes, of them {across_faces} adjacencies across a \
         cube edge and {at_corners} cube corners themselves -- the slope \
         matched bitwise everywhere"
    );
    assert!(across_faces > 0, "no cube edge among those checked");
    assert!(
        at_corners > 0,
        "no corner node among those checked -- nothing confirms W3"
    );
}
