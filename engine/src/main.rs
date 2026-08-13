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

    let result = match shot_path {
        Some(path) => take_shot(&path, options.width, options.height),
        None => app::run(options),
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
