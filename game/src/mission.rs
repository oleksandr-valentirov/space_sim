//! What the game actually launches at J1 (ROADMAP J1).
//!
//! One vessel on a halo orbit near L2 -- catalogue orbit 1151 from JPL, the
//! one running through the whole project: found in CR3BP (C2), carried into
//! the real ephemeris by multiple shooting (C4), drawn by the engine (F6) and
//! first computed live in H5. So its behaviour is already measured, and any
//! deviation here is ours rather than its.
//!
//! **This is not the game's scene and not a save.** Where vessels come from is
//! decided by loading a save (J6); for now the mission is set in code, so
//! there is something to compute and something to compare against.

use std::path::{Path, PathBuf};

use core_rs::{CoreError, Integrator, PropConfig, State};

use crate::plan::{Frame, Manoeuvre, Plan};
use crate::world::{VesselId, World, EARTH};

/// The ephemeris asset, from the repository root.
pub const ASSET: &str = "data/fixture/earth_moon.eph";

/// How many days the mission lasts. The same as H5 measured -- otherwise the
/// two trajectories would have nothing to be compared against.
pub const DAYS: f64 = 101.79;

/// Tolerance and step ceiling: the ones `ex_trajectory` computed the fixture
/// with. One tolerance for prediction and for flight (CLAUDE.md, invariant
/// 5).
pub const TOL_M: f64 = 1e-2;
pub const H_MAX_S: f64 = 3600.0;

/// Default warp: game seconds per second of real time.
///
/// The mission lasts 101.79 days, i.e. 8.8e6 s. At 1e5 it passes in a minute
/// and a half of real time -- slow enough to watch the boundary between
/// history and prediction crawl along the curve, and fast enough not to wait.
/// This is not the ceiling: the horizon sets that (`clock::Stall::Horizon`).
pub const DEFAULT_WARP: f64 = 1.0e5;

/// Where the camera looks from at start, metres above the surface.
///
/// The orbit lies 4.5e8 m from Earth, so at a 60-degree field of view the
/// camera needs at least 8.5e8 m for it to fit in frame. A billion is that
/// number rounded up, not a taste.
pub const CAMERA_ALTITUDE_M: f64 = 1.0e9;

/// The default asset path.
///
/// Assembled from `CARGO_MANIFEST_DIR`, because `cargo run` starts in the
/// current directory while `cargo test` starts in the crate's, and a relative
/// path would mean different things. Temporary in the same sense as in H5: the
/// game will take the real path from application configuration once that
/// exists. The `--asset` argument already allows overriding it without
/// touching code.
pub fn default_asset() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("game must live inside the repository")
        .join(ASSET)
}

pub fn config() -> PropConfig {
    PropConfig {
        integrator: Integrator::Dop853,
        tol_m: TOL_M,
        h_max_s: H_MAX_S,
        max_steps: 0,
        // The USSA-76 profile as the asset holds it. This is where the future
        // "space weather" toggle will land: the game computes the multiplier
        // from the solar activity level and sets it here, because it is
        // constant per leg (`core_rs::PropConfig::density_scale`). With no
        // switch yet it is one, and one changes nothing bitwise.
        density_scale: 1.0,
    }
}

/// The state the mission starts from.
///
/// Taken from the same fixture as H5 (`engine::live`), and deliberately so:
/// for two trajectories to be comparable bitwise they must start from the same
/// bits.
pub fn start() -> State {
    engine::live::fixture_start()
}

/// A plan for display: one braking burn on the tenth day.
///
/// Exists so a manoeuvre can be **seen** rather than only measured in tests.
/// The number is chosen so the difference reads by eye: 12 m/s against a speed
/// of order 200 m/s on this orbit is noticeable, and on an unstable halo orbit
/// with a factor of 594 per revolution (C3) it grows into millions of
/// kilometres over a month.
///
/// Not part of the mission: [`world`] stays without a plan, otherwise the J1
/// comparison against the H5 run would stop meaning anything.
pub fn demo_plan(start_t: f64) -> Plan {
    let mut plan = Plan::new();
    plan.insert(Manoeuvre {
        t: start_t + 10.0 * 86400.0,
        dv: [-12.0, 0.0, 0.0],
        frame: Frame::Vnb { body: EARTH },
    });
    plan
}

/// The same world, but with [`demo_plan`].
pub fn world_with_demo_plan(asset: &Path) -> Result<World, CoreError> {
    let mut world = world(asset)?;
    let start = start();
    world
        .commit_plan(VesselId(0), demo_plan(start.t))
        .expect("the demo plan lies wholly in the future");
    Ok(world)
}

/// A fleet station's parameters: 5000 kg over 20 m^2, `cd` 2.2, `cr` 1.3.
///
/// The ballistic coefficient works out at 114 kg/m^2 -- about the ISS's -- and
/// it is what decides whether a station survives to the end of the measurement
/// or deorbits in the middle of it. `bench_prop` measures 22.7 kg/m^2 (1000 kg
/// over the same 20 m^2), because there the most expensive step is wanted, not
/// the longest life.
pub const STATION_PARAMS: core_rs::VesselParams = core_rs::VesselParams {
    mass_kg: 5000.0,
    area_m2: 20.0,
    cr: 1.3,
    cd: 2.2,
};

