//! Measuring the engine's frame time -- the render half of the performance
//! measurement process (the `perf-probe` skill).
//!
//! **Not tied to any particular scene.** It measures whatever
//! [`crate::frame::Frame::draw`] draws right now: a triangle on F2, a
//! real-scale sphere after F5, a patched planet with LOD and terrain today.
//! The numbers become a measurement of the new scene without a single change
//! in this file. That is exactly why the probe is separate from
//! `depth_probe`/`camera_probe`: those answer the specific geometric question
//! of their own step, this one answers "what does a frame cost" for any step.
//!
//! ## Method
//!
//! A synchronous `submit` plus `device.poll(Wait)` on every frame, without a
//! window and without vsync. This is deliberately NOT what the player sees:
//! the real loop is pipelined (the GPU work of frame N+1 starts without
//! waiting for N to be presented), whereas here every frame waits for the
//! previous one to finish completely. So the number is an **upper bound** on
//! frame time, not a lower one. Comparing runs against each other on the same
//! machine is sound; comparing the absolute number against "on hardware like
//! this the game gives N fps" is not, as long as the render is not pipelined.
//!
//! The first [`WARMUP_FRAMES`] frames are discarded: on many backends the
//! first run of a pipeline compiles the shader lazily, so that one frame is an
//! order of magnitude longer than all the rest, and without discarding it it
//! would spoil both the minimum and the max.

use std::time::Instant;

use crate::cubesphere;
use crate::frame::{self, Frame};
use crate::gpu::Gpu;
use crate::shot;
use crate::sphere;

/// Warm-up frames before the measurement -- shader compilation and the
/// driver's first allocated pipeline have to happen once, outside the
/// measurement.
const WARMUP_FRAMES: u32 = 10;

pub struct Stats {
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub min_ms: f64,
    pub mean_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
}

impl Stats {
    /// Statistics over already collected frame-time samples, in milliseconds.
    ///
    /// Factored out here because there are two probes now: this one and the
    /// one in `game` that measures the real game frame with its panels (U8).
    /// The formula has to be the same for both -- otherwise their numbers
    /// cannot go into one table, and that is precisely what they are computed
    /// for.
    pub fn from_samples(width: u32, height: u32, mut samples: Vec<f64>) -> Stats {
        assert!(!samples.is_empty(), "statistics over zero frames");
        samples.sort_by(f64::total_cmp);

        let frames = samples.len() as u32;
        let min_ms = samples[0];
        let max_ms = *samples.last().expect("non-empty");
        let mean_ms = samples.iter().sum::<f64>() / f64::from(frames);

        // Nearest rank, not interpolation -- over a few hundred frames the
        // difference is invisible, and the formula is an order simpler.
        let p95_index = ((f64::from(frames) * 0.95) as usize).min(samples.len() - 1);

        Stats {
            width,
            height,
            frames,
            min_ms,
            mean_ms,
            p95_ms: samples[p95_index],
            max_ms,
        }
    }

    pub fn fps(&self) -> f64 {
        1000.0 / self.mean_ms
    }

    /// How many milliseconds are left of the frame budget. Negative means the
    /// budget is exceeded.
    pub fn headroom_ms(&self, budget_ms: f64) -> f64 {
        budget_ms - self.mean_ms
    }
}

