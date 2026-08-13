//! Запуск рушія (ROADMAP F1).
//!
//!     cargo run -p engine                        вікно
//!     cargo run -p engine -- --frames 60         вікно, 60 кадрів і вихід
//!     cargo run -p engine -- --shot build/f1.png знімок без вікна
//!
//! Розбір аргументів свій і навмисно дурний: три прапорці не варті
//! залежності, а `clap` приїде тоді, коли їх стане двадцять.

use std::path::PathBuf;

use engine::app;
use engine::gpu::Gpu;
use engine::shot;

fn main() {
    let mut options = app::Options::default();
    let mut shot_path: Option<PathBuf> = None;
    let mut vsync_asked = false;
    let mut depth_probe = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| -> String {
            args.next()
                .unwrap_or_else(|| fail(&format!("{name} без значення")))
        };

        match arg.as_str() {
            "--shot" => shot_path = Some(PathBuf::from(value("--shot"))),
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
            "--help" | "-h" => {
                println!("{}", HELP);
                return;
            }
            other => fail(&format!("невідомий аргумент {other}\n\n{HELP}")),
        }
    }

    // Обмежений прогін за замовчуванням без vsync: інакше він зависає там,
    // де вікно фактично не показується (див. app::Options::vsync).
    if options.frames.is_some() && !vsync_asked {
        options.vsync = false;
    }

    let result = if depth_probe {
        run_depth_probe()
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
  --shot <файл>     намалювати один кадр у PNG, без вікна
  --frames <N>      намалювати N кадрів і вийти (вимикає vsync)
  --vsync           чекати на вертикальну синхронізацію
  --no-vsync        не чекати
  --depth-probe     заміряти роздільність глибини (ROADMAP F3)
  --width <px>      ширина, типово 1280
  --height <px>     висота, типово 720";

fn take_shot(path: &std::path::Path, width: u32, height: u32) -> Result<(), String> {
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("адаптер: {}", gpu.describe());

    let shot = shot::take(&gpu, width, height)?;
    shot.write_png(path)?;

    println!("знімок: {} ({}×{})", path.display(), width, height);
    println!("піксель у центрі: {:?}", shot.pixel(width / 2, height / 2));
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

/// Замір роздільності глибини (ROADMAP F3).
///
/// Друкує таблицю «відстань × зазор» для reversed-Z і для звичайної
/// проєкції поруч. Без другого стовпця перший був би твердженням без
/// порівняння.
fn run_depth_probe() -> Result<(), String> {
    use engine::depth;
    use engine::depth_probe::{measure, Setup};

    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("адаптер: {}\n", gpu.describe());

    let near = 0.1;
    println!("Частка кадру, де ближча поверхня попереду. 1.000 — глибина");
    println!("роздільна; 0.000 — виграв порядок малювання; між — z-fighting.\n");
    println!("near = {near} м, Depth32Float, поле зору 60°\n");
    println!(
        "{:>12} {:>10} {:>12} {:>12} {:>10}",
        "відстань, м", "зазор, м", "reversed-Z", "звичайна", "межа, м"
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

    // Знімок найцікавішого випадку: там, де reversed-Z ще тримає, а
    // звичайна проєкція вже ні.
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
    println!("\nзнімок: build/f3_reversed.png (10⁷ м, зазор 1 м, reversed-Z)");

    Ok(())
}
