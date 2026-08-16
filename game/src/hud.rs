//! The panels that display (ROADMAP-UI.md, U2).
//!
//! ## The two boundaries held here
//!
//! **A panel computes nothing ahead and remembers nothing** (rule 1):
//! everything it shows is derived from the snapshot in that same frame. So the
//! functions here take a snapshot and return **commands** rather than sending
//! them: whoever sends knows about the channel, while a panel need know only
//! what was drawn.
//!
//! **A panel does not call the ephemeris and does not propagate** (rule 5).
//! The calendar date is the only computation here, and it comes from
//! arithmetic rather than from the asset.
//!
//! The consequence for checking: a panel is drawn in a test without a window,
//! and "clicking pause puts exactly `TogglePause` and **nothing else**" is a
//! comparison of the returned vector rather than an observation of the
//! game.

use engine::egui;

use crate::clock::Stall;
use crate::frame_view::ViewFrame;
use crate::mission;
use crate::plan::{Frame, Manoeuvre, Plan};
use crate::porkchop::{cell_at, colour, Grid};
use crate::schedule::{Kind, Marker};
use crate::sim::Command;
use crate::snapshot::{VesselSnapshot, WorldSnapshot};
use crate::text::{tr, Key, Language};
use crate::world::{EARTH, MOON};

/// Seconds in a day. Here a display unit rather than a physical constant.
const DAY_S: f64 = 86400.0;

/// Offset from the asset's scale (TT) to UT1, seconds (ROADMAP K3b).
///
/// A constant, and that is recorded honestly: the real TT-UT1 is
/// unpredictable, because it depends on Earth's rotation, which nobody
/// guarantees in advance. So the date on screen is accurate to a second or two
/// per century -- which is exactly why it is a property of the UI rather than
/// of the physics (this number never travels back the other way).
const TT_MINUS_UT1_S: f64 = 63.8286;

/// The time panel: mission day, date, warp, reason for stalling, buttons.
///
/// Returns commands in the order they were pressed. An empty vector is a
/// normal result: the player is simply looking.
pub fn time_panel(ui: &mut egui::Ui, language: Language, snapshot: &WorldSnapshot) -> Vec<Command> {
    let mut commands = Vec::new();

    let day = (snapshot.t - mission::start().t) / DAY_S;
    // Pause is read from `stall` rather than from warp: `Clock::warp()`
    // returns the **set** multiplier and does not zero on pause -- otherwise
    // pressing "resume" would mean remembering where you stopped.
    let paused = snapshot.stall == Some(Stall::Paused);

    ui.heading(tr(language, Key::Time));
    ui.label(format!(
        "{} {day:.2} / {:.2}",
        tr(language, Key::Day),
        mission::DAYS
    ));
    ui.label(calendar(snapshot.t));
    ui.label(format!("{} ×{:.0}", tr(language, Key::Warp), snapshot.warp));

    // The reason for stalling, in words. Silently easing off looks like a
    // broken game rather than "the prediction is still computing".
    if let Some(stall) = snapshot.stall {
        ui.label(tr(
            language,
            match stall {
                Stall::Paused => Key::StalledPaused,
                Stall::Horizon => Key::StalledHorizon,
                Stall::MissionEnd => Key::StalledMissionEnd,
            },
        ));
    }

    ui.horizontal(|ui| {
        let pause = if paused { Key::Resume } else { Key::Pause };
        if button(ui, PAUSE, tr(language, pause)) {
            commands.push(Command::TogglePause);
        }
        if button(ui, SLOWER, tr(language, Key::Slower)) {
            commands.push(Command::ScaleWarp(0.5));
        }
        if button(ui, FASTER, tr(language, Key::Faster)) {
            commands.push(Command::ScaleWarp(2.0));
        }
    });

    commands
}

/// Stable ids for the time panel's buttons.
///
/// Needed by the checks rather than by the game: without them a test would
/// hunt for a button by guessed pixels, and from the first change of spacing
/// would start clicking into emptiness while staying green. A label will not
/// do for this -- it changes with the language.
pub const PAUSE: &str = "hud.time.pause";
pub const SLOWER: &str = "hud.time.slower";
pub const FASTER: &str = "hud.time.faster";

