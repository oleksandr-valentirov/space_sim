//! The interface wiring (ROADMAP-UI.md, U1b). The game writes widgets, not the
//! engine.
//!
//! ## Why this does not break "the engine does not know about the game"
//!
//! `egui::Context` is a library type, as third-party as `wgpu::Device`. There
//! is no vessel here, no plan and no time: [`Ui::draw`] takes a closure that
//! draws **something** and does not ask what. The direction stays
//! `game -> engine`.
//!
//! ## Why there is no window here either
//!
//! Input arrives as an [`egui::RawInput`] **from outside** rather than being
//! collected inside: in a window `egui-winit` provides it (U1c), in a test a
//! synthetic struct does. The same decision that already holds for the frame:
//! `engine::frame` writes into a texture and knows nothing about a surface. A
//! panel with correct numbers and a panel with NaN look the same until someone
//! looks at them -- so looking must be done with a shot, and a shot has no
//! window.
//!
//! ## Pass order
//!
//! The egui pass is **last**, into the same texture, `load` instead of `clear`,
//! with no depth: the frame draws the scene, then the interface on top of it.
//! Depth is not needed at all here, because widget order is set by egui's own
//! tessellation rather than by a z-buffer.

use crate::gpu::Gpu;

/// Where we draw: target size in pixels and the interface scale.
///
/// The three together rather than three arguments in a row, and the reason is
/// simple: two consecutive `u32`s get swapped silently, and a 720x1280 frame
/// looks like an error only on a wide screen. The texture is not part of it --
/// a struct holding a reference would need a lifetime (CLAUDE.md, Rust
/// style).
#[derive(Clone, Copy)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    /// Pixels per point. A window takes it from `scale_factor`, a shot uses
    /// 1.0.
    pub scale: f32,
}

impl Viewport {
    pub fn new(width: u32, height: u32, scale: f32) -> Viewport {
        Viewport {
            width,
            height,
            scale,
        }
    }

    /// Input in which nothing happens.
    ///
    /// Needed by everyone who draws the interface without a window: shots,
    /// tests, probes. Lives here rather than in a test because `screen_rect` is
    /// given **in points**, not in pixels -- exactly the conversion every caller
    /// would do its own way.
    pub fn quiet_input(&self) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(
                    self.width as f32 / self.scale,
                    self.height as f32 / self.scale,
                ),
            )),
            ..Default::default()
        }
    }
}

/// The egui context and its renderer. One per target: the pipeline is tied to
/// the format, exactly as in [`crate::frame::Frame`] and for the same
/// reason.
pub struct Ui {
    context: egui::Context,
    renderer: egui_wgpu::Renderer,
}

impl Ui {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Ui {
        Ui {
            context: egui::Context::default(),
            renderer: egui_wgpu::Renderer::new(
                &gpu.device,
                format,
                egui_wgpu::RendererOptions {
                    msaa_samples: 1,
                    // No depth in the interface -- tessellation sets the
                    // order.
                    depth_stencil_format: None,
                    // Dithering adds noise to pixels, making "bitwise the
                    // same" unreachable where nothing was drawn (U1a).
                    dithering: false,
                    predictable_texture_filtering: true,
                },
            ),
        }
    }

    /// The context is for whoever collects input (`egui-winit` in U1c). Nobody
    /// else needs it: everything that is drawn is drawn in [`Ui::draw`].
    pub fn context(&self) -> &egui::Context {
        &self.context
    }

    /// Whether the interface took the mouse this frame (ROADMAP-UI.md, U1c).
    ///
    /// Asked **after** [`Ui::draw`]: the answer depends on what was drawn and
    /// where the cursor is, and both are known only then. One place, one order
    /// -- rule 4 of the stage forbids spreading this check across handlers.
    pub fn wants_pointer(&self) -> bool {
        self.context.egui_wants_pointer_input()
    }

    /// The same for the keyboard: a focused text field eats keystrokes, and
    /// the game must not see "w" as a command while the player types a
    /// number.
    pub fn wants_keyboard(&self) -> bool {
        self.context.egui_wants_keyboard_input()
    }

    /// Whether the cursor is over an egui area at all.
    ///
    /// Broader than [`Ui::wants_pointer`]: that one says "egui is using the
    /// mouse", this one "the mouse is over a panel". The U1c fork called this
    /// variant the fallback; which of them is right is decided by measurement,
    /// not taste.
    pub fn pointer_over_panel(&self) -> bool {
        self.context.is_pointer_over_egui()
    }

    /// Draws the interface on top of an already drawn frame.
    ///
    /// Returns an [`egui::PlatformOutput`] -- what egui asks the **platform** to
    /// do: change the cursor, put something in the clipboard. The engine does
    /// not do that itself, because that is already about a window; for a shot it
    /// simply disappears, and that is right.
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        viewport: Viewport,
        input: egui::RawInput,
        build: impl FnMut(&mut egui::Ui),
    ) -> egui::PlatformOutput {
        self.context.set_pixels_per_point(viewport.scale);

        let output = self.context.run_ui(input, build);
        let primitives = self
            .context
            .tessellate(output.shapes, output.pixels_per_point);

        // Textures arrive even when nothing was drawn: the font atlas is a
        // texture too (U1a). One can arrive in several patches, hence the inner
        // loop.
        let mut deltas = output.textures_delta;
        for (id, patches) in &deltas.set {
            for patch in patches {
                self.renderer
                    .update_texture(&gpu.device, &gpu.queue, *id, patch);
            }
        }

        let descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [viewport.width, viewport.height],
            pixels_per_point: viewport.scale,
        };
        self.renderer
            .update_buffers(&gpu.device, &gpu.queue, encoder, &primitives, &descriptor);

        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // `Load` on purpose: the scene is already drawn, and
                        // the interface pass does not erase it.
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                multiview_mask: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.renderer
                .render(&mut pass.forget_lifetime(), &primitives, &descriptor);
        }

        for id in &deltas.free {
            self.renderer.free_texture(id);
        }
        // `TexturesDelta` panics in `Drop` if it was not applied -- and that is
        // useful: a silently skipped texture would look like vanished text.
        deltas.clear();

        output.platform_output
    }
}

