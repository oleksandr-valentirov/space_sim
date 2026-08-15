//! Палітра доїжджає до пікселів, і обидва шляхи дають той самий колір
//! (ROADMAP-UI.md, U7c).
//!
//! Тести всередині `palette` перевіряють числа: контраст рахується формулою,
//! акцент дорівнює кольору прогнозу, панель темніша за небо. Жоден із них не
//! доводить, що ці числа **доходять до екрана**, — а між «константа правильна»
//! і «піксель такий» лежить увесь `egui`, увесь `egui-wgpu` і формат цілі.
//!
//! ## Твердження, заради якого цей файл існує
//!
//! Палітра обіцяє **один колірний простір**: `Colour::scene` ділить байт на
//! 255 для ламаної, `Colour::egui` віддає той самий байт віджету, і жодної
//! гамми ніде. Обіцянка перевірна рівно одним способом — намалювати той самий
//! колір обома шляхами в одну текстуру й прочитати байти. Якщо `egui-wgpu`
//! десь робить перетворення, якого не робить наш шейдер, кольори розійдуться,
//! і «однакова палітра» виявиться двома різними.
//!
//! Це не косметика: саме на цьому стоїть рішення, що акцент інтерфейсу — той
//! самий бурштин, що лінія прогнозу. Розійшлися байти — розійшовся зміст.

use engine::gpu::Gpu;
use engine::shot::{self, Shot};
use engine::ui::{Ui, Viewport};
use engine::{egui, frame};

use game::palette;

const SIZE: u32 = 128;

fn gpu() -> Option<Gpu> {
    Gpu::for_tests()
}

/// Кадр із самою лише панеллю egui поверх звичайного неба.
///
/// Сцена тут порожня навмисно: перевіряється колір інтерфейсу, і планета в
/// кадрі лише додала б пікселів, які нічого не кажуть.
fn ui_shot(gpu: &Gpu, build: impl FnMut(&mut egui::Ui)) -> Shot {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("palette shot"),
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
            label: Some("palette shot"),
        });

    // Сцена — щоб під панеллю було небо, тобто те саме тло, що в грі.
    let mut scene_frame = frame::Frame::new(gpu, shot::FORMAT);
    let scene = engine::scene::Scene::new(frame::default_camera());
    scene_frame.draw(gpu, &mut encoder, &view, SIZE, SIZE, &scene);

    let mut interface = Ui::new(gpu, shot::FORMAT);
    palette::apply(interface.context());
    let viewport = Viewport::new(SIZE, SIZE, 1.0);
    interface.draw(
        gpu,
        &mut encoder,
        &view,
        viewport,
        viewport.quiet_input(),
        build,
    );

    shot::read_back(gpu, encoder, &texture, SIZE, SIZE).expect("кадр мав прочитатися назад")
}

/// Прямокутник заданого кольору в лівому верхньому куті.
fn patch(ui: &mut egui::Ui, colour: egui::Color32) {
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE as f32, SIZE as f32));
    ui.painter().rect_filled(rect, 0.0, colour);
}

/// Колір із палітри доїжджає в піксель тим самим байтом.
///
/// Найпряміше твердження кроку: `Colour::egui` не міняє числа по дорозі.
#[test]
fn a_colour_from_the_palette_lands_in_the_pixel_unchanged() {
    let Some(gpu) = gpu() else { return };

    for colour in [
        palette::ACCENT,
        palette::HISTORY,
        palette::PREVIEW,
        palette::PANEL,
        palette::ALARM,
    ] {
        let shot = ui_shot(&gpu, |ui| patch(ui, colour.egui()));
        let pixel = shot.pixel(SIZE / 2, SIZE / 2);

        assert_eq!(
            [pixel[0], pixel[1], pixel[2]],
            [colour.0, colour.1, colour.2],
            "колір {colour:?} приїхав у піксель як {:?} — egui-wgpu перетворює \
             його по дорозі, і палітра не є одним простором",
            [pixel[0], pixel[1], pixel[2]]
        );
    }
}

/// Той самий колір, покладений у сцену й в інтерфейс, дає ті самі байти.
///
/// Це вже не про egui, а про **обидва шляхи разом**: ламана йде нашим
/// шейдером у `[f32; 4]`, панель — шейдером egui у `Color32`, і зустрічаються
/// вони в одній текстурі. Тест вище пройшов би й тоді, коли сцена малює той
/// самий бурштин помітно іншим.
#[test]
fn the_same_colour_through_the_scene_and_the_interface_matches() {
    let Some(gpu) = gpu() else { return };

    let colour = palette::ACCENT;

    // Шлях інтерфейсу.
    let through_ui = ui_shot(&gpu, |ui| patch(ui, colour.egui()));
    let from_ui = through_ui.pixel(SIZE / 2, SIZE / 2);

    // Шлях сцени: ламана в кольорі палітри, впоперек усього кадру.
    //
    // Товщина лінії — справа рушія, тож шукається не конкретний піксель, а
    // будь-який, який не є небом: питання тесту в тому, ЯКИЙ це колір, а не
    // де саме він лежить.
    // Тіла в сцені немає навмисно: планета сховала б лінію за собою, а фон із
    // самого лише неба робить пошук «першого не-неба» однозначним.
    //
    // Камера стоїть на осі X і дивиться в початок координат, тож лінія
    // кладеться на півдорозі перед нею й поперек погляду — по Y.
    let mut scene = engine::scene::Scene::new(frame::default_camera());
    let camera = scene.camera.position();
    let across = |k: f64| [camera[0] * 0.5, camera[0] * 0.2 * k, 0.0];
    scene.polylines.push(engine::scene::Polyline {
        points: vec![across(-1.0), across(1.0)],
        colour: colour.scene(),
    });
    let through_scene = shot::take_scene(&gpu, SIZE, SIZE, &scene).expect("кадр сцени");

    let mut from_scene = None;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let p = through_scene.pixel(x, y);
            if [p[0], p[1], p[2]] != frame::CLEAR_BYTES {
                from_scene = Some([p[0], p[1], p[2]]);
                break;
            }
        }
        if from_scene.is_some() {
            break;
        }
    }
    let from_scene = from_scene.expect("ламана мала намалюватись хоч одним пікселем");

    assert_eq!(
        [from_ui[0], from_ui[1], from_ui[2]],
        from_scene,
        "той самий колір палітри дав {:?} в інтерфейсі й {from_scene:?} у сцені — \
         тобто один із двох шляхів застосовує гамму, і акцент панелі перестав \
         бути кольором лінії прогнозу",
        [from_ui[0], from_ui[1], from_ui[2]]
    );
}

/// І перевірка, що перевірка вміє провалитися: інший колір дає інші байти.
///
/// Без неї два тести вище були б зелені й на цілі, яка все зафарбовує однією
/// константою.
#[test]
fn two_different_colours_do_not_land_on_the_same_pixel_value() {
    let Some(gpu) = gpu() else { return };

    let accent = ui_shot(&gpu, |ui| patch(ui, palette::ACCENT.egui()));
    let history = ui_shot(&gpu, |ui| patch(ui, palette::HISTORY.egui()));

    assert_ne!(
        accent.pixel(SIZE / 2, SIZE / 2),
        history.pixel(SIZE / 2, SIZE / 2),
        "бурштин і синій дали той самий піксель"
    );
}
