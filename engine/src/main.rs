//! Starting the engine (ROADMAP F1).
//!
//!     cargo run -p engine                        a window
//!     cargo run -p engine -- --frames 60         a window, 60 frames and exit
//!     cargo run -p engine -- --shot build/f1.png a shot with no window
//!     cargo run -p engine -- --demo build/demo   a captioned series of shots
//!
//! The argument parsing is ours and deliberately dumb: three flags are not
//! worth a dependency, and `clap` arrives when there are twenty of them.

use std::path::PathBuf;

use engine::app;
use engine::gpu::Gpu;
use engine::shot;

fn main() {
    let mut options = app::Options::default();
    let mut shot_path: Option<PathBuf> = None;
    let mut ship_demo: Option<PathBuf> = None;
    let mut moon_demo: Option<PathBuf> = None;
    let mut flyby_demo: Option<PathBuf> = None;
    let mut demo_dir: Option<PathBuf> = None;
    let mut vsync_asked = false;
    let mut depth_probe = false;
    let mut perf_probe = false;
    let mut flight_probe = false;
    let mut trajectory_probe = false;
    let mut live_probe = false;
    let mut rotating_probe = false;
    let mut tile_probe = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| -> String {
            args.next()
                .unwrap_or_else(|| fail(&format!("{name} without a value")))
        };

        match arg.as_str() {
            "--shot" => shot_path = Some(PathBuf::from(value("--shot"))),
            "--ship-demo" => ship_demo = Some(PathBuf::from(value("--ship-demo"))),
            "--moon-demo" => moon_demo = Some(PathBuf::from(value("--moon-demo"))),
            "--flyby-demo" => flyby_demo = Some(PathBuf::from(value("--flyby-demo"))),
            "--demo" => demo_dir = Some(PathBuf::from(value("--demo"))),
            "--frames" => options.frames = Some(parse(&value("--frames"), "--frames")),
            "--vsync" => {
                options.vsync = true;
                vsync_asked = true;
            }
            "--no-vsync" => {
                options.vsync = false;
                vsync_asked = true;
            }
            "--width" => options.width = parse(&value("--width"), "--width"),
            "--height" => options.height = parse(&value("--height"), "--height"),
            "--depth-probe" => depth_probe = true,
            "--perf-probe" => perf_probe = true,
            "--flight-probe" => flight_probe = true,
            "--trajectory-probe" => trajectory_probe = true,
            "--live-probe" => live_probe = true,
            "--rotating-probe" => rotating_probe = true,
            "--tile-probe" => tile_probe = true,
            "--help" | "-h" => {
                println!("{}", HELP);
                return;
            }
            other => fail(&format!("unknown argument {other}\n\n{HELP}")),
        }
    }

    // A bounded run defaults to no vsync: otherwise it hangs where the window
    // is not actually shown (see app::Options::vsync).
    if options.frames.is_some() && !vsync_asked {
        options.vsync = false;
    }

    let result = if let Some(dir) = demo_dir {
        run_demo(&dir)
    } else if depth_probe {
        run_depth_probe()
    } else if perf_probe {
        run_perf_probe()
    } else if flight_probe {
        run_flight_probe()
    } else if trajectory_probe {
        run_trajectory_probe()
    } else if live_probe {
        run_live_probe()
    } else if rotating_probe {
        engine::rotating_probe::report();
        Ok(())
    } else if tile_probe {
        engine::tile_probe::report()
    } else if let Some(path) = flyby_demo {
        run_flyby_demo(&path, options.width, options.height, options.frames)
    } else if let Some(path) = moon_demo {
        run_moon_demo(&path, options.width, options.height, options.frames)
    } else if let Some(path) = ship_demo {
        run_ship_demo(&path, options.width, options.height, options.frames)
    } else {
        match shot_path {
            Some(path) => take_shot(&path, options.width, options.height),
            None => app::run(options),
        }
    };

    if let Err(e) = result {
        fail(&e);
    }
}