/// Input collector from a window (ROADMAP-UI.md, U2b).
///
/// A wrapper over `egui-winit`, and that is why the game does not see it: the
/// interface comes from the engine whole -- both drawing and input -- otherwise
/// the game would gain a second dependency on the same library, and with it a
/// way to end up with two versions of it.
///
/// Here too is the boundary of knowledge: `WindowInput` knows about a window,
/// [`Ui`] does not. That is why a shot without a window stays possible even
/// after a window appeared.
pub struct WindowInput {
    state: egui_winit::State,
}

impl WindowInput {
    pub fn new(ui: &Ui, window: &winit::window::Window) -> WindowInput {
        WindowInput {
            state: egui_winit::State::new(
                ui.context().clone(),
                egui::ViewportId::ROOT,
                window,
                Some(window.scale_factor() as f32),
                None,
                None,
            ),
        }
    }

    /// Hands the event to egui and says whether it was consumed.
    ///
    /// `true` means "the game does not see this event". It must be asked
    /// **before** the event goes on, and in exactly one place -- rule 4.
    ///
    /// The one exception is Tab, and it is not a preference: see
    /// [`tab_is_the_games`]. The event is withheld from egui entirely in that
    /// case rather than merely reported as unconsumed, because egui's use for
    /// it is to move focus -- and a focused widget would then want the
    /// keyboard, so the *next* Tab would be eaten for a reason that only the
    /// first one created.
    pub fn on_window_event(
        &mut self,
        window: &winit::window::Window,
        event: &winit::event::WindowEvent,
    ) -> bool {
        if let winit::event::WindowEvent::KeyboardInput { event: key, .. } = event {
            if tab_is_the_games(
                &key.logical_key,
                self.state.egui_ctx().egui_wants_keyboard_input(),
            ) {
                return false;
            }
        }
        self.state.on_window_event(window, event).consumed
    }

    /// Input accumulated since the previous frame.
    pub fn take(&mut self, window: &winit::window::Window) -> egui::RawInput {
        self.state.take_egui_input(window)
    }

    /// What egui asks the platform to do: cursor, clipboard.
    pub fn apply(&mut self, window: &winit::window::Window, output: egui::PlatformOutput) {
        self.state.handle_platform_output(window, output);
    }
}

/// Whether a Tab belongs to the game rather than to egui (stage X, X1).
///
/// **egui reports Tab as consumed unconditionally** -- not because it wants the
/// keyboard, but because Tab moves its focus to the first focusable widget
/// (`egui-winit 0.36.1`, `lib.rs:416`). Taking that at face value cost the game
/// its camera switch entirely: the `Tab` arm added for debt D12 could never
/// run, in any window, from the day it was written.
///
/// It is a free function, and separately testable, because that is the only way
/// it can be tested at all: `WindowInput` needs a real window, and a
/// `winit::event::KeyEvent` cannot be built outside winit -- its
/// `platform_specific` field is not public. So the policy gets the oracle and
/// the plumbing stays one line. A test through a synthetic `WindowEvent` would
/// be the better oracle and is not available.
pub fn tab_is_the_games(key: &winit::keyboard::Key, wants_keyboard: bool) -> bool {
    // Once something in the interface is genuinely typing, Tab is its own again
    // -- there is no text field in the HUD today, so this branch is unreachable
    // for now, and it stays because the first one to appear must not have to
    // rediscover this.
    !wants_keyboard && *key == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Tab)
}

#[cfg(test)]
mod tests {
    use super::tab_is_the_games;
    use winit::keyboard::{Key, NamedKey};

    /// The defect X1 fixed: a Tab with nobody typing is the game's.
    #[test]
    fn a_tab_reaches_the_game_when_nothing_is_typing() {
        assert!(tab_is_the_games(&Key::Named(NamedKey::Tab), false));
    }

    /// And it is not, the moment the interface is actually reading keys.
    #[test]
    fn a_tab_stays_with_the_interface_while_it_types() {
        assert!(!tab_is_the_games(&Key::Named(NamedKey::Tab), true));
    }

    /// Every other key keeps going through egui's own answer -- the exception
    /// is Tab alone, not "keys the game likes".
    #[test]
    fn no_other_key_takes_the_exception() {
        for key in [
            Key::Named(NamedKey::Space),
            Key::Named(NamedKey::Enter),
            Key::Named(NamedKey::Escape),
            Key::Character("p".into()),
        ] {
            assert!(!tab_is_the_games(&key, false), "{key:?} took the exception");
        }
    }
}
