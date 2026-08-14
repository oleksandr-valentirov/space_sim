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
    let mut day: Option<f64> = None;
    let mut save_path: Option<PathBuf> = None;
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
            "--day" => day = Some(parse_f64(&value("--day"), "--day")),
            "--demo-plan" => options.demo_plan = true,
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
            other => fail(&format!("невідомий аргумент {other}\n\n{HELP}")),
        }
    }

    // Обмежений прогін за замовчуванням без vsync: інакше він зависає там, де
    // вікно фактично не показується (engine::window::Options::vsync).
    if options.frames.is_some() && !vsync_asked {
        options.vsync = false;
    }

    let result = match shot_path {
        Some(path) => take_shot(&path, &options, day, save_path.as_deref()),
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
  --day <N>       зупинити курсор на добі N місії (для --shot); типово кінець
  --demo-plan     додати показовий маневр на 10-й добі (ROADMAP J3)
  --load <файл>   підняти гру з сейву замість нової місії (ROADMAP J6)
  --save <файл>   записати сейв після прогону (для --shot); у вікні це F5
  --vsync         чекати на вертикальну синхронізацію
  --no-vsync      не чекати
  --width <px>    ширина, типово 1280
  --height <px>   висота, типово 720";

/// Знімок місії, доведеної до кінця.
///
/// Проганяється до кінця, а не N кадрів: знімок існує, щоб на нього дивитися
/// й звіряти, і половина місії в ньому означала б, що дивимось на швидкість
/// машини, а не на прогноз. Курсор при цьому теж доходить до кінця, тож на
/// картинці все — історія; де саме він стоїть, показує `--frames`.
fn take_shot(
    path: &std::path::Path,
    options: &app::Options,
    day: Option<f64>,
    save_path: Option<&std::path::Path>,
) -> Result<(), String> {
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("адаптер: {}", gpu.describe());

    let mut world = app::build_world(options)?;
    // Секунда «реального» часу на крок: курсор усе одно впирається в
    // горизонт, тобто темп задає інтегратор, а не це число.
    let steps = match day {
        // Курсор ведеться тим самим `step`, що й у вікні, а не ставиться
        // напряму: інакше знімок показував би стан, у який гра потрапити не
        // може.
        Some(day) => world.run_to_day(mission::start().t + day * 86400.0, 1.0, 8),
        None => world.run_to_end(1.0, 8),
    };

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
    println!(
        "кроків: {steps}, курсор на добі {:.2}",
        (snapshot.t - mission::start().t) / 86400.0
    );

    if let Some(save_path) = save_path {
        game::save::write_world(&world, save_path)?;
        println!("сейв: {}", save_path.display());
    }

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

fn parse_f64(text: &str, name: &str) -> f64 {
    text.parse()
        .unwrap_or_else(|_| fail(&format!("{name}: '{text}' не є числом")))
}

fn parse(text: &str, name: &str) -> u32 {
    text.parse()
        .unwrap_or_else(|_| fail(&format!("{name}: '{text}' не є числом")))
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}
