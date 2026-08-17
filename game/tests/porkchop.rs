//! The window grid is computed in the planner thread (ROADMAP-UI.md, U5b).
//!
//! Three claims, none of them about pixels:
//!
//! 1. the grid from the thread is the same as a direct call to the boundary,
//!    cell for cell;
//! 2. where there is no solution a **hole** is left, not a zero;
//! 3. the thread does not go deaf from a grid: it has one cancellation rule
//!    for two kinds of work, and the grid does not break it.
//!
//! The first is about the axes. `t1` and `tof` are both positive and both in
//! seconds, so a transposed grid looks entirely plausible; U5a caught that at
//! the boundary, here the same thing is caught on a dense grid, where a cell
//! also has to land in the right row.

use std::sync::Arc;
use std::time::{Duration, Instant};

use game::mission;
use game::planner::{Planner, PreviewRequest, Request};
use game::porkchop::{Grid, GridRequest};
use game::world::{EARTH, MOON};

const DAY: f64 = 86400.0;
const PATIENCE: Duration = Duration::from_secs(20);

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !done() {
        assert!(Instant::now() < deadline, "never arrived: {what}");
        std::thread::yield_now();
    }
}

fn ephemeris() -> Arc<core_rs::Ephemeris> {
    Arc::new(core_rs::Ephemeris::load(&mission::default_asset()).expect("fixture"))
}

/// Departure states for the grid.
///
/// Taken from the mission's initial state shifted in time rather than from a
/// live trajectory, and that is deliberate: the sweep does not care where the
/// states came from, and running the world would cost seconds per test. That
/// the states really do come from a trajectory is checked by a separate test
/// at the end.
fn departures(count: usize, step: f64, from: f64) -> Vec<core_rs::State> {
    let base = mission::start();
    (0..count)
        .map(|i| core_rs::State {
            t: from + i as f64 * step,
            ..base
        })
        .collect()
}

/// A grid entirely inside the asset's span: the fixture knows 120 days from
/// J2000, so a departure up to day 60 plus a transfer up to 10 is well inside.
fn inside(id: u64, mu: f64) -> GridRequest {
    GridRequest {
        id,
        depart: departures(40, 1.5 * DAY, mission::start().t),
        arrive_body: MOON,
        centre_body: EARTH,
        mu,
        prograde: true,
        tof: (0..30).map(|i| (1.0 + f64::from(i) * 0.3) * DAY).collect(),
    }
}

fn ask_for(planner: &Planner, request: &GridRequest) -> Grid {
    planner.request(Request::Grid(request.clone()));
    let mut got = None;
    wait_until("the grid", || {
        if let Some(grid) = planner.latest_grid() {
            got = Some(grid);
        }
        got.as_ref().is_some_and(|g: &Grid| g.id == request.id)
    });
    got.expect("just checked")
}

