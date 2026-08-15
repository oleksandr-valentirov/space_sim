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
    let mut frame = game::frame_view::ViewFrame::Inertial;
    let mut vsync_asked = false;
    let mut perf_probe_days: Option<f64> = None;
    let mut moon_altitude_km: Option<f64> = None;

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
            "--rotating" => frame = game::frame_view::ViewFrame::Rotating,
            "--moon" => moon_altitude_km = Some(parse_f64(&value("--moon"), "--moon")),
            "--perf-probe" => {
                perf_probe_days = Some(parse_f64(&value("--perf-probe"), "--perf-probe"))
            }
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

    let result = match (perf_probe_days, shot_path) {
        // 300 кадрів — стільки ж, скільки міряє зонд рушія, щоб числа лягали
        // в одну таблицю ROADMAP.
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
  --shot <файл>   намалювати повний прогноз у PNG, без вікна
  --frames <N>    намалювати N кадрів і вийти (вимикає vsync)
  --asset <файл>  ефемерида; типово data/fixture/earth_moon.eph
  --day <N>       зупинити курсор на добі N місії (для --shot); типово кінець
  --demo-plan     додати показовий маневр на 10-й добі (ROADMAP J3)
  --rotating      знімок у обертовому фреймі Земля-Місяць (U6a); типово
                  інерціальний. У вікні це кнопка панелі «VIEW»
  --load <файл>   підняти гру з сейву замість нової місії (ROADMAP J6)
  --save <файл>   записати сейв після прогону (для --shot); у вікні це F5
  --vsync         чекати на вертикальну синхронізацію
  --no-vsync      не чекати
  --moon <км>     знімок Місяця зблизька, з рельєфом: камера на такій висоті
                  над ним, ціль на лімбі (для --shot; D12)
  --width <px>    ширина, типово 1280
  --height <px>   висота, типово 720
  --perf-probe <діб>
                  заміряти справжній кадр гри після місії такої довжини:
                  час кадру з панелями й без, вершини, пам'ять історії
                  (скіл perf-probe, ROADMAP-UI.md U8). Тільки --release";

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
    frame: game::frame_view::ViewFrame,
    moon_altitude_km: Option<f64>,
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

    // Камера біля Місяця, якщо просили: орбітальна камера гри дивиться на
    // Землю й нікуди більше (`engine::orbit`), тож рельєф Місяця з неї — це
    // кілька пікселів. Знімок — єдиний спосіб подивитись на нього до того,
    // як у гри з'явиться камера з вибором цілі (D12).
    if let Some(altitude_km) = moon_altitude_km {
        return shoot_moon(&gpu, path, options, &snapshot, altitude_km);
    }

    // У обертовому фреймі камера дивиться **згори на площину пари**, а не
    // збоку: уся карта — крива нульової швидкості, точки Лагранжа, петля
    // halo — лежить у z = 0, і збоку вона проєктується в лінію. У вікні це
    // робить гравець мишею; знімку робити це нема кому.
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

/// Знімок Місяця зблизька, з рельєфом (D12).
///
/// ## Чому окремим шляхом, а не `shot::take_scene`
///
/// `take_scene` створює кадр усередині себе й одразу малює. Рельєф так не
/// показати: тайли завантажуються **в кадр** (`Frame::load_terrain`, R5c), і
/// між створенням і малюванням має бути крок, якого в тій функції немає. Тому
/// тут кадр свій — рівно як у демо рушія, і з тієї ж причини.
///
/// ## Куди дивиться камера
///
/// На точку **на лімбі**, а не в підкамерну точку: погляд прямо вниз з низької
/// орбіти дає рівне поле кольору й не показує нічого (це вже з'ясувало демо).
/// Ціль — точка рівно на горизонті, `acos(R / (R + h))` від підкамерної,
/// порахована, а не підібрана.
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
            .ok_or_else(|| format!("тіла {id} немає в снапшоті"))
    };
    let earth = body(EARTH)?;
    let moon = body(MOON)?;

    // Сцена геоцентрична (`view.rs`), тож і камера мусить бути в тих самих
    // координатах: Місяць відносно Землі.
    let centre = [
        moon.position[0] - earth.position[0],
        moon.position[1] - earth.position[1],
        moon.position[2] - earth.position[2],
    ];
    let radius = moon.radius_m;
    if radius <= 0.0 {
        return Err("у Місяця немає радіуса в ассеті — нема навколо чого літати".into());
    }

    let altitude = altitude_km * 1000.0;
    let distance = radius + altitude;
    // Камера над «північним» боком тіла, ціль — на горизонті в напрямку +x.
    let eye = [centre[0], centre[1], centre[2] + distance];
    let horizon = (radius / distance).acos();
    let target = [
        centre[0] + radius * horizon.sin(),
        centre[1],
        centre[2] + radius * horizon.cos(),
    ];
    // Вертикаль кадру — назовні від тіла, тобто небо вгорі, поверхня внизу.
    // Та сама угода, що в `engine::demo::along_limb`, і причина та сама.
    let camera = engine::camera::Camera::look_at(eye, target, [0.0, 0.0, 1.0]);

    let mut scene = view::build(snapshot, camera);
    let mut frame = engine::frame::Frame::new(gpu, shot::FORMAT);
    match app::load_moon_terrain(gpu, &mut frame) {
        Some(id) => game::view::attach_terrain(&mut scene, snapshot, MOON, id),
        None => println!("знімок буде з гладким Місяцем"),
    }

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
        "знімок: {} ({width}×{height}), Місяць з {altitude_km:.0} км, ціль на лімбі",
        path.display()
    );
    Ok(())
}
