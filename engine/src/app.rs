//! The engine window: an event loop for probes (ROADMAP F1, I2).
//!
//! The frame comes from [`crate::frame`], the same one that goes into a shot.
//! The surface and everything that happens to it live in [`crate::window`] --
//! since J1 there are two event loops, and the surface is exactly what must
//! not be duplicated.
//!
//! What is drawn here is a scene with the planet alone: the game has its own
//! loop because it owns the world and time (PROJECT.md section 6), while this
//! one stays what it was -- a way to look at the renderer without the game.

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowId;

use crate::frame::{self, Frame};
use crate::gpu::Gpu;
use crate::orbit::Orbit;
use crate::window::{self, Target};

pub struct Options {
    pub width: u32,
    pub height: u32,

    /// How many frames to draw before exiting. `None` -- until closed.
    ///
    /// Exists for checking: a run that finishes by itself can go into a script
    /// and into CI, while "a window opened, take a look" cannot.
    pub frames: Option<u32>,

    /// Wait for vertical sync. Why this is a flag and what it used to hang on
    /// -- [`crate::window::Options::vsync`].
    pub vsync: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            width: 1280,
            height: 720,
            frames: None,
            vsync: true,
        }
    }
}

struct App {
    options: Options,
    drawn: u32,
    resized_once: bool,
    state: Option<State>,
    error: Option<String>,
}

struct State {
    target: Target,
    gpu: Gpu,
    frame: Frame,

    /// The camera and what the player moves it with. The position is derived
    /// from angles and altitude every frame, so nothing accumulates here that
    /// could drift (`crate::orbit`).
    orbit: Orbit,

    /// The left button is held -- we are dragging the camera.
    dragging: bool,
    /// Where the cursor was last time; the difference is the shift. `None` --
    /// the cursor has not appeared in the window yet or has just come back,
    /// and there is nothing to measure the shift from.
    cursor: Option<(f64, f64)>,
}

pub fn run(options: Options) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| format!("no event loop: {e}"))?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App {
        options,
        drawn: 0,
        resized_once: false,
        state: None,
        error: None,
    };

    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("the event loop stopped with an error: {e}"))?;

    match app.error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        match State::new(event_loop, &self.options) {
            Ok(state) => {
                println!("adapter: {}", state.gpu.describe());
                println!("surface: {}", state.target.describe());
                println!(
                    "camera: drag with the left button to rotate, wheel for \
                     altitude ({:.0} m to {:.0e} m), Esc to quit",
                    crate::orbit::MIN_ALTITUDE_M,
                    crate::orbit::MAX_ALTITUDE_M
                );
                self.state = Some(state);
            }
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state.is_pressed() && event.logical_key == Key::Named(NamedKey::Escape) {
                    event_loop.exit();
                }
            }

            WindowEvent::Resized(size) => state.target.resize(&state.gpu, size.width, size.height),

            // Camera control. The window only turns events into numbers --
            // what comes out of them is `orbit`'s decision, which is why it is
            // checked without a window and without a GPU.
            WindowEvent::MouseInput {
                state: button_state,
                button: MouseButton::Left,
                ..
            } => {
                state.dragging = button_state == ElementState::Pressed;
                if !state.dragging {
                    state.cursor = None;
                }
            }

            WindowEvent::CursorLeft { .. } => state.cursor = None,

            WindowEvent::CursorMoved { position, .. } => {
                let now = (position.x, position.y);
                if state.dragging {
                    // The first move after a press has nothing to measure the
                    // shift from: without this the camera would jerk by the
                    // whole distance from the previous cursor position.
                    if let Some(was) = state.cursor {
                        state.orbit.drag(now.0 - was.0, now.1 - was.1);
                    }
                }
                state.cursor = Some(now);
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let notches = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                    // A touchpad gives pixels. Fifty per notch, so that one
                    // finger swipe does not fly through three orders of
                    // altitude.
                    MouseScrollDelta::PixelDelta(p) => p.y / 50.0,
                };
                state.orbit.zoom(notches);
            }

            WindowEvent::RedrawRequested => {
                if let Err(e) = state.draw() {
                    self.error = Some(e);
                    event_loop.exit();
                    return;
                }

                self.drawn += 1;

                // A resize in the middle of a bounded run, so that path is
                // exercised too, not only opening the window. That is where
                // the surface breaks, and it breaks silently.
                if let Some(limit) = self.options.frames {
                    if !self.resized_once && self.drawn == limit / 2 {
                        self.resized_once = true;

                        let (width, height) = (self.options.width / 2, self.options.height / 2);
                        println!("resize: {width}x{height}");
                        state.target.request_size(&state.gpu, width, height);
                    }

                    if self.drawn >= limit {
                        println!("frames drawn: {}", self.drawn);
                        event_loop.exit();
                        return;
                    }
                }

                state.target.window().request_redraw();
            }

            _ => {}
        }
    }
}

impl State {
    fn new(event_loop: &ActiveEventLoop, options: &Options) -> Result<State, String> {
        let (target, gpu) = Target::open(
            event_loop,
            &window::Options {
                title: "space_sim".to_string(),
                width: options.width,
                height: options.height,
                vsync: options.vsync,
            },
        )?;

        // The pipeline is tied to the target format, so it is built after the
        // format is chosen and survives resizes -- they do not touch it.
        let frame = Frame::new(&gpu, target.format());

        target.window().request_redraw();

        Ok(State {
            target,
            gpu,
            frame,
            orbit: Orbit::default(),
            dragging: false,
            cursor: None,
        })
    }

    fn draw(&mut self) -> Result<(), String> {
        let Some(surface) = self.target.acquire(&self.gpu)? else {
            return Ok(());
        };

        let view = surface
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });

        self.frame.draw(
            &self.gpu,
            &mut encoder,
            &view,
            self.target.width(),
            self.target.height(),
            // The engine probe looks at a single body of Earth's radius: the
            // game assembles its own scene, and there is none here (R1e).
            &frame::default_scene(self.orbit.camera()),
        );

        self.gpu.queue.submit([encoder.finish()]);
        // In wgpu 30 presenting moved to the queue: it used to be a method on
        // the texture itself.
        self.gpu.queue.present(surface);
        Ok(())
    }
}
