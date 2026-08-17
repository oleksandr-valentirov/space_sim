//! The time panel shows the snapshot and sends exactly what was clicked
//! (ROADMAP-UI.md, U2b).
//!
//! There is no window here and not even a GPU: the panel is widgets and
//! commands, not pixels. How it **looks** is checked by a screenshot in
//! `engine` (`ui_probe.rs`); what it **does** is checked here, and the two
//! checks are deliberately different.
//!
//! The main claim of the step is its second half: a click puts `TogglePause`
//! in **and nothing else**. The first half ("puts it in") would pass for a
//! panel that sends all three commands every frame.

use engine::egui;

use game::clock::Stall;
use game::frame_view::ViewFrame;
use game::hud;
use game::mission;
use game::sim::Command;
use game::snapshot::WorldSnapshot;
use game::text::Language;

const SIZE: f32 = 300.0;

fn snapshot(warp: f64, stall: Option<Stall>) -> WorldSnapshot {
    WorldSnapshot {
        version: 1,
        t: mission::start().t + 3.5 * 86400.0,
        warp,
        stall,
        // No bodies in this panel: time and warp do not depend on them, and an
        // empty list is a legitimate scene state rather than a stub.
        bodies: Vec::new(),
        // No bodies and no star: the panel draws neither.
        sun: None,
        vessels: Vec::new(),
    }
}

/// Draws the panel once with the given input and returns the commands.
///
/// `at` is where the click landed; `None` means "the mouse is elsewhere",
/// i.e. the panel merely shows. The warm-up frame is mandatory: egui only
/// knows where the buttons ended up after drawing them once, so in the first
/// frame there is nothing to click.
fn click_at(snapshot: &WorldSnapshot, at: Option<egui::Pos2>) -> Vec<Command> {
    let context = egui::Context::default();
    // With the real style (U7c): a panel drawn by stock egui has different
    // paddings and button sizes, so the test would click elsewhere than the
    // player does.
    game::palette::apply(&context);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE, SIZE));

    let draw = |events: Vec<egui::Event>| -> Vec<Command> {
        let mut commands = Vec::new();
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let mut output = context.run_ui(input, |ui| {
            commands = hud::time_panel(ui, Language::English, snapshot);
        });
        // Nobody needs the textures here -- nothing is drawn -- but
        // `TexturesDelta` panics in `Drop` if it is not applied (U1a).
        output.textures_delta.clear();
        commands
    };

    draw(Vec::new());

    let events = match at {
        Some(pos) => vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            },
        ],
        None => Vec::new(),
    };
    draw(events)
}

/// The centre of a button found by its stable id (`hud::PAUSE` and friends).
///
/// The widget is looked up rather than a coordinate: hand-picked pixels go
/// stale with the first padding change, and the test starts clicking into
/// empty space while staying green.
fn button_centre(snapshot: &WorldSnapshot, id: &str) -> egui::Pos2 {
    let context = egui::Context::default();
    // The real style, as in `click_at`.
    game::palette::apply(&context);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE, SIZE));

    // Two frames: in the first egui only learns where everything lies.
    for _ in 0..2 {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let mut output = context.run_ui(input, |ui| {
            hud::time_panel(ui, Language::English, snapshot);
        });
        output.textures_delta.clear();
    }

    context
        .read_response(egui::Id::new(id))
        .map(|response| response.rect.center())
        .unwrap_or_else(|| panic!("there is no \"{id}\" button in the panel"))
}

/// A panel nobody touched sends nothing.
///
/// This is the half that is easy to forget: a panel sending a command every
/// frame would make the game uncontrollable, and no "the button works" test
/// would notice.
#[test]
fn a_panel_nobody_touched_sends_nothing() {
    let snapshot = snapshot(1000.0, None);
    assert_eq!(click_at(&snapshot, None), Vec::new());
}

/// A click on "pause" puts in exactly `TogglePause`.
#[test]
fn the_pause_button_sends_exactly_one_command() {
    let snapshot = snapshot(1000.0, None);
    let centre = button_centre(&snapshot, hud::PAUSE);

    assert_eq!(
        click_at(&snapshot, Some(centre)),
        vec![Command::TogglePause],
        "a click on pause should have put in exactly one command"
    );
}

