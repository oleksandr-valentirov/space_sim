//! The porkchop grid: choosing a transfer window (ROADMAP-UI.md, U5b).
//!
//! ## What flies from where to where
//!
//! **From the vessel to a body, in the central body's coordinates.** Not body
//! to body, and that is a measured decision rather than a taste.
//!
//! `porkchop_compute_eph` (U5a) does a body-to-body sweep in the coordinates
//! the asset sits in. For an interplanetary transfer that is what is wanted:
//! the asset's centre is 1.1e9 m from the Sun, i.e. against 1.5e11 m it is
//! practically it. For Earth-Moon it is not, and twice over:
//!
//! - **the centre is elsewhere.** The fixture is barycentric: Earth lies
//!   1.47e11 m from the origin and travels at 30 km/s. A Lambert arc is built
//!   about the origin, so with Earth's `mu` it came out as an arc around the
//!   Sun with Earth's mass -- a quantity that does not exist;
//! - **there is nowhere to depart from.** Taking Earth as the departure body
//!   means `r1 = 0`, i.e. degeneracy. The vessel meanwhile lies on a halo
//!   orbit 4.5e8 m from Earth, and the question the plot answers is "when
//!   should **I** burn and how long is the flight" rather than "when will
//!   Earth be conveniently placed".
//!
//! The first version's numbers looked plausible (2-9.6 km/s), which is exactly
//! why this is worth saying out loud: **the U5a oracle compared the boundary
//! against itself** -- the grid against `lambert_solve` -- so the same
//! coordinate-frame error stood on both sides and cancelled. An error in the
//! choice of frame is not caught by comparing two paths; only a third number
//! from outside catches it, and here that is the vessel's velocity relative to
//! Earth.
//!
//! So the sweep lives here rather than in C: `lambert_solve` is already on the
//! boundary and already has two independent oracles (D1), and the departure
//! states come from the trajectory rather than from the asset. U5a foresaw
//! exactly this fork: "if the wrapper starts accumulating parameters, stop".
//! The wrapper remains for body-to-body; the debt for it is recorded in
//! ROADMAP (D9).
//!
//! ## The cell that does not exist, and the cell that costs zero
//!
//! Forbidden zones are a property of the instrument rather than an error: a
//! porkchop plot exists precisely so they can be seen. So the grid is dense --
//! `t1.len() * tof.len()` cells -- and holds `None` where there is no
//! solution.
//!
//! The difference between `None` and `Some(0)` is not pedantry. Zero is the
//! cheapest possible transfer, i.e. on the plot it would look like the
//! **best** window; that is exactly where the player would click.
//!
//! ## Why by rows
//!
//! The thread's unit of work is a row, as a leg is for a prediction. Measured
//! on the fixture (the numbers are in `game/tests/porkchop.rs`): the grid does
//! not fit in a frame, so the planner thread computes it (rule 6 of stage U),
//! but the "seconds of black screen" the step's plan feared are not here
//! either -- a coarse grid refined around the minimum (the U5b fork) was not
//! needed.

use core_rs::{Ephemeris, State, Vec3d};

/// A grid cell: what a transfer starting at `t1` and lasting `tof` costs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    /// The departure manoeuvre itself, inertial coordinates, m/s.
    ///
    /// Not a "cost estimate" but the **same** impulse that will go into the
    /// plan: the Lambert arc here is built from the vessel's real state, so
    /// the difference between the arc's velocity and the vessel's is the
    /// manoeuvre. Hence clicking a cell computes nothing anew (U5d).
    pub dv: [f64; 3],
    /// The magnitude of that manoeuvre -- what the cursor shows and what the
    /// plot is coloured by.
    pub dv_m_s: f64,
    /// The speed relative to the destination body the vessel arrives with.
    ///
    /// This is the price of the **second** manoeuvre, not yet computed: the
    /// braking will cost about that much. Hence both numbers are shown
    /// separately -- a window cheap on departure and intolerable on arrival is
    /// not cheap.
    pub v_inf_arrive: f64,
}

impl Cell {
    /// The window's price as one number -- what the plot is coloured and sorted
    /// by.
    ///
    /// The sum of two manoeuvres rather than one of them: the same arithmetic
    /// as in `VesselReadout::total_dv_m_s`, and for the same reason -- both
    /// spend fuel.
    pub fn total(&self) -> f64 {
        self.dv_m_s + self.v_inf_arrive
    }
}