/// A cell tells the truth about its transfer, and it is not the sweep that
/// checks it.
///
/// The oracle here is deliberately **not** "the grid against
/// `porkchop_compute_eph`": both paths would solve the same Lambert problem,
/// and an error in the choice of coordinate frame would cancel on both sides.
/// That is exactly how it lived from U5a until this test -- with a
/// barycentric fixture the arc was built around the origin with Earth's `mu`,
/// i.e. around the Sun with Earth's mass, and the numbers looked plausible
/// (2 to 9.6 km/s).
///
/// So the physics of the cell is checked instead, by three independent
/// claims:
///
/// 1. a vessel given `dv` and flown as **two bodies** arrives where the
///    target body will be at that moment (a Kepler arc, not the integrator);
/// 2. `dv_m_s` is the length of `dv` and not some neighbouring number;
/// 3. `v_inf_arrive` is the speed relative to the body, not to the centre.
#[test]
fn a_cell_is_a_transfer_that_actually_arrives() {
    let eph = ephemeris();
    let mu = eph.body_mu(EARTH);
    assert!(mu > 0.0, "the fixture must know Earth's mass");

    let request = inside(1, mu);
    let planner = Planner::spawn(eph.clone(), mission::config()).expect("the planner");

    let started = Instant::now();
    let grid = ask_for(&planner, &request);
    let took = started.elapsed();

    assert_eq!(grid.cells.len(), request.depart.len() * request.tof.len());
    assert_eq!(grid.t1, request.t1());
    assert_eq!(grid.tof, request.tof);

    let mut checked = 0;
    let mut wild = 0;
    let mut worst_miss: f64 = 0.0;
    for i in 0..grid.t1.len() {
        for j in 0..grid.tof.len() {
            let Some(cell) = grid.at(i, j) else { continue };
            let (t1, tof) = (grid.t1[i], grid.tof[j]);

            // Wild cells are skipped, and the bound is not about physics but
            // about **this test's own solver**: on an arc of a hundred
            // kilometres per second the universal variable loses digits in the
            // hyperbolic cosines, and the check starts failing on its own
            // accuracy rather than on someone else's bug. A player would not
            // pick such a window anyway -- it is an order dearer than anything
            // flown.
            if cell.dv_m_s > 10_000.0 || cell.v_inf_arrive > 10_000.0 {
                wild += 1;
                continue;
            }

            // The vessel state at departure is the one we supplied, with the
            // cell's manoeuvre on top of it.
            let from = request.depart[i];
            let centre = eph
                .body_state(EARTH, t1)
                .expect("Earth within the asset's span");
            let target = eph
                .body_state(MOON, t1 + tof)
                .expect("the Moon within the asset's span");
            let centre_then = eph
                .body_state(EARTH, t1 + tof)
                .expect("Earth within the asset's span");

            // The length of the manoeuvre is the length of its vector. A
            // trifle that is easy to break by showing a neighbouring number.
            let length = (cell.dv[0].powi(2) + cell.dv[1].powi(2) + cell.dv[2].powi(2)).sqrt();
            assert!(
                (length - cell.dv_m_s).abs() <= 1e-9 * length.max(1.0),
                "({i}, {j}): |dv| = {length}, but {} is shown",
                cell.dv_m_s
            );

            // Where this arc leads. Kepler, not our integrator: the cell was
            // computed by a Kepler problem too, and the question is exactly
            // whether it was done in the right frame.
            let r0 = [
                from.r.x - centre.r.x,
                from.r.y - centre.r.y,
                from.r.z - centre.r.z,
            ];
            let v0 = [
                from.v.x - centre.v.x + cell.dv[0],
                from.v.y - centre.v.y + cell.dv[1],
                from.v.z - centre.v.z + cell.dv[2],
            ];
            let (arrive, arrive_v) = kepler(r0, v0, tof, mu);

            let want = [
                target.r.x - centre_then.r.x,
                target.r.y - centre_then.r.y,
                target.r.z - centre_then.r.z,
            ];
            let miss = ((arrive[0] - want[0]).powi(2)
                + (arrive[1] - want[1]).powi(2)
                + (arrive[2] - want[2]).powi(2))
            .sqrt();
            let distance = (want[0].powi(2) + want[1].powi(2) + want[2].powi(2)).sqrt();

            // The tolerance is a fraction of the distance rather than metres:
            // two solutions of the same Kepler problem by different methods
            // are being compared (Lambert against the universal variable). The
            // error the test catches is of a different order entirely: the
            // wrong centre is 1.5e11 m, the wrong body 4e8 m.
            assert!(
                miss <= 1e-6 * distance,
                "({i}, {j}): the arc missed the Moon by {miss:.3e} m at a distance \
                 of {distance:.3e} m -- that is not this transfer",
            );
            worst_miss = worst_miss.max(miss / distance);

            // The arrival speed is relative to the **body**, not to the
            // centre. The difference is the Moon's speed, about a kilometre
            // per second: obvious to the eye in the panel and invisible in the
            // code.
            let moon_v = [
                target.v.x - centre_then.v.x,
                target.v.y - centre_then.v.y,
                target.v.z - centre_then.v.z,
            ];
            let relative = ((arrive_v[0] - moon_v[0]).powi(2)
                + (arrive_v[1] - moon_v[1]).powi(2)
                + (arrive_v[2] - moon_v[2]).powi(2))
            .sqrt();
            // The same tolerance for the same reason: a "relative to the
            // centre" error would be the Moon's speed, a kilometre per second
            // -- nine orders larger.
            assert!(
                (relative - cell.v_inf_arrive).abs() <= 1e-6 * relative,
                "({i}, {j}): relative to the Moon it comes out {relative:.1} m/s, \
                 while the cell says {:.1}",
                cell.v_inf_arrive
            );

            checked += 1;
        }
    }

    // Forty is not a round number but a margin under how many there really
    // are: the departure states in this test are artificial (the position
    // stands still while the Moon travels), so most windows come out wild.
    // What matters is that enough are checked and that they have not vanished
    // entirely.
    assert!(
        checked >= 40,
        "only {checked} cells checked, another {wild} discarded as wild"
    );
    println!(
        "  {checked} cells checked, {wild} wild ones skipped; \
         worst miss of the check {worst_miss:.1e} of the distance"
    );

    let (low, high) = grid.scale().expect("a grid where nothing converged");
    let (i, j, best) = grid.best().expect("the best window");
    println!(
        "  {checked} cells of {} in {took:?}; price from {low:.0} to {high:.0} m/s;\n  \
         cheapest: departure on day {:.1}, transfer {:.1} days, {:.0} + {:.0} m/s",
        grid.cells.len(),
        (grid.t1[i] - mission::start().t) / DAY,
        grid.tof[j] / DAY,
        best.dv_m_s,
        best.v_inf_arrive
    );

    // The best window really is the cheapest of them all, not the first found.
    for cell in grid.cells.iter().flatten() {
        assert!(cell.total() >= best.total());
    }
    assert!(
        (low - best.total()).abs() < 1e-9,
        "the end of the scale and the minimum differ"
    );
}