const HELP: &str = "\
  --demo <dir>      a captioned series of shots of the renderer's current state
  --shot <file>     draw one frame into a PNG, with no window
  --ship-demo <file> an APNG animation of the ship in orbit, 60 fps
  --moon-demo <file> an APNG animation of the approach to the Moon, 60 fps
  --frames <N>      draw N frames and exit (disables vsync)
  --vsync           wait for vertical sync
  --no-vsync        do not wait
  --depth-probe     measure depth resolution (ROADMAP F3)
  --perf-probe      measure render frame time (the perf-probe skill)
  --flight-probe    a flight from 10 m to 1e7 m above a sphere (ROADMAP F5)
  --trajectory-probe  the halo orbit from the fixture, two frames (ROADMAP F6)
  --live-probe      the same orbit computed now through core-rs (ROADMAP H5)
  --rotating-probe  where to compute the rotating frame: f32 on the GPU or f64 on the CPU (U6a1)
  --tile-probe      how many tiles a bindless array takes and what it costs (T2)
  --width <px>      width, 1280 by default
  --height <px>     height, 720 by default";

/// A series of shots of the renderer's current state (the demo).
///
/// Prints the captions alongside, because a picture without a caption does not
/// say what it proves. The directory is overwritten; the manifest is written
/// there too, so the captions do not live apart from the files.
fn run_demo(dir: &std::path::Path) -> Result<(), String> {
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("adapter: {}", gpu.describe());

    let frames = engine::demo::render(&gpu, dir)?;

    let mut manifest = String::new();
    println!();
    for frame in &frames {
        println!("{}\n    {}\n", frame.name, frame.caption);
        manifest.push_str(&format!("{}.png\n    {}\n\n", frame.name, frame.caption));
    }
    std::fs::write(dir.join("manifest.txt"), manifest).map_err(|e| e.to_string())?;
    println!("{} frames in {}", frames.len(), dir.display());
    Ok(())
}

/// An animation of the ship in orbit (stage V, V2). The frame count comes from
/// `--frames`, [`engine::ship_demo::FRAMES`] by default -- four seconds at
/// 60 fps.
fn run_ship_demo(
    path: &std::path::Path,
    width: u32,
    height: u32,
    frames: Option<u32>,
) -> Result<(), String> {
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("adapter: {}", gpu.describe());
    let frames = frames.unwrap_or(engine::ship_demo::FRAMES);

    let started = std::time::Instant::now();
    engine::ship_demo::render(&gpu, width, height, frames, path)?;
    let seconds = started.elapsed().as_secs_f64();

    println!(
        "animation: {} ({}x{}, {} frames, {} fps -- {:.1} s of video)",
        path.display(),
        width,
        height,
        frames,
        engine::ship_demo::FPS,
        f64::from(frames) / f64::from(engine::ship_demo::FPS)
    );
    println!(
        "drawing: {seconds:.1} s, {:.1} ms per frame",
        seconds * 1000.0 / f64::from(frames)
    );
    Ok(())
}

/// A flyby past the Moon on an elliptical orbit (a stage-T probe). The frame
/// count comes from `--frames`, [`engine::flyby_demo::FRAMES`] by default.
fn run_flyby_demo(
    path: &std::path::Path,
    width: u32,
    height: u32,
    frames: Option<u32>,
) -> Result<(), String> {
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("adapter: {}", gpu.describe());
    let frames = frames.unwrap_or(engine::flyby_demo::FRAMES);

    let started = std::time::Instant::now();
    engine::flyby_demo::render(&gpu, width, height, frames, path)?;
    let seconds = started.elapsed().as_secs_f64();

    println!(
        "animation: {} ({}x{}, {} frames, {:.1} s of video)",
        path.display(),
        width,
        height,
        frames,
        f64::from(frames) / f64::from(engine::flyby_demo::FPS)
    );
    println!(
        "drawing: {:.1} s, {:.1} ms per frame",
        seconds,
        1000.0 * seconds / f64::from(frames)
    );
    Ok(())
}

fn run_moon_demo(
    path: &std::path::Path,
    width: u32,
    height: u32,
    frames: Option<u32>,
) -> Result<(), String> {
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("adapter: {}", gpu.describe());
    let frames = frames.unwrap_or(engine::moon_demo::FRAMES);

    let started = std::time::Instant::now();
    engine::moon_demo::render(&gpu, width, height, frames, path)?;
    let seconds = started.elapsed().as_secs_f64();

    println!(
        "animation: {} ({}x{}, {} frames, {:.1} s of video)",
        path.display(),
        width,
        height,
        frames,
        f64::from(frames) / f64::from(engine::moon_demo::FPS)
    );
    println!(
        "drawing: {seconds:.1} s, {:.1} ms per frame",
        seconds * 1000.0 / f64::from(frames)
    );
    Ok(())
}

