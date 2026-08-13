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
use crate::shot;

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

/// Проганяє `frames` кадрів `width`×`height` без вікна й повертає статистику
/// часу кадру в мілісекундах.
pub fn measure(gpu: &Gpu, width: u32, height: u32, frames: u32) -> Result<Stats, String> {
    let mut frame = Frame::new(gpu, shot::FORMAT);
    let camera = frame::default_camera();

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
        frame.draw(gpu, &mut encoder, &view, width, height, &camera);
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
