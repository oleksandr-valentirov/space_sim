//! The mesh cooker against the numbers Blender computed (ROADMAP, T5d2).
//!
//! The main difference from the placeholder's oracle (V1): no analytic table
//! exists for an imported model. So the oracle comes from **another tool** --
//! the same rule as the PDS3 label in `dem-cook`. Recomputing the same model
//! ourselves would check itself.
//!
//! There are three oracles, each catching its own class:
//!
//! 1. **signed volume** from `bmesh.calc_volume(signed=True)` -- reversed
//!    winding (the sign), a lost shell, a forgotten scale;
//! 2. **bounds from the accessor JSON** against our `.bin` reader -- byte
//!    order, component type, `byteOffset`;
//! 3. **bounds in Blender axes** against ours in glTF axes -- the axis
//!    convention, i.e. what a symmetric model would not show at all.

use engine::mesh::Model;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn repository() -> PathBuf {
    // `CARGO_MANIFEST_DIR` is tools/mesh-cook.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn oracle() -> Value {
    let path = repository().join("assets-src/ship.oracle.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).expect("the oracle is JSON")
}

fn number(value: &Value, key: &str) -> f64 {
    value[key].as_f64().unwrap_or_else(|| panic!("no {key}"))
}

fn triple(value: &Value, key: &str) -> [f64; 3] {
    let list = value[key].as_array().unwrap_or_else(|| panic!("no {key}"));
    [
        list[0].as_f64().unwrap(),
        list[1].as_f64().unwrap(),
        list[2].as_f64().unwrap(),
    ]
}

fn ship() -> mesh_cook::Cooked {
    mesh_cook::cook(&repository().join("assets-src/ship.gltf")).expect("the model should have read")
}

/// Our mesh's volume equals Blender's.
///
/// The tolerance is relative and measured rather than eyeballed: glTF
/// coordinates are `f32`, i.e. 1e-7 relative, and on the probe cone the
/// discrepancy came out at 3.1e-8 (skill `blender-assets`). A 1e-6 tolerance
/// leaves an order of margin and still catches any geometry error: reversed
/// winding flips the **sign**, and a lost shell costs percent.
#[test]
fn the_volume_is_the_one_blender_measured() {
    let cooked = ship();
    let expected = number(&oracle(), "volume_m3");
    let off = (cooked.volume_m3 - expected).abs() / expected.abs();
    println!(
        "  volume: {} against {expected} ({off:.2e})",
        cooked.volume_m3
    );
    assert!(
        off < 1e-6,
        "volume diverged: {} against {expected}",
        cooked.volume_m3
    );
    // The sign separately: it reports the winding, and a mirroring bug is
    // exactly what it catches.
    assert!(cooked.volume_m3 > 0.0, "triangle winding is reversed");
}

/// Bounds in glTF axes are Blender's bounds with the `-Y -> +Z` permutation.
///
/// The oracle is deliberately written **in Blender axes**: to reproduce it the
/// reader must go through the same permutation the exporter does. On a
/// symmetric model this check would mean nothing -- which is why the model's
/// nose, fins, porthole and antenna all differ along all three axes.
#[test]
fn the_axes_arrive_the_way_the_convention_says() {
    let cooked = ship();
    let oracle = oracle();
    let low = triple(&oracle, "blender_min");
    let high = triple(&oracle, "blender_max");
    let height_m = cooked.model.height_m;

    // The mesh is already normalised, so our bounds must go back to metres.
    let (mut ours_low, mut ours_high) = mesh_cook::bounds(&cooked.model.mesh);
    for k in 0..3 {
        ours_low[k] *= height_m;
        ours_high[k] *= height_m;
    }

    // glTF: x = x, y = z, z = -y. So the `z` bounds come from `y` reversed.
    let expected_low = [low[0], low[2], -high[1]];
    let expected_high = [high[0], high[2], -low[1]];
    println!("  ours     {ours_low:?} .. {ours_high:?}");
    println!("  expected {expected_low:?} .. {expected_high:?}");
    for k in 0..3 {
        assert!(
            (ours_low[k] - expected_low[k]).abs() < 1e-5,
            "lower bound on axis {k}: {} against {}",
            ours_low[k],
            expected_low[k]
        );
        assert!(
            (ours_high[k] - expected_high[k]).abs() < 1e-5,
            "upper bound on axis {k}: {} against {}",
            ours_high[k],
            expected_high[k]
        );
    }

    // The nose points at `+Z`, and that does not follow from the bounds: the
    // half of the hull ahead of the origin is longer than the half behind.
    assert!(
        ours_high[2] > 0.9 * height_m * 0.5,
        "the nose is not at +Z: {ours_high:?}"
    );
}

/// Length and `extent` are the numbers Blender computed.
///
/// `extent` does not follow from the length: on this model it is 0.552 of the
/// height, i.e. more than half -- a fin's heel sits both below the nozzle and
/// to the side of it. `near` and the third-person camera (V2) rest on it, so
/// an error here is a clipped hull, not cosmetics.
#[test]
fn the_length_and_the_extent_are_blenders_numbers() {
    let cooked = ship();
    let oracle = oracle();
    let length = number(&oracle, "length_m");
    let extent = number(&oracle, "extent_m");

    println!(
        "  length {} against {length}, extent {} against {extent}",
        cooked.model.height_m,
        cooked.model.extent * cooked.model.height_m
    );
    assert!((cooked.model.height_m - length).abs() < 1e-5);
    assert!((cooked.model.extent * cooked.model.height_m - extent).abs() < 1e-5);
    assert!(
        cooked.model.extent > 0.52,
        "extent turned out to be half the height: {}",
        cooked.model.extent
    );
}

/// The file has more vertices than Blender, and that is normal.
///
/// Every normal split splits a vertex, so "how many vertices the model has" in
/// Blender is not the number the game pays (skill `blender-assets`). This
/// check guards exactly that expectation: if the numbers matched, it would
/// mean normals merged somewhere and smooth shading drifted.
#[test]
fn the_file_carries_more_vertices_than_blender_shows() {
    let cooked = ship();
    let oracle = oracle();
    let in_blender = number(&oracle, "vertices_in_blender") as usize;
    let triangles = number(&oracle, "triangles") as usize;

    println!(
        "  vertices: {} in the file against {in_blender} in Blender",
        cooked.model.mesh.positions.len()
    );
    assert!(cooked.model.mesh.positions.len() > in_blender);
    assert_eq!(cooked.model.mesh.indices.len(), 3 * triangles);
}

/// The cooked file reads back and does not depend on the run.
#[test]
fn cooking_twice_gives_the_same_file() {
    let first = ship().model.to_bytes();
    let second = ship().model.to_bytes();
    assert_eq!(first, second, "cooking is not deterministic");

    let read = Model::from_bytes(&first).expect("our own file");
    // The numbers come from the oracle rather than being written in: the model
    // is a source that changes (T9 redrew it from a reference), and a baked
    // literal would make this a test of the author's memory rather than of the
    // byte round-trip.
    let oracle = oracle();
    assert_eq!(
        read.mesh.indices.len(),
        3 * number(&oracle, "triangles") as usize
    );
    assert!((read.height_m - number(&oracle, "length_m")).abs() < 1e-5);
}

/// The paint landed **on the same geometry**, not beside it (T9b).
///
/// There can be no oracle number here: `COLOR_0` is the very `.bin` we read,
/// so comparing it against itself would check nothing. What is checked is
/// **registration**: every colour must be found exactly where the model put
/// it. A stride error in the accessor, or an off-by-one vertex, leaves the
/// colours correct in composition and shuffled in place -- the eye sees that
/// as blotches, this test as an exact number.
///
/// Coordinates are in units of height from the model centre: `along` in the
/// script runs 0 to 1, and `+Z` in the game is `along - 0.5`.
#[test]
fn the_paint_lands_where_the_model_put_it() {
    let cooked = ship();
    let paint = &cooked.model.paint;
    let points = &cooked.model.mesh.positions;
    assert_eq!(paint.len(), points.len(), "paint is not on every vertex");

    let mut palette: Vec<[u32; 3]> = paint.iter().map(|c| c.map(f32::to_bits)).collect();
    palette.sort_unstable();
    palette.dedup();
    println!("  palette: {} colours", palette.len());
    // Six is exactly the six named in `tools/blender/ship.py`: enamel, red,
    // yellow, steel, seam and glass. A count rather than a list of values:
    // what matters here is that the paint did not smear through interpolation
    // or merge into one.
    assert_eq!(palette.len(), 6, "the palette changed");

    let hot = |c: &[f32; 3], k: usize| c[k] > 0.5 && c[k] > 2.0 * c[(k + 2) % 3];
    let mut red = (0, 0);
    let mut yellow = 0;
    for (colour, point) in paint.iter().zip(points) {
        if hot(colour, 0) && colour[1] < 0.2 {
            // Red comes in only two kinds: the nose cone at the top and the
            // fins at the bottom. Between them there is none at all.
            assert!(
                point[2] > 0.36 || point[2] < -0.13,
                "red in the middle of the hull: {point:?}"
            );
            if point[2] > 0.0 {
                red.0 += 1;
            } else {
                red.1 += 1;
            }
        }
        if hot(colour, 0) && colour[1] > 0.4 {
            // Yellow is only the porthole rim: starboard, a circle about its
            // own point. Radius from the model: 0.655 of the hull radius.
            yellow += 1;
            assert!(point[0] > 0.0, "yellow is not to starboard: {point:?}");
            let off = (point[2] - 0.136).hypot(point[1]);
            assert!(off < 0.09, "yellow outside the porthole: {point:?}, {off}");
        }
    }
    println!(
        "  red vertices: {} on the nose, {} on the tail",
        red.0, red.1
    );
    println!("  yellow vertices: {yellow}");
    assert!(red.0 > 0 && red.1 > 0, "red was found on one side only");
    assert!(yellow > 0, "there is no yellow at all");
}

// ---------------------------------------------------------------------------
// Both index types (T5d2)

/// A minimal one-triangle glTF with indices of a given type.
///
/// Synthetic rather than a second Blender export: obtaining `UNSIGNED_INT`
/// naturally would need a model of over 65,535 vertices, i.e. megabytes in git
/// for two lines of reader code.
fn write_triangle(folder: &Path, component: u64) -> PathBuf {
    // A skewed triangle: all three axes differ in extent and `z` is non-zero
    // -- otherwise there is nothing to normalise to unit length.
    let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 2.0, 4.0]];
    let normals: [[f32; 3]; 3] = [[0.0, 0.0, 1.0]; 3];

    let mut bin = Vec::new();
    for p in positions {
        for v in p {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    for n in normals {
        for v in n {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    let indices_at = bin.len();
    for k in 0u32..3 {
        match component {
            5123 => bin.extend_from_slice(&(k as u16).to_le_bytes()),
            _ => bin.extend_from_slice(&k.to_le_bytes()),
        }
    }
    let index_bytes = bin.len() - indices_at;
    std::fs::write(folder.join("triangle.bin"), &bin).expect("writing .bin");

    let json = serde_json::json!({
        "asset": {"version": "2.0"},
        "meshes": [{"primitives": [{
            "attributes": {"POSITION": 0, "NORMAL": 1},
            "indices": 2,
            "mode": 4
        }]}],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
             "min": [0.0, 0.0, 0.0], "max": [1.0, 2.0, 4.0]},
            {"bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3"},
            {"bufferView": 2, "componentType": component, "count": 3, "type": "SCALAR"}
        ],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": 36},
            {"buffer": 0, "byteOffset": 36, "byteLength": 36},
            {"buffer": 0, "byteOffset": indices_at, "byteLength": index_bytes}
        ],
        "buffers": [{"uri": "triangle.bin", "byteLength": bin.len()}]
    });
    let path = folder.join("triangle.gltf");
    std::fs::write(&path, serde_json::to_vec_pretty(&json).unwrap()).expect("writing .gltf");
    path
}

