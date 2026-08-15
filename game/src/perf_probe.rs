//! Скільки коштує **справжній** кадр гри (ROADMAP-UI.md, U8; скіл `perf-probe`).
//!
//! Зонд рушія (`engine::perf_probe`) міряє сцену рушія й **синтетичну** панель:
//! прямокутник і рядок тексту. Це було правильно для U1b, який питав, скільки
//! коштує сам прохід egui. Питання U8 інше — скільки коштує кадр, який бачить
//! гравець, — а в ньому і панелі справжні (п'ять штук, дві колонки), і сцена
//! справжня: ламана прогнозу з усіма її вершинами.
//!
//! ## Чому це той самий вимір, що й у рушія
//!
//! Статистика рахується `engine::perf_probe::Stats::from_samples`, тобто тією
//! самою формулою, і метод той самий синхронний `poll(Wait)` — з тими самими
//! обмеженнями (скіл `perf-probe`, «Обмеження методу»): це **верхня межа**
//! часу кадру, порівнянна між прогонами на одній машині й ні з чим більше.
//!
//! ## Чому тут же міряється D7
//!
//! Борг каже: історія росте без межі, і мільйон вершин ламаної — це ~3 мс CPU
//! і 24 МБ на кадр. U8 не закриває борг, але зобов'язаний **подивитись**: до
//! цього кроку число «мільйон» стояло в ROADMAP як викладка, а не як вимір.
//! Тому зонд друкує поруч із часом кадру ще й те, з чого цей час складається —
//! скільки вершин у сцені, скільки семплів у історії та скільки вони важать.
//!
//! Довжину місії задає викликач (`--perf-probe <діб>`), бо саме вона й
//! вирішує: годинна місія не покаже нічого, а та, що впирається в борг, —
//! покаже.

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

/// Кадрів на прогрів — стільки ж, скільки в зонда рушія.
const WARMUP_FRAMES: u32 = 30;

/// Те, з чого складається кадр: не час, а обсяг.
///
/// Друкується поруч зі `Stats`, бо час кадру без цих чисел не порівняти з
/// наступним виміром: 0.2 мс на тисячі вершин і на мільйоні — це два різні
/// твердження про рушій.
pub struct SceneSize {
    /// Вершин у всіх ламаних сцени разом.
    pub vertices: usize,
    /// Семплів у траєкторіях усіх апаратів.
    pub samples: usize,
    /// Скільки важить історія в пам'яті гри, за 104 байти на семпл (D7).
    pub history_bytes: usize,
    /// Скільки важать вершини в буфері кадру, за 24 байти на вершину (D7).
    pub buffer_bytes: usize,
    /// Ламаних у сцені.
    pub polylines: usize,
}

impl SceneSize {
    pub fn of(scene: &Scene, snapshot: &WorldSnapshot) -> SceneSize {
        let vertices: usize = scene.polylines.iter().map(|line| line.points.len()).sum();
        let samples: usize = snapshot.vessels.iter().map(|v| v.sample_count()).sum();

        SceneSize {
            vertices,
            samples,
            // Числа з D7, і саме тому вони тут константами, а не
            // `size_of::<Sample>()`: борг говорить про них, і вимір мусить
            // говорити тими самими, поки борг не закритий.
            history_bytes: samples * 104,
            buffer_bytes: vertices * 24,
            polylines: scene.polylines.len(),
        }
    }
}

/// Що малюється поверх сцени.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    /// Тільки сцена — щоб було з чим порівняти ціну панелей.
    None,
    /// Справжні панелі гри, обидві колонки.
    Panels,
}

/// Проганяє `frames` кадрів заданої сцени й повертає час кадру.
///
/// Панелі малюються **ті самі**, що в `app::draw`, і зі стилем із `palette`
/// (U7c): панель із типовими відступами egui мала б інший розмір, тобто
/// вимірювався б не той кадр, що в грі.
///
/// Аргументів вісім, і збивати їх у структуру нема сенсу: кожен тут —
/// незалежна вісь виміру, і структура з восьми полів, яку заповнюють на місці
/// виклику, це той самий список, тільки довший.
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

    // COPY_SRC свідомо відсутній — з тієї ж причини, що в зонда рушія: читання
    // пікселів назад у справжньому кадрі не відбувається.
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

    // Друге число кадру, а не третє в дужках: розвилка N1 питає саме про
    // нього — чи справді найдорожче в кадрі це прохід по вершинах ламаних.
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
                    // Дослівно те, що робить `app::draw`, — інакше вимір
                    // описував би панелі, яких у грі немає.
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
            .map_err(|e| format!("не дочекалися GPU: {e}"))?;

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

/// Скільки коштує зібрати сцену зі снапшоту — тобто прохід по всіх вершинах
/// на CPU.
///
/// Друга половина D7 живе саме тут: `view::build_in` проганяє **кожен** семпл
/// кожної ланки, а в обертовому фреймі до цього додається перетворення фрейму
/// на точку (U6a1 виміряв 2.69 → 10.56 нс). Міряється окремо від кадру, бо в
/// грі це теж окрема робота — і бо саме це число вирішує, чи борг про кадр,
/// чи про підготовку до нього.
pub fn build_ms(
    snapshot: &WorldSnapshot,
    camera: impl Fn() -> engine::camera::Camera,
    frame: ViewFrame,
) -> f64 {
    // Прогрів: перший прохід платить за алокацію векторів, і без нього
    // міряється саме вона.
    let _ = view::build_in(snapshot, camera(), frame);

    let start = Instant::now();
    let scene = view::build_in(snapshot, camera(), frame);
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;

    // Щоб оптимізатор не викинув побудову: сцена мусить бути прочитана.
    assert!(!scene.polylines.is_empty() || snapshot.vessels.is_empty());
    elapsed
}