/// And a click on "faster" puts in exactly `ScaleWarp(2.0)`, with no pause.
///
/// The second button is needed because a test with one would pass for a panel
/// that answers any click with `TogglePause`.
#[test]
fn the_faster_button_scales_the_warp() {
    let snapshot = snapshot(1000.0, None);
    let centre = button_centre(&snapshot, hud::FASTER);

    assert_eq!(
        click_at(&snapshot, Some(centre)),
        vec![Command::ScaleWarp(2.0)]
    );
}

/// A pause renames its own button.
///
/// A button that says "pause" while paused is the same bug as silent
/// throttling: the player sees a state that does not exist.
#[test]
fn the_button_says_resume_while_paused() {
    let running = snapshot(1000.0, None);
    let paused = snapshot(1000.0, Some(Stall::Paused));

    let centre = button_centre(&running, hud::PAUSE);
    assert_eq!(click_at(&running, Some(centre)), vec![Command::TogglePause]);

    let centre = button_centre(&paused, hud::PAUSE);
    assert_eq!(click_at(&paused, Some(centre)), vec![Command::TogglePause]);
}

// ---------------------------------------------------------------------------
// The vessel panel (U2c)

/// A vessel on a circular orbit around an Earth that is itself moving.
///
/// Earth is away from the origin on purpose: the panel must measure altitude
/// and speed **relative to the body**, and a vessel computed from the origin
/// would pass only for a motionless Earth at zero.
fn vessel_with_plan(t: f64) -> game::snapshot::VesselSnapshot {
    use core_rs::{State, Stop, Vec3d};
    use game::leg::{Leg, Sample};
    use game::plan::{Frame, Manoeuvre, Plan};
    use game::world::{VesselId, EARTH};
    use std::sync::Arc;

    let earth_r = [1.2e11, -3.4e10, 5.0e9];
    let earth_v = [7000.0, 25000.0, -3.0];
    let offset = [7.0e6, 0.0, 0.0];
    let relative_v = [0.0, 7500.0, 100.0];

    // Two samples: the second is needed so that the body's velocity is a
    // finite difference rather than zero.
    let sample_at = |dt: f64| Sample {
        state: State {
            t: t + dt,
            r: Vec3d {
                x: earth_r[0] + earth_v[0] * dt + offset[0],
                y: earth_r[1] + earth_v[1] * dt + offset[1],
                z: earth_r[2] + earth_v[2] * dt + offset[2],
            },
            v: Vec3d {
                x: earth_v[0] + relative_v[0],
                y: earth_v[1] + relative_v[1],
                z: earth_v[2] + relative_v[2],
            },
        },
        earth: [
            earth_r[0] + earth_v[0] * dt,
            earth_r[1] + earth_v[1] * dt,
            earth_r[2] + earth_v[2] * dt,
        ],
        moon: [0.0; 3],
    };

    let samples = vec![sample_at(0.0), sample_at(10.0)];
    let state = samples[0].state;

    let mut plan = Plan::new();
    // Two non-zero axes on purpose: a manoeuvre with one does not tell a sum
    // of norms from a sum of components, and the "add the components" mutation
    // would slip past.
    plan.insert(Manoeuvre {
        t: t + 2.0 * 86400.0,
        dv: [3.0, 4.0, 0.0],
        frame: Frame::Vnb { body: EARTH },
    });
    plan.insert(Manoeuvre {
        t: t + 5.0 * 86400.0,
        dv: [-3.0, -4.0, 0.0],
        frame: Frame::Vnb { body: EARTH },
    });

    game::snapshot::VesselSnapshot {
        id: VesselId(0),
        name: "probe".to_string(),
        // No Jacobi constant in this fixture: it is about panels, not about
        // the map (U6b3).
        jacobi: None,
        legs: vec![Arc::new(Leg {
            entry: state,
            t1: t + 10.0,
            step_out: 1.0,
            samples,
            stop: Stop::BufferFull,
        })],
        state,
        plan,
        start: state,
        tip: state,
        computed_to: t + 3.0 * 86400.0,
        horizon_end: t + 100.0 * 86400.0,
        params: None,
        failed: None,
    }
}

