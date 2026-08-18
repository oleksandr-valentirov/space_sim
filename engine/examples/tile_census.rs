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
//! ## X5a: the same census at the depths the pool would serve
//!
//! Stage X5 aims at eight levels -- the depth that reaches the source (2.45 km
//! per node against Blue Marble's 1.85 km per pixel). Residency there is not
//! an option: `--tile-probe` measures 1.5 GiB (NVIDIA) and 2.0 GiB (RADV) for
//! **one** pyramid of **one** body. So the number that sizes the slot pool is
//! how many distinct tiles the frame reads at that depth, and it is measured
//! here rather than extrapolated: patches share ancestors, and how much they
//! share is exactly what changes with depth.
//!
//! Pyramid depth is arithmetic (`covering` and `index` take the level count,
//! not the asset), so the deeper columns need no recook.
//!
//! ⚠ **They are a floor, not the answer.** The patch set itself comes from
//! `lod::select` against **today's** assets, and the criterion asks the terrain
//! about slope (R7c). A deeper pyramid measures slope on a shorter base, so it
//! reports steeper, so the criterion splits further: the real set at eight
//! levels is this one or larger. X5e re-measures on the cooked asset, and the
//! pool is sized with that in mind.
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

/// The pyramid depths the census reports, beyond the assets' own (X5a).
///
/// Seven and eight are the two X5 weighed; six is there so the deeper columns
/// are read against a number this file already printed before X5, rather than
/// against nothing.
const DEPTHS: [u32; 3] = [6, 7, 8];

/// What one tile of the Earth's colour costs in video memory, in bytes.
///
/// Measured, not derived from the payload: `--tile-probe` reports 12288 on
/// NVIDIA/Vulkan and 16384 on RADV for `Rgba8Unorm` at 33^2, against 4356
/// bytes of data. The granularity is a step of the allocator and holds from
/// five levels to eight, which is why one constant serves every depth here.
const TILE_BYTES_NVIDIA: usize = 12288;

/// The same on RADV -- the wider of the two steps, i.e. the pessimistic end.
const TILE_BYTES_RADV: usize = 16384;

/// How many distinct tiles a patch set reads from a pyramid of `levels`.
///
/// The same walk the frame does: a patch deeper than the pyramid reads its
/// nearest ancestor's tile, so the count is of **coverings**, not of patches.
fn tiles_read(levels: u32, patches: &[engine::cubesphere::Patch]) -> usize {
    let mut seen: HashSet<usize> = HashSet::new();
    for patch in patches {
        let (covering, _) = tiles::covering(levels, patch);
        if let Some(index) = tiles::index(levels, &covering) {
            seen.insert(index);
        }
    }
    seen.len()
}

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
    // The worst per-pyramid demand seen at each depth, indexed by depth (X5a).
    let mut worst_per_pyramid = [0usize; 9];

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

            // X5a: the same set against the depths the pool would serve. One
            // pyramid's worth per column -- the two pyramids of a body are
            // counted separately because they are separate pools, and a body
            // may well carry them at different depths (T3b).
            for depth in DEPTHS {
                let per_pyramid = tiles_read(depth, &selection.patches);
                worst_per_pyramid[depth as usize] =
                    worst_per_pyramid[depth as usize].max(per_pyramid);
                println!(
                    "               at {depth} levels: {per_pyramid:>4} tiles per pyramid \
                     ({:>6} declared, ratio 1:{:.0})",
                    tiles::count(depth),
                    tiles::count(depth) as f64 / per_pyramid.max(1) as f64,
                );
            }
        }
    }

    println!();
    println!("declared across both bodies: {total_declared} textures");
    println!(
        "at 61-78 ns per texture per frame (NVIDIA/Vulkan, T8/T7h): {:.2}-{:.2} ms",
        total_declared as f64 * 61.0e-6,
        total_declared as f64 * 78.0e-6,
    );

    // X5a: what sizes the pool. Four pyramids are live at once (two bodies,
    // height and colour each), and the worst case is taken per pyramid rather
    // than per frame because a slot is a slot: the pool is one array, and the
    // demand on it is four times the worst a single pyramid shows.
    println!();
    println!("X5a -- the slot pool, sized by what the frame reads:");
    for depth in DEPTHS {
        let worst = worst_per_pyramid[depth as usize];
        let demand = worst * 4;
        println!(
            "  {depth} levels: worst {worst} tiles in one pyramid, {demand} across four; \
             residency would be {:.2} GiB (NVIDIA) / {:.2} GiB (RADV), \
             a pool of 4096 slots is {:.0} MiB / {:.0} MiB",
            (tiles::count(depth) * 4 * TILE_BYTES_NVIDIA) as f64 / 1024.0 / 1024.0 / 1024.0,
            (tiles::count(depth) * 4 * TILE_BYTES_RADV) as f64 / 1024.0 / 1024.0 / 1024.0,
            (4096 * TILE_BYTES_NVIDIA) as f64 / 1024.0 / 1024.0,
            (4096 * TILE_BYTES_RADV) as f64 / 1024.0 / 1024.0,
        );
    }

    Ok(())
}
