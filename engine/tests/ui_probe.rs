//! Інтерфейс у кадрі: розвідка U1a і провід U1b.
//!
//! Питання, на якому стоїть уся перевірка етапу U (ROADMAP-UI.md, правило 3):
//! чи малює `egui-wgpu` у звичайну текстуру без вікна. Якщо ні — жодна панель
//! ніколи не потрапить у знімок, і «UI перевіряється без вікна» довелося б
//! викреслити разом із половиною оракулів етапу.
//!
//! Тверджень тут два, і **обидва обов'язкові**:
//!
//! 1. порожній `egui::Context` не міняє кадру **жодним бітом** — нічого не
//!    намальовано, отже нічого й не змінилось;
//! 2. непорожній міняє, і саме там, де намальовано.
//!
//! Перше без другого пройшло б і на цілком зламаному `egui-wgpu`, який не
//! малює нічого ніколи. Це та сама пара «обидва боки», якою міряються події
//! в `/core`, і причина її та сама: перевірка, що не вміє провалитися,
//! зелена не тому, що код працює.

use engine::egui;
use engine::frame::{self, Frame};
use engine::gpu::Gpu;
use engine::scene::Scene;
use engine::shot::{self, Shot};
use engine::ui::{Ui, Viewport};

const SIZE: u32 = 256;

/// Розмір із перевірки U1b — той, у якому міряється й час кадру.
const WIDE: u32 = 1280;
const TALL: u32 = 720;

fn gpu() -> Option<Gpu> {
    // Пропуск гучний і названий — як у решті тестів рушія: мовчазний пропуск
    // це зелений тест, який нічого не робить.
    match Gpu::new(wgpu::Instance::default(), None) {
        Ok(gpu) => Some(gpu),
        Err(_) => {
            eprintln!("ПРОПУЩЕНО: немає адаптера wgpu (немає драйвера або GPU)");
            None
        }
    }
}

fn target(gpu: &Gpu, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ui probe"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shot::FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

/// Кадр сцени, а поверх нього — те, що намалює `build`.
///
/// Порядок саме той, який задає U1b: сцена, потім інтерфейс, в одну текстуру.
fn draw_with_ui(gpu: &Gpu, width: u32, height: u32, build: impl FnMut(&mut egui::Ui)) -> Shot {
    let (texture, view) = target(gpu, width, height);
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ui probe"),
        });

    let mut scene_frame = Frame::new(gpu, shot::FORMAT);
    let scene = Scene::new(frame::default_camera());
    scene_frame.draw(gpu, &mut encoder, &view, width, height, &scene);

    let mut interface = Ui::new(gpu, shot::FORMAT);
    let viewport = Viewport::new(width, height, 1.0);
    interface.draw(
        gpu,
        &mut encoder,
        &view,
        viewport,
        viewport.quiet_input(),
        build,
    );

    shot::read_back(gpu, encoder, &texture, width, height).expect("кадр мав прочитатися назад")
}

/// Той самий кадр без жодного проходу egui — те, що малює рушій сьогодні.
fn draw_plain(gpu: &Gpu, width: u32, height: u32) -> Shot {
    let (texture, view) = target(gpu, width, height);
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ui probe: без egui"),
        });

    let mut scene_frame = Frame::new(gpu, shot::FORMAT);
    let scene = Scene::new(frame::default_camera());
    scene_frame.draw(gpu, &mut encoder, &view, width, height, &scene);

    shot::read_back(gpu, encoder, &texture, width, height).expect("кадр мав прочитатися назад")
}

/// Прямокутник у лівому верхньому куті, у пікселях цілі.
fn panel(ui: &mut egui::Ui, width: f32, height: f32, colour: egui::Color32) {
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
    ui.painter().rect_filled(rect, 0.0, colour);
}

/// Порожній інтерфейс не міняє кадру жодним бітом.
#[test]
fn an_empty_context_changes_nothing() {
    let Some(gpu) = gpu() else { return };

    let plain = draw_plain(&gpu, SIZE, SIZE);
    let with_ui = draw_with_ui(&gpu, SIZE, SIZE, |_| {});

    assert_eq!(
        plain.pixels, with_ui.pixels,
        "прохід egui без жодного віджета зрушив пікселі — тобто він щось \
         малює сам, і всі майбутні порівняння знімків міряли б це"
    );
}