/// A button with a stable id.
///
/// egui gives widgets automatic `Id`s that cannot be reproduced from outside,
/// so a second interaction is registered over the drawn button -- with our own
/// name and the same rectangle. It is what decides whether a click happened:
/// registered later, i.e. lying on top.
fn button(ui: &mut egui::Ui, id: &str, label: &str) -> bool {
    let drawn = ui.button(label);
    ui.interact(drawn.rect, egui::Id::new(id), egui::Sense::click())
        .clicked()
}

/// The schedule panel: event markers, clicking a row seeks there
/// (ROADMAP-UI.md, U3b).
///
/// Only events **ahead of the cursor** are shown: the past has been flown and
/// the cursor does not go backwards (stage J), so a "periapsis yesterday" row
/// would be a button that always refuses.
pub fn schedule_panel(
    ui: &mut egui::Ui,
    language: Language,
    now: f64,
    markers: &[Marker],
) -> Vec<Command> {
    let mut commands = Vec::new();

    ui.heading(tr(language, Key::Schedule));

    let mut shown = 0;
    for (index, marker) in markers.iter().enumerate() {
        if marker.t <= now {
            continue;
        }

        let name = tr(
            language,
            match marker.kind {
                Kind::Periapsis => Key::Periapsis,
                Kind::Apoapsis => Key::Apoapsis,
            },
        );
        let label = format!(
            "{name}: +{:.2} days, {:.0} km",
            (marker.t - now) / DAY_S,
            marker.distance_m / 1000.0
        );

        // A row's id is its ordinal in the marker list rather than its time:
        // time is an `f64`, and as part of an `Id` it would turn the smallest
        // refinement of the interpolation into a different widget.
        if button(ui, &format!("{SEEK}{index}"), &label) {
            commands.push(Command::SeekTo(marker.t));
        }

        shown += 1;
        if shown >= MAX_ROWS {
            break;
        }
    }

    if shown == 0 {
        ui.label(tr(language, Key::NoEvents));
    }

    commands
}

/// Prefix for the schedule rows' ids.
pub const SEEK: &str = "hud.schedule.seek.";

/// How many events to show. The schedule is "where to seek next" rather than a
/// log: the first few answer that question, the rest only take screen.
const MAX_ROWS: usize = 6;

/// The plan draft the player edits.
///
/// This is the rare UI-owned state rule 1 permits: **a plan being edited but
/// not yet submitted does not exist outside the screen**. As soon as the
/// player asks for a preview or a commit, the draft becomes a `Plan` and goes
/// into the thread -- and from that moment the truth is in the snapshot
/// again.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlanDraft {
    pub manoeuvres: Vec<Manoeuvre>,
}

impl PlanDraft {
    /// A draft from the plan the vessel is currently flying.
    pub fn from_plan(plan: &Plan) -> PlanDraft {
        PlanDraft {
            manoeuvres: plan.manoeuvres().to_vec(),
        }
    }

    /// The plan in the form the world will accept.
    ///
    /// `Plan::insert` keeps time order itself, so the draft need not preserve
    /// it: the player may move a manoeuvre into the past relative to its
    /// neighbour, and that must not become an editing error.
    pub fn plan(&self) -> Plan {
        let mut plan = Plan::new();
        for manoeuvre in &self.manoeuvres {
            plan.insert(*manoeuvre);
        }
        plan
    }
}

/// What the plan panel asks to be done.
///
/// Both variants carry **the same** plan shown on screen, and that is the
/// whole point of the step: the line you saw is the line you will fly (J5).
#[derive(Clone, Debug, PartialEq)]
pub enum PlanAction {
    /// Show what comes out. Goes to the planner, writes nothing into the
    /// world.
    Preview(Plan),
    /// Fly this. Goes to the simulation thread.
    Commit(Plan),
}

/// Ids for the plan's widgets. The same reasoning as `SEEK`: a test must find
/// a widget by name rather than by pixel.
pub const PLAN_ADD: &str = "hud.plan.add";
pub const PLAN_COMMIT: &str = "hud.plan.commit";
pub const PLAN_DELETE: &str = "hud.plan.delete.";

