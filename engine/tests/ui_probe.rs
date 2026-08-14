//! Розвідка U1a: чи малює `egui-wgpu` у звичайну текстуру без вікна.
//!
//! Від цієї відповіді залежить уся перевірка етапу U (ROADMAP-UI.md, правило
//! 3): якщо інтерфейс можна намалювати лише в поверхню вікна, то жодна панель
//! ніколи не потрапить у знімок, і «UI перевіряється без вікна» доведеться
//! викреслити разом із половиною оракулів.
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
use engine::egui_wgpu;
use engine::frame::{self, Frame};
use engine::gpu::Gpu;
use engine::scene::Scene;
use engine::shot::{self, Shot};

const SIZE: u32 = 256;

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

/// Кадр сцени, а поверх нього — те, що намалює `build` в egui.
///
/// Порядок саме такий, як вимагає U1b: прохід egui **останній**, у ту саму
/// текстуру, `load` замість `clear`, без глибини. Кадр малює сцену, потім
/// поверх неї — інтерфейс.
fn draw_with_ui(gpu: &Gpu, build: impl FnMut(&mut egui::Ui)) -> Shot {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ui probe"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
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

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ui probe"),
        });

    let mut scene_frame = Frame::new(gpu, shot::FORMAT);
    let scene = Scene::new(frame::default_camera());
    scene_frame.draw(gpu, &mut encoder, &view, SIZE, SIZE, &scene);

    // Ввід синтетичний: жодного вікна, жодного `egui-winit`. Саме так етап U
    // і збирається перевіряти кліки — правило 3.
    let context = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(SIZE as f32, SIZE as f32),
        )),
        ..Default::default()
    };

    let output = context.run_ui(input, build);
    let primitives = context.tessellate(output.shapes, output.pixels_per_point);

    let mut renderer = egui_wgpu::Renderer::new(
        &gpu.device,
        shot::FORMAT,
        egui_wgpu::RendererOptions {
            msaa_samples: 1,
            depth_stencil_format: None,
            // Дизеринг додав би шум у пікселі, тобто зробив би «бітово те
            // саме» недосяжним навіть там, де нічого не намальовано.
            dithering: false,
            predictable_texture_filtering: true,
        },
    );

    // Одна текстура може приїхати кількома клаптями за кадр — звідси
    // внутрішній цикл; egui так довантажує атлас шрифта частинами.
    //
    // І це треба робити навіть тоді, коли не намальовано нічого: порожній
    // контекст усе одно віддає атлас шрифта, а `TexturesDelta` падає в
    // `Drop`, якщо її не застосували. Тобто «нічого не намальовано» і
    // «нічого не приїхало» — різні речі, і egui наполягає на різниці.
    let mut deltas = output.textures_delta;
    for (id, patches) in &deltas.set {
        for patch in patches {
            renderer.update_texture(&gpu.device, &gpu.queue, *id, patch);
        }
    }

    let descriptor = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [SIZE, SIZE],
        pixels_per_point: 1.0,
    };
    renderer.update_buffers(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        &primitives,
        &descriptor,
    );

    {
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("egui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            multiview_mask: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        renderer.render(&mut pass.forget_lifetime(), &primitives, &descriptor);
    }

    for id in &deltas.free {
        renderer.free_texture(id);
    }
    deltas.clear();

    shot::read_back(gpu, encoder, &texture, SIZE, SIZE).expect("кадр мав прочитатися назад")
}

/// Кадр без жодного проходу egui — те, що малює рушій сьогодні.
fn draw_plain(gpu: &Gpu) -> Shot {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ui probe: без egui"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
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

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ui probe: без egui"),
        });

    let mut scene_frame = Frame::new(gpu, shot::FORMAT);
    let scene = Scene::new(frame::default_camera());
    scene_frame.draw(gpu, &mut encoder, &view, SIZE, SIZE, &scene);

    shot::read_back(gpu, encoder, &texture, SIZE, SIZE).expect("кадр мав прочитатися назад")
}

/// Порожній інтерфейс не міняє кадру жодним бітом.
#[test]
fn an_empty_context_changes_nothing() {
    let Some(gpu) = gpu() else { return };

    let plain = draw_plain(&gpu);
    let with_ui = draw_with_ui(&gpu, |_| {});

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

    let plain = draw_plain(&gpu);
    let with_ui = draw_with_ui(&gpu, |ui| {
        let rect = egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(SIZE as f32 / 2.0, SIZE as f32 / 2.0),
        );
        ui.painter()
            .rect_filled(rect, 0.0, egui::Color32::from_rgb(255, 0, 255));
    });

    // Усередині панелі: пурпуровий поверх усього, що там було.
    let inside = with_ui.pixel(SIZE / 4, SIZE / 4);
    assert_ne!(
        [inside[0], inside[1], inside[2]],
        {
            let p = plain.pixel(SIZE / 4, SIZE / 4);
            [p[0], p[1], p[2]]
        },
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
