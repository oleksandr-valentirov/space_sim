//! The palette reaches the pixels, and both paths give the same colour
//! (ROADMAP-UI.md, U7c).
//!
//! The tests inside `palette` check numbers: contrast follows the formula,
//! the accent equals the forecast colour, the panel is darker than the sky.
//! None of them proves those numbers **reach the screen** -- between "the
//! constant is right" and "the pixel is this" lies all of `egui`, all of
//! `egui-wgpu` and the target format.
//!
//! The claim this file exists for: the palette promises **one colour space**.
//! `Colour::scene` divides the byte by 255 for a polyline, `Colour::egui`
//! hands the same byte to the widget, and there is no gamma anywhere. The
//! only way to check that is to draw the same colour both ways into one
//! texture and read the bytes back. This is not cosmetics: the decision that
//! the interface accent is the same amber as the forecast line rests on it.

use engine::gpu::Gpu;
use engine::shot::{self, Shot};
use engine::ui::{Ui, Viewport};
use engine::{egui, frame};

use game::palette;

const SIZE: u32 = 128;

fn gpu() -> Option<Gpu> {
    Gpu::for_tests()
}

/// A frame holding nothing but an egui panel over the ordinary sky.
///
/// The scene is empty on purpose: what is checked is the interface colour,
/// and a planet would only add pixels that say nothing.
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

    // A scene, so that the panel sits over sky -- the same backdrop as in game.
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

    shot::read_back(gpu, encoder, &texture, SIZE, SIZE).expect("the frame should read back")
}

/// A rectangle of the given colour in the top left corner.
fn patch(ui: &mut egui::Ui, colour: egui::Color32) {
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE as f32, SIZE as f32));
    ui.painter().rect_filled(rect, 0.0, colour);
}

/// A palette colour reaches the pixel as the same byte.
///
/// The most direct claim of the step: `Colour::egui` does not alter the
/// number on the way.
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
            "colour {colour:?} landed in the pixel as {:?} -- egui-wgpu converts \
             it on the way, and the palette is not one space",
            [pixel[0], pixel[1], pixel[2]]
        );
    }
}

/// The same colour put into the scene and into the interface gives the same
/// bytes.
///
/// This is not about egui but about **both paths together**: the polyline
/// goes through our shader as `[f32; 4]`, the panel through egui's shader as
/// `Color32`, and they meet in one texture. The test above would pass even if
/// the scene drew that amber noticeably differently.
#[test]
fn the_same_colour_through_the_scene_and_the_interface_matches() {
    let Some(gpu) = gpu() else { return };

    let colour = palette::ACCENT;

    // The interface path.
    let through_ui = ui_shot(&gpu, |ui| patch(ui, colour.egui()));
    let from_ui = through_ui.pixel(SIZE / 2, SIZE / 2);

    // The scene path: a polyline in the palette colour, right across the frame.
    //
    // Line width is the engine's business, so the search is for any pixel that
    // is not sky rather than for a particular one: the question is WHICH
    // colour, not where it lies. There is no body in the scene on purpose --
    // a planet would hide the line, and a sky-only backdrop makes "the first
    // non-sky pixel" unambiguous.
    //
    // The camera sits on the X axis looking at the origin, so the line is laid
    // half way in front of it and across the view, along Y.
    let mut scene = engine::scene::Scene::new(frame::default_camera());
    let camera = scene.camera.position();
    let across = |k: f64| [camera[0] * 0.5, camera[0] * 0.2 * k, 0.0];
    scene.polylines.push(engine::scene::Polyline {
        points: vec![across(-1.0), across(1.0)],
        colour: colour.scene(),
    });
    let through_scene = shot::take_scene(&gpu, SIZE, SIZE, &scene).expect("scene frame");

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
    let from_scene = from_scene.expect("the polyline should have drawn at least one pixel");

    assert_eq!(
        [from_ui[0], from_ui[1], from_ui[2]],
        from_scene,
        "the same palette colour gave {:?} in the interface and {from_scene:?} in \
         the scene -- one of the two paths applies gamma, and the panel accent \
         has stopped being the colour of the forecast line",
        [from_ui[0], from_ui[1], from_ui[2]]
    );
}

/// And a check that the check can fail: a different colour gives different
/// bytes. Without it the two tests above would be green on a target that
/// paints everything with one constant.
#[test]
fn two_different_colours_do_not_land_on_the_same_pixel_value() {
    let Some(gpu) = gpu() else { return };

    let accent = ui_shot(&gpu, |ui| patch(ui, palette::ACCENT.egui()));
    let history = ui_shot(&gpu, |ui| patch(ui, palette::HISTORY.egui()));

    assert_ne!(
        accent.pixel(SIZE / 2, SIZE / 2),
        history.pixel(SIZE / 2, SIZE / 2),
        "amber and blue gave the same pixel"
    );
}
