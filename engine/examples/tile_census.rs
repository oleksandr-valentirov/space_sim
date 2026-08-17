//! Y1a: how many tiles does a frame actually read? (ROADMAP.md, stage Y)
//!
//! The go/no-go for the whole of Y1, and it said go. Debt D19 charged the
//! frame for the **length** of the bindless array -- 26,616 textures across
//! two bodies -- regardless of what was drawn. A resident set replaces that
//! with the cost of what the frame actually reads, so the saving was worth
//! exactly the ratio between the two numbers, and only one of them was known.
//!
//! If the set had not been two or three orders below 26,616, Y1 would not have
//! paid for itself, and an hour here was a cheap way to learn that. Hence a
//! census before the work, not a measurement after it.
//!
//! It still runs, and it is still the number to look at when a new pyramid is
//! added (Y2-Y4): what the resident set costs is what the frame reads.
//!
//! ## Why this counts `lod::select` rather than the compute cull
//!
//! The cull drops patches behind the limb and outside the frustum (R6b), so
//! its set is smaller -- but it lives on the GPU, and reading it back costs a
//! frame of latency. `lod::select` runs on the CPU before the frame is
//! encoded, and its set is a **superset** of what is drawn. A superset is the
//! safe direction: a surplus bound tile costs its 61-78 ns, a missing one is
//! a hole in the frame. So this is the number the resident set would be built
//! from, which is why it is the number measured.
//!
//! ## Why the camera is not pointed straight down
//!
//! Every fixture of the engine's geometry once stood exactly above the centre
//! of a cube face -- the one point where a wrong distance to a patch gives the
//! right answer -- and D13 and D14 both lived there unseen. The rule that came
//! out of it (CLAUDE.md) is that a new check of body geometry must have at
//! least one asymmetric direction and at least one small altitude. This has
//! both: `drag` turns the camera off the face centre, and 10 km is in the
//! list.
//!
//! Run: `cargo run --release -p engine --example tile_census`

use std::collections::HashSet;

use engine::camera::Camera;
use engine::frame::FOV_Y;
use engine::lod;
use engine::orbit::Orbit;
use engine::tiles::{self, Colour, Terrain};

/// The resolution the census is taken at.
///
/// The level criterion is measured in screen pixels, so the set depends on it
/// -- the same body at 720p asks for fewer patches. 1080p is the larger of the
/// two the performance probes use, i.e. the pessimistic end.
const HEIGHT_PX: f64 = 1080.0;

/// Altitudes above the surface, metres.
///
/// The map view, low orbit and close-up. The last one exists for the reason
/// given in the header: errors of this class need a wide cone and a near
/// camera at once.
const ALTITUDES_M: [f64; 3] = [1.0e9, 400.0e3, 10.0e3];

/// Where the camera stands, as a mouse drag in pixels off the default view.
///
/// Any direction that is not symmetric would do; this one (0.70 rad, 0.35 rad)
/// is over neither a face centre (0, 0) nor a cube corner (pi/4,
/// atan(1/sqrt(2))). Pixels rather than radians because pixels are what
/// `Orbit` takes, and the exact angle is not what matters here.
const DRAG_PX: (f64, f64) = (134.0, 67.0);

/// The camera for this body at this altitude, looking at its centre.
///
/// ⚠ `Orbit::around`, not `Orbit::at_altitude`: the latter measures the
/// altitude above **Earth**, so asking it for "10 km" over the Moon puts the
/// camera 4634 km above the surface and the census reports six patches at
/// every altitude -- a wrong answer that looks like a measurement. Each body
/// must be seen from its own radius, which is why that radius is a parameter.
fn camera_at(radius_m: f64, altitude_m: f64) -> Camera {
    let mut orbit = Orbit::around(radius_m, altitude_m);
    orbit.drag(DRAG_PX.0, DRAG_PX.1);
    orbit.camera()
}

fn main() -> Result<(), String> {
    println!(
        "profile: {}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    println!(
        "resolution: {HEIGHT_PX:.0}px high, fov_y {:.1} deg",
        FOV_Y.to_degrees()
    );

    let mut total_declared = 0usize;

    for (name, dem_path, colour_path) in [
        ("Moon", "assets/moon.dem", "assets/moon.col"),
        ("Earth", "assets/earth.dem", "assets/earth.col"),
    ] {
        let terrain =
            Terrain::from_bytes(&std::fs::read(dem_path).map_err(|e| format!("{dem_path}: {e}"))?)?;
        let colour = Colour::from_bytes(
            &std::fs::read(colour_path).map_err(|e| format!("{colour_path}: {e}"))?,
        )?;

        // The whole pyramid, both channels. Until Y1b this was also what got
        // bound every frame, and so exactly what D19 charged for; now it is
        // the denominator -- what the resident set is being compared against.
        let declared = tiles::count(terrain.levels) + tiles::count(colour.levels);
        total_declared += declared;

        println!();
        println!(
            "=== {name}: radius {:.1} km, {} terrain levels ({} tiles), {} colour levels ({} tiles)",
            terrain.reference_m / 1000.0,
            terrain.levels,
            tiles::count(terrain.levels),
            colour.levels,
            tiles::count(colour.levels),
        );

        let body = lod::Body::still([0.0, 0.0, 0.0], terrain.reference_m);
        let focal = lod::focal_px(FOV_Y, HEIGHT_PX);

        for altitude in ALTITUDES_M {
            let camera = camera_at(terrain.reference_m, altitude);

            let selection = lod::select(&body, &camera, focal, Some(&terrain));

            // Two distinct sets, because the two pyramids have different
            // depths: a patch deeper than a pyramid reads its nearest
            // ancestor's tile (`covering`), and with five terrain levels
            // against six colour ones the same patch can share a terrain tile
            // with its sibling while owning its colour tile alone.
            let mut height_tiles: HashSet<usize> = HashSet::new();
            let mut colour_tiles: HashSet<usize> = HashSet::new();
            for patch in &selection.patches {
                let (covering, _) = tiles::covering(terrain.levels, patch);
                if let Some(index) = tiles::index(terrain.levels, &covering) {
                    height_tiles.insert(index);
                }
                let (covering, _) = tiles::covering(colour.levels, patch);
                if let Some(index) = tiles::index(colour.levels, &covering) {
                    colour_tiles.insert(index);
                }
            }

            let read = height_tiles.len() + colour_tiles.len();
            println!(
                "  {:>9.0} km: {:>5} patches -> {:>4} height + {:>4} colour = {:>4} tiles read, \
                 {:>5} declared, ratio 1:{:.0}",
                altitude / 1000.0,
                selection.patches.len(),
                height_tiles.len(),
                colour_tiles.len(),
                read,
                declared,
                declared as f64 / read.max(1) as f64,
            );
        }
    }

    println!();
    println!("declared across both bodies: {total_declared} textures");
    println!(
        "at 61-78 ns per texture per frame (NVIDIA/Vulkan, T8/T7h): {:.2}-{:.2} ms",
        total_declared as f64 * 61.0e-6,
        total_declared as f64 * 78.0e-6,
    );

    Ok(())
}