/// What the camera-relative pass over the vertices of a UV sphere used to
/// cost.
///
/// **The frame no longer does this** (R1d): the planet is drawn as patches,
/// and subtracting the camera costs six numbers instead of 8385. The function
/// stayed precisely because a number without a second number means nothing --
/// it is printed next to [`patch_pass_ms`], and the difference between them is
/// the gain.
///
/// Returns milliseconds per pass.
pub fn camera_pass_ms(passes: u32) -> f64 {
    let mesh = sphere::generate(sphere::EARTH_RADIUS_M, 64, 128);
    let camera = frame::default_camera();
    let mut bytes: Vec<u8> = Vec::with_capacity(mesh.positions.len() * 12);

    // Warm-up: the first pass pays for the memory pages behind `bytes`.
    for _ in 0..2 {
        bytes.clear();
        for &p in &mesh.positions {
            for value in camera.relative(p) {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    }

    let start = Instant::now();
    for _ in 0..passes {
        bytes.clear();
        for &p in &mesh.positions {
            for value in camera.relative(p) {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    // So the optimiser does not throw the loop away entirely.
    assert_eq!(bytes.len(), mesh.positions.len() * 12);

    start.elapsed().as_secs_f64() * 1000.0 / f64::from(passes)
}

/// The same for a planet made of patches -- what the frame does **now** (R1d,
/// R1e).
///
/// The work here is the same in shape (subtracting the camera in `double`,
/// narrowing to `f32`) and different in volume: one origin per patch instead
/// of a position per vertex. Hence it is measured by the same function from
/// outside: two numbers from one run are comparable, from different runs they
/// are not.
///
/// Since R1e the pass gained the body's rotation and the multiplication by the
/// radius -- nine multiplications per patch origin instead of none. That is
/// what the frame really does for **one** body; for N bodies the pass is
/// multiplied by N.
pub fn patch_pass_ms(passes: u32) -> f64 {
    let camera = frame::default_camera();
    let eye = camera.position();

    // The same patches as in the frame: six level-zero faces on the unit
    // sphere.
    let origins: Vec<[f64; 3]> = (0..cubesphere::FACES)
        .map(|face| {
            cubesphere::Patch {
                face,
                level: 0,
                i: 0,
                j: 0,
            }
            .mesh(1.0)
            .origin
        })
        .collect();

    // A body as in the scene: Earth's radius and a 45 deg turn about (1,1,1)
    // -- a matrix without a single zero, so that the measurement does not
    // depend on which particular numbers ended up in it.
    let radius = sphere::EARTH_RADIUS_M;
    let centre = [0.0, 0.0, 0.0];
    let rotation = frame::rotation([0.923_880, 0.220_942, 0.220_942, 0.220_942]);

    let mut bytes: Vec<u8> = Vec::with_capacity(origins.len() * 16);
    let mut run = || {
        bytes.clear();
        for origin in &origins {
            for k in 0..3 {
                let turned = rotation[k][0] * origin[0]
                    + rotation[k][1] * origin[1]
                    + rotation[k][2] * origin[2];
                let value = (centre[k] + radius * turned - eye[k]) as f32;
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(&0.0f32.to_le_bytes());
        }
    };

    for _ in 0..2 {
        run();
    }

    let start = Instant::now();
    for _ in 0..passes {
        run();
    }
    assert_eq!(bytes.len(), origins.len() * 16);

    start.elapsed().as_secs_f64() * 1000.0 / f64::from(passes)
}

/// What gets drawn on top of the scene during a measurement.
///
/// The interface is a substantial new cost (ROADMAP-UI.md, U1b), and it has to
/// be measured **in the same run**, not a separate one: two runs on one
/// machine differ by more than a panel costs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    /// A frame with no egui pass -- how every number before U1b was measured.
    None,
    /// The egui pass is there but empty: the price of the wiring itself.
    EmptyUi,
    /// The egui pass with a panel -- the price of the wiring together with
    /// something actually drawn.
    Panel,
}

/// Runs `frames` frames of `width`x`height` without a window and returns the
/// frame-time statistics in milliseconds.
pub fn measure(
    gpu: &Gpu,
    width: u32,
    height: u32,
    frames: u32,
    overlay: Overlay,
    altitude_m: f64,
) -> Result<Stats, String> {
    let distance = crate::sphere::EARTH_RADIUS_M + altitude_m;
    let camera =
        crate::camera::Camera::look_at([distance, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    measure_scene(
        gpu,
        width,
        height,
        frames,
        overlay,
        &frame::default_scene(camera),
    )
}

/// What a frame costs with air and without it (ROADMAP-ATMOSPHERE.md, S5, S7).
///
/// The same engine-probe scene, with one single difference -- whether the body
/// has an atmosphere. Two numbers from one run are comparable, from different
/// runs they are not, and that is exactly why both are measured here rather
/// than in different places.
///
/// **The altitudes around the S5 condition are not round, and that is
/// deliberate.** The condition is the layer's thickness in frame pixels, and
/// it crosses one at 6.24e7 m: a hundred kilometres of air at that distance
/// take up exactly one pixel. So 6.0e7 and 6.5e7 are the same scene to within
/// eight per cent of the distance, in which the aerial-perspective volume is
/// and is not computed. The difference between them is the price of the
/// volume; at 1e9 m it is the same, and that is what skipping it saves.
pub fn air_cost(
    gpu: &Gpu,
    width: u32,
    height: u32,
    frames: u32,
    altitude_m: f64,
    air: bool,
) -> Result<Stats, String> {
    let distance = crate::sphere::EARTH_RADIUS_M + altitude_m;
    let camera =
        crate::camera::Camera::look_at([distance, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    let mut scene = frame::default_scene(camera);
    if air {
        scene.bodies[0].air =
            Some(crate::scene::Atmosphere::EARTH.with_surface(sphere::EARTH_RADIUS_M));
    }
    measure_scene(gpu, width, height, frames, Overlay::None, &scene)
}

/// What a frame costs with a ship and without one (stage V, step V6).
///
/// The same engine-probe scene, with one single difference -- whether a ship
/// stands in front of the camera and at what range. Both numbers from one run:
/// from different runs they are not comparable.
///
/// WARNING: **The difference here is not the price of fifteen hundred
/// vertices.** A ship metres from the camera drags `near` along with it (V2),
/// and `near` together with the scene's span decides how many depth passes
/// there will be (V3). So in low orbit a frame with a ship draws the planet
/// **twice**, and that is the main thing in the difference. Without this
/// explanation the number would read as "a ship is expensive".
pub fn ship_cost(
    gpu: &Gpu,
    width: u32,
    height: u32,
    frames: u32,
    altitude_m: f64,
    range_m: Option<f64>,
) -> Result<Stats, String> {
    let distance = crate::sphere::EARTH_RADIUS_M + altitude_m;
    let eye = [distance, 0.0, 0.0];
    let camera = crate::camera::Camera::look_at(eye, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    let mut scene = frame::default_scene(camera);
    if let Some(range) = range_m {
        // In front of the camera, i.e. between it and the planet -- where it
        // does sit in the third-person view.
        scene.ships.push(crate::scene::Ship {
            centre: [eye[0] - range, 0.0, 0.0],
            orientation: [1.0, 0.0, 0.0, 0.0],
            height_m: crate::ship::DEFAULT_HEIGHT_M,
            extent_m: 0.5 * crate::ship::DEFAULT_HEIGHT_M,
            colour: [0.72, 0.74, 0.78, 1.0],
            roughness: crate::ship::HULL_ROUGHNESS,
            metallic: crate::ship::HULL_METALLIC,
        });
    }
    measure_scene(gpu, width, height, frames, Overlay::None, &scene)
}

/// What a frame costs with colour tiles and without them (stage T, step T8).
///
/// The same scene, the same height pyramid, the only difference being whether
/// the colour is loaded. Two numbers from one run: from different runs they
/// are not comparable, and that is the main reason both are measured here.
///
/// WARNING: **The tiles are real, not synthetic.** The price of colour is a
/// second bindless fetch per fragment **and** the eighth thousand of textures
/// in the bind group; a synthetic two-level pyramid would have neither.
pub fn tile_cost(
    gpu: &Gpu,
    width: u32,
    height: u32,
    frames: u32,
    altitude_m: f64,
    terrain: &crate::tiles::Terrain,
    colour: Option<&crate::tiles::Colour>,
) -> Result<Stats, String> {
    let radius = terrain.reference_m;
    let distance = radius + altitude_m;
    // The camera is off to the side rather than above the centre of a cube
    // face: a symmetric point hides geometry errors (D13, D14), and here it
    // also yields a different set of patches, i.e. a different amount of work.
    let camera = crate::camera::Camera::look_at(
        [distance * 0.82, distance * 0.42, distance * 0.39],
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
    );
    let mut frame = Frame::new(gpu, shot::FORMAT);
    let id = frame.load_surface(gpu, terrain, colour)?;

    let mut scene = crate::scene::Scene::new(camera);
    scene.sun = [1.0, 0.0, 0.0];
    scene.bodies.push(crate::scene::Body {
        centre: [0.0, 0.0, 0.0],
        radius_m: radius,
        orientation: [1.0, 0.0, 0.0, 0.0],
        tiles: crate::scene::TileSet::Loaded(id),
        colour: frame::COLOUR,
        air: None,
    });

    measure_with_frame(
        gpu,
        &mut frame,
        width,
        height,
        frames,
        Overlay::None,
        &scene,
    )
}

/// What a frame costs with **two** bodies that have tiles of their own (T7h,
/// debt D19).
///
/// The debt's question is literal: a texture array pays every frame for its
/// size rather than for what is drawn, so two bodies with pyramids pay twice.
/// T8 measured this on one body and predicted the sum; here the sum is
/// checked.
///
/// Both bodies in frame are small -- the camera stands so that each takes a
/// few pixels. That is deliberate: otherwise the work of the second set of
/// patches would enter the difference, and the question is not about that.
///
/// The bodies are separated along the `x` axis by ten of their radii: closer
/// and they would overlap, further and they would leave the frame.
pub fn two_body_cost(
    gpu: &Gpu,
    width: u32,
    height: u32,
    frames: u32,
    distance_m: f64,
    first: (&crate::tiles::Terrain, Option<&crate::tiles::Colour>),
    second: Option<(&crate::tiles::Terrain, Option<&crate::tiles::Colour>)>,
) -> Result<Stats, String> {
    let mut frame = Frame::new(gpu, shot::FORMAT);
    let camera = crate::camera::Camera::look_at(
        [distance_m * 0.82, distance_m * 0.42, distance_m * 0.39],
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
    );
    let mut scene = crate::scene::Scene::new(camera);
    scene.sun = [1.0, 0.0, 0.0];

    let place = |frame: &mut Frame,
                 scene: &mut crate::scene::Scene,
                 surface: (&crate::tiles::Terrain, Option<&crate::tiles::Colour>),
                 offset: f64|
     -> Result<(), String> {
        let (terrain, colour) = surface;
        let id = frame.load_surface(gpu, terrain, colour)?;
        scene.bodies.push(crate::scene::Body {
            centre: [offset * terrain.reference_m, 0.0, 0.0],
            radius_m: terrain.reference_m,
            orientation: [1.0, 0.0, 0.0, 0.0],
            tiles: crate::scene::TileSet::Loaded(id),
            colour: frame::COLOUR,
            air: None,
        });
        Ok(())
    };

    place(&mut frame, &mut scene, first, 0.0)?;
    if let Some(surface) = second {
        place(&mut frame, &mut scene, surface, 10.0)?;
    }

    measure_with_frame(
        gpu,
        &mut frame,
        width,
        height,
        frames,
        Overlay::None,
        &scene,
    )
}

/// The same for a scene somebody else assembled.
pub fn measure_scene(
    gpu: &Gpu,
    width: u32,
    height: u32,
    frames: u32,
    overlay: Overlay,
    scene: &crate::scene::Scene,
) -> Result<Stats, String> {
    let mut frame = Frame::new(gpu, shot::FORMAT);
    measure_with_frame(gpu, &mut frame, width, height, frames, overlay, scene)
}

/// A measurement with a frame the caller has already prepared -- with assets,
/// for instance.
pub fn measure_with_frame(
    gpu: &Gpu,
    frame: &mut Frame,
    width: u32,
    height: u32,
    frames: u32,
    overlay: Overlay,
    scene: &crate::scene::Scene,
) -> Result<Stats, String> {
    let mut interface = crate::ui::Ui::new(gpu, shot::FORMAT);
    // A scene without polylines: the measurement stays comparable with the I3
    // numbers, where there were none yet. When the prediction becomes part of
    // the scene, that will be a separate row of the table rather than quietly
    // a different number in the same one (the `perf-probe` skill).
    // The altitude is a parameter rather than a constant (R8): the patch count
    // depends on it, i.e. the main thing LOD added to the cost of a frame. One
    // row of the table no longer describes the frame -- two are needed, from
    // afar and from low orbit.
    // COPY_SRC is deliberately absent: this measurement does not read the
    // pixels back, and reading back is a separate cost that a real frame does
    // not have (that one goes to a surface, not to a buffer). Adding it here
    // would mean measuring not the frame but the frame plus something foreign.
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("perf probe"),
        size: wgpu::Extent3d {
            width,
            height,
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

    let mut draw_once = || -> Result<f64, String> {
        let start = Instant::now();

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("perf probe"),
            });
        frame.draw(gpu, &mut encoder, &view, width, height, scene);

        if overlay != Overlay::None {
            let viewport = crate::ui::Viewport::new(width, height, 1.0);
            interface.draw(
                gpu,
                &mut encoder,
                &view,
                viewport,
                viewport.quiet_input(),
                |ui| {
                    if overlay == Overlay::Panel {
                        // As much as the time panel from U2b will take: a
                        // rectangle and a line of text, i.e. both geometry and
                        // a fetch from the font atlas.
                        let rect =
                            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 180.0));
                        ui.painter()
                            .rect_filled(rect, 0.0, egui::Color32::from_rgb(20, 24, 28));
                        ui.painter().text(
                            egui::pos2(16.0, 16.0),
                            egui::Align2::LEFT_TOP,
                            "MET 000d 00:00:00",
                            egui::FontId::monospace(14.0),
                            egui::Color32::from_rgb(180, 220, 255),
                        );
                    }
                },
            );
        }

        gpu.queue.submit([encoder.finish()]);

        gpu.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|e| format!("gave up waiting for the GPU: {e}"))?;

        Ok(start.elapsed().as_secs_f64() * 1000.0)
    };

    for _ in 0..WARMUP_FRAMES {
        draw_once()?;
    }

    let mut samples = Vec::with_capacity(frames as usize);
    for _ in 0..frames {
        samples.push(draw_once()?);
    }

    Ok(Stats::from_samples(width, height, samples))
}
