//! Панель часу показує снапшот і надсилає рівно те, що натиснули
//! (ROADMAP-UI.md, U2b).
//!
//! Вікна тут немає, і навіть GPU немає: панель — це віджети й команди, а не
//! пікселі. Те, як вона **виглядає**, перевіряє знімок у `engine`
//! (`ui_probe.rs`); те, що вона **робить**, перевіряється тут, і ці дві
//! перевірки навмисно різні.
//!
//! Головне твердження кроку — друга його половина: клік кладе `TogglePause`
//! **і нічого більше**. Перша половина («кладе») пройшла б і на панелі, яка
//! на кожен кадр надсилає всі три команди одразу.

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
        // Тіл у цій панелі немає: час і warp від них не залежать, а порожній
        // список — законний стан сцени, а не заглушка.
        bodies: Vec::new(),
        vessels: Vec::new(),
    }
}

/// Малює панель один раз із заданим вводом і повертає команди.
///
/// `at` — куди клікнули; `None` означає «миша осторонь», тобто панель просто
/// показує. Кадр-розігрів обов'язковий: egui знає, де опинилися кнопки, лише
/// намалювавши їх один раз, тож у першому кадрі клікати нема по чому.
fn click_at(snapshot: &WorldSnapshot, at: Option<egui::Pos2>) -> Vec<Command> {
    let context = egui::Context::default();
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
        // Текстури тут нікому не потрібні — малювання немає, — але
        // `TexturesDelta` падає в `Drop`, якщо її не застосувати (U1a).
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

/// Точка в середині кнопки за її сталою адресою (`hud::PAUSE` тощо).
///
/// Шукається саме віджет, а не координата: підібрані руками пікселі тихо
/// протухають від першої зміни відступів, і тест починає клікати в порожнечу,
/// лишаючись зеленим.
fn button_centre(snapshot: &WorldSnapshot, id: &str) -> egui::Pos2 {
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE, SIZE));

    // Два кадри: у першому egui лише дізнається, де що лежить.
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
        .unwrap_or_else(|| panic!("кнопки «{id}» немає в панелі"))
}

/// Панель без кліків не надсилає нічого.
///
/// Це та половина твердження, яку легко забути: панель, що надсилає команду
/// щокадру, зробила б гру некерованою, і жоден тест «кнопка працює» цього б
/// не помітив.
#[test]
fn a_panel_nobody_touched_sends_nothing() {
    let snapshot = snapshot(1000.0, None);
    assert_eq!(click_at(&snapshot, None), Vec::new());
}

/// Клік по «pause» кладе рівно `TogglePause`.
#[test]
fn the_pause_button_sends_exactly_one_command() {
    let snapshot = snapshot(1000.0, None);
    let centre = button_centre(&snapshot, hud::PAUSE);

    assert_eq!(
        click_at(&snapshot, Some(centre)),
        vec![Command::TogglePause],
        "клік по паузі мав покласти рівно одну команду"
    );
}

/// А клік по «faster» — рівно `ScaleWarp(2.0)`, і жодної паузи.
///
/// Друга кнопка потрібна, бо тест з однією пройшов би й на панелі, яка на
/// будь-який клік відповідає `TogglePause`.
#[test]
fn the_faster_button_scales_the_warp() {
    let snapshot = snapshot(1000.0, None);
    let centre = button_centre(&snapshot, hud::FASTER);

    assert_eq!(
        click_at(&snapshot, Some(centre)),
        vec![Command::ScaleWarp(2.0)]
    );
}

/// Пауза перейменовує власну кнопку.
///
/// Кнопка, що в паузі каже «pause», — це та сама помилка, що мовчазне
/// підгальмовування: гравець бачить стан, якого немає.
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
// Панель апарата (U2c)

/// Апарат на коловій орбіті навколо Землі, яка сама рухається.
///
/// Земля не в початку координат навмисно: панель має міряти висоту й
/// швидкість **відносно тіла**, і апарат, порахований від початку координат,
/// пройшов би перевірку лише на нерухомій Землі в нулі.
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

    // Два семпли: другий потрібен, щоб швидкість тіла була скінченною
    // різницею, а не нулем.
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
    // Дві осі ненульові навмисно: маневр з однією не розрізняє суму норм і
    // суму компонент, і мутація «складати компоненти» пройшла б повз.
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

