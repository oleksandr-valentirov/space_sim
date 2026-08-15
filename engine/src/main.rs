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
    let mut flight_probe = false;
    let mut trajectory_probe = false;
    let mut live_probe = false;
    let mut rotating_probe = false;

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
            "--flight-probe" => flight_probe = true,
            "--trajectory-probe" => trajectory_probe = true,
            "--live-probe" => live_probe = true,
            "--rotating-probe" => rotating_probe = true,
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
    } else if flight_probe {
        run_flight_probe()
    } else if trajectory_probe {
        run_trajectory_probe()
    } else if live_probe {
        run_live_probe()
    } else if rotating_probe {
        engine::rotating_probe::report();
        Ok(())
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
  --flight-probe    проліт 10 м -> 10⁷ м над сферою (ROADMAP F5)
  --trajectory-probe  halo-орбіта з фікстури, два фрейми (ROADMAP F6)
  --live-probe      та сама орбіта, порахована зараз через core-rs (ROADMAP H5)
  --rotating-probe  де рахувати обертовий фрейм: f32 на GPU чи f64 на CPU (U6a1)
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
    use engine::perf_probe::{camera_pass_ms, measure, patch_pass_ms, Overlay};

    const FRAMES: u32 = 300;
    const BUDGET_60: f64 = 1000.0 / 60.0;
    const BUDGET_30: f64 = 1000.0 / 30.0;

    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("адаптер: {}\n", gpu.describe());
    // Профіль друкується поруч із числами, і це не косметика: вимір
    // CPU-зв'язаний, а між debug і release тут тринадцятикратна різниця в
    // часі кадру. Число без профілю непорівнянне з жодним іншим.
    let profile = if cfg!(debug_assertions) {
        "debug (cargo run)"
    } else {
        "release (cargo run --release)"
    };
    println!(
        "{FRAMES} кадрів на роздільність, синхронний submit+poll (верхня межа, не конвеєр).\n\
         профіль: {profile}\n\
         сцена: одне тіло радіуса Землі, шість патчів 32×32 — 6534 вершини / \
         12288 трикутників, camera-relative раз на патч (R1d), виклик \
         малювання на тіло (R1e)\n"
    );
    println!(
        "{:>10} {:>10} {:>8} {:>8} {:>8} {:>8} {:>8} {:>9} {:>9}",
        "розд.", "інтерфейс", "min мс", "mean мс", "p95 мс", "max мс", "fps", "запас60", "запас30"
    );

    // Три рядки на роздільність в одному прогоні, а не три прогони: різниця
    // між прогонами на одній машині більша за те, що коштує панель.
    for (width, height) in [(1280, 720), (1920, 1080)] {
        for (overlay, label) in [
            (Overlay::None, "немає"),
            (Overlay::EmptyUi, "порожній"),
            (Overlay::Panel, "панель"),
        ] {
            let stats = measure(&gpu, width, height, FRAMES, overlay)?;
            println!(
                "{:>5}×{:<4} {:>10} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.1} {:>+9.3} {:>+9.3}",
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

    // Окремо — CPU-прохід планети, до й після R1d. Два числа, бо одне без
    // другого не каже, чи виграш узагалі є.
    let was_ms = camera_pass_ms(200);
    let now_ms = patch_pass_ms(200);
    println!(
        "\nCPU-прохід планети:\n  \
         було (UV-сфера, 8385 вершин щокадру): {:.1} мкс = {:.2}% бюджету 60 Hz\n  \
         стало (шість патчів, по одному початку): {:.3} мкс = {:.4}%\n  \
         виграш: у {:.0} разів",
        was_ms * 1000.0,
        100.0 * was_ms / BUDGET_60,
        now_ms * 1000.0,
        100.0 * now_ms / BUDGET_60,
        was_ms / now_ms
    );

    Ok(())
}

/// Проліт від поверхні до орбіти (ROADMAP F5).
///
/// Друкує таблицю «висота × покриття кадру» — виміряне проти аналітичного
/// (`asin(R/(R+висота))`, точна формула для опуклої сфери) там, де його
/// можна порахувати без наближень.
fn run_flight_probe() -> Result<(), String> {
    use engine::flight_probe::{expected_coverage, sweep};
    use engine::sphere;

    const SIZE: u32 = 512;
    const STEPS: u32 = 15;

    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("адаптер: {}\n", gpu.describe());

    let mesh = sphere::generate(sphere::EARTH_RADIUS_M, 64, 128);
    println!(
        "меш: R = {:.0} м, {} вершин, {} трикутників\n",
        sphere::EARTH_RADIUS_M,
        mesh.positions.len(),
        mesh.indices.len() / 3
    );

    println!("Частка кадру, яку займає сфера. Виміряне проти аналітичного диска");
    println!("силуету — там, де його можна порахувати без наближень.\n");
    println!(
        "{:>12} {:>10} {:>12} {:>12}",
        "висота, м", "покриття", "аналітично", "різниця"
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
                    "  ПОПЕРЕДЖЕННЯ: покриття зросло з висотою ({previous:.4} -> {:.4}) — \
                     не має бути стрибків",
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
            "знімок: build/f5_surface.png (висота {:.0e} м)",
            first.altitude
        );
    }

    if let Some(last) = samples.last() {
        last.shot
            .write_png(std::path::Path::new("build/f5_flight.png"))?;
        println!(
            "знімок: build/f5_flight.png (висота {:.0e} м)",
            last.altitude
        );
    }

    Ok(())
}

/// Halo-орбіта з етапу C, два фрейми з тих самих вершинних буферів
/// (ROADMAP F6).
///
/// Обидва знімки йдуть з ОДНОГО завантаження й ОДНОГО набору буферів —
/// перемикання фрейму це прапорець в uniform, не перезавантаження вершин.
fn run_trajectory_probe() -> Result<(), String> {
    use engine::trajectory;
    use engine::trajectory_render::{geocentric_framing, render, rotating_framing, Params};

    const SIZE: u32 = 720;

    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("адаптер: {}\n", gpu.describe());

    let samples = trajectory::load();
    println!(
        "траєкторія: {} семплів, {:.1} діб, mu = {}\n",
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
    println!("знімок: build/f6_geocentric.png (інерціальний, геоцентричний)");

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
    println!("знімок: build/f6_rotating.png (обертовий, синодичний, біля L2)");

    Ok(())
}

/// Траєкторія, порахована зараз, а не прочитана з CSV (ROADMAP H5).
///
/// Перший зонд, у якому рушій викликає ядро: стан апарата береться з першого
/// семпла фікстури, а далі все рахує `prop_run` — і поруч, тим самим
/// рендером, малюється сама фікстура, щоб різницю було видно оком, а не лише
/// у числах з `engine/tests/live.rs`.
fn run_live_probe() -> Result<(), String> {
    use engine::live;
    use engine::trajectory;
    use engine::trajectory_render::{render, rotating_framing, Params};

    const SIZE: u32 = 720;
    const DAYS: f64 = 101.79;

    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("адаптер: {}\n", gpu.describe());

    let start = live::fixture_start();
    let live = live::propagate(&start, DAYS, &live::repo_asset())
        .map_err(|e| format!("прогноз не порахувався: {e}"))?;

    println!(
        "прогноз: {} семплів за {} викликів prop_run, {:.1} діб",
        live.samples.len(),
        live.legs,
        (live.samples.last().unwrap().t - start.t) / 86400.0
    );

    // Кадрування спільне — від еталона, — інакше дві картинки мали б різні
    // масштаби й порівнювати їх було б ні до чого.
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
    println!("знімок: build/h5_live.png (порахований зараз)");

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
    println!("знімок: build/h5_reference.png (фікстура, той самий кадр)");

    Ok(())
}