/// What to compute.
///
/// The axes arrive as ready lists rather than "from, to, how many": whoever
/// draws an axis decides how it is divided, and the grid need not guess that a
/// second time.
#[derive(Clone, Debug, PartialEq)]
pub struct GridRequest {
    pub id: u64,
    /// The vessel's state at each departure instant -- **from the snapshot**,
    /// not from the asset.
    ///
    /// The `t1` axis comes from here too: two lists would diverge, one cannot.
    /// The caller must take these states only from what is already computed:
    /// past the horizon `leg::state_at` returns the endpoint, and a grid built
    /// on it would show a transfer from a place the vessel will not be.
    pub depart: Vec<State>,
    /// The destination body.
    pub arrive_body: i32,
    /// The body the arc is built about. Its state is subtracted from both
    /// ends, because Lambert's problem lives in exactly that body's
    /// coordinates.
    pub centre_body: i32,
    /// The central body's `mu`, read from the asset once at startup
    /// (`Ephemeris::body_mu`, U5a) -- rule 5 forbids calling the ephemeris
    /// from a frame, and a body's mass does not change.
    pub mu: f64,
    pub prograde: bool,
    /// Flight times, ascending.
    pub tof: Vec<f64>,
}

impl GridRequest {
    /// Departure instants -- the `t1` axis.
    pub fn t1(&self) -> Vec<f64> {
        self.depart.iter().map(|s| s.t).collect()
    }
}

/// A computed grid. Not world state: the planner writes nothing into the
/// world.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    /// The number of the request this answers.
    pub id: u64,
    pub t1: Vec<f64>,
    pub tof: Vec<f64>,
    /// `t1.len() * tof.len()` cells; a row is one `t1`.
    pub cells: Vec<Option<Cell>>,
}

impl Grid {
    /// A cell by its axis indices.
    pub fn at(&self, i_t1: usize, i_tof: usize) -> Option<Cell> {
        if i_t1 >= self.t1.len() || i_tof >= self.tof.len() {
            return None;
        }
        self.cells[i_t1 * self.tof.len() + i_tof]
    }

    /// The cheapest window: the axis indices and the cell itself.
    ///
    /// `None` if nothing converged anywhere -- i.e. the whole plot is
    /// forbidden. That result must be shown too rather than hidden behind an
    /// empty screen.
    pub fn best(&self) -> Option<(usize, usize, Cell)> {
        let mut best: Option<(usize, usize, Cell)> = None;
        for (index, cell) in self.cells.iter().enumerate() {
            let Some(cell) = *cell else { continue };
            if best.is_none_or(|(_, _, was)| cell.total() < was.total()) {
                best = Some((index / self.tof.len(), index % self.tof.len(), cell));
            }
        }
        best
    }

    /// The colour scale's bounds -- **not** the real spread of prices.
    ///
    /// The top is clipped to [`SCALE_SPAN`] times the cheapest, and without
    /// that the plot would be one colour: measured on the fixture, from
    /// 349 m/s to 3.4e6 m/s, four decades. The expensive cells there are not
    /// "somewhat dearer" but unreachable altogether (a transfer across half
    /// the system in a day), and giving them the whole gradient would crush
    /// into one colour exactly the region where the choice is made.
    ///
    /// Paper porkchop plots do the same: contours are drawn as multiples of
    /// the minimum rather than from zero to maximum.
    pub fn scale(&self) -> Option<(f64, f64)> {
        let mut low = f64::INFINITY;
        let mut high: f64 = 0.0;
        for cell in self.cells.iter().flatten() {
            low = low.min(cell.total());
            high = high.max(cell.total());
        }
        if !low.is_finite() {
            return None;
        }
        Some((low, high.min(low * SCALE_SPAN)))
    }
}

/// How many times the cheapest window the colour scale stretches.
///
/// Four: a window four times dearer than the best is already a "no" rather
/// than "somewhat worse", so there is nothing to distinguish beyond that
/// bound. A number from the practice of drawing plots rather than from
/// physics, which is exactly why it stands here alone and visible.
pub const SCALE_SPAN: f64 = 4.0;

