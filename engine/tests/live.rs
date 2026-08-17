//! The live prediction against the committed reference (ROADMAP H5).
//!
//! Now that the engine computes the trajectory itself, a question appears that
//! did not exist before H5: is it the same thing that lies in the fixture? The
//! answer is neither "yes" nor "no", and that is exactly why it is worth
//! measuring.
//!
//! The fixture is a multiple-shooting solution: seven legs, each from its own
//! node, with discontinuities at the seams (ROADMAP C4). The live prediction is
//! continuous. On a halo orbit with a multiplier of 594 per revolution (C3) the
//! two curves are **obliged** to diverge; the only question is when -- and C4
//! already gave the expectation by measuring the "baseline without correction"
//! for this same orbit: 1.31 revolutions.

use core_rs::{Ephemeris, PropConfig, Propagator, State};
use engine::live;
use engine::trajectory;
use std::sync::Arc;

const DAY: f64 = 86400.0;

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// How long the live prediction holds to the reference.
///
/// The comparison is at the fixture's epochs, so the integration proceeds in
/// legs that land on those instants. For comparing **trajectories** that is
/// correct; the sequence of steps is thereby different from a continuous run,
/// and that is exactly why it is not compared here (H1 measured it
/// separately).
#[test]
fn the_live_prediction_tracks_the_reference_and_then_leaves_it() {
    let reference = trajectory::load();
    let eph = Arc::new(Ephemeris::load(&live::repo_asset()).expect("the asset"));

    let cfg = PropConfig {
        tol_m: 1e-2,
        h_max_s: 3600.0,
        ..PropConfig::default()
    };
    let mut prop = Propagator::new(eph, cfg).expect("the propagator");

    let mut state = live::fixture_start();
    let mut step = 0.0;

    // The instant the discrepancy first exceeds a threshold. The thresholds are
    // not an acceptance criterion but a grid on which the SHAPE of the
    // divergence is visible: exponential or not.
    let mut first_over = [None::<f64>; 4];
    let thresholds = [1.0e3, 1.0e5, 1.0e7, 1.0e9];

    for sample in reference.iter().skip(1) {
        let run = prop
            .run(&state, None, sample.t, &[], &mut [], &mut step)
            .expect("a run within the asset's span");
        state = run.final_state;

        let live_r = [state.r.x, state.r.y, state.r.z];
        let miss = distance(live_r, sample.vessel);

        for (i, limit) in thresholds.iter().enumerate() {
            if first_over[i].is_none() && miss > *limit {
                first_over[i] = Some(sample.t);
            }
        }
    }

    for (limit, when) in thresholds.iter().zip(first_over.iter()) {
        match when {
            Some(t) => println!("  diverged by {limit:e} m after {:.2} days", t / DAY),
            None => println!("  the discrepancy never exceeded {limit:e} m"),
        }
    }

    // A kilometre is already far beyond the integration error (a centimetre of
    // tolerance) and still incomparably smaller than the orbit itself (radius
    // ~4e8 m). So the first threshold catches the moment the instability starts
    // working, not noise.
    let km = first_over[0].expect("a kilometre of discrepancy should have been seen");
    assert!(
        km > 5.0 * DAY,
        "the live prediction tore away from the reference in {:.2} days -- that \
         is far too fast for an orbit with a multiplier of 594 per revolution \
         (14.5 days)",
        km / DAY
    );

    // And the divergence must be visible: if the curves have not diverged in a
    // hundred days, then what is being compared is not what one thinks.
    assert!(
        first_over[3].is_some(),
        "in 101 days an unstable halo orbit should have left the reference by \
         more than 1e9 m -- that it did not means an error in the comparison"
    );

    // What matters here is not "when" but "how fast": four decades between the
    // first and the third threshold give a growth rate, and it must agree with
    // what ROADMAP C3 extracted from the monodromy matrix of this same orbit --
    // 594 per revolution.
    //
    // That is the check that they diverge because of the orbit's instability
    // rather than because of an error in the state, the units or the epoch: such
    // an error would give either linear growth or a completely different rate.
    let revolution = 101.8 * DAY / 7.0;
    let decades = 4.0;
    let span = first_over[2].unwrap() - first_over[0].unwrap();
    let per_revolution = 10f64.powf(decades * revolution / span);
    println!(
        "  rate: x{per_revolution:.0} per revolution ({:.2} days), C3's prediction x594",
        revolution / DAY
    );

    assert!(
        (100.0..10_000.0).contains(&per_revolution),
        "a divergence rate of x{per_revolution:.0} per revolution does not look \
         like the x594 from the monodromy matrix -- they diverge for some reason \
         other than instability"
    );
}