/// Kepler propagation of a state by `dt` -- universal variable, bisection.
///
/// A second implementation of the same physics, and that is precisely why it
/// is here: were the check to call `lambert_solve`, it would compare the
/// sweep with itself. It returns position and velocity: the first says
/// whether the arc really leads to the Moon, the second whether
/// `v_inf_arrive` is measured relative to the body.
///
/// Bisection rather than Newton, and that is not laziness: time as a function
/// of the universal anomaly increases monotonically, so a bracketed search
/// always converges, while Newton with an elliptic initial guess flies apart
/// on a hyperbolic arc -- and a burn from a halo orbit to the Moon is often
/// hyperbolic. A test that fails because of its own solver is worse than no
/// test: it says the sound thing is broken.
fn kepler(r0: [f64; 3], v0: [f64; 3], dt: f64, mu: f64) -> ([f64; 3], [f64; 3]) {
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let r = dot(r0, r0).sqrt();
    let alpha = 2.0 / r - dot(v0, v0) / mu; // 1/a
    let rv = dot(r0, v0);
    let root = mu.sqrt();

    // The time the universal anomaly x leads to.
    let time_of = |x: f64| -> f64 {
        let (c, s) = stumpff(alpha * x * x);
        (rv / root * x * x * c + (1.0 - alpha * r) * x * x * x * s + r * x) / root
    };

    // The bracket: push the upper bound out until it overshoots dt.
    let mut low = 0.0;
    let mut high = 1.0;
    while time_of(high) < dt {
        high *= 2.0;
        assert!(high < 1e12, "the arc never reaches {dt} s at any anomaly");
    }

    // A hundred halvings is 2^-100 of the initial bracket, far beyond double
    // precision; the loop is stopped by the bounds meeting.
    for _ in 0..100 {
        let mid = 0.5 * (low + high);
        if mid <= low || mid >= high {
            break;
        }
        if time_of(mid) < dt {
            low = mid;
        } else {
            high = mid;
        }
    }

    let x = 0.5 * (low + high);
    let z = alpha * x * x;
    let (c, s) = stumpff(z);
    let f = 1.0 - x * x / r * c;
    let g = dt - x * x * x / root * s;

    let position = [
        f * r0[0] + g * v0[0],
        f * r0[1] + g * v0[1],
        f * r0[2] + g * v0[2],
    ];

    let r_new =
        (position[0] * position[0] + position[1] * position[1] + position[2] * position[2]).sqrt();
    let fdot = root / (r * r_new) * x * (z * s - 1.0);
    let gdot = 1.0 - x * x / r_new * c;

    let velocity = [
        fdot * r0[0] + gdot * v0[0],
        fdot * r0[1] + gdot * v0[1],
        fdot * r0[2] + gdot * v0[2],
    ];

    (position, velocity)
}

