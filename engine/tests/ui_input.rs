//! Ввід має власника на кожен кадр (ROADMAP-UI.md, U1c, правило 4).
//!
//! Твердження перевіряється **обома боками**, і це не формальність: тест лише
//! на «тягнення в панелі не крутить камеру» пройшов би й на камері, яка не
//! рухається взагалі.
//!
//! Вікна тут немає — як і скрізь на етапі. `egui-winit` збирає `RawInput` із
//! подій winit, але сам `RawInput` — звичайна структура, тож клік у тесті
//! робиться руками.
//!
//! ## Що з'ясував вимір
//!
//! `egui_wants_pointer_input()` — це `is_using_pointer() || is_pointer_over_egui()`,
//! і перша половина **липка**: доки кнопка миші не відпущена, інтерфейс
//! вважає мишу своєю, навіть коли курсор зійшов з панелі. Це не вада, а те,
//! чого хочеться: почав тягнути повзунок — тягнеш його далі, куди б не
//! поїхала рука. Але тест, який тисне кнопку щокадру й ніколи не відпускає,
//! отримає `true` в будь-якій точці екрана — і саме так виглядав перший
//! варіант цього файлу.

use engine::egui;
use engine::gpu::Gpu;
use engine::orbit::Orbit;
use engine::shot;
use engine::ui::{Ui, Viewport};

const SIZE: u32 = 256;

/// Бічна панель займає ліву чверть екрана. Саме панель, а не вікно: у панелі
/// геометрія точна, а вікно стискається до свого вмісту — і перевірка
/// «за 50 пікселів убік» міряла б не те, що думає той, хто її читає.
const PANEL: f32 = 128.0;

/// Точка на повзунку — другий віджет панелі, приблизно посередині його
/// доріжки. Кнопка вище й тягнення не тримає.
const SLIDER: egui::Pos2 = egui::Pos2::new(60.0, 45.0);

fn gpu() -> Option<Gpu> {
    match Gpu::new(wgpu::Instance::default(), None) {
        Ok(gpu) => Some(gpu),
        Err(_) => {
            eprintln!("ПРОПУЩЕНО: немає адаптера wgpu (немає драйвера або GPU)");
            None
        }
    }
}

/// Що робить миша цього кадру.
enum Mouse {
    /// Курсор просто там.
    Hover,
    /// Натиснули й відпустили — повний клік, без липкого стану після нього.
    Click,
    /// Натиснули й тримають: саме той стан, у якому власник липкий.
    Hold,
}

fn input(viewport: Viewport, at: egui::Pos2, mouse: &Mouse) -> egui::RawInput {
    let mut raw = viewport.quiet_input();
    let button = |pressed: bool| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };

    raw.events = match mouse {
        Mouse::Hover => vec![egui::Event::PointerMoved(at)],
        Mouse::Click => vec![egui::Event::PointerMoved(at), button(true), button(false)],
        Mouse::Hold => vec![egui::Event::PointerMoved(at), button(true)],
    };
    raw
}

/// Один кадр інтерфейсу з бічною панеллю. Повертає, чи забрав інтерфейс мишу.
///
/// `slider` живе поза кадром, бо повзунок — це стан: саме він робить тягнення
/// липким, і без нього перевірка липкості нічого не міряла б.
fn owner_asks(gpu: &Gpu, ui: &mut Ui, at: egui::Pos2, mouse: Mouse, slider: &mut f32) -> bool {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ui input"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shot::FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ui input"),
        });

    let viewport = Viewport::new(SIZE, SIZE, 1.0);
    ui.draw(
        gpu,
        &mut encoder,
        &view,
        viewport,
        input(viewport, at, &mouse),
        |ui| {
            egui::Panel::left("панель")
                .exact_size(PANEL)
                .resizable(false)
                .show(ui, |ui| {
                    // Справжні віджети, а не намальований прямокутник:
                    // питання «чия це подія» ставиться до того, з чим гравець
                    // взаємодіє. Повзунок тут не для краси — він єдиний, хто
                    // вміє тримати тягнення довше за один кадр.
                    let _ = ui.button("пауза");
                    let _ = ui.add(egui::Slider::new(slider, 0.0..=1.0));
                });
        },
    );

    gpu.queue.submit([encoder.finish()]);
    ui.wants_pointer()
}

