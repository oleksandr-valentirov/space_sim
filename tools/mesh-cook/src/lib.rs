//! Mesh cooker: glTF from Blender to `SSMSH` (ROADMAP, T5d2).
//!
//! Split the same way as `dem-cook`: the cooking itself is a library and the
//! binary only parses arguments. The reason is not layering -- a test cannot
//! call a function out of a binary.

pub mod gltf;

use engine::mesh::{self, Model};
use std::path::Path;

/// Cook a model: read the glTF, normalise to unit height, and return both what
/// goes into the file and the numbers the caller should check.
#[derive(Debug)]
pub struct Cooked {
    pub model: Model,
    /// Bounds the exporter published in the accessor JSON.
    pub published: gltf::Published,
    /// Signed volume **in metres**, i.e. before normalisation.
    pub volume_m3: f64,
    pub index_component: u64,
}

pub fn cook(path: &Path) -> Result<Cooked, String> {
    let loaded = gltf::load(path)?;

    // Volume is computed **before** normalisation: metres is what Blender
    // reports it in, and metres is where it can be compared. After dividing by
    // height it falls by the cube of the height, and the comparison would need
    // one more multiplication -- one more place to get it wrong.
    let volume_m3 = mesh::signed_volume(&loaded.mesh);

    // The JSON bounds are checked here rather than in a test, and these are
    // different things: a test catches a regression in our reader, while this
    // check catches a **corrupt asset**, i.e. `.bin` and `.gltf` having
    // diverged.
    let (low, high) = bounds(&loaded.mesh);
    for k in 0..3 {
        for (ours, theirs, what) in [
            (low[k], loaded.published.min[k], "min"),
            (high[k], loaded.published.max[k], "max"),
        ] {
            // Tolerance scaled by model size: the JSON numbers are printed as
            // decimal strings, so they have already round-tripped through
            // text.
            let scale = (high[k] - low[k]).abs().max(1.0);
            if (ours - theirs).abs() > 1e-6 * scale {
                return Err(format!(
                    "{what}[{k}]: {ours} in .bin, {theirs} in the accessor \
                     JSON -- the files have diverged"
                ));
            }
        }
    }

    let model = Model::from_metres(loaded.mesh, loaded.paint)?;
    Ok(Cooked {
        model,
        published: loaded.published,
        volume_m3,
        index_component: loaded.index_component,
    })
}

/// Mesh bounds along each axis.
pub fn bounds(mesh: &engine::sphere::Mesh) -> ([f64; 3], [f64; 3]) {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for p in &mesh.positions {
        for k in 0..3 {
            low[k] = low[k].min(p[k]);
            high[k] = high[k].max(p[k]);
        }
    }
    (low, high)
}