/// The plan panel: manoeuvres as rows, time and three dv components in VNB.
///
/// `draft` is UI-owned state (see [`PlanDraft`]); `notice` is what the world
/// answered to the previous attempt, and the panel shows exactly that rather
/// than its own assumption of success (rule 8).
pub fn plan_panel(
    ui: &mut egui::Ui,
    language: Language,
    now: f64,
    body: i32,
    draft: &mut PlanDraft,
    notice: Option<&str>,
) -> Vec<PlanAction> {
    let mut actions = Vec::new();
    let mut changed = false;

    ui.heading(tr(language, Key::Plan));

    let mut delete = None;
    for (index, manoeuvre) in draft.manoeuvres.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            // Time in days from "now" -- what the player thinks in. Into the
            // plan it goes absolute, as `Manoeuvre::t` requires.
            let mut days = (manoeuvre.t - now) / DAY_S;
            let response = ui.add(
                egui::DragValue::new(&mut days)
                    .speed(0.01)
                    .range(-3650.0..=3650.0),
            );
            if response.changed() {
                manoeuvre.t = now + days * DAY_S;
                changed = true;
            }

            for axis in 0..3 {
                let mut value = manoeuvre.dv[axis];
                let response = ui.add(egui::DragValue::new(&mut value).speed(0.1));
                if response.changed() {
                    manoeuvre.dv[axis] = value;
                    changed = true;
                }
            }

            if button(ui, &format!("{PLAN_DELETE}{index}"), "×") {
                delete = Some(index);
            }
        });
    }

    if let Some(index) = delete {
        draft.manoeuvres.remove(index);
        changed = true;
    }

    ui.horizontal(|ui| {
        if button(ui, PLAN_ADD, tr(language, Key::AddBurn)) {
            draft.manoeuvres.push(Manoeuvre {
                // A day ahead rather than "now": the world rejects a manoeuvre
                // at the current instant, because the cursor is already
                // passing it.
                t: now + DAY_S,
                dv: [0.0; 3],
                frame: Frame::Vnb { body },
            });
            changed = true;
        }
        if button(ui, PLAN_COMMIT, tr(language, Key::Commit)) {
            actions.push(PlanAction::Commit(draft.plan()));
        }
    });

    if let Some(text) = notice {
        ui.label(text);
    }

    // The preview comes after the commit in the action list, because this
    // frame's change may have been exactly the one the commit takes away.
    if changed {
        actions.push(PlanAction::Preview(draft.plan()));
    }

    actions
}

/// The window plot's state, which exists only on screen (U5c).
///
/// The same exception to rule 1 as [`PlanDraft`]: the texture is the grid
/// translated into pixels, and the selected window is "what I am looking at",
/// and outside the screen neither exists. No numbers are remembered
/// meanwhile: everything shown is derived from `Grid` each time.
#[derive(Default)]
pub struct PlotState {
    /// The texture and the number of the grid it was made from.
    ///
    /// The number is not for tidiness but to avoid rebuilding the image every
    /// frame: a 100x100 grid is 1e4 pixels, and it does not change at all
    /// until the next one arrives.
    texture: Option<(u64, egui::TextureHandle)>,
    /// The selected window as axis indices rather than a time: whoever asked
    /// for the grid sets the axes, and keeping a second copy of their values
    /// would let them diverge.
    pub chosen: Option<(usize, usize)>,
}

/// What the plot asks to be done.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PorkchopAction {
    /// Compute the grid -- a button. Whoever sends the request chooses the
    /// axes.
    Compute,
    /// The player selected a window: indices on the grid's axes.
    Choose(usize, usize),
}

/// Ids for the plot's widgets.
pub const PLOT_COMPUTE: &str = "hud.porkchop.compute";
pub const PLOT_IMAGE: &str = "hud.porkchop.image";

/// How many screen pixels to give the plot. A square: the axes differ in
/// meaning but are equal in importance, and a stretched plot reads as "one of
/// them is more precise".
const PLOT_SIDE: f32 = 200.0;