/// The Stumpff functions C(z) and S(z), by series near zero.
fn stumpff(z: f64) -> (f64, f64) {
    if z > 1e-6 {
        let sz = z.sqrt();
        ((1.0 - sz.cos()) / z, (sz - sz.sin()) / (z * sz))
    } else if z < -1e-6 {
        let sz = (-z).sqrt();
        ((sz.cosh() - 1.0) / -z, (sz.sinh() - sz) / (-z * sz))
    } else {
        (0.5 - z / 24.0, 1.0 / 6.0 - z / 120.0)
    }
}

/// Past the end of the asset a cell **disappears** rather than costing zero.
///
/// That is the very difference the dense grid exists for: zero is the
/// cheapest transfer possible, so on the plot it would look like the best
/// window and the player would click exactly there. The fixture covers 120
/// days, so a transfer landing later gives the ephemeris nothing to compute
/// from.
#[test]
fn a_window_past_the_end_of_the_asset_is_a_hole_not_a_bargain() {
    let eph = ephemeris();
    let mu = eph.body_mu(EARTH);
    let planner = Planner::spawn(eph, mission::config()).expect("the planner");

    // Departure on day 115, transfers from one day to twelve: the first
    // columns are still inside the 120 days, the last are past the edge.
    let request = GridRequest {
        id: 7,
        depart: departures(1, DAY, 115.0 * DAY),
        arrive_body: MOON,
        centre_body: EARTH,
        mu,
        prograde: true,
        tof: (1..=12).map(|i| f64::from(i) * DAY).collect(),
    };

    let grid = ask_for(&planner, &request);

    let inside = grid
        .at(0, 0)
        .expect("a one-day transfer still fits into 120 days");
    assert!(
        inside.total() > 0.0,
        "a cell inside the span cannot cost zero"
    );
    assert_eq!(
        grid.at(0, 11),
        None,
        "a transfer to day 127 is past the end of the asset, yet the grid knows \
         something about it"
    );

    let holes = grid.cells.iter().filter(|c| c.is_none()).count();
    println!("  {holes} holes out of {} cells", grid.cells.len());
    assert!(holes > 0, "no forbidden zones in sight -- nothing to check");

    // And the best window is looked for among what exists, not among the
    // holes.
    let (_, j, _) = grid.best().expect("at least one window");
    assert!(
        grid.at(0, j).is_some(),
        "a hole was named the best window -- which is exactly what the dense grid \
         does not allow"
    );
}

