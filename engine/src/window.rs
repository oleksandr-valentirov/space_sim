//! Window and surface: everything a shot does not have (ROADMAP J1).
//!
//! Split out of `app` because since J1 there are two event loops -- the engine
//! probes stay in [`crate::app`], and the game has its own, because it owns the
//! world and time (PROJECT.md section 6). Duplicating the body of `App` back
//! and forth is harmless: it is a translator of `winit` events into calls, and
//! each of the two has its own.
//!
//! But **the surface must not be duplicated**, and that is not taste. Three
//! cases live here, each of which has hung dead once and has never once failed
//! with an error:
//!
//!   1. reconfiguring with the wrong size ([`Target::resync`]);
//!   2. `Outdated` / `Lost` as ordinary states, not errors ([`Target::acquire`]);
//!   3. `request_inner_size`, which resizes IMMEDIATELY and sends no `Resized`
//!      ([`Target::request_size`]).
//!
//! Two places with this logic drift apart, and the drift looks like "in one
//! mode it somehow does not draw".

use std::sync::Arc;

use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use crate::gpu::Gpu;

pub struct Options {
    pub title: String,
    pub width: u32,
    pub height: u32,

    /// Wait for vertical sync.
    ///
    /// For the game -- yes, of course. For a bounded run -- no, and that is
    /// measured rather than guessed: under X11 the window of an unfocused
    /// process may not be shown at all, and then the Fifo queue never releases
    /// a frame -- `get_current_texture` blocks forever. The run stopped at
    /// exactly frame twenty and hung without a single error.
    pub vsync: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            title: "space_sim".to_string(),
            width: 1280,
            height: 720,
            vsync: true,
        }
    }
}

/// The window, the surface, and the configuration that surface currently has.
pub struct Target {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

impl Target {
    /// Opens a window and creates a device for it.
    ///
    /// The device is returned alongside rather than taken as an argument: the
    /// adapter must be able to draw into this very surface
    /// (`Gpu::new(.., Some(&surface))`), and a surface exists only after a
    /// window. The order here is wgpu's, not ours.
    pub fn open(event_loop: &ActiveEventLoop, options: &Options) -> Result<(Target, Gpu), String> {
        let attributes = Window::default_attributes()
            .with_title(options.title.clone())
            .with_inner_size(winit::dpi::PhysicalSize::new(options.width, options.height));

        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| format!("the window will not be created: {e}"))?,
        );

        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| format!("the surface will not be created: {e}"))?;

        let gpu = Gpu::new(instance, Some(&surface))?;

        let size = window.inner_size();
        let capabilities = surface.get_capabilities(&gpu.adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(capabilities.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            // The backend picks the colour space: we do not care, and pinning
            // a specific one would cut off surfaces that do not support it.
            color_space: wgpu::SurfaceColorSpace::default(),
            present_mode: if options.vsync {
                wgpu::PresentMode::AutoVsync
            } else {
                wgpu::PresentMode::AutoNoVsync
            },
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&gpu.device, &config);

        Ok((
            Target {
                window,
                surface,
                config,
            },
            gpu,
        ))
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    pub fn width(&self) -> u32 {
        self.config.width
    }

    pub fn height(&self) -> u32 {
        self.config.height
    }

    /// The surface format. The pipeline is tied to it, so `Frame` is built
    /// after the format is chosen and survives resizes.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    pub fn describe(&self) -> String {
        format!(
            "{}×{}, {:?}, {:?}",
            self.config.width, self.config.height, self.config.format, self.config.present_mode
        )
    }

    pub fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) {
        // A minimised window gives zero, and a zero-sized surface is a
        // validation error. Skip rather than clamp to one: the frame has
        // nowhere to go anyway.
        if width == 0 || height == 0 {
            return;
        }

        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&gpu.device, &self.config);
    }

    /// Reconfigure to the **actual** window size.
    ///
    /// Actual, not stored. The first version reconfigured the surface with the
    /// same `config`, and that hung dead: if the window is already a different
    /// size, the surface stays `Outdated`, no frame is drawn, the counter does
    /// not grow -- forever. There is no error at all; the program simply stops
    /// drawing.
    pub fn resync(&mut self, gpu: &Gpu) {
        let size = self.window.inner_size();
        self.resize(gpu, size.width, size.height);
    }

    /// Ask for a different window size.
    ///
    /// `request_inner_size` may resize IMMEDIATELY and return the new size --
    /// and then there is no `Resized` event at all. Missing that case means
    /// leaving the surface at the old size, and everything hangs after that
    /// (see [`Target::resync`]).
    pub fn request_size(&mut self, gpu: &Gpu, width: u32, height: u32) {
        let asked = winit::dpi::PhysicalSize::new(width, height);
        if let Some(now) = self.window.request_inner_size(asked) {
            self.resize(gpu, now.width, now.height);
        }
    }

    /// The next surface texture, or `None` if there will be no frame this
    /// time.
    ///
    /// wgpu 30 returns an enum of states rather than a `Result`, and most of
    /// them are not errors but ordinary events: a resize, an occluded monitor,
    /// a lost surface. There is nowhere to draw in them, but nothing to fail
    /// over either -- reconfigure and wait for the next frame.
    pub fn acquire(&mut self, gpu: &Gpu) -> Result<Option<wgpu::SurfaceTexture>, String> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(target) => Ok(Some(target)),
            wgpu::CurrentSurfaceTexture::Suboptimal(target) => Ok(Some(target)),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.resync(gpu);
                Ok(None)
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                Ok(None)
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                Err("the surface was rejected by validation".to_string())
            }
        }
    }
}
