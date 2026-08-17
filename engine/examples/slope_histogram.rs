//! Distribution of slope in a terrain tileset (stage T, steps T4c and T7f).
//!
//! It exists for one number -- [`engine::material::SLOPE_REF`], the slope at
//! which the slope tint reaches full strength. That number cannot be taken
//! from physics, and the two numbers that show why come from different places:
//!
//! * **0.3 is physics** -- the angle of repose of regolith. It is a true
//!   statement about the **local** slope, i.e. about a scale this asset does
//!   not have;
//! * **0.035 is this example** -- the median `Terrain::slope_at` of
//!   `assets/moon.dem`, which measures over the base of the finest node of the
//!   pyramid, 5330 m. Smoothed by that base, and smoothed is the point.
//!
//! Roughly tenfold apart, so a threshold set "from physics" switched the rule
//! off on 999 nodes out of 1000. Both numbers are reproduced by the command
//! below; the same conclusion is recorded in CLAUDE.md, "Поріг нахилу в
//! правилі матеріалу виміряний з ассета".
//!
//! **The second table is about the patch level, and it appeared not for the
//! sake of the asset but for the sake of an artefact** (T7f): on Earth from
//! 1e6 m the frame came out striped along the patches. Lighting has nothing to
//! do with it -- the normal in the shader is the sphere's normal -- so the
//! brightness can differ only by the material multiplier, and that one reads
//! the slope. So the question is put directly: does the slope depend on which
//! level of patch asked for it.
//!
//!     cargo run --release -p engine --example slope_histogram [asset]

use engine::cubesphere::{Patch, FACES, SIDE};
use engine::{demo, material, tiles};

/// The quantiles both tables print.
const QUANTILES: [f64; 7] = [0.5, 0.75, 0.9, 0.95, 0.99, 0.999, 1.0];

fn main() -> Result<(), String> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| demo::TERRAIN_ASSET.to_string());
    let bytes = std::fs::read(&path)
        .map_err(|e| format!("{path}: {e}\nto fix: make cook-dem or make cook-earth"))?;
    let terrain = tiles::Terrain::from_bytes(&bytes)?;
    println!(
        "{path}: {} levels, step {:.0} m",
        terrain.levels,
        terrain.step_m()
    );

    // The deepest level: there the slope is measured over the shortest base the
    // asset has at all, and it is the one the frame reads from up close.
    let deepest = terrain.levels.saturating_sub(1);
    let values = sample(&terrain, deepest);
    let at = |q: f64| values[((values.len() - 1) as f64 * q) as usize];
    println!("nodes {}", values.len());
    for q in QUANTILES {
        let slope = at(q);
        println!(
            "  {:>5.1}% : slope {slope:.4} ({:.2} deg), tint {:.3}",
            q * 100.0,
            slope.atan().to_degrees(),
            material::tint(slope, 0.0, false)
        );
    }
    println!("SLOPE_REF is now {:.3}", material::SLOPE_REF);

    // --- Under water the slope is just as steep, and the colour there is
    // already truthful ---
    //
    // The T7f question: is the slope rule entitled to work at sea. The mosaic
    // over the ocean shows water, while the DEM under it shows ridges and
    // trenches; if their slope is no smaller than the dry-land one, the rule
    // will draw bathymetry on the surface of the sea.
    let mut water = Vec::new();
    let mut land = Vec::new();
    let side = 1u32 << deepest;
    for face in 0..FACES {
        for i in (0..side).step_by(2) {
            for j in (0..side).step_by(2) {
                let patch = Patch {
                    face,
                    level: deepest,
                    i,
                    j,
                };
                for a in (0..=SIDE).step_by(4) {
                    for b in (0..=SIDE).step_by(4) {
                        let slope = terrain.slope_at(&patch, a, b);
                        if terrain.height_m(&patch, a, b) < 0.0 {
                            water.push(slope);
                        } else {
                            land.push(slope);
                        }
                    }
                }
            }
        }
    }
    if !water.is_empty() && !land.is_empty() {
        water.sort_by(|a, b| a.partial_cmp(b).expect("a slope is never NaN"));
        land.sort_by(|a, b| a.partial_cmp(b).expect("a slope is never NaN"));
        let share = water.len() as f64 / (water.len() + land.len()) as f64;
        println!();
        println!("below zero {:.1}% of nodes:", 100.0 * share);
        println!("        |   median |     90% |      99% | tint 90%");
        for (name, values) in [("water", &water), ("land ", &land)] {
            let at = |q: f64| values[((values.len() - 1) as f64 * q) as usize];
            println!(
                "  {name} | {:>8.4} | {:>7.4} | {:>8.4} | {:.3}",
                at(0.5),
                at(0.9),
                at(0.99),
                material::tint(at(0.9), 0.0, false)
            );
        }
    }

    // --- Is the slope the same at every patch level ---
    //
    // It will **not** be the same, and the reason is not in the construction
    // but in the data: every level measures the slope over its own step
    // (`node_step_m`), and a coarse tile is a sparser sampling of the same
    // surface, i.e. a smoother one. This table says by how much -- in the slope
    // and straight away in the multiplier, i.e. in what the eye sees.
    //
    // WARNING: these two columns do **not** read as "the slope depends on the
    // patch level": on a node shared by two neighbouring patches the slope is
    // bitwise identical (stage W), because both take it from the same node of
    // the same tile.
    println!();
    println!("slope by patch level (the same step in metres, different data):");
    println!("   level |   median |   90%  | median tint");
    for level in 0..=deepest {
        let values = sample(&terrain, level);
        let at = |q: f64| values[((values.len() - 1) as f64 * q) as usize];
        println!(
            "  {level:>6} | {:>8.4} | {:>6.4} | {:.4}",
            at(0.5),
            at(0.9),
            material::tint(at(0.5), 0.0, false)
        );
    }
    Ok(())
}

/// The slope at the nodes of patches of a given level -- decimated and sorted.
///
/// The decimation is the same for every level (every second patch, every
/// fourth node), so on coarse levels the sample comes out smaller -- and that
/// is honest: there really are only that many patches there.
fn sample(terrain: &tiles::Terrain, level: u32) -> Vec<f64> {
    let side = 1u32 << level;
    let step = if side >= 2 { 2 } else { 1 };
    let mut values = Vec::new();
    for face in 0..FACES {
        for i in (0..side).step_by(step) {
            for j in (0..side).step_by(step) {
                let patch = Patch { face, level, i, j };
                for a in (0..=SIDE).step_by(4) {
                    for b in (0..=SIDE).step_by(4) {
                        values.push(terrain.slope_at(&patch, a, b));
                    }
                }
            }
        }
    }
    values.sort_by(|a, b| a.partial_cmp(b).expect("a slope is never NaN"));
    values
}