/// The numbers of the selected (or hovered) window -- what the cursor shows.
///
/// Separate from drawing for the same reason as [`VesselReadout`]: the step's
/// oracle is "the number in the panel equals the number in the grid", and
/// pixels cannot be compared with numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowReadout {
    /// The departure instant, absolute asset time.
    pub t1: f64,
    /// The flight time, seconds.
    pub tof: f64,
    /// The cell; `None` is a forbidden zone, and that is exactly how it must
    /// be shown.
    pub cell: Option<crate::porkchop::Cell>,
}

/// A window's numbers by axis indices. Computes nothing beyond the lookup.
pub fn read_window(grid: &Grid, i_t1: usize, i_tof: usize) -> Option<WindowReadout> {
    Some(WindowReadout {
        t1: *grid.t1.get(i_t1)?,
        tof: *grid.tof.get(i_tof)?,
        cell: grid.at(i_t1, i_tof),
    })
}

/// The plot panel: the grid as an image, axes in dates, a cursor with numbers.
///
/// `grid` is what the planner thread computed (rule 6); `None` means "not
/// asked yet" or "still computing", and in that case the panel shows the
/// button and invents nothing.
pub fn porkchop_panel(
    ui: &mut egui::Ui,
    language: Language,
    grid: Option<&Grid>,
    state: &mut PlotState,
) -> Vec<PorkchopAction> {
    let mut actions = Vec::new();

    ui.heading(tr(language, Key::Porkchop));

    if button(ui, PLOT_COMPUTE, tr(language, Key::ComputeWindows)) {
        actions.push(PorkchopAction::Compute);
    }

    let Some(grid) = grid else {
        ui.label(tr(language, Key::NoGrid));
        return actions;
    };

    // The texture is built once per grid rather than per frame.
    let texture = match &state.texture {
        Some((id, texture)) if *id == grid.id => texture.clone(),
        _ => {
            let texture = ui.ctx().load_texture(
                "porkchop",
                image_of(grid),
                // No filtering: a pixel is a cell, and a blurred boundary
                // between a cell and a hole is an invented intermediate
                // state.
                egui::TextureOptions::NEAREST,
            );
            state.texture = Some((grid.id, texture.clone()));
            texture
        }
    };

    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(PLOT_SIDE, PLOT_SIDE),
        egui::Sense::click_and_drag(),
    );
    ui.painter().image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );

    // A second interaction with our own name -- the same reason as in
    // `button`: a test must find the plot by name rather than by a guessed
    // pixel.
    let named = ui.interact(rect, egui::Id::new(PLOT_IMAGE), egui::Sense::click());

    // The cheapest window is marked with a cross: the plot exists to find it,
    // and hunting the minimum by eye along a gradient is work the machine has
    // already done.
    if let Some((i, j, _)) = grid.best() {
        let at = cell_centre(rect, grid, i, j);
        let arm = 4.0;
        let stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
        ui.painter().line_segment(
            [
                egui::pos2(at.x - arm, at.y - arm),
                egui::pos2(at.x + arm, at.y + arm),
            ],
            stroke,
        );
        ui.painter().line_segment(
            [
                egui::pos2(at.x - arm, at.y + arm),
                egui::pos2(at.x + arm, at.y - arm),
            ],
            stroke,
        );
    }

    // The axes: dates at the edges instead of labels on every tick. A plot 200
    // pixels wide will not take more, and two ends already say what interval
    // this is.
    let (first, last) = (grid.t1[0], grid.t1[grid.t1.len() - 1]);
    ui.label(format!(
        "{} — {}",
        &calendar(first)[..10],
        &calendar(last)[..10]
    ));
    ui.label(format!(
        "{}: {:.1} — {:.1} {}",
        tr(language, Key::FlightTime),
        grid.tof[0] / DAY_S,
        grid.tof[grid.tof.len() - 1] / DAY_S,
        tr(language, Key::Days)
    ));

    // Under the cursor, what is being looked at; without a cursor, what was
    // selected.
    let under_pointer = response
        .hover_pos()
        .and_then(|at| cell_at(grid, from_left(rect, at), from_bottom(rect, at)));
    let shown = under_pointer.or(state.chosen);

    match shown.and_then(|(i, j)| read_window(grid, i, j)) {
        Some(readout) => {
            ui.label(format!(
                "{} {}",
                tr(language, Key::Depart),
                calendar(readout.t1)
            ));
            ui.label(format!(
                "{}: {:.2} {}",
                tr(language, Key::FlightTime),
                readout.tof / DAY_S,
                tr(language, Key::Days)
            ));
            match readout.cell {
                Some(cell) => {
                    // Two numbers separated by a slash: the departure
                    // manoeuvre -- the one that goes into the plan -- and the
                    // speed relative to the body on arrival, i.e. the price of
                    // the braking not yet computed.
                    ui.label(format!(
                        "{}: {:.0} / {:.0} m/s",
                        tr(language, Key::Vinf),
                        cell.dv_m_s,
                        cell.v_inf_arrive
                    ));
                }
                // A hole is called a hole. An empty line here would read as
                // "free".
                None => {
                    ui.label(tr(language, Key::NoSolution));
                }
            }
        }
        None => {
            ui.label(tr(language, Key::PickWindow));
        }
    }

    if named.clicked() {
        if let Some((i, j)) = ui
            .ctx()
            .pointer_interact_pos()
            .and_then(|at| cell_at(grid, from_left(rect, at), from_bottom(rect, at)))
        {
            state.chosen = Some((i, j));
            actions.push(PorkchopAction::Choose(i, j));
        }
    }

    actions
}