/// The fleet's lowest shell, metres.
///
/// 600 km rather than 400: the asset carries USSA-76, and at 400 km even a
/// station would approach atmospheric entry over a hundred days -- the
/// measurement would end in a failed run rather than a number. Above 600 km
/// drag does not remove even a kilometre over the same time.
const SHELL_FLOOR_M: f64 = 600.0e3;

/// How much each successive shell rises, metres.
///
/// 25 km x 29 vessels is a band of 600-1300 km, in which the sample rate
/// changes little (`bench_prop`: 171 per day at 400 km against 129 at
/// 2000 km), while no two vessels fly the same orbit.
const SHELL_STEP_M: f64 = 25.0e3;

/// Orbital planes: a pair of integer vectors each -- the radial direction and
/// a companion Gram-Schmidt takes the velocity direction from.
///
/// Integer triples rather than angles, and not out of pedantry: after
/// normalising by `sqrt` the vector is bit-identical on any platform, whereas
/// `sin`/`cos` are not (invariant 3). A fixture that cannot be reproduced on
/// the machine next door is not worth measuring with.
///
/// Seven of them against twenty-nine shells, so the repetition period (203)
/// exceeds any fleet anyone might want to set up.
const PLANES: [([f64; 3], [f64; 3]); 7] = [
    ([1.0, 0.0, 0.0], [0.0, 12.0, 5.0]),
    ([3.0, 4.0, 0.0], [0.0, 3.0, 4.0]),
    ([2.0, -1.0, 2.0], [1.0, 2.0, 0.0]),
    ([0.0, 5.0, 12.0], [1.0, 0.0, 0.0]),
    ([-4.0, 1.0, 8.0], [2.0, 9.0, 1.0]),
    ([6.0, -2.0, 3.0], [0.0, 1.0, 7.0]),
    ([1.0, 7.0, -4.0], [5.0, 0.0, 2.0]),
];

/// The fleet for the N1 measurement: the showcase halo orbit plus `stations`
/// stations in low Earth orbit.
///
/// **Why mixed.** The trail's limit shows through the number of vessels, and
/// the density comes from the low orbits: the lunar one gives about 28 samples
/// per day against 171 in LEO (`bench_prop`), so thirty copies of the halo
/// would give 84 thousand vertices instead of 616. But the halo stays the
/// first vessel, because everything else in the game rests on it -- the
/// zero-velocity curve, the panels, the demo plan.
///
/// **This is a measurement fixture, not the game's scene.** Where vessels
/// really come from is decided by the save; here they exist so debt D7 shows
/// itself as a number.
pub fn fleet(asset: &Path, stations: usize) -> Result<World, CoreError> {
    let mut world = world(asset)?;
    let eph = world.ephemeris();
    let start = start();
    let earth = eph.body_state(EARTH, start.t)?;
    let mu = eph.body_mu(EARTH);
    let surface = eph.body_radius(EARTH);

    for index in 0..stations {
        let radius = surface + SHELL_FLOOR_M + SHELL_STEP_M * (index % 29) as f64;
        let (out, along) = PLANES[index % PLANES.len()];
        let out = normalize(out);
        // The velocity lies in the (out, along) plane, perpendicular to the
        // radius: Gram-Schmidt, because a table of orthogonal vector pairs
        // would read worse than a table of any two.
        let along = normalize(reject(along, out));
        let speed = (mu / radius).sqrt();

        let state = core_rs::State {
            t: start.t,
            r: core_rs::Vec3d {
                x: earth.r.x + out[0] * radius,
                y: earth.r.y + out[1] * radius,
                z: earth.r.z + out[2] * radius,
            },
            v: core_rs::Vec3d {
                x: earth.v.x + along[0] * speed,
                y: earth.v.y + along[1] * speed,
                z: earth.v.z + along[2] * speed,
            },
        };

        world.add_vessel(
            &format!("station {:02}", index + 1),
            state,
            start.t + DAYS * 86400.0,
            Some(STATION_PARAMS),
        );
    }

    Ok(world)
}

fn normalize(v: [f64; 3]) -> [f64; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / length, v[1] / length, v[2] / length]
}

/// The component of `v` perpendicular to the unit vector `unit`.
fn reject(v: [f64; 3], unit: [f64; 3]) -> [f64; 3] {
    let dot = v[0] * unit[0] + v[1] * unit[1] + v[2] * unit[2];
    [
        v[0] - unit[0] * dot,
        v[1] - unit[1] * dot,
        v[2] - unit[2] * dot,
    ]
}

/// A world with one vessel, ready for its first tick.
pub fn world(asset: &Path) -> Result<World, CoreError> {
    let start = start();
    // The cursor starts where the vessel starts: the asset epoch is time zero
    // for the ephemeris, not for the mission.
    let mut world = World::new(asset, config(), start.t, DEFAULT_WARP)?;
    // Without area: radiation pressure (K6b) is in the force model, but this
    // vessel does not fly through it. The demo's halo orbit was selected
    // without it, and adding it here would change what the demonstration shows
    // under the pretext of a technical step -- that is a decision about
    // content, not about wiring.
    world.add_vessel("halo 1151", start, start.t + DAYS * 86400.0, None);
    Ok(world)
}