/// The chosen window really takes the vessel to the Moon (ROADMAP-UI.md,
/// U5d).
///
/// An end-to-end check, and the only one that does not take the grid at its
/// word: the departure states come from a live trajectory, the manoeuvre from
/// a cell, and the vessel flies under the **full force model** in the planner
/// thread. Kepler takes no part here; there is exactly one question -- did it
/// arrive.
///
/// The number the test was written for is in the printout: by how much the
/// two-body patched conic parts with reality. That is neither an error nor a
/// debt but the price of the instrument itself: the grid **chooses a window**,
/// and the trajectory is then refined by differential correction (PROJECT.md
/// §8). Hence the coarse tolerance below -- it claims "that way and close",
/// not "hit".
#[test]
fn a_chosen_window_flies_the_vessel_to_the_moon() {
    let sim = game::sim::Sim::spawn(mission::world(&mission::default_asset()).expect("world"))
        .expect("the simulation thread");
    sim.send(game::sim::Command::TogglePause);

    // Let the forecast run as far ahead as it can: the departure axis has no
    // right to reach past what is computed (`app::grid_request`), or the
    // departure states are made up.
    wait_until("the horizon", || {
        let s = sim.snapshot();
        s.vessels[0].computed_to > s.t + 30.0 * DAY
    });

    let snapshot = sim.snapshot();
    let vessel = &snapshot.vessels[0];
    let eph = sim.ephemeris();
    let planner = Planner::spawn(eph.clone(), mission::config()).expect("the planner");

    // Exactly what `app::grid_request` does, only from the test's own code:
    // departure states from the trajectory, the step chosen to fit inside what
    // is computed.
    let span = vessel.computed_to - snapshot.t;
    let step = (span / 40.0).min(DAY);
    let depart: Vec<core_rs::State> = (0..40)
        .map(|i| game::leg::state_at(&vessel.legs, vessel.start, snapshot.t + i as f64 * step))
        .collect();

    let grid = ask_for(
        &planner,
        &GridRequest {
            id: 11,
            depart,
            arrive_body: MOON,
            centre_body: EARTH,
            mu: eph.body_mu(EARTH),
            prograde: true,
            tof: (1..=28).map(|j| f64::from(j) * 0.5 * DAY).collect(),
        },
    );

    let (i, j, cell) = grid.best().expect("at least one window");
    let (t1, tof) = (grid.t1[i], grid.tof[j]);
    println!(
        "  chosen window: departure on day {:.2}, transfer {:.2} days, \
         manoeuvre {:.1} m/s, arrival {:.1} m/s",
        (t1 - mission::start().t) / DAY,
        tof / DAY,
        cell.dv_m_s,
        cell.v_inf_arrive
    );

    // Exactly the manoeuvre `app::choose_window` would put in the draft.
    let mut plan = game::plan::Plan::new();
    plan.insert(game::plan::Manoeuvre {
        t: t1,
        dv: cell.dv,
        frame: game::plan::Frame::Inertial,
    });

    let restart = game::leg::restart_at(&vessel.legs, vessel.start, t1);
    planner.request(Request::Preview(PreviewRequest {
        id: 12,
        vessel: vessel.id,
        from: restart.state,
        step: restart.step,
        plan,
        params: vessel.params,
        horizon_end: vessel.horizon_end.max(t1 + tof + DAY),
    }));

    let mut preview = None;
    wait_until("the preview of the chosen window", || {
        if let Some(got) = planner.latest() {
            preview = Some(got);
        }
        preview.as_ref().is_some_and(|p| p.id == 12)
    });
    let preview = preview.expect("just checked");

    // The closest approach to the Moon, over the preview's samples. The Moon's
    // position is carried by the sample itself (`leg::Sample::moon`), so no
    // ephemeris is needed here.
    let closest = |legs: &[std::sync::Arc<game::leg::Leg>]| -> (f64, f64) {
        let mut best = (f64::INFINITY, 0.0);
        for leg in legs {
            for sample in &leg.samples {
                if sample.state.t < t1 {
                    continue;
                }
                let d = ((sample.state.r.x - sample.moon[0]).powi(2)
                    + (sample.state.r.y - sample.moon[1]).powi(2)
                    + (sample.state.r.z - sample.moon[2]).powi(2))
                .sqrt();
                if d < best.0 {
                    best = (d, sample.state.t);
                }
            }
        }
        best
    };

    let (with_burn, when) = closest(&preview.legs);
    let (without_burn, _) = closest(&vessel.legs);

    // And separately, the distance at exactly the instant the cell promised.
    // The closest approach can happen earlier: the full model leads the vessel
    // differently from a two-body arc, and the difference between these two
    // numbers is what the correction later removes.
    let at_arrival = preview
        .legs
        .iter()
        .flat_map(|leg| leg.samples.iter())
        .min_by(|a, b| {
            let (x, y) = (
                (a.state.t - (t1 + tof)).abs(),
                (b.state.t - (t1 + tof)).abs(),
            );
            x.partial_cmp(&y).expect("time is not NaN")
        })
        .map(|s| {
            ((s.state.r.x - s.moon[0]).powi(2)
                + (s.state.r.y - s.moon[1]).powi(2)
                + (s.state.r.z - s.moon[2]).powi(2))
            .sqrt()
        })
        .expect("the preview is not empty");
    println!(
        "  at the promised arrival instant: {:.0} km",
        at_arrival / 1000.0
    );

    println!(
        "  closest approach: {:.0} km on day {:.2} (without the manoeuvre -- {:.0} km)",
        with_burn / 1000.0,
        (when - mission::start().t) / DAY,
        without_burn / 1000.0
    );

    assert!(
        with_burn < without_burn,
        "with the manoeuvre the vessel came no closer than without it: \
         {with_burn:.3e} against {without_burn:.3e} m -- that is no transfer to the Moon"
    );
    assert!(
        with_burn < 1.0e8,
        "a closest approach of {with_burn:.3e} m is a quarter of the distance to \
         the Moon and more, i.e. the window leads nowhere"
    );
}

