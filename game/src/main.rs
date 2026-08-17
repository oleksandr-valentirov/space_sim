//! Launching the game (ROADMAP J1).
//!
//!     cargo run -p game                          a window
//!     cargo run -p game -- --frames 120          a window, 120 frames, exit
//!     cargo run -p game -- --shot build/j1.png   a windowless capture
//!
//! Argument parsing is our own and deliberately stupid -- the same decision as
//! in `engine`: `clap` arrives when there are twenty flags.

use std::path::PathBuf;

use engine::gpu::Gpu;
use engine::shot;

use game::{app, mission, view};

fn main() {
    let mut options = app::Options::default();
    let mut shot_path: Option<PathBuf> = None;
    let mut day: Option<f64> = None;
    let mut save_path: Option<PathBuf> = None;
    let mut frame = game::frame_view::ViewFrame::Inertial;
    let mut vsync_asked = false;
    let mut perf_probe_days: Option<f64> = None;
    let mut moon_altitude_km: Option<f64> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| -> String {
            args.next()
                .unwrap_or_else(|| fail(&format!("{name} without a value")))
        };

        match arg.as_str() {
            "--shot" => shot_path = Some(PathBuf::from(value("--shot"))),
            "--frames" => options.frames = Some(parse(&value("--frames"), "--frames")),
            "--asset" => options.asset = PathBuf::from(value("--asset")),
            "--day" => day = Some(parse_f64(&value("--day"), "--day")),
            "--demo-plan" => options.demo_plan = true,
            "--rotating" => frame = game::frame_view::ViewFrame::Rotating,
            "--moon" => moon_altitude_km = Some(parse_f64(&value("--moon"), "--moon")),
            "--perf-probe" => {
                perf_probe_days = Some(parse_f64(&value("--perf-probe"), "--perf-probe"))
            }
            "--stations" => options.stations = parse(&value("--stations"), "--stations") as usize,
            "--load" => options.load = Some(PathBuf::from(value("--load"))),
            "--save" => save_path = Some(PathBuf::from(value("--save"))),
            "--width" => options.width = parse(&value("--width"), "--width"),
            "--height" => options.height = parse(&value("--height"), "--height"),
            "--vsync" => {
                options.vsync = true;
                vsync_asked = true;
            }
            "--no-vsync" => {
                options.vsync = false;
                vsync_asked = true;
            }
            "--help" | "-h" => {
                println!("{HELP}");
                return;
            }
            other => fail(&format!("unknown argument {other}\n\n{HELP}")),
        }
    }

    // A bounded run defaults to no vsync: otherwise it hangs where the window
    // is not actually shown (engine::window::Options::vsync).
    if options.frames.is_some() && !vsync_asked {
        options.vsync = false;
    }

    let result = match (perf_probe_days, shot_path) {
        // 300 frames, as many as the engine's probe measures, so the numbers
        // land in one ROADMAP table.
        (Some(days), _) => game::perf_probe::run(&options, days, 300),
        (None, Some(path)) => take_shot(
            &path,
            &options,
            day,
            save_path.as_deref(),
            frame,
            moon_altitude_km,
        ),
        (None, None) => app::run(options),
    };

    if let Err(e) = result {
        fail(&e);
    }
}

const HELP: &str = "\
  --shot <file>   draw the full prediction into a PNG, without a window
  --frames <N>    draw N frames and exit (disables vsync)
  --asset <file>  ephemeris; defaults to data/fixture/earth_moon.eph
  --day <N>       stop the cursor on mission day N (for --shot); default: end
  --demo-plan     add the showcase manoeuvre on day 10 (ROADMAP J3)
  --rotating      capture in the Earth-Moon rotating frame (U6a); default is
                  inertial. In a window this is the panel's VIEW button
  --load <file>   raise the game from a save instead of a new mission (J6)
  --save <file>   write a save after the run (for --shot); in a window this
                  is F5
  --vsync         wait for vertical sync
  --no-vsync      do not wait
  --moon <km>     close capture of the Moon, with terrain: camera at that
                  altitude above it, target on the limb (for --shot; D12)
  --width <px>    width, default 1280
  --height <px>   height, default 720
  --perf-probe <days>
                  measure the game's real frame after a mission that long:
                  frame time with and without panels, vertices, history memory
                  (skill perf-probe, ROADMAP-UI.md U8). --release only
  --stations <n>  add n stations in low orbit to the mission. The trail hits
                  its ceiling from the number of vessels rather than from
                  years, so this number is what makes debt D7 visible
                  (ROADMAP.md, N1)";