/// Скільки коштує сама крива нульової швидкості.
///
/// Окремо від [`build_ms`], і не заради повноти: перший прогін U8 показав, що
/// в обертовому фреймі побудова сцени дорожча за інерціальну **на порядок**,
/// а зайвих вершин при цьому лічені сотні. Отже платить не перетворення точок
/// (U6a1: 10.56 нс на точку), а щось інше — і поки це не виміряно окремо,
/// «обертовий фрейм дорогий» лишається здогадом про причину.
///
/// Повертає `None`, якщо кривої в цьому снапшоті немає: у інерціальному
/// фреймі її не рахують узагалі.
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

/// Прогін зонда цілком: побудувати світ, догнати місію, поміряти, надрукувати.
///
/// Живе тут, а не в `main`, з тієї ж причини, що й у рушія: `main` розбирає
/// аргументи, а що саме міряється — це вимір, і воно має лежати поруч із
/// методикою.
pub fn run(options: &app::Options, days: f64, frames: u32) -> Result<(), String> {
    let gpu = Gpu::new(wgpu::Instance::default(), None)?;
    println!("адаптер: {}", gpu.describe());
    println!(
        "профіль: {}",
        if cfg!(debug_assertions) {
            "debug — числа непорівнянні з release, різниця тринадцятикратна"
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

    // Густина семплів — та величина, на якій стоїть уся викладка D7 (171 на
    // добу з `bench_prop`), тож вона друкується, а не лишається в голові.
    let samples: usize = snapshot.vessels.iter().map(|v| v.sample_count()).sum();
    let vessel_days = snapshot.vessels.len() as f64 * flown_days;
    println!(
        "флот: {} апаратів, {:.0} семплів на апарат за добу",
        snapshot.vessels.len(),
        samples as f64 / vessel_days.max(f64::MIN_POSITIVE)
    );

    // Апарат, що зійшов з орбіти, тихо зменшує вимір — тож про нього кажемо
    // вголос. Станція на 600 км за сто діб цього робити не повинна, і саме
    // тому мовчазна відмова тут була б найгіршим із результатів.
    for vessel in &snapshot.vessels {
        if let Some(error) = &vessel.failed {
            println!("  ⚠ {} не долетів: {error}", vessel.name);
        }
    }

    // Замиканням, а не значенням: `Camera` не `Copy`, а створити її наново
    // дешевше за будь-яку гру з клонами (той самий прийом, що в
    // `game/tests/scene.rs`).
    let camera = || engine::orbit::Orbit::at_altitude(crate::mission::CAMERA_ALTITUDE_M).camera();

    for frame in [ViewFrame::Inertial, ViewFrame::Rotating] {
        let scene = view::build_in(&snapshot, camera(), frame);
        let size = SceneSize::of(&scene, &snapshot);
        let build = build_ms(&snapshot, camera, frame);

        println!();
        println!("=== фрейм {frame:?}, доба {flown_days:.1} ({steps} кроків)");
        println!(
            "  сцена: {} вершин у {} ламаних, {} семплів історії, {} апаратів",
            size.vertices,
            size.polylines,
            size.samples,
            snapshot.vessels.len()
        );
        println!(
            "  пам'ять: історія {:.1} МіБ, буфер кадру {:.2} МіБ на кадр",
            size.history_bytes as f64 / (1024.0 * 1024.0),
            size.buffer_bytes as f64 / (1024.0 * 1024.0)
        );
        println!("  view::build_in: {build:.3} мс на кадр (CPU, без рушія)");
        if frame == ViewFrame::Rotating {
            match zvc_ms(&snapshot) {
                Some((ms, vertices)) => println!(
                    "    з них крива нульової швидкості: {ms:.3} мс на {vertices} вершин \
                     ({:.0} нс на вершину)",
                    ms * 1.0e6 / vertices.max(1) as f64
                ),
                None => println!("    кривої в цьому снапшоті немає"),
            }
        }

        for (width, height) in [(1280u32, 720u32), (1920, 1080)] {
            // Проріджена сцена будується під **свою** роздільність: критерій
            // екранний, і на 1080p пів пікселя — це інша величина в метрах.
            let viewport = view::Viewport {
                width_px: width,
                height_px: height,
            };
            let thinned_start = Instant::now();
            let thinned = view::build_thinned(&snapshot, camera(), &[], frame, viewport);
            let thinned_ms = thinned_start.elapsed().as_secs_f64() * 1000.0;
            let thinned_size = SceneSize::of(&thinned, &snapshot);

            println!(
                "  {width}×{height}, проріджування: {} → {} вершин (×{:.0}), \
                 побудова {thinned_ms:.3} мс",
                size.vertices,
                thinned_size.vertices,
                size.vertices as f64 / thinned_size.vertices.max(1) as f64
            );

            for (overlay, name) in [(Overlay::None, "немає"), (Overlay::Panels, "панелі")]
            {
                for (scene, size, label) in [
                    (&scene, &size, "повний"),
                    (&thinned, &thinned_size, "проріджений"),
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
                        "  {width}×{height}, інтерфейс {name}, слід {label}: \
                         mean {:.3} мс, p95 {:.3} мс, запас до 60 Hz {:+.2} мс",
                        stats.mean_ms,
                        stats.p95_ms,
                        stats.headroom_ms(1000.0 / 60.0)
                    );
                    println!(
                        "    з них Lines::upload: {:.3} мс ({:.0}% кадру, {:.1} нс на вершину)",
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