/// A grid does not break the cancellation rule shared by two kinds of work.
///
/// Empty axes are a request about nothing, and there is no answer to it (the
/// thread does not invent an empty plot). "Nothing arrived" can only be
/// checked through what arrives next: had the thread gone deaf on such a
/// request, the following answer would never come.
#[test]
fn an_empty_axis_leaves_the_thread_working() {
    let eph = ephemeris();
    let mu = eph.body_mu(EARTH);
    let planner = Planner::spawn(eph, mission::config()).expect("the planner");

    planner.request(Request::Grid(GridRequest {
        id: 1,
        depart: Vec::new(),
        arrive_body: MOON,
        centre_body: EARTH,
        mu,
        prograde: true,
        tof: vec![DAY],
    }));

    let grid = ask_for(&planner, &inside(2, mu));
    assert_eq!(grid.id, 2);
    assert!(grid.cells.iter().flatten().count() > 0);
}

/// A preview asked for after a grid still arrives -- and the other way round.
///
/// Both kinds of work go over one channel exactly for this: the "newer
/// cancels older" rule stays single, and neither kind has a queue of its own
/// to get stuck in.
#[test]
fn a_preview_asked_after_a_grid_still_arrives() {
    let sim = game::sim::Sim::spawn(mission::world(&mission::default_asset()).expect("world"))
        .expect("the simulation thread");
    sim.send(game::sim::Command::TogglePause);

    let burn_t = mission::start().t + 30.0 * DAY;
    wait_until("the horizon", || {
        sim.snapshot().vessels[0].computed_to > burn_t
    });

    let snapshot = sim.snapshot();
    let vessel = &snapshot.vessels[0];
    let restart = game::leg::restart_at(&vessel.legs, vessel.start, burn_t);

    let eph = sim.ephemeris();
    let mu = eph.body_mu(EARTH);
    let planner = Planner::spawn(eph, mission::config()).expect("the planner");

    let mut plan = game::plan::Plan::new();
    plan.insert(game::plan::Manoeuvre {
        t: burn_t,
        dv: [-8.0, 0.0, 0.0],
        frame: game::plan::Frame::Vnb { body: EARTH },
    });

    planner.request(Request::Grid(inside(1, mu)));
    planner.request(Request::Preview(PreviewRequest {
        id: 2,
        vessel: vessel.id,
        from: restart.state,
        step: restart.step,
        plan,
        params: vessel.params,
        horizon_end: vessel.horizon_end,
    }));

    let mut preview = None;
    wait_until("the preview after the grid", || {
        if let Some(got) = planner.latest() {
            preview = Some(got);
        }
        preview.as_ref().is_some_and(|p| p.id == 2)
    });
    assert!(!preview.expect("just checked").legs.is_empty());
}

// ---------------------------------------------------------------------------
// The plot: image, axes, cursor (U5c)
//
// Neither the asset nor the thread appears below -- the grid is built by hand.
// That is the point: everything here checks the translation of a grid into a
// screen, and that has no right to depend on where the grid came from.

use engine::egui;
use game::hud;
use game::porkchop::{cell_at, colour, Cell};
use game::text::Language;

/// A 4x3 grid with a hole in the corner: prices grow with both indices.
fn handmade() -> Grid {
    let t1: Vec<f64> = (0..4).map(|i| f64::from(i) * DAY).collect();
    let tof: Vec<f64> = (1..4).map(|j| f64::from(j) * DAY).collect();

    let mut cells = Vec::new();
    for i in 0..t1.len() {
        for j in 0..tof.len() {
            // The top right corner is a forbidden zone.
            cells.push(if i == 3 && j == 2 {
                None
            } else {
                Some(Cell {
                    dv: [100.0 * (i + 1) as f64, 0.0, 0.0],
                    dv_m_s: 100.0 * (i + 1) as f64,
                    v_inf_arrive: 10.0 * (j + 1) as f64,
                })
            });
        }
    }

    Grid {
        id: 42,
        t1,
        tof,
        cells,
    }
}