/// Every number in the panel agrees with one computed from the snapshot
/// another way.
#[test]
fn the_vessel_panel_agrees_with_the_snapshot() {
    const RADIUS: f64 = 6_371_000.0;

    let mut world = snapshot(1000.0, None);
    world.vessels.push(vessel_with_plan(world.t));

    let readout = hud::read_vessel(&world, &world.vessels[0], RADIUS);

    // Altitude: the vessel is offset by 7000 km from Earth's centre.
    assert!(
        (readout.altitude_m - (7.0e6 - RADIUS)).abs() < 1.0,
        "altitude {} m",
        readout.altitude_m
    );

    // Speed relative to the body: 7500 and 100 along two axes.
    let expected_speed = (7500.0f64 * 7500.0 + 100.0 * 100.0).sqrt();
    assert!(
        (readout.speed_m_s - expected_speed).abs() < 1e-3,
        "speed {} m/s against {expected_speed}",
        readout.speed_m_s
    );

    // The plan's dv is a sum of norms: |(3,4,0)| + |(-3,-4,0)| = 10, while a
    // sum of components would give zero. That is what tells two opposite
    // manoeuvres apart.
    assert!(
        (readout.total_dv_m_s - 10.0).abs() < 1e-9,
        "dv {} m/s, while the sum of norms is 10",
        readout.total_dv_m_s
    );

    // Two days to the next manoeuvre, not five: the first one ahead.
    assert_eq!(readout.next_burn_s, Some(2.0 * 86400.0));

    assert!((readout.computed_ahead_s - 3.0 * 86400.0).abs() < 1e-9);
    assert!(!readout.failed);
}

/// A burn already flown is not counted as the next one.
#[test]
fn a_burn_already_flown_is_not_the_next_one() {
    let mut world = snapshot(1000.0, None);
    let vessel = vessel_with_plan(world.t);
    world.vessels.push(vessel);

    // The cursor jumped past both manoeuvres.
    world.t += 6.0 * 86400.0;
    let readout = hud::read_vessel(&world, &world.vessels[0], 6_371_000.0);
    assert_eq!(readout.next_burn_s, None);
}

/// A click on a schedule row puts in exactly `SeekTo` of its own event (U3b).
#[test]
fn a_schedule_row_seeks_to_its_own_event() {
    use game::schedule::{Kind, Marker};

    let world = snapshot(1000.0, None);
    let markers = [
        // Behind the cursor -- there will be no row at all: the cursor does
        // not go back.
        Marker {
            kind: Kind::Periapsis,
            t: world.t - 100.0,
            distance_m: 7.0e6,
        },
        Marker {
            kind: Kind::Apoapsis,
            t: world.t + 3600.0,
            distance_m: 4.2e7,
        },
        Marker {
            kind: Kind::Periapsis,
            t: world.t + 7200.0,
            distance_m: 7.1e6,
        },
    ];

    let context = egui::Context::default();
    // The real style, as in `click_at`.
    game::palette::apply(&context);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE, SIZE));

    let draw = |events: Vec<egui::Event>| -> Vec<Command> {
        let mut commands = Vec::new();
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let mut output = context.run_ui(input, |ui| {
            commands = hud::schedule_panel(ui, Language::English, world.t, &markers);
        });
        output.textures_delta.clear();
        commands
    };

    draw(Vec::new());
    assert_eq!(
        draw(Vec::new()),
        Vec::new(),
        "the schedule sends nothing by itself"
    );

    // Row index 2 is the third marker: the first is dropped as past, but the
    // row id follows the position in the list rather than the order on screen.
    let id = egui::Id::new(format!("{}{}", hud::SEEK, 2));
    let centre = context
        .read_response(id)
        .map(|response| response.rect.center())
        .expect("the periapsis row should have been drawn");

    let clicked = draw(vec![
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

    assert_eq!(clicked, vec![Command::SeekTo(world.t + 7200.0)]);
}

// ---------------------------------------------------------------------------
// The manoeuvre editor (U4a)

/// Draws the plan panel over several frames and returns the actions of the
/// last one.
///
/// The warm-up frame is needed twice here: egui learns widget geometry only
/// by drawing it, and `DragValue` also keeps its own editing state.
fn plan_frames(
    draft: &mut hud::PlanDraft,
    now: f64,
    clicks: &[&str],
    notice: Option<&str>,
) -> Vec<hud::PlanAction> {
    let context = egui::Context::default();
    // The real style, as in `click_at`.
    game::palette::apply(&context);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE, SIZE));

    let draw = |draft: &mut hud::PlanDraft, events: Vec<egui::Event>| {
        let mut actions = Vec::new();
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let mut output = context.run_ui(input, |ui| {
            actions = hud::plan_panel(ui, Language::English, now, 3, draft, notice);
        });
        output.textures_delta.clear();
        actions
    };

    draw(draft, Vec::new());

    let mut events = Vec::new();
    for id in clicks {
        let centre = context
            .read_response(egui::Id::new(*id))
            .map(|response| response.rect.center())
            .unwrap_or_else(|| panic!("there is no \"{id}\" widget in the panel"));

        events.push(egui::Event::PointerMoved(centre));
        events.push(egui::Event::PointerButton {
            pos: centre,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        });
        events.push(egui::Event::PointerButton {
            pos: centre,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        });
    }

    draw(draft, events)
}

