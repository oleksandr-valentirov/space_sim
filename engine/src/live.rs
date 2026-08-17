//! A trajectory computed now, not read from a CSV (ROADMAP H5).
//!
//! This is the first place where physics and the renderer see each other.
//! Before it the engine drew fixtures: `data/fixture/halo_inertial.csv`, a
//! ready export of `core/export/ex_trajectory`, and `engine` did not even link
//! `core-rs`. Now it does, and the line in the frame is the output of
//! `prop_run` rather than a column of text.
//!
//! What happens here, in three lines:
//!
//!   1. the vessel state from the fixture's first sample -- `vx,vy,vz` were
//!      brought back into the export exactly for this: the propagator needs a
//!      state, not a position;
//!   2. `core_rs::Propagator` carries it through the field of the asset's ten
//!      bodies, leg by leg over the buffer -- exactly as the game will;
//!   3. for every sample we ask the ephemeris where Earth and the Moon were
//!      then -- because that is what `trajectory_render` draws, and the frame
//!      transform stands on it (PROJECT.md section 7).
//!
//! ## Why this is not just "the same, only slower"
//!
//! The fixture is not one trajectory. It is a multiple-shooting solution:
//! seven legs, each integrated from its own node, with 2.3e-2 m discontinuities
//! at the seams (ROADMAP C4). A live prediction has no discontinuities by
//! construction, and on an unstable halo orbit (594x per revolution) that is
//! not a detail -- which is why how long the two curves stay together is a
//! measurable statement rather than a tautology. `engine/tests/live.rs`
//! measures it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use core_rs::{CoreError, Ephemeris, PropConfig, Propagator, State};

use crate::trajectory::{self, Sample};

/// The ephemeris asset, relative to the repository root.
pub const ASSET: &str = "data/fixture/earth_moon.eph";

/// The same asset as an absolute path, built from `CARGO_MANIFEST_DIR`.
///
/// This is the path **for probes and tests**, not for the game: `cargo test`
/// runs the binary from the crate directory, `cargo run` from wherever it was
/// called, and a relative path would mean different things in those two cases.
/// The game will get its asset path from the application when there is one;
/// inventing a configuration layer for that now is work without a criterion.
pub fn repo_asset() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("engine must live inside the repository")
        .join(ASSET)
}

/// Body indices in the cooker's order (`core/cook/cook_fixture.c`).
const EARTH: i32 = 3;
const MOON: i32 = 4;

/// The tolerance -- the same centimetre `ex_trajectory` computed the fixture
/// with. One tolerance for prediction and for physics (CLAUDE.md, invariant 5);
/// here it also matches the reference, otherwise comparing two curves would
/// measure a difference of settings instead of a difference of trajectories.
const TOL_M: f64 = 1e-2;

/// Step ceiling. Given explicitly, because with zero the integrator picks it
/// from the leg length -- and then a stitched run leaves behind a different
/// step than a continuous one (`core/prop.h`, measured).
const H_MAX_S: f64 = 3600.0;

/// How many samples one `run` call takes. Deliberately few: not an
/// optimisation but how this will work in the game -- the prediction is
/// computed in chunks between which control can be given back, and the
/// stitching path must be the everyday one rather than a rare branch that
/// fires for the first time under load.
const LEG: usize = 64;

pub struct Live {
    pub samples: Vec<Sample>,
    /// How many `run` calls were needed. What is interesting is not the
    /// number but that it is greater than one: stitching legs is not a
    /// hypothetical path.
    pub legs: usize,
}

/// A prediction from state `start`, `days` days forward.
pub fn propagate(start: &State, days: f64, asset: &Path) -> Result<Live, CoreError> {
    let eph = Arc::new(Ephemeris::load(asset)?);

    let cfg = PropConfig {
        tol_m: TOL_M,
        h_max_s: H_MAX_S,
        ..PropConfig::default()
    };
    let mut prop = Propagator::new(eph.clone(), cfg)?;

    let t_end = start.t + days * 86400.0;

    let mut buffer = vec![State::default(); LEG];
    let mut step = 0.0;
    let mut state = *start;
    let mut legs = 0;
    let mut samples = Vec::new();

    loop {
        let run = prop.run(&state, None, t_end, &[], &mut buffer, &mut step)?;
        legs += 1;

        for s in &buffer[..run.filled] {
            samples.push(Sample {
                t: s.t,
                vessel: [s.r.x, s.r.y, s.r.z],
                velocity: [s.v.x, s.v.y, s.v.z],
                earth: position(&eph, EARTH, s.t)?,
                moon: position(&eph, MOON, s.t)?,
                z_axis: [0.0, 0.0, 0.0],
                // There is no oracle and cannot be one: nobody computed this
                // trajectory in advance. The renderer computes the synodic
                // coordinates itself.
                synodic_reference: [0.0, 0.0, 0.0],
            });
        }

        state = run.final_state;

        if run.stop == core_rs::Stop::ReachedEnd {
            break;
        }
    }

    trajectory::fill_axes(&mut samples);

    Ok(Live { samples, legs })
}

fn position(eph: &Ephemeris, body: i32, t: f64) -> Result<[f64; 3], CoreError> {
    let s = eph.body_state(body, t)?;
    Ok([s.r.x, s.r.y, s.r.z])
}

/// The state the F6 fixture starts from -- and the only thing taken from it.
///
/// Same instant, same vessel, so the two curves can be laid side by side.
pub fn fixture_start() -> State {
    let samples = trajectory::load();
    let first = &samples[0];

    State {
        r: core_rs::Vec3d {
            x: first.vessel[0],
            y: first.vessel[1],
            z: first.vessel[2],
        },
        v: core_rs::Vec3d {
            x: first.velocity[0],
            y: first.velocity[1],
            z: first.velocity[2],
        },
        t: first.t,
    }
}
