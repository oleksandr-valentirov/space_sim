//! Save, load, carry on -- and notice nothing (ROADMAP J6).
//!
//! PROJECT.md §4, rule 4, is finally checked rather than declared: **the
//! integrator state is part of the save.** One claim: continuing after a load
//! equals bitwise continuing without one.
//!
//! The easiest way to break it is to forget the step. The save then does not
//! fail and even looks right: the trajectory after loading is plausible,
//! merely **different**, and in N-body the divergence grows exponentially.

use core_rs::State;
use game::mission;
use game::plan::{Frame, Manoeuvre, Plan};
use game::save::Save;
use game::world::{VesselId, World};

const DAY: f64 = 86400.0;

fn plan_at(start_t: f64) -> Plan {
    let mut plan = Plan::new();
    plan.insert(Manoeuvre {
        t: start_t + 15.0 * DAY,
        dv: [-6.0, 0.0, 0.0],
        frame: Frame::Vnb {
            body: game::world::EARTH,
        },
    });
    plan.insert(Manoeuvre {
        t: start_t + 45.0 * DAY,
        dv: [0.0, 2.5, 0.0],
        frame: Frame::Inertial,
    });
    plan
}

fn planned_world() -> World {
    let mut world = mission::world(&mission::default_asset()).expect("the world builds");
    world
        .commit_plan(VesselId(0), plan_at(mission::start().t))
        .expect("a plan in the future");
    world
}

fn samples_after(world: &World, t: f64) -> Vec<State> {
    world.vessels()[0]
        .trajectory
        .legs()
        .iter()
        .flat_map(|leg| leg.samples.iter())
        .map(|s| s.state)
        .filter(|s| s.t > t)
        .collect()
}

fn assert_same(a: &[State], b: &[State], what: &str) {
    assert_eq!(a.len(), b.len(), "{what}: different sample counts");
    assert!(!a.is_empty(), "{what}: nothing was computed");

    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        for (name, p, q) in [
            ("t", x.t, y.t),
            ("r.x", x.r.x, y.r.x),
            ("r.y", x.r.y, y.r.y),
            ("r.z", x.r.z, y.r.z),
            ("v.x", x.v.x, y.v.x),
            ("v.y", x.v.y, y.v.y),
            ("v.z", x.v.z, y.v.z),
        ] {
            assert_eq!(
                p.to_bits(),
                q.to_bits(),
                "{what}: sample {i}, {name}: {p:e} against {q:e}"
            );
        }
    }
}

/// The main check of J6.
#[test]
fn saving_and_loading_changes_nothing() {
    let start = mission::start();

    // A run with no saving -- the reference.
    let mut plain = planned_world();
    plain.run_to_end(1.0, 8);
    let reference = samples_after(&plain, start.t + 30.0 * DAY);

    // The same run, but on day 30 it is saved and brought back up anew.
    let mut interrupted = planned_world();
    interrupted.run_to_day(start.t + 30.0 * DAY, 1.0, 8);

    let saved = Save::of(&interrupted);
    let cut = saved.vessels[0].tip.t;
    assert!(
        saved.vessels[0].step > 0.0,
        "the step in the save is zero -- then there is nothing to check"
    );

    let text = saved.to_text();
    let mut loaded = Save::from_text(&text)
        .expect("the save is read")
        .into_world(interrupted.ephemeris(), mission::config())
        .expect("a world from the save");

    loaded.run_to_end(1.0, 8);

    // Everything after the save point is compared: before it the loaded world
    // has no history by construction (§4: the trajectory is not part of the
    // save).
    let after_reload = samples_after(&loaded, cut);
    let after_plain = samples_after(&plain, cut);

    println!(
        "  saved on day {:.3}, {} samples compared",
        (cut - start.t) / DAY,
        after_reload.len()
    );
    assert!(
        after_reload.len() > 500,
        "plenty of samples should have been left after the save point"
    );

    assert_same(&after_plain, &after_reload, "after loading");

    // The save point is no later than the cursor and is not the cursor itself:
    // it is the last leg boundary before it. That is why a loaded game jumps
    // neither forward (into an uncomputed forecast) nor to an arbitrary point
    // from which continuing bitwise is impossible.
    let cursor = start.t + 30.0 * DAY;
    assert!(cut <= cursor, "saved ahead of the cursor: {cut} > {cursor}");
    assert!(cut > start.t, "saved at the very start");
    assert!(!reference.is_empty());
}