/// A capture of the mission run to its end.
///
/// Run to the end rather than for N frames: the capture exists to be looked at
/// and compared, and half a mission in it would mean looking at the machine's
/// speed rather than at the prediction. The cursor reaches the end too, so
/// everything in the picture is history; where exactly it stands is shown by
/// `--frames`.
fn take_shot(
    path: &std::path::Path,
    options: &app::Options,
    day: Option<f64>,
    save_path: Option<&std::path::Path>,
    frame: game::frame_view::ViewFrame,
    moon_altitude_km: Option<f64>,
) -> Result<(), String> {
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("adapter: {}", gpu.describe());

    let mut world = app::build_world(options)?;
    // A second of "real" time per step: the cursor hits the horizon anyway, so
    // the integrator sets the pace rather than this number.
    let steps = match day {
        // The cursor is led by the same `step` as in a window rather than set
        // directly: otherwise the capture would show a state the game cannot
        // reach.
        Some(day) => world.run_to_day(mission::start().t + day * 86400.0, 1.0, 8),
        None => world.run_to_end(1.0, 8),
    };

    let snapshot = world.snapshot();
    for vessel in &snapshot.vessels {
        println!(
            "{}: {} legs, {} samples{}",
            vessel.name,
            vessel.legs.len(),
            vessel.sample_count(),
            match vessel.failed {
                Some(e) => format!(", stopped: {e}"),
                None => String::new(),
            }
        );
    }
    println!(
        "steps: {steps}, cursor on day {:.2}",
        (snapshot.t - mission::start().t) / 86400.0
    );

    if let Some(save_path) = save_path {
        game::save::write_world(&world, save_path)?;
        println!("save: {}", save_path.display());
    }

    // The camera near the Moon if asked: the game's orbital camera looks at
    // Earth and nowhere else (`engine::orbit`), so the Moon's terrain from it
    // is a few pixels. A capture is the only way to look at it until the game
    // gains a camera with target selection (D12).
    if let Some(altitude_km) = moon_altitude_km {
        return shoot_moon(&gpu, path, options, &snapshot, altitude_km);
    }

    // In the rotating frame the camera looks **down on the pair's plane**
    // rather than from the side: the whole map -- the zero-velocity curve, the
    // Lagrange points, the halo loop -- lies at z = 0, and from the side it
    // projects to a line. In a window the player does this with the mouse; a
    // capture has nobody to do it.
    let camera = match frame {
        game::frame_view::ViewFrame::Rotating => engine::camera::Camera::look_at(
            [0.0, 0.0, mission::CAMERA_ALTITUDE_M],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
        ),
        game::frame_view::ViewFrame::Inertial => {
            engine::orbit::Orbit::at_altitude(mission::CAMERA_ALTITUDE_M).camera()
        }
    };
    let scene = view::build_in(&snapshot, camera, frame);

    let taken = shot::take_scene(&gpu, options.width, options.height, &scene)?;
    taken.write_png(path)?;

    println!(
        "capture: {} ({}x{})",
        path.display(),
        options.width,
        options.height
    );
    Ok(())
}

fn parse_f64(text: &str, name: &str) -> f64 {
    text.parse()
        .unwrap_or_else(|_| fail(&format!("{name}: '{text}' is not a number")))
}

fn parse(text: &str, name: &str) -> u32 {
    text.parse()
        .unwrap_or_else(|_| fail(&format!("{name}: '{text}' is not a number")))
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

/// A close capture of the Moon, with terrain (D12).
///
/// ## Why its own path rather than `shot::take_scene`
///
/// `take_scene` creates a frame inside itself and draws immediately. Terrain
/// cannot be shown that way: tiles are loaded **into the frame**
/// (`Frame::load_terrain`, R5c), and there must be a step between creation and
/// drawing that the function does not have. So the frame here is its own --
/// exactly as in the engine's demo, and for the same reason.
///
/// ## Where the camera looks
///
/// At a point **on the limb** rather than at the subcamera point: looking
/// straight down from low orbit gives a flat field of colour and shows nothing
/// (the demo already established that). The target is a point exactly on the
/// horizon, `acos(R / (R + h))` from the subcamera point, computed rather than
/// tuned.
fn shoot_moon(
    gpu: &Gpu,
    path: &std::path::Path,
    options: &app::Options,
    snapshot: &game::snapshot::WorldSnapshot,
    altitude_km: f64,
) -> Result<(), String> {
    use game::world::{EARTH, MOON};

    let body = |id: i32| {
        snapshot
            .bodies
            .iter()
            .find(|b| b.body == id)
            .ok_or_else(|| format!("body {id} is not in the snapshot"))
    };
    let earth = body(EARTH)?;
    let moon = body(MOON)?;

    // The scene is geocentric (`view.rs`), so the camera must be in the same
    // coordinates: the Moon relative to Earth.
    let centre = [
        moon.position[0] - earth.position[0],
        moon.position[1] - earth.position[1],
        moon.position[2] - earth.position[2],
    ];
    let radius = moon.radius_m;
    if radius <= 0.0 {
        return Err("the Moon has no radius in the asset -- nothing to orbit".into());
    }

    let altitude = altitude_km * 1000.0;
    let distance = radius + altitude;
    // The camera above the body's "northern" side, the target on the horizon
    // towards +x.
    let eye = [centre[0], centre[1], centre[2] + distance];
    let horizon = (radius / distance).acos();
    let target = [
        centre[0] + radius * horizon.sin(),
        centre[1],
        centre[2] + radius * horizon.cos(),
    ];
    // The frame's up is outwards from the body, i.e. sky above, surface below.
    // The same convention as in `engine::demo::along_limb`, for the same
    // reason.
    let camera = engine::camera::Camera::look_at(eye, target, [0.0, 0.0, 1.0]);

    let mut scene = view::build(snapshot, camera);
    let mut frame = engine::frame::Frame::new(gpu, shot::FORMAT);
    match app::load_moon_terrain(gpu, &mut frame) {
        Some(id) => game::view::attach_terrain(&mut scene, snapshot, MOON, id),
        None => println!("the capture will have a smooth Moon"),
    }
    app::load_stars(
        gpu,
        &mut frame,
        std::path::Path::new(app::STAR_CATALOGUE_ASSET),
    );

    let (width, height) = (options.width, options.height);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("moon shot"),
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
    let target_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("moon shot"),
        });
    frame.draw(gpu, &mut encoder, &target_view, width, height, &scene);

    let taken = shot::read_back(gpu, encoder, &texture, width, height)?;
    taken.write_png(path)?;

    println!(
        "capture: {} ({width}x{height}), Moon from {altitude_km:.0} km, target on the limb",
        path.display()
    );
    Ok(())
}
