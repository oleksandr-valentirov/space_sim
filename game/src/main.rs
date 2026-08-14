//! Запуск гри (ROADMAP J1).
//!
//!     cargo run -p game                          вікно
//!     cargo run -p game -- --frames 120          вікно, 120 кадрів і вихід
//!     cargo run -p game -- --shot build/j1.png   знімок без вікна
//!
//! Розбір аргументів свій і навмисно дурний — те саме рішення, що в
//! `engine`: `clap` приїде тоді, коли прапорців стане двадцять.

use std::path::PathBuf;

use engine::gpu::Gpu;
use engine::shot;

use game::{app, mission, view};

fn main() {
    let mut options = app::Options::default();
    let mut shot_path: Option<PathBuf> = None;
    let mut vsync_asked = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| -> String {
            args.next()
                .unwrap_or_else(|| fail(&format!("{name} без значення")))
        };

        match arg.as_str() {
            "--shot" => shot_path = Some(PathBuf::from(value("--shot"))),
            "--frames" => options.frames = Some(parse(&value("--frames"), "--frames")),
            "--asset" => options.asset = PathBuf::from(value("--asset")),
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
            other => fail(&format!("невідомий аргумент {other}\n\n{HELP}")),
        }
    }

    // Обмежений прогін за замовчуванням без vsync: інакше він зависає там, де
    // вікно фактично не показується (engine::window::Options::vsync).
    if options.frames.is_some() && !vsync_asked {
        options.vsync = false;
    }

    let result = match shot_path {
        Some(path) => take_shot(&path, &options),
        None => app::run(options),
    };

    if let Err(e) = result {
        fail(&e);
    }
}

const HELP: &str = "\
  --shot <файл>   намалювати повний прогноз у PNG, без вікна
  --frames <N>    намалювати N кадрів і вийти (вимикає vsync)
  --asset <файл>  ефемерида; типово data/fixture/earth_moon.eph
  --vsync         чекати на вертикальну синхронізацію
  --no-vsync      не чекати
  --width <px>    ширина, типово 1280
  --height <px>   висота, типово 720";

/// Знімок повністю порахованої місії.
///
/// Тікає до кінця, а не N разів: знімок існує, щоб на нього дивитися й
/// звіряти, і половина траєкторії в ньому означала б, що дивимось на швидкість
/// машини, а не на прогноз.
fn take_shot(path: &std::path::Path, options: &app::Options) -> Result<(), String> {
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("адаптер: {}", gpu.describe());

    let mut world = mission::world(&options.asset)
        .map_err(|e| format!("світ не будується ({}): {e}", options.asset.display()))?;
    let ticks = world.run_to_horizon(8);

    let snapshot = world.snapshot();
    for vessel in &snapshot.vessels {
        println!(
            "{}: {} ланок, {} семплів{}",
            vessel.name,
            vessel.legs.len(),
            vessel.sample_count(),
            match vessel.failed {
                Some(e) => format!(", зупинився: {e}"),
                None => String::new(),
            }
        );
    }
    println!("тіків: {ticks}");

    let camera = engine::orbit::Orbit::at_altitude(mission::CAMERA_ALTITUDE_M).camera();
    let scene = view::build(&snapshot, camera);

    let taken = shot::take_scene(&gpu, options.width, options.height, &scene)?;
    taken.write_png(path)?;

    println!(
        "знімок: {} ({}×{})",
        path.display(),
        options.width,
        options.height
    );
    Ok(())
}

fn parse(text: &str, name: &str) -> u32 {
    text.parse()
        .unwrap_or_else(|_| fail(&format!("{name}: '{text}' не є числом")))
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}