/// The reader tells `UNSIGNED_SHORT` and `UNSIGNED_INT` apart rather than
/// assuming one.
///
/// The exporter chooses the index type: up to 65,535 vertices it emits
/// `UNSIGNED_SHORT` (which is what our model holds), beyond that
/// `UNSIGNED_INT`. A reader knowing one type breaks when the **shape**
/// changed, not the code.
#[test]
fn both_index_types_read_the_same() {
    let mut meshes = Vec::new();
    for component in [5123u64, 5125] {
        let folder = std::env::temp_dir().join(format!("mesh-cook-{component}"));
        std::fs::create_dir_all(&folder).expect("temporary directory");
        let path = write_triangle(&folder, component);
        let cooked = mesh_cook::cook(&path).expect("the triangle should have read");
        assert_eq!(cooked.index_component, component);
        meshes.push(cooked.model.mesh.indices.clone());
        std::fs::remove_dir_all(&folder).ok();
    }
    assert_eq!(
        meshes[0], meshes[1],
        "the index types gave different triangles"
    );
    assert_eq!(meshes[0], vec![0, 1, 2]);
}

/// A file whose `.bin` diverged from its JSON is an error, not a quiet
/// asset.
#[test]
fn a_bin_that_disagrees_with_the_json_is_an_error() {
    let folder = std::env::temp_dir().join("mesh-cook-broken");
    std::fs::create_dir_all(&folder).expect("temporary directory");
    let path = write_triangle(&folder, 5123);

    // What is corrupted is the accessor's `min` -- what the exporter
    // published.
    let text = std::fs::read_to_string(&path).unwrap();
    let mut json: Value = serde_json::from_str(&text).unwrap();
    json["accessors"][0]["max"][1] = serde_json::json!(7.0);
    std::fs::write(&path, serde_json::to_vec_pretty(&json).unwrap()).unwrap();

    let message = mesh_cook::cook(&path).expect_err("the divergence should have been an error");
    println!("  {message}");
    assert!(message.contains("diverged"), "wrong message: {message}");
    std::fs::remove_dir_all(&folder).ok();
}
