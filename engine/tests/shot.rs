//! Кадр справді містить те, що мав (ROADMAP F1, F2, I1).
//!
//! Це той самий шлях, що йде у вікно: `Frame` один на обидва. Тест ловить не
//! «впало», а «намалювало не те» — випадок, у якому вікно відкривається,
//! програма не падає, і все одно нічого не працює.
//!
//! З I1 кадр малює планету, а не трикутник, тож перевірка стала сильнішою:
//! оракул тепер не «який канал переважає», а частка кадру, яку зобов'язаний
//! зайняти диск силуету — `asin(R/(R+висота))`, та сама формула, якою
//! міряли F5. Число проти числа.

use engine::frame::{self, Frame};
use engine::gpu::Gpu;
use engine::shot::{self, Shot};
use engine::{flight_probe, sphere};

const SIZE: u32 = 128;

fn gpu() -> Option<Gpu> {
    // На машині без жодного адаптера перевіряти нічого. Пропуск гучний і
    // названий: мовчазний пропуск — це зелений тест, який нічого не робить.
    match Gpu::new(wgpu::Instance::default(), None) {
        Ok(gpu) => Some(gpu),
        Err(_) => {
            eprintln!("ПРОПУЩЕНО: немає адаптера wgpu (немає драйвера або GPU)");
            None
        }
    }
}

fn coverage(shot: &Shot) -> f64 {
    let mut lit = 0u64;
    for y in 0..shot.height {
        for x in 0..shot.width {
            let p = shot.pixel(x, y);
            if [p[0], p[1], p[2]] != frame::CLEAR_BYTES {
                lit += 1;
            }
        }
    }
    lit as f64 / (u64::from(shot.width) * u64::from(shot.height)) as f64
}

/// Скільки кадру мала б зайняти планета з висоти за замовчуванням.
fn expected(width: u32, height: u32) -> f64 {
    let distance = sphere::EARTH_RADIUS_M + frame::DEFAULT_ALTITUDE_M;
    let half_angle = (sphere::EARTH_RADIUS_M / distance).asin();
    flight_probe::expected_coverage(half_angle, f64::from(width) / f64::from(height))
        .expect("диск цілком у кадрі на цій висоті — інакше формула не визначена")
}

#[test]
fn the_background_stays_the_colour_we_asked_for() {
    let Some(gpu) = gpu() else { return };
    let taken = shot::take(&gpu, SIZE, SIZE).expect("кадр мав намалюватися");

    assert_eq!(taken.width, SIZE);
    assert_eq!(
        taken.pixels.len(),
        (SIZE * SIZE * 4) as usize,
        "доповнення рядків не зрізалося"
    );

    // Кути — поза диском планети: з 10⁷ м вона займає близько 42% кадру й
    // кутів не дістає (F5).
    for (x, y) in [(1, 1), (SIZE - 2, 1), (1, SIZE - 2), (SIZE - 2, SIZE - 2)] {
        let pixel = taken.pixel(x, y);
        assert_eq!(
            [pixel[0], pixel[1], pixel[2]],
            frame::CLEAR_BYTES,
            "піксель ({x}, {y}) мав лишитися фоном"
        );
        assert_eq!(pixel[3], 255, "піксель ({x}, {y}) не непрозорий");
    }
}

/// Планета займає рівно ту частку кадру, яку мала б за геометрією.
///
/// Це і є перевірка масштабу, камери й проєкції разом: сфера радіуса Землі,
/// камера-relative на кожну вершину, reversed-Z — усе на тому шляху, яким
/// кадр іде у вікно, а не в окремому зонді.
#[test]
fn the_planet_covers_the_share_geometry_demands() {
    let Some(gpu) = gpu() else { return };
    let taken = shot::take(&gpu, SIZE, SIZE).expect("кадр мав намалюватися");

    let measured = coverage(&taken);
    let analytic = expected(SIZE, SIZE);

    // Півтора відсотка кадру — це межа дискретизації на 128×128 (край диска
    // проходить по пікселях), а не запас про всяк випадок: на 512×512 F5
    // отримав розбіжність 3·10⁻⁴.
    assert!(
        (measured - analytic).abs() < 0.015,
        "покриття {measured:.4} проти аналітичних {analytic:.4}"
    );
}

/// Той самий `Frame` малює у ціль іншого розміру.
///
/// Заради цього depth-текстура й живе всередині `Frame`: якщо вона не
/// переслідує розмір цілі, валідація wgpu впаде на розбіжності вкладень —
/// а якщо переслідує, але проєкція не перерахувалася, зміниться покриття.
/// Тому перевіряються обидва кадри, кожен проти свого аспекту.
#[test]
fn one_frame_draws_into_two_different_sizes() {
    let Some(gpu) = gpu() else { return };

    let mut frame = Frame::new(&gpu, shot::FORMAT);
    let camera = frame::default_camera();

    // Ширше, тоді менше. Портретних співвідношень тут немає навмисно: з
    // 10⁷ м диск ширший за вузький бік такого кадру, і аналітична формула
    // на обрізаному диску не визначена (`flight_probe::expected_coverage`).
    for (width, height) in [(SIZE, SIZE), (SIZE * 2, SIZE), (100, 100)] {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("resize test"),
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
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("resize test"),
            });
        frame.draw(&gpu, &mut encoder, &view, width, height, &camera);

        let taken = shot::read_back(&gpu, encoder, &texture, width, height)
            .expect("кадр мав прочитатися назад");

        let measured = coverage(&taken);
        let analytic = expected(width, height);

        assert!(
            (measured - analytic).abs() < 0.015,
            "{width}×{height}: покриття {measured:.4} проти аналітичних {analytic:.4}"
        );
    }
}
