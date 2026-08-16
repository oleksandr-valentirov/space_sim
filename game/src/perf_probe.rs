//! What the **real** game frame costs (ROADMAP-UI.md, U8; skill `perf-probe`).
//!
//! The engine's probe (`engine::perf_probe`) measures the engine's scene and a
//! **synthetic** panel: a rectangle and a line of text. That was right for
//! U1b, which asked what the egui pass itself costs. U8's question is
//! different -- what the frame the player sees costs -- and in it both the
//! panels are real (five of them, two columns) and the scene is real: the
//! prediction polyline with all its vertices.
//!
//! ## Why this is the same measurement as the engine's
//!
//! The statistics are computed by
//! `engine::perf_probe::Stats::from_samples`, i.e. the same formula, and the
//! method is the same synchronous `poll(Wait)` -- with the same limits (skill
//! `perf-probe`, "Обмеження методу"): this is an **upper bound** on frame
//! time, comparable between runs on one machine and with nothing else.
//!
//! ## Why D7 is measured here too
//!
//! The debt says: history grows without bound, and a million polyline vertices
//! is about 3 ms of CPU and 24 MB per frame. U8 does not close the debt but is
//! obliged to **look**: before this step the "million" stood in ROADMAP as a
//! derivation rather than a measurement. So the probe prints, beside the frame
//! time, what that time is made of -- how many vertices are in the scene, how
//! many samples are in history and how much they weigh.
//!
//! The caller sets the mission's length (`--perf-probe <days>`), because that
//! is what decides: an hour-long mission shows nothing, while one that hits
//! the debt shows it.

use std::time::Instant;

use engine::gpu::Gpu;
use engine::perf_probe::Stats;
use engine::scene::Scene;
use engine::shot;
use engine::ui::{Ui, Viewport};

use crate::app;
use crate::frame_view::ViewFrame;
use crate::hud;
use crate::palette;
use crate::schedule;
use crate::snapshot::WorldSnapshot;
use crate::text::Language;
use crate::view;
use crate::world::EARTH;

/// Warm-up frames -- as many as the engine's probe uses.
const WARMUP_FRAMES: u32 = 30;

/// What a frame is made of: volume rather than time.
///
/// Printed beside `Stats`, because without these numbers a frame time cannot
/// be compared with the next measurement: 0.2 ms at a thousand vertices and at
/// a million are two different claims about the engine.
pub struct SceneSize {
    /// Vertices across all the scene's polylines.
    pub vertices: usize,
    /// Samples across all vessels' trajectories.
    pub samples: usize,
    /// How much history weighs in the game's memory, at 104 bytes per sample
    /// (D7).
    pub history_bytes: usize,
    /// How much the vertices weigh in the frame buffer, at 24 bytes per
    /// vertex (D7).
    pub buffer_bytes: usize,
    /// Polylines in the scene.
    pub polylines: usize,
}

impl SceneSize {
    pub fn of(scene: &Scene, snapshot: &WorldSnapshot) -> SceneSize {
        let vertices: usize = scene.polylines.iter().map(|line| line.points.len()).sum();
        let samples: usize = snapshot.vessels.iter().map(|v| v.sample_count()).sum();

        SceneSize {
            vertices,
            samples,
            // The numbers come from D7, which is exactly why they are
            // constants here rather than `size_of::<Sample>()`: the debt
            // speaks in them, and the measurement must speak in the same ones
            // until the debt is closed.
            history_bytes: samples * 104,
            buffer_bytes: vertices * 24,
            polylines: scene.polylines.len(),
        }
    }
}

/// What is drawn over the scene.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    /// The scene alone -- so the panels' price has something to be compared
    /// against.
    None,
    /// The game's real panels, both columns.
    Panels,
}