/// A line computed right now really does get drawn -- in both frames.
///
/// The same class of check as `both_frames_draw_visible_pixels` for the fixture
/// (F6): an empty frame and a correct one look the same, so pixels are
/// counted.
#[test]
fn the_live_trajectory_draws_in_both_frames() {
    use engine::gpu::Gpu;
    use engine::trajectory_render::{geocentric_framing, render, rotating_framing, Params};

    const SIZE: u32 = 256;

    let Some(gpu) = Gpu::for_tests() else {
        return;
    };

    let live =
        live::propagate(&live::fixture_start(), 14.0, &live::repo_asset()).expect("the prediction");

    assert!(
        live.legs > 1,
        "the buffer should have cut the prediction into legs, but one came out"
    );
    assert!(live.samples.len() > 100, "too few samples for a line");

    for (rotating, framing) in [
        (false, geocentric_framing(&live.samples)),
        (true, rotating_framing(&live.samples)),
    ] {
        let shot = render(
            &gpu,
            SIZE,
            SIZE,
            &live.samples,
            &Params {
                rotating,
                framing,
                colour: [0.9, 0.6, 0.2, 1.0],
            },
        )
        .expect("the render");

        let mut lit = 0u64;
        for y in 0..shot.height {
            for x in 0..shot.width {
                let p = shot.pixel(x, y);
                if p[0] > 5 || p[1] > 5 || p[2] > 5 {
                    lit += 1;
                }
            }
        }

        assert!(lit > 100, "frame rotating={rotating}: {lit} pixels");
    }
}

/// A prediction is the same trajectory as a flight.
///
/// Two legs through the buffer against a single run to the same time: bitwise
/// the same (CLAUDE.md, invariant 5). H1 measured this in C; here it goes
/// through the whole chain, from the side of the boundary the game lives on.
#[test]
fn a_prediction_and_a_flight_are_the_same_trajectory() {
    let eph = Arc::new(Ephemeris::load(&live::repo_asset()).expect("the asset"));
    let cfg = PropConfig {
        tol_m: 1e-2,
        h_max_s: 3600.0,
        ..PropConfig::default()
    };

    let start = live::fixture_start();
    let t_end = start.t + 3.0 * DAY;

    // The "flight": no samples, a single run.
    let mut flight = Propagator::new(eph.clone(), cfg).expect("the propagator");
    let mut flight_step = 0.0;
    let flown = flight
        .run(&start, None, t_end, &[], &mut [], &mut flight_step)
        .expect("the flight");

    // The "prediction": in legs through the buffer, the way the planner will
    // compute it.
    let mut predict = Propagator::new(eph, cfg).expect("the propagator");
    let mut buffer = [State::default(); 16];
    let mut predict_step = 0.0;
    let mut state = start;
    let mut legs = 0;

    loop {
        let run = predict
            .run(&state, None, t_end, &[], &mut buffer, &mut predict_step)
            .expect("a leg of the prediction");
        state = run.final_state;
        legs += 1;

        if run.stop == core_rs::Stop::ReachedEnd {
            break;
        }
    }

    assert!(legs > 1, "the prediction should have been cut into legs");
    assert_eq!(state.r.x.to_bits(), flown.final_state.r.x.to_bits());
    assert_eq!(state.r.y.to_bits(), flown.final_state.r.y.to_bits());
    assert_eq!(state.r.z.to_bits(), flown.final_state.r.z.to_bits());
    assert_eq!(state.v.x.to_bits(), flown.final_state.v.x.to_bits());
    assert_eq!(state.v.y.to_bits(), flown.final_state.v.y.to_bits());
    assert_eq!(state.v.z.to_bits(), flown.final_state.v.z.to_bits());
    assert_eq!(predict_step.to_bits(), flight_step.to_bits());
}