/// Drop the step from a save and the game becomes a different one.
///
/// The same check for teeth as in H1 and J3, but with the worst price: the
/// difference is not in the work done but in the loaded game flying somewhere
/// other than the saved one did.
#[test]
fn dropping_the_step_from_a_save_gives_a_different_game() {
    let start = mission::start();

    let mut world = planned_world();
    world.run_to_day(start.t + 30.0 * DAY, 1.0, 8);

    let mut honest = Save::of(&world);
    let cut = honest.vessels[0].tip.t;
    let step = honest.vessels[0].step;

    let mut careless = Save::of(&world);
    careless.vessels[0].step = 0.0;

    let run = |save: Save| -> Vec<State> {
        let mut world = save
            .into_world(
                core_rs::Ephemeris::load(&mission::default_asset())
                    .map(std::sync::Arc::new)
                    .expect("asset"),
                mission::config(),
            )
            .expect("world");
        world.run_to_end(1.0, 8);
        samples_after(&world, cut)
    };

    honest.vessels[0].step = step;
    let with_step = run(honest);
    let without = run(careless);

    println!(
        "  with the step: {} samples; without it: {}",
        with_step.len(),
        without.len()
    );

    assert_ne!(
        with_step.len(),
        without.len(),
        "a save without the step gave exactly the same trajectory -- then rule 4 \
         of PROJECT.md §4 means nothing and H1 measured something else"
    );
}

/// A manoeuvre at the save point is flown neither twice nor never.
///
/// The quietest save bug: the states before and after an impulse have **the
/// same time**. A rule of "apply everything no later than" would fly the
/// manoeuvre twice, "apply everything earlier than" not at all. Hence
/// `applied` sits in the file as a number, and the restart point is always
/// pre-impulse.
#[test]
fn a_manoeuvre_at_the_save_point_is_flown_exactly_once() {
    let start = mission::start();
    let burn_t = start.t + 15.0 * DAY;

    let mut world = planned_world();
    // The cursor slightly PAST the manoeuvre: then the last leg boundary
    // before it is exactly the manoeuvre instant, because the leg ends there.
    world.run_to_day(burn_t + 0.5 * DAY, 1.0, 8);

    let saved = Save::of(&world);
    assert_eq!(
        saved.vessels[0].tip.t.to_bits(),
        burn_t.to_bits(),
        "the test wanted to save exactly on the boundary that coincides with the manoeuvre"
    );
    assert_eq!(
        saved.vessels[0].applied, 0,
        "the state on the boundary is pre-impulse, so the manoeuvre is not applied yet"
    );

    // The post-impulse state from the original world: the leg that BEGINS at
    // the manoeuvre.
    let original = world.vessels()[0]
        .trajectory
        .legs()
        .iter()
        .find(|leg| leg.entry.t == burn_t)
        .expect("a new leg should have begun after the manoeuvre")
        .entry;

    let mut loaded = Save::from_text(&saved.to_text())
        .expect("reads back")
        .into_world(world.ephemeris(), mission::config())
        .expect("a world from the save");
    loaded.tick(4);

    let resumed = loaded.vessels()[0]
        .trajectory
        .legs()
        .iter()
        .find(|leg| leg.entry.t == burn_t)
        .expect("after loading the manoeuvre should have been flown")
        .entry;

    for (name, a, b) in [
        ("v.x", original.v.x, resumed.v.x),
        ("v.y", original.v.y, resumed.v.y),
        ("v.z", original.v.z, resumed.v.z),
    ] {
        assert_eq!(
            a.to_bits(),
            b.to_bits(),
            "{name} after loading is {b:e} against {a:e} -- the manoeuvre was not \
             flown exactly once"
        );
    }
}