fn from_left(rect: egui::Rect, at: egui::Pos2) -> f32 {
    (at.x - rect.min.x) / rect.width()
}

fn from_bottom(rect: egui::Rect, at: egui::Pos2) -> f32 {
    (rect.max.y - at.y) / rect.height()
}

/// A cell's centre in pixels -- for the mark on the plot.
fn cell_centre(rect: egui::Rect, grid: &Grid, i_t1: usize, i_tof: usize) -> egui::Pos2 {
    let x = (i_t1 as f32 + 0.5) / grid.t1.len() as f32;
    let y = (i_tof as f32 + 0.5) / grid.tof.len() as f32;
    egui::pos2(
        rect.min.x + x * rect.width(),
        rect.max.y - y * rect.height(),
    )
}

/// The grid into an image: a pixel is a cell, row 0 is the longest flight.
///
/// The flip happens here, in one place: beyond it only [`from_bottom`] knows
/// about it, and both obey one convention -- `tof` grows upwards.
fn image_of(grid: &Grid) -> egui::ColorImage {
    let (low, high) = grid.scale().unwrap_or((0.0, 1.0));
    let (w, h) = (grid.t1.len(), grid.tof.len());

    let mut pixels = vec![egui::Color32::TRANSPARENT; w * h];
    for i in 0..w {
        for j in 0..h {
            let [r, g, b, a] = colour(grid.at(i, j), low, high);
            pixels[(h - 1 - j) * w + i] = egui::Color32::from_rgba_unmultiplied(r, g, b, a);
        }
    }

    egui::ColorImage::new([w, h], pixels)
}

/// The vessel panel's numbers, taken from the snapshot (U2c).
///
/// Deliberately separate from drawing: the step's oracle is "the value in the
/// panel equals one computed independently from the snapshot", and numbers
/// cannot be compared with pixels. Every field is read by [`vessel_panel`], so
/// there is no struct here that nobody reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VesselReadout {
    /// Altitude above the body's **surface**: distance from the centre minus
    /// the asset's mean radius (`eph_body_radius`, U2a). Not above the drawn
    /// sphere and not above the harmonics reference radius -- for the Moon
    /// those are different numbers, differing by 470 m (K5e).
    pub altitude_m: f64,
    /// Speed **relative to the body**, not barycentric.
    pub speed_m_s: f64,
    /// How long remains to the plan's next manoeuvre; `None` means there are
    /// no manoeuvres ahead.
    pub next_burn_s: Option<f64>,
    /// The plan's total dv -- **the sum of norms** rather than the norm of the
    /// sum: two manoeuvres in opposite directions both cost fuel.
    pub total_dv_m_s: f64,
    /// How far the prediction runs ahead of the cursor.
    pub computed_ahead_s: f64,
    /// The vessel stopped with an error.
    pub failed: bool,
}