/// A hole is transparent, a price is not, and cheap does not look like dear.
///
/// Three properties of the colour that decide whether the plot can be
/// trusted. The first matters most: an opaque hole would sit on the same
/// scale as the prices, and the eye would start comparing it with them.
#[test]
fn a_hole_is_transparent_and_a_price_is_not() {
    let cheap = Cell {
        dv: [100.0, 0.0, 0.0],
        dv_m_s: 100.0,
        v_inf_arrive: 10.0,
    };
    let costly = Cell {
        dv: [900.0, 0.0, 0.0],
        dv_m_s: 900.0,
        v_inf_arrive: 90.0,
    };
    let (low, high) = (cheap.total(), costly.total());

    assert_eq!(colour(None, low, high)[3], 0, "a hole must be transparent");
    assert_eq!(colour(Some(cheap), low, high)[3], 255);
    assert_eq!(colour(Some(costly), low, high)[3], 255);
    assert_ne!(
        colour(Some(cheap), low, high),
        colour(Some(costly), low, high),
        "the ends of the scale are painted alike -- the plot shows nothing"
    );

    // A uniform grid is the cheap end, not the dear one and not a division by
    // zero.
    let flat = colour(Some(cheap), low, low);
    assert_eq!(flat, colour(Some(cheap), low, high));
    assert_eq!(flat[3], 255);
}

/// The scale is monotone: dearer is not "different" but further one way.
#[test]
fn the_scale_goes_one_way() {
    let (low, high) = (100.0, 1000.0);
    let mut previous = colour(
        Some(Cell {
            dv: [low, 0.0, 0.0],
            dv_m_s: low,
            v_inf_arrive: 0.0,
        }),
        low,
        high,
    );

    for step in 1..=9 {
        let cell = Cell {
            dv: [low + f64::from(step) * 100.0, 0.0, 0.0],
            dv_m_s: low + f64::from(step) * 100.0,
            v_inf_arrive: 0.0,
        };
        let now = colour(Some(cell), low, high);
        assert!(
            now[0] >= previous[0] && now[2] <= previous[2],
            "at step {step} the scale turned back: {previous:?} -> {now:?}"
        );
        previous = now;
    }
}

/// The bottom of the plot is the shortest flight, and this is where the axis
/// flip breaks.
///
/// The image goes in rows from the top down, while `tof` on the plot grows
/// upwards. Forgetting the flip is easy, and it looks like a perfectly
/// plausible plot in which the cursor merely answers mirrored.
#[test]
fn the_bottom_of_the_plot_is_the_shortest_flight() {
    let grid = handmade();

    assert_eq!(
        cell_at(&grid, 0.01, 0.01),
        Some((0, 0)),
        "the bottom left corner"
    );
    assert_eq!(cell_at(&grid, 0.99, 0.99), Some((3, 2)), "the top right");
    assert_eq!(
        cell_at(&grid, 0.01, 0.99),
        Some((0, 2)),
        "the top left: first departure, longest transfer"
    );

    // Outside the plot there is no cell -- otherwise a miss past the edge
    // would read as picking the outermost one.
    assert_eq!(cell_at(&grid, -0.1, 0.5), None);
    assert_eq!(cell_at(&grid, 0.5, 1.2), None);
}