/// The save text round-trips bitwise, and comments do not get in the way.
#[test]
fn the_text_round_trips_bit_for_bit() {
    let mut world = planned_world();
    world.run_to_day(mission::start().t + 10.0 * DAY, 1.0, 8);

    let saved = Save::of(&world);
    let text = saved.to_text();

    // The decimal values on the lines are for the eye; the parser reads bits.
    assert!(
        text.contains('#'),
        "the save has no comments for the reader"
    );

    let back = Save::from_text(&text).expect("reads back");

    assert_eq!(back.t.to_bits(), saved.t.to_bits());
    assert_eq!(back.warp.to_bits(), saved.warp.to_bits());
    assert_eq!(back.vessels.len(), saved.vessels.len());

    let (a, b) = (&saved.vessels[0], &back.vessels[0]);
    assert_eq!(a.name, b.name);
    assert_eq!(a.step.to_bits(), b.step.to_bits());
    assert_eq!(a.horizon_end.to_bits(), b.horizon_end.to_bits());
    assert_eq!(a.applied, b.applied);
    assert_eq!(a.tip.r.x.to_bits(), b.tip.r.x.to_bits());
    assert_eq!(a.tip.v.z.to_bits(), b.tip.v.z.to_bits());
    assert_eq!(a.tip.t.to_bits(), b.tip.t.to_bits());
    assert_eq!(a.plan, b.plan);
}

/// Someone else's format is not read in silence.
#[test]
fn a_file_that_is_not_a_save_is_refused() {
    assert!(Save::from_text("something else\nt 0\n").is_err());
    assert!(Save::from_text("").is_err());
    assert!(Save::from_text("space_sim save v1\n").is_err(), "no 't'");
}

/// A vessel that feels light pressure is saved and comes back as the same
/// vessel (ROADMAP K6b).
///
/// The same check as the integrator step above, and for the same reason: area
/// and mass enter the force model, so a save that lost them returns a ship
/// flying somewhere other than the saved one. The only difference is that the
/// step was visible in the file at once, while this field is easy to forget.
#[test]
fn a_vessel_with_area_survives_the_save() {
    use core_rs::VesselParams;

    // Both coefficients are non-zero on purpose. With `cd: 0.0` this test
    // would pass even if the save lost the field entirely: zero cannot be told
    // apart from "not written" (ROADMAP K7c).
    let sail = VesselParams {
        mass_kg: 1000.0,
        area_m2: 20.0,
        cr: 1.3,
        cd: 2.2,
    };

    let cursor = mission::start().t + 4.0 * 3600.0;

    // The same world and the same vessel, only with an area. Built directly
    // rather than through mission::world, because the demo vessel deliberately
    // flies without one.
    let build = || {
        let mut world = World::new(
            &mission::default_asset(),
            mission::config(),
            mission::start().t,
            1.0,
        )
        .expect("the world builds");
        world.add_vessel(
            "sail",
            mission::start(),
            mission::start().t + 3.0 * 86400.0,
            Some(sail),
        );
        world
    };

    let mut plain = build();
    while plain.clock().t() < cursor + 3600.0 {
        if plain.step(60.0, 64).legs == 0 {
            break;
        }
    }

    let mut interrupted = build();
    while interrupted.clock().t() < cursor {
        if interrupted.step(60.0, 64).legs == 0 {
            break;
        }
    }

    let save = Save::of(&interrupted);

    // Through the text, not through the struct in memory: if `params` were not
    // printed or not read back, this is where it shows.
    let text = save.to_text();
    assert!(
        text.contains("params"),
        "the area should have reached the file:\n{text}"
    );

    let reloaded = Save::from_text(&text).expect("the save is read");
    assert_eq!(
        reloaded.vessels[0].params,
        Some(sail),
        "the vessel came back a different ship"
    );

    let mut loaded = reloaded
        .into_world(interrupted.ephemeris(), mission::config())
        .expect("a world from the save");
    while loaded.clock().t() < cursor + 3600.0 {
        if loaded.step(60.0, 64).legs == 0 {
            break;
        }
    }

    assert_same(
        &samples_after(&plain, cursor),
        &samples_after(&loaded, cursor),
        "a vessel with an area after loading",
    );
}