/// Takes the vessel's numbers from the snapshot.
///
/// Does not call the ephemeris (rule 5): the body's position comes from the
/// nearest sample, which already carries it (`leg::Sample::earth` is there for
/// exactly this). The radius arrives as an argument, because it is a property
/// of the body rather than of the frame: it is read once at startup.
pub fn read_vessel(
    snapshot: &WorldSnapshot,
    vessel: &VesselSnapshot,
    body_radius_m: f64,
) -> VesselReadout {
    let body = body_near(vessel, snapshot.t);

    let dr = [
        vessel.state.r.x - body.0[0],
        vessel.state.r.y - body.0[1],
        vessel.state.r.z - body.0[2],
    ];
    let distance = (dr[0] * dr[0] + dr[1] * dr[1] + dr[2] * dr[2]).sqrt();

    let dv = [
        vessel.state.v.x - body.1[0],
        vessel.state.v.y - body.1[1],
        vessel.state.v.z - body.1[2],
    ];
    let speed = (dv[0] * dv[0] + dv[1] * dv[1] + dv[2] * dv[2]).sqrt();

    let next_burn_s = vessel
        .plan
        .manoeuvres()
        .iter()
        .find(|m| m.t > snapshot.t)
        .map(|m| m.t - snapshot.t);

    let total_dv_m_s = vessel
        .plan
        .manoeuvres()
        .iter()
        .map(|m| (m.dv[0] * m.dv[0] + m.dv[1] * m.dv[1] + m.dv[2] * m.dv[2]).sqrt())
        .sum();

    VesselReadout {
        altitude_m: distance - body_radius_m,
        speed_m_s: speed,
        next_burn_s,
        total_dv_m_s,
        computed_ahead_s: vessel.computed_to - snapshot.t,
        failed: vessel.failed.is_some(),
    }
}

/// The vessel panel. Sends nothing -- U2 only displays.
pub fn vessel_panel(ui: &mut egui::Ui, language: Language, name: &str, readout: &VesselReadout) {
    ui.heading(tr(language, Key::Vessel));
    ui.label(name);
    ui.label(format!(
        "{}: {:.1} km",
        tr(language, Key::Altitude),
        readout.altitude_m / 1000.0
    ));
    ui.label(format!(
        "{}: {:.1} m/s",
        tr(language, Key::Speed),
        readout.speed_m_s
    ));
    ui.label(match readout.next_burn_s {
        Some(seconds) => format!(
            "{}: {:.2} days",
            tr(language, Key::NextBurn),
            seconds / DAY_S
        ),
        None => tr(language, Key::NoBurns).to_string(),
    });
    ui.label(format!(
        "{}: {:.2} m/s",
        tr(language, Key::TotalDv),
        readout.total_dv_m_s
    ));
    ui.label(format!(
        "{}: {:.2} days",
        tr(language, Key::ComputedAhead),
        readout.computed_ahead_s / DAY_S
    ));
    if readout.failed {
        ui.label(tr(language, Key::Failed));
    }
}

/// Earth's position and velocity in the sample nearest to `t`.
///
/// A sample carries the body's position for exactly this (`crate::leg`), so no
/// ephemeris is needed in the frame. A sample does not carry velocity, so that
/// is taken as a finite difference between two adjacent samples -- of the same
/// order of accuracy as the line on screen itself.
fn body_near(vessel: &VesselSnapshot, t: f64) -> ([f64; 3], [f64; 3]) {
    let mut best: Option<(f64, [f64; 3], [f64; 3])> = None;

    for leg in &vessel.legs {
        for (i, sample) in leg.samples.iter().enumerate() {
            let gap = (sample.state.t - t).abs();
            if best.is_some_and(|(was, _, _)| gap >= was) {
                continue;
            }

            // A neighbour for the difference: the next one if there is one,
            // otherwise the previous.
            let velocity = match leg.samples.get(i + 1).or_else(|| {
                if i > 0 {
                    leg.samples.get(i - 1)
                } else {
                    None
                }
            }) {
                Some(other) => {
                    let dt = other.state.t - sample.state.t;
                    if dt == 0.0 {
                        [0.0; 3]
                    } else {
                        [
                            (other.earth[0] - sample.earth[0]) / dt,
                            (other.earth[1] - sample.earth[1]) / dt,
                            (other.earth[2] - sample.earth[2]) / dt,
                        ]
                    }
                }
                None => [0.0; 3],
            };

            best = Some((gap, sample.earth, velocity));
        }
    }

    best.map_or(([0.0; 3], [0.0; 3]), |(_, r, v)| (r, v))
}