/// A cell's colour: RGBA, eight bits per channel.
///
/// A hole is **transparent**, and that is a checkable property rather than
/// styling: a forbidden zone must read as "there is nothing here" rather than
/// as a cell with some price. Any opaque colour for it, black included, would
/// lie on the same scale as the prices and the eye would compare it with them.
///
/// The rest is a linear scale from cheap to dear. Deliberately not a rainbow:
/// seven colours give seven false boundaries where the quantity changes
/// smoothly. What matters is that the scale is monotonic and that both its
/// ends differ from a hole.
///
/// The scale's ends moved into `palette` (U7c), and the expensive end is the
/// prediction's colour: amber in this game always means "what it costs",
/// whether on a plot or on the line ahead of the vessel.
///
/// `low == high` (the whole grid identical, or one cell) gives the cheap end:
/// there is no need to divide by zero, and "all the same" is not "all the
/// most expensive".
pub fn colour(cell: Option<Cell>, low: f64, high: f64) -> [u8; 4] {
    let Some(cell) = cell else {
        return [0, 0, 0, 0];
    };

    let span = high - low;
    let u = if span > 0.0 {
        ((cell.total() - low) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };

    // Cheap is cold and dark, dear is hot and bright.
    let cheap = crate::palette::CHEAP;
    let costly = crate::palette::COSTLY;

    let mix = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * u).round() as u8;
    [
        mix(cheap.0, costly.0),
        mix(cheap.1, costly.1),
        mix(cheap.2, costly.2),
        255,
    ]
}

/// Which cell lies under a point given as a fraction from the left edge and
/// from the **bottom**.
///
/// Fractions rather than pixels, and no egui type: that way the function is
/// checked by arithmetic and the widget stays thin. From the bottom, because
/// on the plot the `tof` axis grows upwards as on every porkchop plot, while
/// image rows run top to bottom; the place where that flip is forgotten looks
/// like a perfectly plausible plot with a mirrored answer.
///
/// `None` means the point is outside the plot. A hole cell is instead returned
/// like any other: the cursor must say "there is no solution here" rather than
/// stay silent.
pub fn cell_at(grid: &Grid, from_left: f32, from_bottom: f32) -> Option<(usize, usize)> {
    if grid.t1.is_empty() || grid.tof.is_empty() {
        return None;
    }
    if !(0.0..1.0).contains(&from_left) || !(0.0..1.0).contains(&from_bottom) {
        return None;
    }
    let i = (f64::from(from_left) * grid.t1.len() as f64) as usize;
    let j = (f64::from(from_bottom) * grid.tof.len() as f64) as usize;
    Some((i.min(grid.t1.len() - 1), j.min(grid.tof.len() - 1)))
}

/// One grid row: every `tof` for one departure instant.
///
/// A row rather than the whole grid, so the thread can look at the channel
/// between rows (see the module intro). A row of nothing but holes is a normal
/// result: that is what an instant the ephemeris has no states for looks
/// like.
pub fn row(eph: &Ephemeris, request: &GridRequest, i_t1: usize) -> Vec<Option<Cell>> {
    let mut dense = vec![None; request.tof.len()];
    let Some(&from) = request.depart.get(i_t1).as_ref() else {
        return dense;
    };

    // The centre once per row: both ends need it, but on departure the time is
    // the same for the whole row.
    let Ok(centre_at_t1) = eph.body_state(request.centre_body, from.t) else {
        return dense;
    };
    let r1 = sub(from.r, centre_at_t1.r);
    let v1 = sub(from.v, centre_at_t1.v);

    for (j, &tof) in request.tof.iter().enumerate() {
        // A non-positive duration is not a transfer; `lambert_solve` would
        // refuse itself, but a hole for that reason is not worth a call across
        // the boundary.
        if tof <= 0.0 {
            continue;
        }
        let t2 = from.t + tof;

        // Past the asset's edge there are no states -- the cell stays a
        // hole.
        let (Ok(centre), Ok(target)) = (
            eph.body_state(request.centre_body, t2),
            eph.body_state(request.arrive_body, t2),
        ) else {
            continue;
        };
        let r2 = sub(target.r, centre.r);
        let v2 = sub(target.v, centre.v);

        // Lambert did not converge -- also a hole, and also expected:
        // degenerate geometry and an unreachable duration are an ordinary part
        // of the plot.
        let Ok((depart, arrive)) =
            core_rs::lambert_solve(r1, r2, tof, request.mu, request.prograde, 0)
        else {
            continue;
        };

        let dv = sub(depart, v1);
        dense[j] = Some(Cell {
            dv: [dv.x, dv.y, dv.z],
            dv_m_s: length(dv),
            v_inf_arrive: length(sub(arrive, v2)),
        });
    }

    dense
}

fn sub(a: Vec3d, b: Vec3d) -> Vec3d {
    Vec3d {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

fn length(v: Vec3d) -> f64 {
    (v.x * v.x + v.y * v.y + v.z * v.z).sqrt()
}

// There is deliberately no "compute the whole grid in one go" function: the
// only caller that needs the grid is the planner thread, and it needs breaks
// between rows (`crate::planner::sweep`). A convenient wrapper call would mean
// a second route to the same answer that nobody exercises -- the same thing as
// a struct none of whose fields is read.
