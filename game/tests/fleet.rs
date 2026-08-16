//! The fleet fixture debt D7 is measured with (ROADMAP.md, N1).
//!
//! A measurement made with it is worth exactly what the fixture is worth, so
//! what is checked is not "something built" but three claims the N1 numbers
//! depend on:
//!
//! 1. **The vessels differ.** Thirty copies of one orbit is one vessel
//!    measured thirty times; an error of that kind is visible neither in frame
//!    time nor in vertex count.
//! 2. **The orbits really are circular and at the stated altitude.** The
//!    sample density the fleet exists for is a property of altitude; a station
//!    on a stretched ellipse would give a different one, quietly.
//! 3. **The stations survive.** Drag in low orbit is not decoration: a vessel
//!    that deorbits mid-measurement shrinks it silently.

use game::mission;
use game::world::EARTH;

/// How many stations to check. More than seven (the plane table's period) and
/// more than four, so a repetition, if there is one, has time to show.
const STATIONS: usize = 12;

fn build() -> game::world::World {
    mission::fleet(&mission::default_asset(), STATIONS).expect("the fleet builds on the fixture")
}

#[test]
fn every_station_flies_its_own_orbit() {
    let world = build();
    let eph = world.ephemeris();
    let start = mission::start();
    let earth = eph
        .body_state(EARTH, start.t)
        .expect("Earth is in the asset");

    // The halo stays first -- the rest of the game rests on it.
    assert_eq!(world.vessels().len(), STATIONS + 1);
    assert_eq!(world.vessels()[0].name, "halo 1151");

    let mut directions: Vec<[f64; 3]> = Vec::new();
    let mut radii: Vec<f64> = Vec::new();
    for vessel in &world.vessels()[1..] {
        let r = [
            vessel.tip.r.x - earth.r.x,
            vessel.tip.r.y - earth.r.y,
            vessel.tip.r.z - earth.r.z,
        ];
        let radius = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
        directions.push([r[0] / radius, r[1] / radius, r[2] / radius]);
        radii.push(radius);
    }

    // The shells differ -- that is what gives different sample densities.
    for (i, a) in radii.iter().enumerate() {
        for b in &radii[i + 1..] {
            assert!((a - b).abs() > 1.0e3, "two stations on one shell");
        }
    }

    // **And the directions differ too, as its own claim.** The radius
    // differences alone are enough for "two stations on one orbit" never to
    // fire, so a check stopping there would not see a fleet whose plane table
    // was read with one and the same index. That is exactly the fixture this
    // must not be (D13, D14: a symmetric fixture hides a bug three times).
    let mut distinct = 0;
    for (i, a) in directions.iter().enumerate() {
        if !directions[..i]
            .iter()
            .any(|b| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1.0e-9))
        {
            distinct += 1;
        }
    }
    assert!(
        distinct >= 7,
        "only {distinct} distinct planes for {STATIONS} stations -- the plane table is not being read"
    );
}

#[test]
fn every_station_starts_circular_at_its_shell() {
    let world = build();
    let eph = world.ephemeris();
    let start = mission::start();
    let earth = eph
        .body_state(EARTH, start.t)
        .expect("Earth is in the asset");
    let mu = eph.body_mu(EARTH);
    let surface = eph.body_radius(EARTH);

    for vessel in &world.vessels()[1..] {
        let r = [
            vessel.tip.r.x - earth.r.x,
            vessel.tip.r.y - earth.r.y,
            vessel.tip.r.z - earth.r.z,
        ];
        let v = [
            vessel.tip.v.x - earth.v.x,
            vessel.tip.v.y - earth.v.y,
            vessel.tip.v.z - earth.v.z,
        ];
        let radius = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
        let speed = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let altitude = radius - surface;

        // The band from `mission`: 600 km plus 25 km per vessel, 29 shells.
        assert!(
            (600.0e3..=1300.0e3).contains(&altitude),
            "{} starts at an altitude of {altitude:.0} m",
            vessel.name
        );

        // Circular is two conditions, and the second (perpendicularity)
        // catches what the first does not see: a velocity of the right
        // magnitude along the radius would give a fall to Earth with the same
        // modulus.
        let circular = (mu / radius).sqrt();
        assert!(
            (speed - circular).abs() < 1.0e-6,
            "{}: {speed} against circular {circular}",
            vessel.name
        );

        let along_radius = (r[0] * v[0] + r[1] * v[1] + r[2] * v[2]) / (radius * speed);
        assert!(
            along_radius.abs() < 1.0e-12,
            "{}: the velocity is not perpendicular to the radius ({along_radius})",
            vessel.name
        );
    }
}

#[test]
fn the_fleet_survives_the_span_it_is_measured_over() {
    let mut world = build();
    let start = mission::start();

    // Ten days rather than a hundred: in debug a hundred would cost minutes,
    // and what this test catches -- atmospheric entry -- would already show as
    // a step error at 600 km over ten days if the altitude were chosen wrongly.
    // The probe measures the full span (`--perf-probe 101 --stations 30`), and
    // a failure is printed there too.
    world.run_to_day(start.t + 10.0 * 86400.0, 1.0, 8);

    for vessel in world.vessels() {
        assert!(
            vessel.failed.is_none(),
            "{} did not make it: {:?}",
            vessel.name,
            vessel.failed
        );
    }

    // A fleet that never flew also "did not fail". Sample density is what the
    // fixture exists for, so that is what is checked: below a hundred per day
    // would mean the stations are not where they were meant to be.
    let snapshot = world.snapshot();
    let samples: usize = snapshot.vessels.iter().map(|v| v.sample_count()).sum();
    let per_vessel_day = samples as f64 / (snapshot.vessels.len() as f64 * 10.0);
    assert!(
        per_vessel_day > 100.0,
        "sample density {per_vessel_day:.0} per vessel per day -- the fleet is not where it was meant to be"
    );
}
