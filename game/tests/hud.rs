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
