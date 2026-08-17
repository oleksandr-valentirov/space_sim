//! Input has an owner every frame (ROADMAP-UI.md, U1c, rule 4).
//!
//! The statement is checked from **both sides**, and that is not a formality: a
//! test only for "a drag in the panel does not turn the camera" would pass on a
//! camera that does not move at all.
//!
//! There is no window here -- as everywhere in this stage. `egui-winit`
//! assembles `RawInput` from winit events, but `RawInput` itself is an ordinary
//! struct, so a click in a test is made by hand.
//!
//! ## What the measurement established
//!
//! `egui_wants_pointer_input()` is `is_using_pointer() || is_pointer_over_egui()`,
//! and the first half is **sticky**: until the mouse button is released the
//! interface considers the mouse its own, even when the cursor has left the
//! panel. That is not a defect but what one wants: start dragging a slider and
//! you keep dragging it, wherever the hand goes. But a test that presses the
//! button every frame and never releases it will get `true` anywhere on screen
//! -- and that is exactly what the first version of this file looked like.

use engine::egui;
use engine::gpu::Gpu;
use engine::orbit::Orbit;
use engine::shot;
use engine::ui::{Ui, Viewport};

const SIZE: u32 = 256;

/// The side panel takes the left quarter of the screen. A panel specifically,
/// not a window: a panel's geometry is exact, while a window shrinks to its
/// contents -- and a check "50 pixels to the side" would then measure something
/// other than what its reader thinks.
const PANEL: f32 = 128.0;

/// A point on the slider -- the panel's second widget, roughly in the middle of
/// its track. The button is above it and does not hold a drag.
const SLIDER: egui::Pos2 = egui::Pos2::new(60.0, 45.0);

fn gpu() -> Option<Gpu> {
    // The engine's shared helper: it also decides whether skipping is allowed
    // (`SPACE_SIM_REQUIRE_GPU`, U6c) and prints the adapter name into the log.
    Gpu::for_tests()
}

/// What the mouse does this frame.
enum Mouse {
    /// The cursor is simply there.
    Hover,
    /// Pressed and released -- a full click, with no sticky state after it.
    Click,
    /// Pressed and held: exactly the state in which the owner is sticky.
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

/// One interface frame with a side panel. Returns whether the interface took
/// the mouse.
///
/// `slider` lives outside the frame because a slider is state: it is what makes
/// a drag sticky, and without it the stickiness check would measure nothing.
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
            egui::Panel::left("panel")
                .exact_size(PANEL)
                .resizable(false)
                .show(ui, |ui| {
                    // Real widgets, not a painted rectangle: the question
                    // "whose event is this" is asked of what the player
                    // interacts with. The slider is not here for looks -- it is
                    // the only one able to hold a drag longer than one frame.
                    let _ = ui.button("pause");
                    let _ = ui.add(egui::Slider::new(slider, 0.0..=1.0));
                });
        },
    );

    gpu.queue.submit([encoder.finish()]);
    ui.wants_pointer()
}

/// The panel takes the mouse over itself and does not take it outside itself.
#[test]
fn the_interface_takes_the_pointer_only_over_itself() {
    let Some(gpu) = gpu() else { return };
    let mut ui = Ui::new(&gpu, shot::FORMAT);
    let mut slider = 0.5;

    // A warm-up frame: egui knows widget sizes only after drawing them once, so
    // in the first frame the panel does not yet know where it is.
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
        "the cursor is in the panel and the interface does not want the mouse \
         -- the game would turn the camera on top of its own button"
    );
    assert!(
        !owner_asks(
            &gpu,
            &mut ui,
            egui::pos2(PANEL + 50.0, 20.0),
            Mouse::Hover,
            &mut slider
        ),
        "the cursor is 50 pixels away from the panel and the interface took the \
         mouse anyway -- the camera would never rotate"
    );
}

/// A drag started in the panel stays with it, even when the cursor has left it.
///
/// This is not a side effect but what one wants: a slider must not get lost
/// under the hand. The check exists so that the stickiness is a **measured**
/// property rather than a surprise that will one day explain itself.
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

    // On the slider specifically, not on the button: a button does not hold a
    // drag, and "stickiness" would not appear on it even in correct code.
    assert!(
        owner_asks(&gpu, &mut ui, SLIDER, Mouse::Hold, &mut slider),
        "a press on the slider should have belonged to the panel"
    );
    assert!(
        owner_asks(
            &gpu,
            &mut ui,
            egui::pos2(PANEL + 50.0, 20.0),
            Mouse::Hover,
            &mut slider
        ),
        "the cursor left the panel with the button held down, and the drag was lost"
    );
}

/// The camera turns only when the interface did not take the event.
///
/// Both statements are mandatory -- that the camera stands still when the owner
/// is the interface, and that it moves when the owner is the world.
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

    // A click in the panel: the owner is the interface, the world does not see
    // the event.
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
        "a drag in the panel turned the camera"
    );

    // The same click to the side: the owner is the world.
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
        "a drag outside the panel did not move the camera"
    );
}
