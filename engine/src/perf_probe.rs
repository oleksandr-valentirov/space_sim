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

use crate::cubesphere;
use crate::frame::{self, Frame};
use crate::gpu::Gpu;
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
    /// Статистика з уже зібраних зразків часу кадру, у мілісекундах.
    ///
    /// Винесено сюди, бо зондів стало два: цей і той, що в `game` міряє
    /// справжній кадр гри з її панелями (U8). Формула мусить бути одна на
    /// обидва — інакше їхні числа не можна класти в одну таблицю, а саме для
    /// цього вони й рахуються.
    pub fn from_samples(width: u32, height: u32, mut samples: Vec<f64>) -> Stats {
        assert!(!samples.is_empty(), "статистика з нуля кадрів");
        samples.sort_by(f64::total_cmp);

        let frames = samples.len() as u32;
        let min_ms = samples[0];
        let max_ms = *samples.last().expect("непорожній");
        let mean_ms = samples.iter().sum::<f64>() / f64::from(frames);

        // Найближчий ранг, не інтерполяція — на кількасот кадрів різниця не
        // помітна, а формула на порядок простіша.
        let p95_index = ((f64::from(frames) * 0.95) as usize).min(samples.len() - 1);

        Stats {
            width,
            height,
            frames,
            min_ms,
            mean_ms,
            p95_ms: samples[p95_index],
            max_ms,
        }
    }

    pub fn fps(&self) -> f64 {
        1000.0 / self.mean_ms
    }

    /// Скільки мілісекунд лишається до бюджету кадру. Від'ємне — бюджет
    /// перевищено.
    pub fn headroom_ms(&self, budget_ms: f64) -> f64 {
        budget_ms - self.mean_ms
    }
}

/// Скільки коштував прохід camera-relative по вершинах UV-сфери.
///
/// **Кадр цього більше не робить** (R1d): планета малюється патчами, і
/// віднімання камери коштує шість чисел замість 8385. Функція лишилася саме
/// тому, що число без другого числа нічого не означає — вона друкується
/// поруч із [`patch_pass_ms`], і різниця між ними і є той виграш.
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

/// Те саме для планети з патчів — те, що кадр робить **зараз** (R1d, R1e).
///
/// Робота тут та сама за формою (віднімання камери в `double`, звуження до
/// `f32`) і різна за обсягом: один початок на патч замість позиції на
/// вершину. Тому й міряється тією самою функцією ззовні: два числа з одного
/// прогону порівнянні, з різних — ні.
///
/// З R1e у прохід додалися поворот тіла й множення на радіус — дев'ять
/// множень на початок патча замість жодного. Це те, що кадр справді робить на
/// **одне** тіло; на N тіл прохід множиться на N.
pub fn patch_pass_ms(passes: u32) -> f64 {
    let camera = frame::default_camera();
    let eye = camera.position();

    // Ті самі патчі, що й у кадрі: шість граней нульового рівня на одиничній
    // сфері.
    let origins: Vec<[f64; 3]> = (0..cubesphere::FACES)
        .map(|face| {
            cubesphere::Patch {
                face,
                level: 0,
                i: 0,
                j: 0,
            }
            .mesh(1.0)
            .origin
        })
        .collect();

    // Тіло, як у сцені: радіус Землі й поворот на 45° навколо (1,1,1) —
    // матриця без жодного нуля, щоб вимір не залежав від того, які саме числа
    // в ній опинились.
    let radius = sphere::EARTH_RADIUS_M;
    let centre = [0.0, 0.0, 0.0];
    let rotation = frame::rotation([0.923_880, 0.220_942, 0.220_942, 0.220_942]);

    let mut bytes: Vec<u8> = Vec::with_capacity(origins.len() * 16);
    let mut run = || {
        bytes.clear();
        for origin in &origins {
            for k in 0..3 {
                let turned = rotation[k][0] * origin[0]
                    + rotation[k][1] * origin[1]
                    + rotation[k][2] * origin[2];
                let value = (centre[k] + radius * turned - eye[k]) as f32;
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(&0.0f32.to_le_bytes());
        }
    };

    for _ in 0..2 {
        run();
    }

    let start = Instant::now();
    for _ in 0..passes {
        run();
    }
    assert_eq!(bytes.len(), origins.len() * 16);

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
    altitude_m: f64,
) -> Result<Stats, String> {
    let distance = crate::sphere::EARTH_RADIUS_M + altitude_m;
    let camera =
        crate::camera::Camera::look_at([distance, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    measure_scene(
        gpu,
        width,
        height,
        frames,
        overlay,
        &frame::default_scene(camera),
    )
}

/// Скільки коштує кадр із повітрям і без нього (ROADMAP-ATMOSPHERE.md, S5, S7).
///
/// Та сама сцена зондів рушія, з єдиною відмінністю — чи має тіло атмосферу.
/// Два числа з одного прогону порівнянні, з різних — ні, і саме тому обидва
/// міряються тут, а не в різних місцях.
///
/// **Висоти навколо умови S5 не круглі, і це навмисно.** Умова — товщина шару
/// в пікселях кадру, і вона перетинає одиницю на 6.24·10⁷ м: сто кілометрів
/// повітря на такій відстані займають рівно піксель. Тобто 6.0·10⁷ і 6.5·10⁷ —
/// це та сама сцена з точністю до восьми відсотків відстані, у якій об'єм
/// аеральної перспективи рахується й не рахується. Різниця між ними і є ціна
/// об'єму; на 10⁹ м вона та сама, і саме її пропуск економить.
pub fn air_cost(
    gpu: &Gpu,
    width: u32,
    height: u32,
    frames: u32,
    altitude_m: f64,
    air: bool,
) -> Result<Stats, String> {
    let distance = crate::sphere::EARTH_RADIUS_M + altitude_m;
    let camera =
        crate::camera::Camera::look_at([distance, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    let mut scene = frame::default_scene(camera);
    if air {
        scene.bodies[0].air =
            Some(crate::scene::Atmosphere::EARTH.with_surface(sphere::EARTH_RADIUS_M));
    }
    measure_scene(gpu, width, height, frames, Overlay::None, &scene)
}

/// Те саме для сцени, яку зібрав хтось інший.
pub fn measure_scene(
    gpu: &Gpu,
    width: u32,
    height: u32,
    frames: u32,
    overlay: Overlay,
    scene: &crate::scene::Scene,
) -> Result<Stats, String> {
    let mut frame = Frame::new(gpu, shot::FORMAT);
    let mut interface = crate::ui::Ui::new(gpu, shot::FORMAT);
    // Сцена без ламаних: вимір лишається порівнюваним із числами I3, де їх
    // ще не було. Коли прогноз стане частиною сцени, це буде окремий рядок
    // таблиці, а не тихо інше число в тому самому (скіл `perf-probe`).
    // Висота параметром, а не сталою (R8): від неї залежить кількість патчів,
    // тобто головне, що LOD додав до вартості кадру. Один рядок таблиці більше
    // не описує кадру — потрібні два, здалеку й з низької орбіти.
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
        frame.draw(gpu, &mut encoder, &view, width, height, scene);

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

    Ok(Stats::from_samples(width, height, samples))
}