fn take_shot(path: &std::path::Path, width: u32, height: u32) -> Result<(), String> {
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("adapter: {}", gpu.describe());

    let shot = shot::take(&gpu, width, height)?;
    shot.write_png(path)?;

    println!("shot: {} ({}x{})", path.display(), width, height);
    println!("centre pixel: {:?}", shot.pixel(width / 2, height / 2));
    Ok(())
}

fn parse(text: &str, name: &str) -> u32 {
    text.parse()
        .unwrap_or_else(|_| fail(&format!("{name}: '{text}' is not a number")))
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

/// Measuring depth resolution (ROADMAP F3).
///
/// Prints a "distance x gap" table for reversed-Z and for the conventional
/// projection side by side. Without the second column the first would be a
/// claim without a comparison.
fn run_depth_probe() -> Result<(), String> {
    use engine::depth;
    use engine::depth_probe::{measure, Setup};

    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("adapter: {}\n", gpu.describe());

    let near = 0.1;
    println!("Share of the frame where the nearer surface is in front. 1.000 --");
    println!("depth resolves; 0.000 -- draw order won; between -- z-fighting.\n");
    println!("near = {near} m, Depth32Float, 60 degree field of view\n");
    println!(
        "{:>12} {:>10} {:>12} {:>12} {:>10}",
        "distance, m", "gap, m", "reversed-Z", "conventional", "limit, m"
    );

    for distance in [1e4, 1e5, 1e6, 1e7, 1e8] {
        for gap in [1.0, 100.0] {
            let make = |reversed| Setup {
                reversed,
                near,
                distance,
                gap,
            };

            let r = measure(&gpu, 256, 256, &make(true))?;
            let c = measure(&gpu, 256, 256, &make(false))?;

            println!(
                "{distance:>12.0e} {gap:>10.0} {:>12.3} {:>12.3} {:>10.3}",
                r.near_wins,
                c.near_wins,
                depth::resolvable_gap(distance)
            );
        }
    }

    // A shot of the most interesting case: where reversed-Z still holds and
    // the conventional projection no longer does.
    let shown = measure(
        &gpu,
        480,
        270,
        &Setup {
            reversed: true,
            near,
            distance: 1e7,
            gap: 1.0,
        },
    )?;
    shown
        .shot
        .write_png(std::path::Path::new("build/f3_reversed.png"))?;
    println!("\nshot: build/f3_reversed.png (1e7 m, 1 m gap, reversed-Z)");

    Ok(())
}

/// Measuring render frame time (the `perf-probe` skill).
///
/// Prints min/mean/p95/max in ms and the headroom against the 60 fps (16.6 ms)
/// and 30 fps (33.3 ms) budgets for several resolutions. The method and its
/// limits -- see `engine::perf_probe`.
fn run_perf_probe() -> Result<(), String> {
    use engine::perf_probe::{camera_pass_ms, measure, patch_pass_ms, Overlay};

    const FRAMES: u32 = 300;

    // Two altitudes rather than one (R8): from afar LOD gives the planet a
    // handful of patches, from low orbit dozens. One number stopped describing
    // the frame exactly when the patch set began to depend on the camera.
    const ALTITUDES_M: [(f64, &str); 2] = [
        (engine::frame::DEFAULT_ALTITUDE_M, "1e7 m"),
        (1.0e5, "100 km"),
    ];
    const BUDGET_60: f64 = 1000.0 / 60.0;
    const BUDGET_30: f64 = 1000.0 / 30.0;

    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("adapter: {}\n", gpu.describe());
    // The profile is printed next to the numbers, and that is not cosmetic:
    // the measurement is CPU-bound, and between debug and release there is a
    // thirteenfold difference in frame time. A number without its profile is
    // comparable with nothing.
    let profile = if cfg!(debug_assertions) {
        "debug (cargo run)"
    } else {
        "release (cargo run --release)"
    };
    println!(
        "{FRAMES} frames per resolution, synchronous submit+poll (an upper bound, not a pipeline).\n\
         profile: {profile}\n\
         scene: one body of Earth's radius, 32x32 patches, the set chosen by LOD \
         from screen-space error (R2a); limb and frustum culling in compute, \
         drawing by `draw_indirect`, one call per body (R6)\n"
    );
    println!(
        "{:>8} {:>10} {:>10} {:>8} {:>8} {:>8} {:>8} {:>8} {:>9} {:>9}",
        "altitude",
        "res.",
        "overlay",
        "min ms",
        "mean ms",
        "p95 ms",
        "max ms",
        "fps",
        "head60",
        "head30"
    );

    // Three rows per resolution in one run rather than three runs: the
    // difference between runs on one machine is larger than what a panel
    // costs.
    for (altitude, altitude_label) in ALTITUDES_M {
        for (width, height) in [(1280, 720), (1920, 1080)] {
            for (overlay, label) in [
                (Overlay::None, "none"),
                (Overlay::EmptyUi, "empty"),
                (Overlay::Panel, "panel"),
            ] {
                let stats = measure(&gpu, width, height, FRAMES, overlay, altitude)?;
                println!(
                "{:>8} {:>5}×{:<4} {:>10} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.1} {:>+9.3} {:>+9.3}",
                altitude_label,
                width,
                height,
                label,
                stats.min_ms,
                stats.mean_ms,
                stats.p95_ms,
                stats.max_ms,
                stats.fps(),
                stats.headroom_ms(BUDGET_60),
                stats.headroom_ms(BUDGET_30),
            );
            }
        }
    }

    // Separately -- the cost of air (stage S). Five altitudes: three "ordinary"
    // ones and two on either side of the S5 condition at which the
    // aerial-perspective volume stops being computed. The last pair is the same
    // scene to within eight percent of the distance, so the difference between
    // its rows is the cost of the volume.
    println!(
        "\nAir (stage S). The condition: it is drawn only when the layer's thickness\n\
         in the frame is at least a pixel -- for Earth that is 6.24e7 m.\n"
    );
    println!(
        "{:>10} {:>12} {:>10} {:>10} {:>10} {:>8}",
        "altitude", "air", "without, ms", "with air", "difference", "times"
    );
    for (altitude, label, note) in [
        (1.0e4, "10 km", "drawn"),
        (5.0e5, "500 km", "drawn"),
        (1.0e7, "1e7 m", "drawn"),
        (6.0e7, "6.0e7 m", "drawn"),
        (6.5e7, "6.5e7 m", "skipped"),
        (1.0e9, "1e9 m", "skipped"),
    ] {
        let bare = engine::perf_probe::air_cost(&gpu, 1280, 720, FRAMES, altitude, false)?;
        let with_air = engine::perf_probe::air_cost(&gpu, 1280, 720, FRAMES, altitude, true)?;
        println!(
            "{:>10} {:>12} {:>10.3} {:>10.3} {:>+10.3} {:>8.2}",
            label,
            note,
            bare.mean_ms,
            with_air.mean_ms,
            with_air.mean_ms - bare.mean_ms,
            with_air.mean_ms / bare.mean_ms
        );
    }

    // Separately -- the cost of the ship in the frame (stage V). Two altitudes,
    // and the difference between them is the main thing: in low orbit the ship
    // adds a **depth pass**, because `near` comes from the hull rather than
    // from the altitude.
    println!(
        "\nThe ship in the frame (stage V). The difference is not only the cost of\n\
         1614 vertices: a hull fifteen metres away pulls `near`, and `near` decides\n\
         the number of depth passes. The third row separates one from the other.\n"
    );
    println!(
        "{:>10} {:>20} {:>10} {:>12} {:>10} {:>8}",
        "altitude", "ship", "without, ms", "with ship", "difference", "times"
    );
    for (altitude, label, range, note) in [
        (4.0e5, "400 km", 15.0, "15 m, two passes"),
        (1.0e7, "1e7 m", 15.0, "15 m, two passes"),
        // The same mesh but far away: `near` stays large and there is one pass.
        // So this row is the cost of the drawing alone, without a range.
        (1.0e7, "1e7 m", 1.0e6, "1e6 m, one pass"),
    ] {
        let bare = engine::perf_probe::ship_cost(&gpu, 1280, 720, FRAMES, altitude, None)?;
        let with_ship =
            engine::perf_probe::ship_cost(&gpu, 1280, 720, FRAMES, altitude, Some(range))?;
        println!(
            "{:>10} {:>20} {:>10.3} {:>12.3} {:>+10.3} {:>8.2}",
            label,
            note,
            bare.mean_ms,
            with_ship.mean_ms,
            with_ship.mean_ms - bare.mean_ms,
            with_ship.mean_ms / bare.mean_ms
        );
    }

    // Separately -- the cost of colour tiles (stage T, T8). The asset may not be
    // on disk (`/assets/` is not in git), and then the row simply is not there:
    // it cannot be invented, and a synthetic pyramid would measure the wrong
    // thing.
    match tile_assets() {
        Some((terrain, colour)) => {
            println!(
                "\nColour tiles in the frame (stage T). The difference is a second\n\
                 bindless sample per fragment and twice the texture array in the bind group.\n"
            );
            println!(
                "{:>10} {:>10} {:>12} {:>10} {:>8}",
                "altitude", "without, ms", "with colour", "difference", "times"
            );
            // The last row is the one that separates the two explanations of
            // the difference. If the cost of colour is in the sampling, it must
            // fall along with the number of covered pixels; if it is in binding
            // the texture array, it will remain.
            for (altitude, label) in [
                (1.0e5, "100 km"),
                (1.0e6, "1e6 m"),
                (1.0e7, "1e7 m"),
                (1.0e9, "1e9 m"),
            ] {
                let bare = engine::perf_probe::tile_cost(
                    &gpu, 1280, 720, FRAMES, altitude, &terrain, None,
                )?;
                let with_colour = engine::perf_probe::tile_cost(
                    &gpu,
                    1280,
                    720,
                    FRAMES,
                    altitude,
                    &terrain,
                    Some(&colour),
                )?;
                println!(
                    "{:>10} {:>10.3} {:>12.3} {:>+10.3} {:>8.2}",
                    label,
                    bare.mean_ms,
                    with_colour.mean_ms,
                    with_colour.mean_ms - bare.mean_ms,
                    with_colour.mean_ms / bare.mean_ms
                );
            }
        }
        None => println!(
            "\nColour tiles: the assets are not on disk -- the row is skipped.\n               to fix: make cook-dem && make cook-colour"
        ),
    }

    // And separately -- what that cost depends on. The pyramid is truncated by
    // levels, so **only the number of textures in the array** changes: the
    // scene, the camera, the altitude and the number of covered pixels stay the
    // same.
    if let Some((terrain, colour)) = tile_assets() {
        println!(
            "\nWhat the cost of colour depends on: the camera at 1e9 m, the body a\n\
             few pixels across, only the pyramid depth changing.\n"
        );
        println!("{:>8} {:>10} {:>10}", "levels", "tiles", "frame, ms");
        for levels in 1..=colour.levels {
            let short = truncated(&colour, levels);
            let stats = engine::perf_probe::tile_cost(
                &gpu,
                1280,
                720,
                FRAMES,
                1.0e9,
                &terrain,
                Some(&short),
            )?;
            println!(
                "{:>8} {:>10} {:>10.3}",
                levels,
                engine::tiles::count(levels),
                stats.mean_ms
            );
        }
    }

    // Debt D19 on its own threshold: two tiled bodies in one frame (T7h). The
    // debt's question is literal -- a texture array pays for its size rather
    // than for what is drawn -- so it is the sum that is checked.
    match (
        surface_assets("assets/moon.dem", "assets/moon.col"),
        surface_assets("assets/earth.dem", "assets/earth.col"),
    ) {
        (Some((moon_dem, moon_col)), Some((earth_dem, earth_col))) => {
            let textures = |t: &engine::tiles::Terrain, c: &engine::tiles::Colour| {
                engine::tiles::count(t.levels) + engine::tiles::count(c.levels)
            };
            let moon_textures = textures(&moon_dem, &moon_col);
            let earth_textures = textures(&earth_dem, &earth_col);
            println!(
                "\nTwo tiled bodies (debt D19): the camera at 1e9 m, both bodies a\n\
                 few pixels across -- the difference is array binding, not drawing.\n\n\
                 Moon {moon_textures} textures, Earth {earth_textures}, {} together.\n",
                moon_textures + earth_textures
            );
            println!("{:>16} {:>10} {:>12}", "scene", "frame, ms", "ns/texture");
            for (label, first, second, count) in [
                (
                    "Moon only",
                    (&moon_dem, Some(&moon_col)),
                    None,
                    moon_textures,
                ),
                (
                    "Earth only",
                    (&earth_dem, Some(&earth_col)),
                    None,
                    earth_textures,
                ),
                (
                    "both",
                    (&moon_dem, Some(&moon_col)),
                    Some((&earth_dem, Some(&earth_col))),
                    moon_textures + earth_textures,
                ),
            ] {
                let stats = engine::perf_probe::two_body_cost(
                    &gpu, 1280, 720, FRAMES, 1.0e9, first, second,
                )?;
                println!(
                    "{:>16} {:>10.3} {:>12.1}",
                    label,
                    stats.mean_ms,
                    stats.mean_ms * 1.0e6 / count as f64
                );
            }
        }
        _ => println!(
            "\nTwo tiled bodies (D19): both assets are missing from disk -- row skipped.\n\
             \x20              to fix: make cook-dem && make cook-colour, then\n\
             \x20              cargo run -p dem-cook -- --body earth [--colour]"
        ),
    }

    // Separately -- the planet's CPU pass, before and after R1d. Two numbers,
    // because one without the other does not say whether there is a gain at
    // all.
    let was_ms = camera_pass_ms(200);
    let now_ms = patch_pass_ms(200);
    println!(
        "\nThe planet's CPU pass:\n  \
         was (UV sphere, 8385 vertices per frame): {:.1} us = {:.2}% of the 60 Hz budget\n  \
         now (one origin per patch): {:.3} us = {:.4}%\n  \
         gain: {:.0} times",
        was_ms * 1000.0,
        100.0 * was_ms / BUDGET_60,
        now_ms * 1000.0,
        100.0 * now_ms / BUDGET_60,
        was_ms / now_ms
    );

    Ok(())
}

/// The Moon's cooked tiles, if they are on disk.
///
/// Both together or neither: the measurement compares a frame **with colour and
/// without**, and half the pair gives neither.
fn tile_assets() -> Option<(engine::tiles::Terrain, engine::tiles::Colour)> {
    surface_assets("assets/moon.dem", "assets/moon.col")
}

/// A cooked surface from disk -- terrain and colour together, or nothing.
fn surface_assets(dem: &str, col: &str) -> Option<(engine::tiles::Terrain, engine::tiles::Colour)> {
    let terrain = engine::tiles::Terrain::from_bytes(&std::fs::read(dem).ok()?).ok()?;
    let colour = engine::tiles::Colour::from_bytes(&std::fs::read(col).ok()?).ok()?;
    Some((terrain, colour))
}

/// The same colour pyramid, truncated to `levels` levels.
///
/// Needed by one measurement: what the **size of the texture array** costs by
/// itself. Truncating by levels keeps the pyramid's geometry correct -- the
/// tiles lie level by level (`tiles::index`), so the first `count(levels)` of
/// them are a complete pyramid of smaller depth.
fn truncated(colour: &engine::tiles::Colour, levels: u32) -> engine::tiles::Colour {
    let grids: Vec<Vec<u8>> = (0..engine::tiles::count(levels))
        .map(|i| colour.tile_bytes(i).to_vec())
        .collect();
    engine::tiles::Colour::build(levels, colour.channels, colour.scale, colour.srgb, &grids)
}

/// A flight from the surface to orbit (ROADMAP F5).
///
/// Prints an "altitude x frame coverage" table -- measured against analytic
/// (`asin(R/(R+altitude))`, the exact formula for a convex sphere) wherever it
/// can be computed without approximations.
fn run_flight_probe() -> Result<(), String> {
    use engine::flight_probe::{expected_coverage, sweep};
    use engine::sphere;

    const SIZE: u32 = 512;
    const STEPS: u32 = 15;

    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("adapter: {}\n", gpu.describe());

    let mesh = sphere::generate(sphere::EARTH_RADIUS_M, 64, 128);
    println!(
        "mesh: R = {:.0} m, {} vertices, {} triangles\n",
        sphere::EARTH_RADIUS_M,
        mesh.positions.len(),
        mesh.indices.len() / 3
    );

    println!("Share of the frame the sphere takes. Measured against the analytic");
    println!("silhouette disc -- where it can be computed without approximations.\n");
    println!(
        "{:>12} {:>10} {:>12} {:>12}",
        "altitude, m", "coverage", "analytic", "difference"
    );

    let samples = sweep(&gpu, SIZE, &mesh, STEPS)?;
    let mut previous_coverage: Option<f64> = None;
    for s in &samples {
        let expected = expected_coverage(s.expected_half_angle, 1.0);
        let expected_text = expected.map_or("—".to_string(), |e| format!("{e:.3}"));
        let diff_text = expected.map_or("—".to_string(), |e| format!("{:+.4}", s.coverage - e));

        println!(
            "{:>12.1e} {:>10.3} {:>12} {:>12}",
            s.altitude, s.coverage, expected_text, diff_text
        );

        if let Some(previous) = previous_coverage {
            if s.coverage > previous + 1e-9 {
                println!(
                    "  WARNING: coverage grew with altitude ({previous:.4} -> {:.4}) -- \
                     there must be no jumps",
                    s.coverage
                );
            }
        }
        previous_coverage = Some(s.coverage);
    }

    if let Some(first) = samples.first() {
        first
            .shot
            .write_png(std::path::Path::new("build/f5_surface.png"))?;
        println!(
            "shot: build/f5_surface.png (altitude {:.0e} m)",
            first.altitude
        );
    }

    if let Some(last) = samples.last() {
        last.shot
            .write_png(std::path::Path::new("build/f5_flight.png"))?;
        println!(
            "shot: build/f5_flight.png (altitude {:.0e} m)",
            last.altitude
        );
    }

    Ok(())
}

/// The halo orbit from stage C, two frames from the same vertex buffers
/// (ROADMAP F6).
///
/// Both shots come from ONE load and ONE set of buffers -- switching frames is a
/// flag in a uniform, not a re-upload of vertices.
fn run_trajectory_probe() -> Result<(), String> {
    use engine::trajectory;
    use engine::trajectory_render::{geocentric_framing, render, rotating_framing, Params};

    const SIZE: u32 = 720;

    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("adapter: {}\n", gpu.describe());

    let samples = trajectory::load();
    println!(
        "trajectory: {} samples, {:.1} days, mu = {}\n",
        samples.len(),
        samples.last().unwrap().t / 86400.0,
        trajectory::MU
    );

    let geocentric = render(
        &gpu,
        SIZE,
        SIZE,
        &samples,
        &Params {
            rotating: false,
            framing: geocentric_framing(&samples),
            colour: [0.9, 0.6, 0.2, 1.0],
        },
    )?;
    geocentric.write_png(std::path::Path::new("build/f6_geocentric.png"))?;
    println!("shot: build/f6_geocentric.png (inertial, geocentric)");

    let rotating = render(
        &gpu,
        SIZE,
        SIZE,
        &samples,
        &Params {
            rotating: true,
            framing: rotating_framing(&samples),
            colour: [0.3, 0.8, 0.9, 1.0],
        },
    )?;
    rotating.write_png(std::path::Path::new("build/f6_rotating.png"))?;
    println!("shot: build/f6_rotating.png (rotating, synodic, near L2)");

    Ok(())
}

/// A trajectory computed now rather than read from a CSV (ROADMAP H5).
///
/// The first probe in which the engine calls the core: the vessel state is taken
/// from the fixture's first sample, and everything after that is computed by
/// `prop_run` -- with the fixture itself drawn alongside by the same renderer,
/// so the difference is visible by eye and not only in the numbers from
/// `engine/tests/live.rs`.
fn run_live_probe() -> Result<(), String> {
    use engine::live;
    use engine::trajectory;
    use engine::trajectory_render::{render, rotating_framing, Params};

    const SIZE: u32 = 720;
    const DAYS: f64 = 101.79;

    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("adapter: {}\n", gpu.describe());

    let start = live::fixture_start();
    let live = live::propagate(&start, DAYS, &live::repo_asset())
        .map_err(|e| format!("the prediction did not compute: {e}"))?;

    println!(
        "prediction: {} samples over {} prop_run calls, {:.1} days",
        live.samples.len(),
        live.legs,
        (live.samples.last().unwrap().t - start.t) / 86400.0
    );

    // The framing is shared -- taken from the reference -- otherwise the two
    // pictures would have different scales and comparing them would be
    // pointless.
    let reference = trajectory::load();
    let framing = rotating_framing(&reference);

    let shot = render(
        &gpu,
        SIZE,
        SIZE,
        &live.samples,
        &Params {
            rotating: true,
            framing,
            colour: [0.9, 0.6, 0.2, 1.0],
        },
    )?;
    shot.write_png(std::path::Path::new("build/h5_live.png"))?;
    println!("shot: build/h5_live.png (computed now)");

    let shot = render(
        &gpu,
        SIZE,
        SIZE,
        &reference,
        &Params {
            rotating: true,
            framing,
            colour: [0.3, 0.8, 0.9, 1.0],
        },
    )?;
    shot.write_png(std::path::Path::new("build/h5_reference.png"))?;
    println!("shot: build/h5_reference.png (the fixture, the same framing)");

    Ok(())
}
