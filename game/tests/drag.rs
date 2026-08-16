//! A vessel in the game really does feel air (ROADMAP K7c).
//!
//! K7b carried drag from the asset to the boundary and stopped at "`cd` can be
//! passed". Who passes it was left unanswered, and a field nobody sets is
//! exactly the dead code K4b found in K4: `field.c` could do harmonics and
//! nobody switched them on.
//!
//! So the check sits at the `/game` level rather than `core-rs`: the world,
//! the clock, the legs, `Vessel::params` -- the whole path by which a number
//! from the game reaches the integrator. Below that it is already checked
//! twice (`core/test/test_prop.c` and `core-rs/tests/ephemeris.rs`), which is
//! exactly why neither tolerances nor physics are needed here: it suffices to
//! show that two worlds differing **only** in `cd` diverge, while two with a
//! zero `cd` do not.
//!
//! The demo mission stays without drag deliberately (`mission::world`), for
//! the same reason it stayed without a sail in K6b: the halo orbit was
//! selected without it, and adding a force there would change what the
//! demonstration shows under the pretext of a technical step. At L2 the air is
//! exactly zero anyway.

use core_rs::{State, VesselParams};
use game::mission;
use game::world::World;

/// Earth in the fixture asset.
const EARTH: i32 = 3;

/// 220 km above Earth's mean radius -- inside a USSA-76 table band rather than
/// on its edge (the K7a lesson: at a boundary the model is discontinuous, and
/// a comparison could fall either way).
const ALTITUDE: f64 = 220.0e3;

/// How long the flight lasts. Twenty minutes at that altitude is already
/// hundreds of metres of divergence -- a quantity not to be confused with
/// integrator noise at a centimetre tolerance.
const FLIGHT_S: f64 = 1200.0;

fn blunt(cd: f64) -> VesselParams {
    VesselParams {
        mass_kg: 1000.0,
        area_m2: 20.0,
        cr: 0.0,
        cd,
    }
}

/// A world with one vessel in low orbit, with a given `cd`.
///
/// The state is built from the asset: Earth's position plus a radius and
/// circular speed, tilted so no component is zero -- the rotating atmosphere's
/// wind must be oblique to the motion, or an error could hide in a zero (the
/// same reason as in `core/scenario/sc_dragflight.c`).
fn low_orbit_world(cd: f64) -> World {
    // The cursor starts where the vessel does: the asset epoch is time zero
    // for the ephemeris rather than for the mission (`mission::world` does the
    // same).
    let t0 = 86_400.0;
    let mut world =
        World::new(&mission::default_asset(), mission::config(), t0, 1.0).expect("the world builds");

    let eph = world.ephemeris();
    let earth = eph.body_state(EARTH, t0).expect("Earth is within the asset");

    // Earth's mean radius in the asset is 6371010 m
    // (core/cook/cook_fixture.c). It is needed here only to end up inside the
    // air, so a hundred metres either way decides nothing.
    let radius = 6_371_010.0 + ALTITUDE;
    let speed = (3.986_004_418e14_f64 / radius).sqrt();

    let mut start = State {
        r: earth.r,
        v: earth.v,
        t: t0,
    };
    start.r.x += radius;
    start.v.y += 0.8 * speed;
    start.v.z += 0.6 * speed;

    world.add_vessel("probe", start, t0 + FLIGHT_S, Some(blunt(cd)));
    world
}

fn flown(cd: f64) -> State {
    let mut world = low_orbit_world(cd);
    world.run_to_end(1.0, 64);
    world.vessels()[0].tip
}

/// The same vessel with and without `cd` arrives in different places.
#[test]
fn a_vessel_with_cd_flies_a_different_trajectory() {
    let with = flown(2.2);
    let without = flown(0.0);

    let dx = with.r.x - without.r.x;
    let dy = with.r.y - without.r.y;
    let dz = with.r.z - without.r.z;
    let moved = (dx * dx + dy * dy + dz * dz).sqrt();

    assert!(
        moved > 1.0,
        "drag should have moved the vessel, and a shift of {moved} m is noise"
    );

    // Both worlds reached the mission's end; otherwise the difference would
    // simply be that one computed less.
    assert_eq!(with.t, without.t, "different instants are being compared");

    println!("{FLIGHT_S} s of drag moved the vessel by {moved:.4} m");
}

/// Two worlds without drag are bit-identical.
///
/// A control experiment: without it the first test would prove only that two
/// runs differ at all, rather than that `cd` is what distinguishes them.
#[test]
fn without_cd_the_two_worlds_agree_to_the_bit() {
    let a = flown(0.0);
    let b = flown(0.0);

    assert_eq!(a.r.x.to_bits(), b.r.x.to_bits());
    assert_eq!(a.r.y.to_bits(), b.r.y.to_bits());
    assert_eq!(a.r.z.to_bits(), b.r.z.to_bits());
    assert_eq!(a.v.x.to_bits(), b.v.x.to_bits());
    assert_eq!(a.v.y.to_bits(), b.v.y.to_bits());
    assert_eq!(a.v.z.to_bits(), b.v.z.to_bits());
}