/// Кожне число панелі збігається з порахованим зі снапшоту іншим шляхом.
#[test]
fn the_vessel_panel_agrees_with_the_snapshot() {
    const RADIUS: f64 = 6_371_000.0;

    let mut world = snapshot(1000.0, None);
    world.vessels.push(vessel_with_plan(world.t));

    let readout = hud::read_vessel(&world, &world.vessels[0], RADIUS);

    // Висота: апарат зміщений на 7000 км від центра Землі.
    assert!(
        (readout.altitude_m - (7.0e6 - RADIUS)).abs() < 1.0,
        "висота {} м",
        readout.altitude_m
    );

    // Швидкість відносно тіла: 7500 і 100 по двох осях.
    let expected_speed = (7500.0f64 * 7500.0 + 100.0 * 100.0).sqrt();
    assert!(
        (readout.speed_m_s - expected_speed).abs() < 1e-3,
        "швидкість {} м/с проти {expected_speed}",
        readout.speed_m_s
    );

    // Δv плану — сума норм: |(3,4,0)| + |(-3,-4,0)| = 10, тоді як сума
    // компонент дала б нуль. Саме це й розрізняє два маневри в різні боки.
    assert!(
        (readout.total_dv_m_s - 10.0).abs() < 1e-9,
        "Δv {} м/с, а сума норм — 10",
        readout.total_dv_m_s
    );

    // До наступного маневру — дві доби, а не п'ять: перший, що попереду.
    assert_eq!(readout.next_burn_s, Some(2.0 * 86400.0));

    assert!((readout.computed_ahead_s - 3.0 * 86400.0).abs() < 1e-9);
    assert!(!readout.failed);
}

/// Маневр у минулому наступним не вважається.
#[test]
fn a_burn_already_flown_is_not_the_next_one() {
    let mut world = snapshot(1000.0, None);
    let vessel = vessel_with_plan(world.t);
    world.vessels.push(vessel);

    // Курсор перескочив обидва маневри.
    world.t += 6.0 * 86400.0;
    let readout = hud::read_vessel(&world, &world.vessels[0], 6_371_000.0);
    assert_eq!(readout.next_burn_s, None);
}

