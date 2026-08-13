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
    let mut perf_probe = false;

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
            "--perf-probe" => perf_probe = true,
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
    } else if perf_probe {
        run_perf_probe()
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
  --perf-probe      заміряти час кадру рендера (скіл perf-probe)
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

/// Замір часу кадру рендера (скіл `perf-probe`).
///
/// Друкує min/mean/p95/max у мс і запас до бюджетів 60 fps (16.6 мс) та
/// 30 fps (33.3 мс) для кількох роздільностей. Метод і його межі — див.
/// `engine::perf_probe`.
fn run_perf_probe() -> Result<(), String> {
    use engine::perf_probe::measure;

    const FRAMES: u32 = 300;
    const BUDGET_60: f64 = 1000.0 / 60.0;
    const BUDGET_30: f64 = 1000.0 / 30.0;

    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("адаптер: {}\n", gpu.describe());
    println!(
        "{FRAMES} кадрів на роздільність, синхронний submit+poll (верхня межа, не конвеєр).\n"
    );
    println!(
        "{:>10} {:>8} {:>8} {:>8} {:>8} {:>8} {:>9} {:>9}",
        "розд.", "min мс", "mean мс", "p95 мс", "max мс", "fps", "запас60", "запас30"
    );

    for (width, height) in [(1280, 720), (1920, 1080)] {
        let stats = measure(&gpu, width, height, FRAMES)?;
        println!(
            "{:>5}×{:<4} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.1} {:>+9.3} {:>+9.3}",
            width,
            height,
            stats.min_ms,
            stats.mean_ms,
            stats.p95_ms,
            stats.max_ms,
            stats.fps(),
            stats.headroom_ms(BUDGET_60),
            stats.headroom_ms(BUDGET_30),
        );
    }

    Ok(())
}