/// Runs `frames` frames of a given scene and returns the frame time.
///
/// The panels drawn are **the same** ones as in `app::draw`, with the style
/// from `palette` (U7c): a panel with egui's default spacing would be a
/// different size, i.e. the frame measured would not be the game's.
///
/// There are eight arguments, and collecting them into a struct is pointless:
/// each is an independent axis of the measurement, and an eight-field struct
/// filled at the call site is the same list, only longer.
#[allow(clippy::too_many_arguments)]
pub fn measure(
    gpu: &Gpu,
    width: u32,
    height: u32,
    frames: u32,
    scene: &Scene,
    snapshot: &WorldSnapshot,
    overlay: Overlay,
    earth_radius_m: f64,
) -> Result<(Stats, Stats), String> {
    let mut frame = engine::frame::Frame::new(gpu, shot::FORMAT);
    let mut interface = Ui::new(gpu, shot::FORMAT);
    palette::apply(interface.context());

    // COPY_SRC is deliberately absent, for the same reason as in the engine's
    // probe: reading pixels back does not happen in a real frame.
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("game perf probe"),
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

    // The frame's second number rather than a third in brackets: the N1 fork
    // asks about exactly this -- whether the pass over polyline vertices really
    // is the most expensive thing in the frame.
    let mut draw_once = || -> Result<(f64, f64), String> {
        let start = Instant::now();

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("game perf probe"),
            });
        frame.draw(gpu, &mut encoder, &view, width, height, scene);

        if overlay == Overlay::Panels {
            let viewport = Viewport::new(width, height, 1.0);
            let mut draft = hud::PlanDraft::default();
            let mut plot = hud::PlotState::default();
            interface.draw(
                gpu,
                &mut encoder,
                &view,
                viewport,
                viewport.quiet_input(),
                |ui| {
                    // Literally what `app::draw` does -- otherwise the
                    // measurement would describe panels the game does not
                    // have.
                    engine::egui::Panel::left("time")
                        .exact_size(220.0)
                        .resizable(false)
                        .show(ui, |ui| {
                            hud::time_panel(ui, Language::English, snapshot);
                            ui.separator();
                            if let Some(vessel) = snapshot.vessels.first() {
                                let readout = hud::read_vessel(snapshot, vessel, earth_radius_m);
                                hud::vessel_panel(ui, Language::English, &vessel.name, &readout);

                                ui.separator();
                                let markers = schedule::scan(&vessel.legs);
                                hud::schedule_panel(ui, Language::English, snapshot.t, &markers);

                                ui.separator();
                                hud::plan_panel(
                                    ui,
                                    Language::English,
                                    snapshot.t,
                                    EARTH,
                                    &mut draft,
                                    None,
                                );
                            }

                            ui.separator();
                            hud::view_panel(
                                ui,
                                Language::English,
                                ViewFrame::Inertial,
                                hud::read_curve(snapshot),
                            );
                        });

                    engine::egui::Panel::right("windows")
                        .exact_size(230.0)
                        .resizable(false)
                        .show(ui, |ui| {
                            hud::porkchop_panel(ui, Language::English, None, &mut plot);
                        });
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

        Ok((
            start.elapsed().as_secs_f64() * 1000.0,
            frame.lines_upload_ms(),
        ))
    };

    for _ in 0..WARMUP_FRAMES {
        draw_once()?;
    }

    let mut samples = Vec::with_capacity(frames as usize);
    let mut uploads = Vec::with_capacity(frames as usize);
    for _ in 0..frames {
        let (whole, upload) = draw_once()?;
        samples.push(whole);
        uploads.push(upload);
    }

    Ok((
        Stats::from_samples(width, height, samples),
        Stats::from_samples(width, height, uploads),
    ))
}

/// What assembling a scene from a snapshot costs -- i.e. the pass over every
/// vertex on the CPU.
///
/// The second half of D7 lives exactly here: `view::build_in` walks **every**
/// sample of every leg, and in the rotating frame the frame transform per
/// point is added to that (U6a1 measured 2.69 -> 10.56 ns). Measured
/// separately from the frame, because in the game it is separate work too --
/// and because this number decides whether the debt is about the frame or
/// about preparing for it.
pub fn build_ms(
    snapshot: &WorldSnapshot,
    camera: impl Fn() -> engine::camera::Camera,
    frame: ViewFrame,
) -> f64 {
    // Warm-up: the first pass pays for allocating the vectors, and without it
    // that is what gets measured.
    let _ = view::build_in(snapshot, camera(), frame);

    let start = Instant::now();
    let scene = view::build_in(snapshot, camera(), frame);
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;

    // So the optimiser does not discard the build: the scene must be read.
    assert!(!scene.polylines.is_empty() || snapshot.vessels.is_empty());
    elapsed
}

/// What the zero-velocity curve itself costs.
///
/// Separate from [`build_ms`], and not for completeness: the first U8 run
/// showed that in the rotating frame building the scene is **an order**
/// dearer than the inertial one, while the extra vertices number in the
/// hundreds. So what pays is not the point transform (U6a1: 10.56 ns per
/// point) but something else -- and until that is measured separately, "the
/// rotating frame is expensive" stays a guess about the cause.
///
/// Returns `None` if the snapshot has no curve: in the inertial frame it is
/// not computed at all.
pub fn zvc_ms(snapshot: &WorldSnapshot) -> Option<(f64, usize)> {
    let earth = snapshot.bodies.iter().find(|b| b.body == EARTH)?;
    let moon = snapshot
        .bodies
        .iter()
        .find(|b| b.body == crate::world::MOON)?;
    let mu = core_rs::cr3bp_mu(earth.mu, moon.mu);
    let c = hud::read_curve(snapshot)?.jacobi;

    let run = || crate::zvc::curves(mu, c, crate::frame_view::SYNODIC_SCALE_M);
    let _ = run();

    let start = Instant::now();
    let curves = run();
    let ms = start.elapsed().as_secs_f64() * 1000.0;

    let vertices: usize = curves.iter().map(|line| line.points.len()).sum();
    Some((ms, vertices))
}

/// The whole probe run: build the world, catch the mission up, measure, print.
///
/// It lives here rather than in `main`, for the same reason as in the engine:
/// `main` parses arguments, while what is measured is the measurement itself
/// and belongs beside the methodology.
pub fn run(options: &app::Options, days: f64, frames: u32) -> Result<(), String> {
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("adapter: {}", gpu.describe());
    println!(
        "profile: {}",
        if cfg!(debug_assertions) {
            "debug -- numbers incomparable with release, a thirteenfold difference"
        } else {
            "release"
        }
    );

    let mut world = app::build_world(options)?;
    let earth_radius_m = world.ephemeris().body_radius(EARTH);

    let start_t = crate::mission::start().t;
    let steps = world.run_to_day(start_t + days * 86400.0, 1.0, 8);
    let snapshot = world.snapshot();
    let flown_days = (snapshot.t - start_t) / 86400.0;

    // Sample density is the quantity the whole D7 derivation rests on (171 per
    // day from `bench_prop`), so it is printed rather than left in someone's
    // head.
    let samples: usize = snapshot.vessels.iter().map(|v| v.sample_count()).sum();
    let vessel_days = snapshot.vessels.len() as f64 * flown_days;
    println!(
        "fleet: {} vessels, {:.0} samples per vessel per day",
        snapshot.vessels.len(),
        samples as f64 / vessel_days.max(f64::MIN_POSITIVE)
    );

    // A vessel that deorbited quietly shrinks the measurement -- so it is
    // announced. A station at 600 km should not do that over a hundred days,
    // which is exactly why a silent failure here would be the worst
    // outcome.
    for vessel in &snapshot.vessels {
        if let Some(error) = &vessel.failed {
            println!("  WARNING: {} did not make it: {error}", vessel.name);
        }
    }

    // A closure rather than a value: `Camera` is not `Copy`, and building it
    // anew is cheaper than any game with clones (the same trick as in
    // `game/tests/scene.rs`).
    let camera = || engine::orbit::Orbit::at_altitude(crate::mission::CAMERA_ALTITUDE_M).camera();

    for frame in [ViewFrame::Inertial, ViewFrame::Rotating] {
        let scene = view::build_in(&snapshot, camera(), frame);
        let size = SceneSize::of(&scene, &snapshot);
        let build = build_ms(&snapshot, camera, frame);

        println!();
        println!("=== frame {frame:?}, day {flown_days:.1} ({steps} steps)");
        println!(
            "  scene: {} vertices in {} polylines, {} history samples, {} vessels",
            size.vertices,
            size.polylines,
            size.samples,
            snapshot.vessels.len()
        );
        println!(
            "  memory: history {:.1} MiB, frame buffer {:.2} MiB per frame",
            size.history_bytes as f64 / (1024.0 * 1024.0),
            size.buffer_bytes as f64 / (1024.0 * 1024.0)
        );
        println!("  view::build_in: {build:.3} ms per frame (CPU, no engine)");
        if frame == ViewFrame::Rotating {
            match zvc_ms(&snapshot) {
                Some((ms, vertices)) => println!(
                    "    of which the zero-velocity curve: {ms:.3} ms for {vertices} \
                     vertices ({:.0} ns per vertex)",
                    ms * 1.0e6 / vertices.max(1) as f64
                ),
                None => println!("    this snapshot has no curve"),
            }
        }

        for (width, height) in [(1280u32, 720u32), (1920, 1080)] {
            // The thinned scene is built for **its own** resolution: the
            // criterion is on screen, and at 1080p half a pixel is a different
            // quantity in metres. A fresh cache per resolution: the criterion
            // depends on it, and a cache warmed at another resolution would
            // measure the wrong thing.
            let mut cache = crate::trail::Cache::new();
            let mut thinning = view::Thinning {
                cache: &mut cache,
                height_px: height,
            };
            // The first pass fills the cache, the second is what the game pays
            // every frame.
            let _ = view::build_thinned(&snapshot, camera(), &[], frame, &mut thinning);
            let thinned_start = Instant::now();
            let thinned = view::build_thinned(&snapshot, camera(), &[], frame, &mut thinning);
            let thinned_ms = thinned_start.elapsed().as_secs_f64() * 1000.0;
            let thinned_size = SceneSize::of(&thinned, &snapshot);

            println!(
                "  {width}x{height}, thinning: {} -> {} vertices (x{:.0}), \
                 build with a warm cache {thinned_ms:.3} ms, legs cached {}",
                size.vertices,
                thinned_size.vertices,
                size.vertices as f64 / thinned_size.vertices.max(1) as f64,
                thinning.cache.len()
            );

            for (overlay, name) in [(Overlay::None, "none"), (Overlay::Panels, "panels")] {
                for (scene, size, label) in [
                    (&scene, &size, "full"),
                    (&thinned, &thinned_size, "thinned"),
                ] {
                    let (stats, upload) = measure(
                        &gpu,
                        width,
                        height,
                        frames,
                        scene,
                        &snapshot,
                        overlay,
                        earth_radius_m,
                    )?;
                    println!(
                        "  {width}x{height}, interface {name}, trail {label}: \
                         mean {:.3} ms, p95 {:.3} ms, margin to 60 Hz {:+.2} ms",
                        stats.mean_ms,
                        stats.p95_ms,
                        stats.headroom_ms(1000.0 / 60.0)
                    );
                    println!(
                        "    of which Lines::upload: {:.3} ms ({:.0}% of the frame, {:.1} ns per vertex)",
                        upload.mean_ms,
                        100.0 * upload.mean_ms / stats.mean_ms.max(f64::MIN_POSITIVE),
                        upload.mean_ms * 1.0e6 / size.vertices.max(1) as f64
                    );
                }
            }
        }
    }

    Ok(())
}