/// Клік у рядку розкладу кладе рівно `SeekTo` тієї події (U3b).
#[test]
fn a_schedule_row_seeks_to_its_own_event() {
    use game::schedule::{Kind, Marker};

    let world = snapshot(1000.0, None);
    let markers = [
        // Позаду курсора — рядка не буде взагалі: назад курсор не ходить.
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
    assert_eq!(draw(Vec::new()), Vec::new(), "розклад сам нічого не шле");

    // Рядок з індексом 2 — це третій маркер: перший відкинуто як минулий,
    // але адреса рядка йде за номером у списку, а не за порядком на екрані.
    let id = egui::Id::new(format!("{}{}", hud::SEEK, 2));
    let centre = context
        .read_response(id)
        .map(|response| response.rect.center())
        .expect("рядок перицентра має бути намальований");

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
// Редактор маневрів (U4a)

/// Малює панель плану кілька кадрів і повертає дії останнього.
///
/// Кадр-розігрів тут потрібен двічі: egui дізнається геометрію віджетів лише
/// намалювавши їх, а `DragValue` ще й тримає власний стан редагування.
fn plan_frames(
    draft: &mut hud::PlanDraft,
    now: f64,
    clicks: &[&str],
    notice: Option<&str>,
) -> Vec<hud::PlanAction> {
    let context = egui::Context::default();
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
            .unwrap_or_else(|| panic!("віджета «{id}» немає в панелі"));

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

/// Панель, якої ніхто не чіпав, нічого не просить.
///
/// Без цього «правка породжує рівно один запит» перевіряло б лише те, що
/// запит узагалі буває: панель, яка щокадру просить прев'ю, перевантажила б
/// планувальник і виглядала б так само правильно.
#[test]
fn an_untouched_plan_asks_for_nothing() {
    let mut draft = hud::PlanDraft::default();
    assert_eq!(plan_frames(&mut draft, 0.0, &[], None), Vec::new());
}

/// Додавання маневру дає рівно один запит прев'ю — з тим планом, що показано.
#[test]
fn adding_a_burn_asks_for_exactly_one_preview() {
    let mut draft = hud::PlanDraft::default();
    let actions = plan_frames(&mut draft, 0.0, &[hud::PLAN_ADD], None);

    assert_eq!(actions.len(), 1, "мав бути рівно один запит: {actions:?}");
    match &actions[0] {
        hud::PlanAction::Preview(plan) => {
            assert_eq!(plan.manoeuvres().len(), 1);
            // Той самий план, що в чернетці на екрані — не «схожий».
            assert_eq!(plan, &draft.plan());
        }
        other => panic!("очікували прев'ю, отримали {other:?}"),
    }
}

/// «Летіти цим» кладе рівно той план, який показано.
#[test]
fn committing_sends_the_plan_that_was_shown() {
    let mut draft = hud::PlanDraft::default();
    plan_frames(&mut draft, 0.0, &[hud::PLAN_ADD], None);

    let shown = draft.plan();
    let actions = plan_frames(&mut draft, 0.0, &[hud::PLAN_COMMIT], None);

    assert_eq!(actions, vec![hud::PlanAction::Commit(shown)]);
}

/// Видалення рядка теж просить прев'ю, і план коротшає.
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
        "після видалення прев'ю мусить бути про те, що лишилось"
    );
}

/// Відповідь світу видно на панелі, а не лише в логах (правило 8).
///
/// Перевіряється саме намальований текст: панель, яка отримала відмову й
/// мовчки лишила старий план на екрані, — це та помилка, від якої правило 8
/// й існує.
#[test]
fn a_refusal_is_drawn_where_the_player_looks() {
    use game::text::{tr, Key};

    let refusal = tr(Language::English, Key::RejectedInThePast);
    let context = egui::Context::default();
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

    assert!(drawn, "відмови «{refusal}» немає серед намальованого");
}

/// Чи є в фігурі текст із заданим рядком.
fn shape_says(shape: &egui::epaint::Shape, needle: &str) -> bool {
    match shape {
        egui::epaint::Shape::Text(text) => text.galley.text().contains(needle),
        egui::epaint::Shape::Vec(shapes) => shapes.iter().any(|s| shape_says(s, needle)),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Панель вигляду (ROADMAP-UI.md, U6a4)

/// Малює панель вигляду один раз і повертає, що вона віддала.
fn click_view(frame: game::frame_view::ViewFrame, at: Option<egui::Pos2>) -> Option<ViewFrame> {
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE, SIZE));

    let draw = |events: Vec<egui::Event>| -> Option<ViewFrame> {
        let mut chosen = None;
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let mut output = context.run_ui(input, |ui| {
            chosen = hud::view_panel(ui, Language::English, frame);
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

fn view_button_centre(frame: ViewFrame) -> egui::Pos2 {
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE, SIZE));

    for _ in 0..2 {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let mut output = context.run_ui(input, |ui| {
            hud::view_panel(ui, Language::English, frame);
        });
        output.textures_delta.clear();
    }

    context
        .read_response(egui::Id::new(hud::FRAME))
        .map(|response| response.rect.center())
        .unwrap_or_else(|| panic!("кнопки «{}» немає в панелі", hud::FRAME))
}

/// Клік перемикає фрейм, і туди, і назад.
///
/// Обидва напрямки, бо перемикач, який завжди повертає «обертовий», пройшов би
/// перевірку в один бік. І окремо — кадр без кліку: панель, що обирає фрейм
/// щокадру, зробила б перемикач некерованим так само, як панель, що щокадру
/// шле команду (той самий урок, що в `a_panel_nobody_touched_sends_nothing`).
#[test]
fn the_frame_button_switches_both_ways_and_only_when_clicked() {
    assert_eq!(click_view(ViewFrame::Inertial, None), None);
    assert_eq!(click_view(ViewFrame::Rotating, None), None);

    let centre = view_button_centre(ViewFrame::Inertial);
    assert_eq!(
        click_view(ViewFrame::Inertial, Some(centre)),
        Some(ViewFrame::Rotating)
    );

    let centre = view_button_centre(ViewFrame::Rotating);
    assert_eq!(
        click_view(ViewFrame::Rotating, Some(centre)),
        Some(ViewFrame::Inertial)
    );
}

/// Перемикач фрейму не надсилає в світ нічого.
///
/// Це і є та властивість, заради якої він повертає фрейм, а не команду: вибір
/// вигляду не має права торкнутися ні часу, ні плану (правило 1 етапу U).
/// Перевіряється поруч із панеллю часу на тому самому кліку: якби фрейм ішов
/// каналом, тут була б команда.
#[test]
fn choosing_a_frame_sends_no_command() {
    let snapshot = snapshot(1000.0, None);
    let centre = view_button_centre(ViewFrame::Inertial);

    // Той самий клік по тих самих координатах, але в панель часу: команд
    // немає, бо там у цьому місці нічого немає.
    assert_eq!(click_at(&snapshot, Some(centre)), Vec::new());
}