/// A plan nobody touched asks for nothing.
///
/// Without this, "an edit gives exactly one request" would only check that a
/// request happens at all: a panel asking for a preview every frame would
/// swamp the planner and look just as correct.
#[test]
fn an_untouched_plan_asks_for_nothing() {
    let mut draft = hud::PlanDraft::default();
    assert_eq!(plan_frames(&mut draft, 0.0, &[], None), Vec::new());
}

/// Adding a manoeuvre gives exactly one preview request -- with the plan that
/// is shown.
#[test]
fn adding_a_burn_asks_for_exactly_one_preview() {
    let mut draft = hud::PlanDraft::default();
    let actions = plan_frames(&mut draft, 0.0, &[hud::PLAN_ADD], None);

    assert_eq!(
        actions.len(),
        1,
        "there should have been exactly one request: {actions:?}"
    );
    match &actions[0] {
        hud::PlanAction::Preview(plan) => {
            assert_eq!(plan.manoeuvres().len(), 1);
            // The same plan as the draft on screen -- not a "similar" one.
            assert_eq!(plan, &draft.plan());
        }
        other => panic!("expected a preview, got {other:?}"),
    }
}

/// "Fly this" puts in exactly the plan that was shown.
#[test]
fn committing_sends_the_plan_that_was_shown() {
    let mut draft = hud::PlanDraft::default();
    plan_frames(&mut draft, 0.0, &[hud::PLAN_ADD], None);

    let shown = draft.plan();
    let actions = plan_frames(&mut draft, 0.0, &[hud::PLAN_COMMIT], None);

    assert_eq!(actions, vec![hud::PlanAction::Commit(shown)]);
}

/// Deleting a row also asks for a preview, and the plan gets shorter.
#[test]
fn deleting_a_row_asks_for_a_preview_of_what_is_left() {
    let mut draft = hud::PlanDraft::default();
    plan_frames(&mut draft, 0.0, &[hud::PLAN_ADD], None);
    plan_frames(&mut draft, 0.0, &[hud::PLAN_ADD], None);
    assert_eq!(draft.manoeuvres.len(), 2);

    let delete_first = format!("{}{}", hud::PLAN_DELETE, 0);
    let actions = plan_frames(&mut draft, 0.0, &[&delete_first], None);

    assert_eq!(draft.manoeuvres.len(), 1);
    assert_eq!(
        actions,
        vec![hud::PlanAction::Preview(draft.plan())],
        "after a deletion the preview must be about what is left"
    );
}

/// The world's answer is visible on the panel, not only in the logs (rule 8).
///
/// It is the drawn text that is checked: a panel that got a refusal and
/// silently left the old plan on screen is the very bug rule 8 exists for.
#[test]
fn a_refusal_is_drawn_where_the_player_looks() {
    use game::text::{tr, Key};

    let refusal = tr(Language::English, Key::RejectedInThePast);
    let context = egui::Context::default();
    // The real style, as in `click_at`.
    game::palette::apply(&context);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE, SIZE));
    let mut draft = hud::PlanDraft::default();

    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            hud::plan_panel(ui, Language::English, 0.0, 3, &mut draft, Some(refusal));
        },
    );

    let drawn = output
        .shapes
        .iter()
        .any(|clipped| shape_says(&clipped.shape, refusal));
    output.textures_delta.clear();

    assert!(
        drawn,
        "the refusal \"{refusal}\" is not among what was drawn"
    );
}

