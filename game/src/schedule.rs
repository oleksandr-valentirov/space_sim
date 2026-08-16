//! Event markers for the eye (ROADMAP-UI.md, U3a).
//!
//! ## Why by scanning rather than by an armed event
//!
//! This decision is already taken, and it is about physics rather than
//! convenience: **an armed event stops the run and changes the step sequence
//! after it** (ROADMAP, "Фізика й пропагація"). So a marker added for the sake
//! of the screen would change the trajectory -- and two players, one of whom
//! opened the schedule, would fly different paths. Today the game arms no
//! events, and stage U does not change that.
//!
//! ## Why minimum distance rather than the sign of `r.v`
//!
//! The two definitions are the same thing, but their discrete forms differ.
//! `r.v` needs velocity **relative to the body**, while a sample carries only
//! the body's position: the velocity would have to be taken by finite
//! difference and the sign of a near-zero quantity compared. Comparing three
//! adjacent distances has no such problem, and the "look the other way"
//! mutation gives exactly apoapses instead of periapses -- which is what the
//! step's check names.
//!
//! ## Cached per leg
//!
//! A leg is immutable from the moment it was computed (PROJECT.md §6), so the
//! list of its events is immutable too. Hence [`scan_leg`] takes a leg rather
//! than a trajectory: whoever caches, caches per leg.

use crate::leg::Leg;

/// What exactly was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The point closest to the body.
    Periapsis,
    /// The furthest.
    Apoapsis,
}

/// A found event: when, and at what distance from the body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marker {
    pub kind: Kind,
    pub t: f64,
    /// Distance from the body centre at that moment, metres.
    pub distance_m: f64,
}

/// Finds periapses and apoapses relative to Earth among already computed
/// samples.
///
/// The time is refined by a parabola through three adjacent distances -- not
/// for a prettier number but because the samples are accepted integrator steps
/// rather than a grid: they crowd near periapsis but do not land on it.
pub fn scan_leg(leg: &Leg) -> Vec<Marker> {
    let mut markers = Vec::new();
    if leg.samples.len() < 3 {
        return markers;
    }

    let distance = |i: usize| -> f64 {
        let s = &leg.samples[i];
        let dx = s.state.r.x - s.earth[0];
        let dy = s.state.r.y - s.earth[1];
        let dz = s.state.r.z - s.earth[2];
        (dx * dx + dy * dy + dz * dz).sqrt()
    };

    for i in 1..leg.samples.len() - 1 {
        let (before, here, after) = (distance(i - 1), distance(i), distance(i + 1));

        let kind = if here < before && here < after {
            Kind::Periapsis
        } else if here > before && here > after {
            Kind::Apoapsis
        } else {
            continue;
        };

        let (t, distance_m) = refine(
            (leg.samples[i - 1].state.t, before),
            (leg.samples[i].state.t, here),
            (leg.samples[i + 1].state.t, after),
        );

        markers.push(Marker {
            kind,
            t,
            distance_m,
        });
    }

    markers
}

/// The vertex of a parabola through three points. Returns `(t, value)`.
///
/// If the points lie on a line (the denominator is zero), the middle one is
/// returned -- there is no extremum there, and nothing to invent one from.
fn refine(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> (f64, f64) {
    let (t0, d0) = a;
    let (t1, d1) = b;
    let (t2, d2) = c;

    let h0 = t1 - t0;
    let h1 = t2 - t1;
    if h0 == 0.0 || h1 == 0.0 {
        return b;
    }

    // Derivatives by finite differences on a non-uniform grid.
    let left = (d1 - d0) / h0;
    let right = (d2 - d1) / h1;
    let curvature = (right - left) / (0.5 * (h0 + h1));
    if curvature == 0.0 {
        return b;
    }

    // Vertex: t1 - f'(t1)/f''(t1), where f'(t1) is the central difference.
    let slope = (right * h0 + left * h1) / (h0 + h1);
    let dt = -slope / curvature;

    // The vertex must not leave the neighbours' interval; if it did, three
    // points are not enough, and keeping the middle is more honest than
    // extrapolating.
    if dt < -h0 || dt > h1 {
        return b;
    }

    (t1 + dt, d1 + 0.5 * slope * dt)
}

/// All the trajectory's markers, earliest first.
///
/// The legs are already ordered in time, so there is nothing to sort -- but
/// the boundary between legs misses its own extremum: it has no three
/// neighbours on one side. That is no loss: the next leg starts at the same
/// place, and if there was a periapsis there it shows as the first or last
/// sample.
pub fn scan(legs: &[std::sync::Arc<Leg>]) -> Vec<Marker> {
    legs.iter().flat_map(|leg| scan_leg(leg)).collect()
}