/// A calendar date from seconds since the asset epoch (J2000 TDB), UTC-like.
///
/// TDB instead of TT changes nothing here: the difference between them is
/// periodic and does not exceed 1.7 ms, i.e. lies three orders below the
/// second this string ends with. Recorded so the next reader does not hunt for
/// an error where there is none.
///
/// The conversion lives in the game rather than in the physics: it calls
/// exactly what cannot exist in the integration loop, and never travels back
/// into it (ROADMAP-UI.md, U2b). The algorithm is the civil calendar from a
/// day number (Howard Hinnant, `civil_from_days`), integer and with no
/// trigonometry at all.
pub fn calendar(t: f64) -> String {
    // J2000 is 2000-01-01 12:00:00, i.e. noon. Half a day of offset turns it
    // into the midnight days are counted from.
    let seconds = t - TT_MINUS_UT1_S + 0.5 * DAY_S;
    let days = seconds.div_euclid(DAY_S);
    let rest = seconds.rem_euclid(DAY_S);

    // There are 10957 days from 1970-01-01 to 2000-01-01.
    let (year, month, day) = civil_from_days(days as i64 + 10957);

    let hour = (rest / 3600.0) as u32;
    let minute = ((rest % 3600.0) / 60.0) as u32;
    let second = (rest % 60.0) as u32;

    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
}

/// A day since 1970-01-01 to (year, month, day). Integer, no libraries.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The view panel: which frame the scene is shown in (U6a4).
///
/// Returns a **new frame** rather than a command, and that is no small thing:
/// a command would go into the world thread, while a frame changes no number
/// in the world -- it is view state, which rule 1 of stage U explicitly allows
/// the UI to hold. Hence the return differs from `time_panel`'s: that one
/// sends commands, this one hands back a choice.
///
/// `None` means "the player pressed nothing" rather than "inertial": the
/// difference is that the first overwrites nothing.
/// What the panel says about the zero-velocity curve (U6b4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurveReadout {
    /// The Jacobi constant the curve is built from.
    pub jacobi: f64,
    /// The vessel is far from the pair -- there `C` stops meaning anything.
    pub far_away: bool,
}

/// Reads the curve from the snapshot: the vessel's `C` and whether it is still
/// near the pair.
///
/// Derived from the snapshot, not accumulated (rule 1 of stage U), and calls
/// no ephemeris (rule 5): `C` is already computed in the world thread, and the
/// distance is a subtraction of two positions lying right here.
///
/// `far_away` is measured in **distances between the bodies**: anything beyond
/// two is no longer the pair's neighbourhood but what CR3BP was not written
/// for. Measured at U6b1: that is where `C` along the mission stops being
/// constant and its spread jumps from 0.08% to
/// 82%.
pub fn read_curve(snapshot: &WorldSnapshot) -> Option<CurveReadout> {
    let vessel = snapshot.vessels.first()?;
    let jacobi = vessel.jacobi?;

    let body = |index: i32| snapshot.bodies.iter().find(|b| b.body == index);
    let (earth, moon) = (body(EARTH)?, body(MOON)?);

    let d = [
        moon.position[0] - earth.position[0],
        moon.position[1] - earth.position[1],
        moon.position[2] - earth.position[2],
    ];
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let from_earth = [
        vessel.state.r.x - earth.position[0],
        vessel.state.r.y - earth.position[1],
        vessel.state.r.z - earth.position[2],
    ];
    let distance = (from_earth[0].powi(2) + from_earth[1].powi(2) + from_earth[2].powi(2)).sqrt();

    Some(CurveReadout {
        jacobi,
        far_away: l > 0.0 && distance > 2.0 * l,
    })
}