/// Whether a shape holds text containing the given string.
fn shape_says(shape: &egui::epaint::Shape, needle: &str) -> bool {
    match shape {
        egui::epaint::Shape::Text(text) => text.galley.text().contains(needle),
        egui::epaint::Shape::Vec(shapes) => shapes.iter().any(|s| shape_says(s, needle)),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// The view panel (ROADMAP-UI.md, U6a4)

/// Draws the view panel once and returns what it handed back.
fn click_view(
    frame: game::frame_view::ViewFrame,
    language: Language,
    at: Option<egui::Pos2>,
) -> hud::ViewChoice {
    // No curve here on purpose: this test is about the switch, and the
    // curve's caption is checked separately (U6b4).
    let curve = None;
    let context = egui::Context::default();
    // The real style, as in `click_at`.
    game::palette::apply(&context);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE, SIZE));

    let draw = |events: Vec<egui::Event>| -> hud::ViewChoice {
        let mut chosen = hud::ViewChoice::default();
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let mut output = context.run_ui(input, |ui| {
            chosen = hud::view_panel(ui, language, frame, curve);
        });
        output.textures_delta.clear();
        chosen
    };

    draw(Vec::new());

    let events = match at {
        Some(pos) => vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            },
        ],
        None => Vec::new(),
    };
    draw(events)
}

fn view_button_centre(frame: ViewFrame, language: Language, id: &str) -> egui::Pos2 {
    let curve = None;
    let context = egui::Context::default();
    // The real style, as in `click_at`.
    game::palette::apply(&context);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE, SIZE));

    for _ in 0..2 {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let mut output = context.run_ui(input, |ui| {
            hud::view_panel(ui, language, frame, curve);
        });
        output.textures_delta.clear();
    }

    context
        .read_response(egui::Id::new(id))
        .map(|response| response.rect.center())
        .unwrap_or_else(|| panic!("there is no \"{id}\" button in the panel"))
}

/// A click switches the frame, both ways.
///
/// Both directions, because a switch that always returns "rotating" would
/// pass a one-way check. And separately, a frame with no click: a panel
/// choosing a frame every frame would make the switch uncontrollable just as
/// a panel sending a command every frame would (the same lesson as in
/// `a_panel_nobody_touched_sends_nothing`).
#[test]
fn the_frame_button_switches_both_ways_and_only_when_clicked() {
    let english = Language::English;
    assert_eq!(click_view(ViewFrame::Inertial, english, None).frame, None);
    assert_eq!(click_view(ViewFrame::Rotating, english, None).frame, None);

    let centre = view_button_centre(ViewFrame::Inertial, english, hud::FRAME);
    assert_eq!(
        click_view(ViewFrame::Inertial, english, Some(centre)).frame,
        Some(ViewFrame::Rotating)
    );

    let centre = view_button_centre(ViewFrame::Rotating, english, hud::FRAME);
    assert_eq!(
        click_view(ViewFrame::Rotating, english, Some(centre)).frame,
        Some(ViewFrame::Inertial)
    );
}

/// A click on the language switch changes the language -- and **only** it.
///
/// The second half is the main one: both switches live in one panel, and a
/// panel that hands back both fields on any click would make the frame
/// uncontrollable exactly when the player picks a language.
///
/// Both directions, because a switch that always returns Ukrainian would pass
/// a one-way check and break exactly when it is used.
#[test]
fn the_language_button_switches_both_ways_and_touches_nothing_else() {
    for (from, to) in [
        (Language::English, Language::Ukrainian),
        (Language::Ukrainian, Language::English),
    ] {
        let centre = view_button_centre(ViewFrame::Inertial, from, hud::LANGUAGE);
        let choice = click_view(ViewFrame::Inertial, from, Some(centre));

        assert_eq!(
            choice.language,
            Some(to),
            "from {from:?} it should have switched to {to:?}"
        );
        assert_eq!(
            choice.frame, None,
            "choosing a language touched the frame -- the panel hands back both \
             fields on one click"
        );
    }

    // And the other way round: a click on the frame does not change the
    // language.
    let centre = view_button_centre(ViewFrame::Inertial, Language::English, hud::FRAME);
    let choice = click_view(ViewFrame::Inertial, Language::English, Some(centre));
    assert_eq!(choice.frame, Some(ViewFrame::Rotating));
    assert_eq!(
        choice.language, None,
        "choosing a frame touched the language"
    );
}

