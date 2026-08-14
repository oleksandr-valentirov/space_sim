//! Замір часу кадру рушія — рендерна половина процесу вимірювання
//! продуктивності (скіл `perf-probe`).
//!
//! **Не прив'язаний до конкретної сцени.** Міряє те, що [`crate::frame::Frame::draw`]
//! малює просто зараз — сьогодні це трикутник F2, після F5 це буде сфера
//! в реальному масштабі, пізніше планета з LOD. Числа стають виміром нової
//! сцени без жодної зміни в цьому файлі. Саме тому проба окрема від
//! `depth_probe`/`camera_probe`: ті відповідають на конкретне геометричне
//! питання свого кроку, а ця — на «скільки коштує кадр» для будь-якого кроку.
//!
//! ## Метод
//!
//! Синхронний `submit` + `device.poll(Wait)` на кожному кадрі, без вікна
//! й без vsync. Це навмисно НЕ те, що бачить гравець: реальний цикл
//! конвеєрний (GPU кадру N+1 починається, не чекаючи презентації N), а тут
//! кожен кадр чекає на повне завершення попереднього. Тобто число —
//! **верхня межа** часу кадру, не нижня. Порівнювати прогони між собою на
//! цій самій машині — коректно; порівнювати абсолютне число з «на такому
//! залізі гра дає N fps» — ні, поки рендер не конвеєрний.
//!
//! Перші [`WARMUP_FRAMES`] кадрів відкидаються: перший запуск пайплайна на
//! багатьох бекендах компілює шейдер лінивою, тому саме він на порядок
//! довший за всі наступні, і без відкидання зіпсував би і мінімум, і max.

use std::time::Instant;

use crate::frame::{self, Frame};
use crate::gpu::Gpu;
use crate::scene::Scene;
use crate::shot;
use crate::sphere;

/// Кадрів для розігріву перед виміром — компіляція шейдера й перший
/// алокований конвеєр драйвера мають встигнути один раз, поза виміром.
const WARMUP_FRAMES: u32 = 10;

pub struct Stats {
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub min_ms: f64,
    pub mean_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
}

impl Stats {
    pub fn fps(&self) -> f64 {
        1000.0 / self.mean_ms
    }

    /// Скільки мілісекунд лишається до бюджету кадру. Від'ємне — бюджет
    /// перевищено.
    pub fn headroom_ms(&self, budget_ms: f64) -> f64 {
        budget_ms - self.mean_ms
    }
}

/// Скільки коштує сам прохід camera-relative по вершинах меша, без GPU.
///
/// Міряється окремо, бо це єдина частина кадру, про яку заздалегідь відомо,
/// що вона тимчасова: `Frame` перераховує позиції всіх вершин у `double`
/// щокадру (ROADMAP F5, I1), а M4 замінить це зсувом по патчах. Загальний
/// час кадру цього не показує — там воно змішане з синхронізацією й
/// растеризацією, і на швидкій машині одне тоне в іншому.
///
/// Повертає мілісекунди на один прохід.
pub fn camera_pass_ms(passes: u32) -> f64 {
    let mesh = sphere::generate(sphere::EARTH_RADIUS_M, 64, 128);
    let camera = frame::default_camera();
    let mut bytes: Vec<u8> = Vec::with_capacity(mesh.positions.len() * 12);

    // Розігрів: перший прохід платить за сторінки пам'яті під `bytes`.
    for _ in 0..2 {
        bytes.clear();
        for &p in &mesh.positions {
            for value in camera.relative(p) {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    }

    let start = Instant::now();
    for _ in 0..passes {
        bytes.clear();
        for &p in &mesh.positions {
            for value in camera.relative(p) {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    // Щоб оптимізатор не викинув цикл цілком.
    assert_eq!(bytes.len(), mesh.positions.len() * 12);

    start.elapsed().as_secs_f64() * 1000.0 / f64::from(passes)
}

/// Що малюється поверх сцени в замірі.
///
/// Інтерфейс — істотна нова вартість (ROADMAP-UI.md, U1b), і міряти його
/// треба **тим самим прогоном**, а не окремим: різні прогони на одній машині
/// різняться більше, ніж коштує панель.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    /// Кадр без проходу egui — те, чим міряні всі числа до U1b.
    None,
    /// Прохід egui є, але порожній: ціна самого проводу.
    EmptyUi,
    /// Прохід egui з панеллю — ціна проводу разом із чимось намальованим.
    Panel,
}

/// Проганяє `frames` кадрів `width`×`height` без вікна й повертає статистику
/// часу кадру в мілісекундах.
pub fn measure(
    gpu: &Gpu,
    width: u32,
    height: u32,
    frames: u32,
    overlay: Overlay,
) -> Result<Stats, String> {
    let mut frame = Frame::new(gpu, shot::FORMAT);
    let mut interface = crate::ui::Ui::new(gpu, shot::FORMAT);
    // Сцена без ламаних: вимір лишається порівнюваним із числами I3, де їх
    // ще не було. Коли прогноз стане частиною сцени, це буде окремий рядок
    // таблиці, а не тихо інше число в тому самому (скіл `perf-probe`).
    let scene = Scene::new(frame::default_camera());

    // COPY_SRC свідомо відсутній: цей вимір не читає пікселі назад, а
    // читання назад — окрема вартість, якої немає в реальному кадрі
    // (той іде в surface, не в буфер). Додавати її сюди означало б міряти
    // не кадр, а кадр-плюс-щось-чужe.
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("perf probe"),
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

    let mut draw_once = || -> Result<f64, String> {
        let start = Instant::now();

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("perf probe"),
            });
        frame.draw(gpu, &mut encoder, &view, width, height, &scene);

        if overlay != Overlay::None {
            let viewport = crate::ui::Viewport::new(width, height, 1.0);
            interface.draw(
                gpu,
                &mut encoder,
                &view,
                viewport,
                viewport.quiet_input(),
                |ui| {
                    if overlay == Overlay::Panel {
                        // Стільки ж, скільки займе панель часу з U2b:
                        // прямокутник і рядок тексту, тобто і геометрія,
                        // і вибірка з атласа шрифта.
                        let rect =
                            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 180.0));
                        ui.painter()
                            .rect_filled(rect, 0.0, egui::Color32::from_rgb(20, 24, 28));
                        ui.painter().text(
                            egui::pos2(16.0, 16.0),
                            egui::Align2::LEFT_TOP,
                            "MET 000d 00:00:00",
                            egui::FontId::monospace(14.0),
                            egui::Color32::from_rgb(180, 220, 255),
                        );
                    }
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

        Ok(start.elapsed().as_secs_f64() * 1000.0)
    };

    for _ in 0..WARMUP_FRAMES {
        draw_once()?;
    }

    let mut samples = Vec::with_capacity(frames as usize);
    for _ in 0..frames {
        samples.push(draw_once()?);
    }

    samples.sort_by(f64::total_cmp);

    let min_ms = samples[0];
    let max_ms = *samples.last().expect("frames > 0");
    let mean_ms = samples.iter().sum::<f64>() / f64::from(frames);

    // Найближчий ранг, не інтерполяція — на кількасот кадрів різниця не
    // помітна, а формула на порядок простіша.
    let p95_index = ((f64::from(frames) * 0.95) as usize).min(samples.len() - 1);
    let p95_ms = samples[p95_index];

    Ok(Stats {
        width,
        height,
        frames,
        min_ms,
        mean_ms,
        p95_ms,
        max_ms,
    })
}