/// The numbers under the cursor are that cell's numbers, not a neighbour's.
///
/// The panel is drawn without a window: `RawInput` with a mouse position, and
/// the result is looked for among the drawn text. Pixels have nothing to do
/// with it -- a panel full of NaN looks exactly like a panel with the right
/// numbers.
#[test]
fn the_readout_shows_the_cell_under_the_cursor() {
    let grid = handmade();
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 500.0));
    let mut state = hud::PlotState::default();

    let mut draw = |events: Vec<egui::Event>| -> Vec<String> {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let mut output = context.run_ui(input, |ui| {
            hud::porkchop_panel(ui, Language::English, Some(&grid), &mut state);
        });
        output.textures_delta.clear();
        output
            .shapes
            .iter()
            .flat_map(|clipped| texts(&clipped.shape))
            .collect()
    };

    // The warm-up frame: before the first draw the plot has neither place nor
    // size.
    draw(Vec::new());

    let rect = context
        .read_response(egui::Id::new(hud::PLOT_IMAGE))
        .expect("the plot must have been drawn")
        .rect;

    // Point at cell (2, 0): the third departure, the shortest transfer.
    let at = egui::pos2(
        rect.min.x + rect.width() * (2.5 / 4.0),
        rect.max.y - rect.height() * (0.5 / 3.0),
    );
    let said = draw(vec![egui::Event::PointerMoved(at)]);
    let all = said.join(" | ");

    let cell = grid.at(2, 0).expect("cell (2, 0) is not a hole");
    assert!(
        all.contains(&format!("{:.0} / {:.0}", cell.dv_m_s, cell.v_inf_arrive)),
        "the numbers of cell (2, 0) are not among what was drawn: {all}"
    );
    assert!(
        all.contains("1.00 days"),
        "the transfer of cell (2, 0) is one day, but the panel says: {all}"
    );

    // And now the hole -- it must name itself rather than stay silent.
    let hole = egui::pos2(
        rect.min.x + rect.width() * (3.5 / 4.0),
        rect.max.y - rect.height() * (2.5 / 3.0),
    );
    let said = draw(vec![egui::Event::PointerMoved(hole)]).join(" | ");
    assert!(
        said.contains(game::text::tr(
            Language::English,
            game::text::Key::NoSolution
        )),
        "the forbidden zone said nothing about itself: {said}"
    );
}

/// A click on the plot chooses a window -- the one being looked at.
#[test]
fn a_click_chooses_the_window_under_the_pointer() {
    let grid = handmade();
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 500.0));
    let mut state = hud::PlotState::default();

    let draw = |state: &mut hud::PlotState, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let mut actions = Vec::new();
        let mut output = context.run_ui(input, |ui| {
            actions = hud::porkchop_panel(ui, Language::English, Some(&grid), state);
        });
        output.textures_delta.clear();
        actions
    };

    assert_eq!(
        draw(&mut state, Vec::new()),
        Vec::new(),
        "the plot does not click by itself"
    );

    let rect = context
        .read_response(egui::Id::new(hud::PLOT_IMAGE))
        .expect("the plot must have been drawn")
        .rect;
    let at = egui::pos2(
        rect.min.x + rect.width() * (1.5 / 4.0),
        rect.max.y - rect.height() * (1.5 / 3.0),
    );

    let actions = draw(
        &mut state,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            },
        ],
    );

    assert_eq!(actions, vec![hud::PorkchopAction::Choose(1, 1)]);
    assert_eq!(state.chosen, Some((1, 1)));
}

/// The button asks for a grid -- and only that, with no window chosen.
#[test]
fn the_button_asks_for_a_grid_and_nothing_else() {
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 500.0));
    let mut state = hud::PlotState::default();

    let mut draw = |events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let mut actions = Vec::new();
        let mut output = context.run_ui(input, |ui| {
            actions = hud::porkchop_panel(ui, Language::English, None, &mut state);
        });
        output.textures_delta.clear();
        actions
    };

    draw(Vec::new());
    let centre = context
        .read_response(egui::Id::new(hud::PLOT_COMPUTE))
        .expect("the button must have been drawn")
        .rect
        .center();

    let actions = draw(vec![
        egui::Event::PointerMoved(centre),
        egui::Event::PointerButton {
            pos: centre,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        },
        egui::Event::PointerButton {
            pos: centre,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        },
    ]);

    assert_eq!(actions, vec![hud::PorkchopAction::Compute]);
}

/// All the text of a shape, as a flat list of strings.
fn texts(shape: &egui::epaint::Shape) -> Vec<String> {
    match shape {
        egui::epaint::Shape::Text(text) => vec![text.galley.text().to_string()],
        egui::epaint::Shape::Vec(shapes) => shapes.iter().flat_map(texts).collect(),
        _ => Vec::new(),
    }
}