/// What the player chose in the view panel during this frame.
///
/// A struct rather than a pair of `Option`s: two `Option`s of the same size
/// swap places silently, and the compiler will not say so. Both fields are
/// read in `app::draw` -- otherwise they would not be here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewChoice {
    /// A new frame, if the toggle was pressed.
    pub frame: Option<ViewFrame>,
    /// A new language, if its toggle was pressed.
    pub language: Option<Language>,
}

pub fn view_panel(
    ui: &mut egui::Ui,
    language: Language,
    frame: ViewFrame,
    curve: Option<CurveReadout>,
) -> ViewChoice {
    let mut choice = ViewChoice::default();

    ui.heading(tr(language, Key::View));
    ui.label(format!(
        "{}: {}",
        tr(language, Key::Frame),
        tr(
            language,
            match frame {
                ViewFrame::Inertial => Key::FrameInertial,
                ViewFrame::Rotating => Key::FrameRotating,
            }
        )
    ));

    // One toggle button rather than two: there are exactly two frames, and a
    // pair of buttons would imply a "neither pressed" state that does not
    // exist.
    let next = match frame {
        ViewFrame::Inertial => ViewFrame::Rotating,
        ViewFrame::Rotating => ViewFrame::Inertial,
    };
    let label = tr(
        language,
        match next {
            ViewFrame::Inertial => Key::FrameInertial,
            ViewFrame::Rotating => Key::FrameRotating,
        },
    );
    if button(ui, FRAME, label) {
        choice.frame = Some(next);
    }

    // The curve exists only in the rotating frame, so its label does too.
    //
    // **The caveat is printed always, not on trouble.** A curve shown as a
    // wall the vessel then calmly flies through is worse than none: `C` is
    // conserved in CR3BP, and the game flies in the full ephemeris.
    if frame == ViewFrame::Rotating {
        if let Some(curve) = curve {
            ui.separator();
            ui.label(format!(
                "{} {:.4}",
                tr(language, Key::ZeroVelocity),
                curve.jacobi
            ));
            ui.label(tr(language, Key::CurveIsAdvice));
            if curve.far_away {
                ui.label(tr(language, Key::CurveFarAway));
            }
        }
    }

    // The language is a property of the view too, and so lives in this panel
    // rather than its own: the egui pass must stay single (U1b measured that
    // the pass is what pays, not the widgets). At the bottom, because it is
    // switched once and for all while the frame is switched constantly.
    //
    // The button is labelled with the name of the language it will **switch
    // to** rather than the current one. The same choice as the frame toggle
    // above, for the same reason: a button labelled with the current state
    // reads as "press to leave as is".
    ui.separator();
    ui.label(format!(
        "{}: {}",
        tr(language, Key::Language),
        tr(language, language.name_key())
    ));
    let next_language = language.next();
    if button(ui, LANGUAGE, tr(language, next_language.name_key())) {
        choice.language = Some(next_language);
    }

    choice
}

/// A stable id for the frame toggle -- for the same reason as [`PAUSE`].
pub const FRAME: &str = "hud.view.frame";

/// The same for the language toggle.
pub const LANGUAGE: &str = "hud.view.language";

#[cfg(test)]
mod tests {
    use super::*;

    /// The asset epoch is J2000, i.e. noon on the first of January 2000.
    ///
    /// The oracle here is the epoch's definition rather than another
    /// implementation of the same conversion: a second implementation would
    /// err along with the first if the offset's direction were confused.
    #[test]
    fn the_epoch_is_noon_on_the_first_of_january_2000() {
        let text = calendar(0.0);
        assert!(
            text.starts_with("2000-01-01 11:58:5"),
            "the epoch gave {text}, but should give noon minus 63.8 s (TT-UT1)"
        );
    }

    /// A day forward is the next day at the same minute.
    #[test]
    fn a_day_later_is_the_next_day() {
        assert!(calendar(DAY_S).starts_with("2000-01-02 11:58:5"));
    }

    /// And a leap year is walked through rather than around.
    ///
    /// 2000 is a leap year (divisible by 400), the case that breaks the naive
    /// "every four years except centuries".
    #[test]
    fn the_year_2000_has_a_twenty_ninth_of_february() {
        // 59 days from 1 January (noon) is 29 February.
        let text = calendar(59.0 * DAY_S);
        assert!(text.starts_with("2000-02-29"), "got {text}");
    }
}