/// А непорожній — міняє, і саме там, де намальовано.
///
/// Панель прибита до лівого верхнього кута фіксованим розміром, тож перевірка
/// знає, який піксель зобов'язаний змінитися й який зобов'язаний лишитися.
/// Без другої половини це був би тест «щось десь стало іншим».
#[test]
fn a_panel_lands_where_it_was_put() {
    let Some(gpu) = gpu() else { return };

    let plain = draw_plain(&gpu, SIZE, SIZE);
    let with_ui = draw_with_ui(&gpu, SIZE, SIZE, |ui| {
        panel(
            ui,
            SIZE as f32 / 2.0,
            SIZE as f32 / 2.0,
            egui::Color32::from_rgb(255, 0, 255),
        );
    });

    let inside = with_ui.pixel(SIZE / 4, SIZE / 4);
    let was = plain.pixel(SIZE / 4, SIZE / 4);
    assert_ne!(
        [inside[0], inside[1], inside[2]],
        [was[0], was[1], was[2]],
        "піксель усередині панелі не змінився — egui-wgpu не намалював нічого"
    );
    // Звіряється переважання каналу, а не точний колір: ціль знімка лінійна,
    // поверхня вікна sRGB, і той самий колір дає в них різні байти (ROADMAP
    // «Рендер»).
    assert!(
        inside[0] > inside[1] && inside[2] > inside[1],
        "усередині панелі мали переважати червоний і синій, а вийшло {inside:?}"
    );

    // Поза панеллю кадр лишився тим самим — прохід egui не зачепив нічого,
    // крім своїх ножиць.
    for (x, y) in [(SIZE - 2, SIZE - 2), (SIZE - 2, 1), (1, SIZE - 2)] {
        assert_eq!(
            plain.pixel(x, y),
            with_ui.pixel(x, y),
            "піксель ({x}, {y}) поза панеллю змінився"
        );
    }
}

/// Перевірка U1b дослівно: 1280×720, одна панель, піксель усередині — її,
/// піксель поза нею — небо.
///
/// «Небо» тут не будь-що інше, а саме [`frame::CLEAR_BYTES`]: кут кадру з
/// висоти за замовчуванням лежить поза диском планети (це вже виміряно в
/// `shot.rs`), тож у ньому має бути колір очищення й нічого більше. Панель
/// зелена — канал, якого немає ні у фону, ні в планети.
#[test]
fn a_panel_covers_the_sky_and_only_it() {
    let Some(gpu) = gpu() else { return };

    let with_ui = draw_with_ui(&gpu, WIDE, TALL, |ui| {
        panel(ui, 300.0, 200.0, egui::Color32::from_rgb(0, 255, 0));
    });

    let inside = with_ui.pixel(150, 100);
    assert!(
        inside[1] > inside[0] && inside[1] > inside[2],
        "усередині панелі мав переважати зелений, а вийшло {inside:?}"
    );

    let outside = with_ui.pixel(WIDE - 2, 2);
    assert_eq!(
        [outside[0], outside[1], outside[2]],
        frame::CLEAR_BYTES,
        "піксель поза панеллю мав лишитися небом"
    );
}

/// Масштаб — це масштаб, а не зміна розміру цілі.
///
/// Та сама панель у точках при `scale = 2.0` займає вдвічі більше пікселів.
/// Перевірка дешева, а ловить помилку, яка інакше знаходиться очима на
/// екрані з високим DPI — і лише в того, у кого такий екран є.
#[test]
fn the_scale_factor_scales() {
    let Some(gpu) = gpu() else { return };

    let (texture, view) = target(&gpu, SIZE, SIZE);
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("scale"),
        });

    let mut scene_frame = Frame::new(&gpu, shot::FORMAT);
    let scene = Scene::new(frame::default_camera());
    scene_frame.draw(&gpu, &mut encoder, &view, SIZE, SIZE, &scene);

    let mut interface = Ui::new(&gpu, shot::FORMAT);
    let viewport = Viewport::new(SIZE, SIZE, 2.0);
    interface.draw(
        &gpu,
        &mut encoder,
        &view,
        viewport,
        viewport.quiet_input(),
        |ui| panel(ui, 32.0, 32.0, egui::Color32::from_rgb(0, 255, 0)),
    );

    let doubled = shot::read_back(&gpu, encoder, &texture, SIZE, SIZE).expect("кадр мав читатися");

    // 32 точки при масштабі 2 — це 64 пікселі. Дивимось у піксель 40: він
    // усередині подвоєної панелі й поза одинарною.
    let inside = doubled.pixel(40, 40);
    assert!(
        inside[1] > inside[0] && inside[1] > inside[2],
        "піксель (40, 40) мав бути всередині подвоєної панелі, а вийшло {inside:?}"
    );

    let single = draw_with_ui(&gpu, SIZE, SIZE, |ui| {
        panel(ui, 32.0, 32.0, egui::Color32::from_rgb(0, 255, 0))
    });
    let same_pixel = single.pixel(40, 40);
    assert_ne!(
        [inside[0], inside[1], inside[2]],
        [same_pixel[0], same_pixel[1], same_pixel[2]],
        "масштаб нічого не змінив — 32 точки лишились 32 пікселями"
    );
}