/// Панель забирає мишу над собою й не забирає поза собою.
#[test]
fn the_interface_takes_the_pointer_only_over_itself() {
    let Some(gpu) = gpu() else { return };
    let mut ui = Ui::new(&gpu, shot::FORMAT);
    let mut slider = 0.5;

    // Кадр-розігрів: egui знає розміри віджетів лише намалювавши їх один раз,
    // тож у першому кадрі панель ще не знає, де вона.
    owner_asks(
        &gpu,
        &mut ui,
        egui::pos2(20.0, 20.0),
        Mouse::Hover,
        &mut slider,
    );

    assert!(
        owner_asks(
            &gpu,
            &mut ui,
            egui::pos2(20.0, 20.0),
            Mouse::Hover,
            &mut slider
        ),
        "курсор у панелі, а інтерфейс миші не хоче — гра крутила б камеру \
         поверх власної кнопки"
    );
    assert!(
        !owner_asks(
            &gpu,
            &mut ui,
            egui::pos2(PANEL + 50.0, 20.0),
            Mouse::Hover,
            &mut slider
        ),
        "курсор за 50 пікселів від панелі, а інтерфейс усе одно забрав мишу — \
         камера не оберталася б ніколи"
    );
}

/// Почате в панелі тягнення лишається її, навіть коли курсор зійшов з неї.
///
/// Це не побічний ефект, а те, чого хочеться: повзунок не має губитися під
/// рукою. Перевірка існує, щоб липкість була **виміряною** властивістю, а не
/// сюрпризом, який колись поясниться сам.
#[test]
fn a_drag_that_started_in_the_panel_stays_with_it() {
    let Some(gpu) = gpu() else { return };
    let mut ui = Ui::new(&gpu, shot::FORMAT);
    let mut slider = 0.5;
    owner_asks(
        &gpu,
        &mut ui,
        egui::pos2(20.0, 20.0),
        Mouse::Hover,
        &mut slider,
    );

    // Саме по повзунку, а не по кнопці: кнопка тягнення не тримає, і
    // «липкість» на ній не з'явилася б навіть у правильному коді.
    assert!(
        owner_asks(&gpu, &mut ui, SLIDER, Mouse::Hold, &mut slider),
        "натискання на повзунку мало належати панелі"
    );
    assert!(
        owner_asks(
            &gpu,
            &mut ui,
            egui::pos2(PANEL + 50.0, 20.0),
            Mouse::Hover,
            &mut slider
        ),
        "курсор виїхав з панелі з затиснутою кнопкою, і тягнення загубилося"
    );
}

/// Камера повертається лише тоді, коли подію не забрав інтерфейс.
///
/// Обидва твердження обов'язкові — і що камера стоїть, коли власник
/// інтерфейс, і що вона рухається, коли власник світ.
#[test]
fn the_camera_turns_only_when_the_interface_did_not_take_the_drag() {
    let Some(gpu) = gpu() else { return };
    let mut ui = Ui::new(&gpu, shot::FORMAT);
    let mut slider = 0.5;
    owner_asks(
        &gpu,
        &mut ui,
        egui::pos2(20.0, 20.0),
        Mouse::Hover,
        &mut slider,
    );

    let mut orbit = Orbit::default();
    let before = orbit.camera().position();

    // Клік у панелі: власник — інтерфейс, світ події не бачить.
    if !owner_asks(
        &gpu,
        &mut ui,
        egui::pos2(20.0, 20.0),
        Mouse::Click,
        &mut slider,
    ) {
        orbit.drag(50.0, 0.0);
    }
    assert_eq!(
        orbit.camera().position(),
        before,
        "тягнення в панелі повернуло камеру"
    );

    // Той самий клік осторонь: власник — світ.
    if !owner_asks(
        &gpu,
        &mut ui,
        egui::pos2(PANEL + 50.0, 20.0),
        Mouse::Click,
        &mut slider,
    ) {
        orbit.drag(50.0, 0.0);
    }
    assert_ne!(
        orbit.camera().position(),
        before,
        "тягнення поза панеллю камери не зрушило"
    );
}
