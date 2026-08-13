//! Кадр справді малюється тим кольором, яким мав (ROADMAP F1).
//!
//! Це той самий шлях, що йде у вікно: `frame::draw` один на обидва. Тест
//! ловить не «впало», а «намалювало не те» — випадок, у якому вікно
//! відкривається, програма не падає, і все одно нічого не працює.

use engine::frame;
use engine::gpu::Gpu;
use engine::shot;

const SIZE: u32 = 64;

#[test]
fn the_frame_is_cleared_to_the_colour_we_asked_for() {
    // На машині без жодного адаптера перевіряти нічого. Пропуск гучний і
    // названий: мовчазний пропуск — це зелений тест, який нічого не робить.
    let Ok(gpu) = Gpu::new(wgpu::Instance::default(), None) else {
        eprintln!("ПРОПУЩЕНО: немає адаптера wgpu (немає драйвера або GPU)");
        return;
    };

    let taken = shot::take(&gpu, SIZE, SIZE).expect("кадр мав намалюватися");

    assert_eq!(taken.width, SIZE);
    assert_eq!(taken.height, SIZE);
    assert_eq!(
        taken.pixels.len(),
        (SIZE * SIZE * 4) as usize,
        "доповнення рядків не зрізалося"
    );

    // Кути й центр: заливка має бути всюди, а не в одному місці.
    for (x, y) in [
        (0, 0),
        (SIZE - 1, 0),
        (0, SIZE - 1),
        (SIZE - 1, SIZE - 1),
        (SIZE / 2, SIZE / 2),
    ] {
        let pixel = taken.pixel(x, y);
        assert_eq!(
            [pixel[0], pixel[1], pixel[2]],
            frame::CLEAR_BYTES,
            "піксель ({x}, {y}) не того кольору"
        );
        assert_eq!(pixel[3], 255, "піксель ({x}, {y}) не непрозорий");
    }
}