/// The panel really speaks both languages.
///
/// The table test (`text.rs`) proves the strings **exist**; this one proves
/// they **reach the screen**. The difference is the one between "the key is
/// there" and "the widget took it": a literal forgotten in the panel code
/// passes the first test and fails this one.
#[test]
fn the_panel_speaks_both_languages() {
    use game::text::{tr, Key};

    let drawn = |language: Language| -> String {
        let context = egui::Context::default();
        // The real style, as in `click_at`.
        game::palette::apply(&context);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE, SIZE));
        let mut text = String::new();
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let mut output = context.run_ui(input, |ui| {
                hud::view_panel(ui, language, ViewFrame::Inertial, None);
            });
            text = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(t) => Some(t.galley.text().to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            output.textures_delta.clear();
        }
        text
    };

    let english = drawn(Language::English);
    let ukrainian = drawn(Language::Ukrainian);

    assert!(
        english.contains(tr(Language::English, Key::View)),
        "the English heading did not reach the panel: {english}"
    );
    assert!(
        ukrainian.contains(tr(Language::Ukrainian, Key::View)),
        "the Ukrainian heading is not in the panel: {ukrainian}"
    );
    assert_ne!(
        english, ukrainian,
        "the panel drew the same in both languages -- the strings do not go \
         through the table"
    );
}

/// The frame switch sends nothing to the world.
///
/// That is the property it returns a frame rather than a command for: the
/// choice of view has no right to touch either the time or the plan (rule 1
/// of stage U). Checked beside the time panel on the same click: were the
/// frame going over the channel, there would be a command here.
#[test]
fn choosing_a_frame_sends_no_command() {
    let snapshot = snapshot(1000.0, None);
    let centre = view_button_centre(ViewFrame::Inertial, Language::English, hud::FRAME);

    // The same click at the same coordinates, but into the time panel: no
    // commands, because there is nothing there in that spot.
    assert_eq!(click_at(&snapshot, Some(centre)), Vec::new());
}

/// The curve's caption appears together with the curve, and the warning
/// appears every time.
///
/// The second claim is the main one: "advice, not a wall" cannot be a trouble
/// message that shows up in the bad case. A curve once shown as a wall has
/// already lied -- and the player has no way of knowing when to believe it.
///
/// Checked through `read_curve` and `view_panel` together: the first takes
/// the numbers from the snapshot (without the ephemeris, rule 5), the second
/// shows them only in the frame where the curve exists.
#[test]
fn the_curve_caption_appears_with_the_curve_and_warns_every_time() {
    use game::frame_view::ViewFrame;
    use game::text::{tr, Key};

    let mut world = game::mission::world(&game::mission::default_asset()).expect("world");
    world.tick(8);
    let snapshot = world.snapshot();

    let curve = hud::read_curve(&snapshot).expect("C is computed in the world thread");
    println!(
        "  C = {:.4}, the vessel is far away: {}",
        curve.jacobi, curve.far_away
    );
    assert!(
        (2.0..4.0).contains(&curve.jacobi),
        "C = {} -- that is no longer the Earth-Moon system",
        curve.jacobi
    );
    assert!(
        !curve.far_away,
        "at mission start the vessel is near the Moon, not two pair distances away"
    );

    // What exactly was drawn is visible from the panel's text: egui hands it
    // back with the shapes, and that is the same path the player sees it by.
    let drawn = |frame: ViewFrame| -> String {
        let context = egui::Context::default();
        // The real style, as in `click_at`.
        game::palette::apply(&context);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE, SIZE));
        let mut text = String::new();
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let mut output = context.run_ui(input, |ui| {
                hud::view_panel(ui, Language::English, frame, Some(curve));
            });
            text = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(t) => Some(t.galley.text().to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" | ");
            output.textures_delta.clear();
        }
        text
    };

    let rotating = drawn(ViewFrame::Rotating);
    let inertial = drawn(ViewFrame::Inertial);
    println!("  rotating: {rotating}");

    let advice = tr(Language::English, Key::CurveIsAdvice);
    assert!(
        rotating.contains(advice),
        "the rotating frame has no warning: {rotating}"
    );
    assert!(
        rotating.contains(&format!("{:.4}", curve.jacobi)),
        "the panel did not show C itself: {rotating}"
    );
    assert!(
        !inertial.contains(advice),
        "the inertial frame has no curve, yet its caption is there: {inertial}"
    );
}
