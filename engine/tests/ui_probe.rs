//! The interface in the frame: U1a's reconnaissance and U1b's wiring.
//!
//! The question the whole verification of stage U stands on (ROADMAP-UI.md,
//! rule 3): does `egui-wgpu` draw into an ordinary texture without a window. If
//! not, no panel will ever reach a screenshot, and "the UI is checked without a
//! window" would have to be struck out along with half the stage's oracles.
//!
//! There are two statements here, and **both are mandatory**:
//!
//! 1. an empty `egui::Context` does not change the frame by **a single bit** --
//!    nothing was drawn, so nothing changed either;
//! 2. a non-empty one does change it, and exactly where it drew.
//!
//! The first without the second would pass on a completely broken `egui-wgpu`
//! that never draws anything at all. This is the same "both sides" pair events
//! in `/core` are measured with, and the reason is the same: a check that
//! cannot fail is green for reasons other than the code working.

use engine::egui;
use engine::frame::{self, Frame};
use engine::gpu::Gpu;
use engine::shot::{self, Shot};
use engine::ui::{Ui, Viewport};

const SIZE: u32 = 256;

/// The size from U1b's check -- the one the frame time is measured at too.
const WIDE: u32 = 1280;
const TALL: u32 = 720;

fn gpu() -> Option<Gpu> {
    // The engine's shared helper: it also decides whether skipping is allowed
    // (`SPACE_SIM_REQUIRE_GPU`, U6c) and prints the adapter name into the log.
    Gpu::for_tests()
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

/// A frame of the scene, and on top of it whatever `build` draws.
///
/// The order is exactly the one U1b sets: scene, then interface, into one
/// texture.
fn draw_with_ui(gpu: &Gpu, width: u32, height: u32, build: impl FnMut(&mut egui::Ui)) -> Shot {
    let (texture, view) = target(gpu, width, height);
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ui probe"),
        });

    let mut scene_frame = Frame::new(gpu, shot::FORMAT);
    let scene = frame::default_scene(frame::default_camera());
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

    shot::read_back(gpu, encoder, &texture, width, height)
        .expect("the frame should have been read back")
}

/// The same frame without any egui pass -- what the engine draws today.
fn draw_plain(gpu: &Gpu, width: u32, height: u32) -> Shot {
    let (texture, view) = target(gpu, width, height);
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ui probe: no egui"),
        });

    let mut scene_frame = Frame::new(gpu, shot::FORMAT);
    let scene = frame::default_scene(frame::default_camera());
    scene_frame.draw(gpu, &mut encoder, &view, width, height, &scene);

    shot::read_back(gpu, encoder, &texture, width, height)
        .expect("the frame should have been read back")
}

/// A rectangle in the top-left corner, in pixels of the target.
fn panel(ui: &mut egui::Ui, width: f32, height: f32, colour: egui::Color32) {
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
    ui.painter().rect_filled(rect, 0.0, colour);
}

/// An empty interface does not change the frame by a single bit.
#[test]
fn an_empty_context_changes_nothing() {
    let Some(gpu) = gpu() else { return };

    let plain = draw_plain(&gpu, SIZE, SIZE);
    let with_ui = draw_with_ui(&gpu, SIZE, SIZE, |_| {});

    assert_eq!(
        plain.pixels, with_ui.pixels,
        "an egui pass with no widget at all moved pixels -- meaning it draws \
         something of its own, and every future screenshot comparison would be \
         measuring that"
    );
}

/// And a non-empty one does change it, exactly where it drew.
///
/// The panel is nailed to the top-left corner at a fixed size, so the check
/// knows which pixel is obliged to change and which is obliged to stay. Without
/// the second half this would be a test of "something somewhere became
/// different".
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
        "the pixel inside the panel did not change -- egui-wgpu drew nothing"
    );
    // What is compared is which channel dominates, not the exact colour: the
    // screenshot target is linear, the window surface sRGB, and the same colour
    // gives different bytes in them (ROADMAP, "Рендер").
    assert!(
        inside[0] > inside[1] && inside[2] > inside[1],
        "red and blue should have dominated inside the panel, but it came out {inside:?}"
    );

    // Outside the panel the frame stayed the same -- the egui pass touched
    // nothing beyond its own scissor.
    for (x, y) in [(SIZE - 2, SIZE - 2), (SIZE - 2, 1), (1, SIZE - 2)] {
        assert_eq!(
            plain.pixel(x, y),
            with_ui.pixel(x, y),
            "pixel ({x}, {y}) outside the panel changed"
        );
    }
}

/// U1b's check verbatim: 1280x720, one panel, a pixel inside it is the panel's,
/// a pixel outside it is sky.
///
/// "Sky" here is not just anything else but exactly [`frame::CLEAR_BYTES`]: the
/// corner of the frame at the default altitude lies outside the planet's disc
/// (already measured in `shot.rs`), so it must hold the clear colour and
/// nothing more. The panel is green -- a channel neither the background nor the
/// planet has.
#[test]
fn a_panel_covers_the_sky_and_only_it() {
    let Some(gpu) = gpu() else { return };

    let with_ui = draw_with_ui(&gpu, WIDE, TALL, |ui| {
        panel(ui, 300.0, 200.0, egui::Color32::from_rgb(0, 255, 0));
    });

    let inside = with_ui.pixel(150, 100);
    assert!(
        inside[1] > inside[0] && inside[1] > inside[2],
        "green should have dominated inside the panel, but it came out {inside:?}"
    );

    let outside = with_ui.pixel(WIDE - 2, 2);
    assert_eq!(
        [outside[0], outside[1], outside[2]],
        frame::CLEAR_BYTES,
        "the pixel outside the panel should have stayed sky"
    );
}

/// Scale is scale, not a change of the target's size.
///
/// The same panel in points at `scale = 2.0` takes twice as many pixels. The
/// check is cheap, and it catches a bug that is otherwise found by eye on a
/// high-DPI screen -- and only by someone who has one.
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
    let scene = frame::default_scene(frame::default_camera());
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

    let doubled =
        shot::read_back(&gpu, encoder, &texture, SIZE, SIZE).expect("the frame should have read");

    // 32 points at scale 2 make 64 pixels. We look at pixel 40: it is inside the
    // doubled panel and outside the single one.
    let inside = doubled.pixel(40, 40);
    assert!(
        inside[1] > inside[0] && inside[1] > inside[2],
        "pixel (40, 40) should have been inside the doubled panel, but it came out {inside:?}"
    );

    let single = draw_with_ui(&gpu, SIZE, SIZE, |ui| {
        panel(ui, 32.0, 32.0, egui::Color32::from_rgb(0, 255, 0))
    });
    let same_pixel = single.pixel(40, 40);
    assert_ne!(
        [inside[0], inside[1], inside[2]],
        [same_pixel[0], same_pixel[1], same_pixel[2]],
        "the scale changed nothing -- 32 points stayed 32 pixels"
    );
}
