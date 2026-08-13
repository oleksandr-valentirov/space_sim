//! Кадр справді містить те, що мав (ROADMAP F1, F2).
//!
//! Це той самий шлях, що йде у вікно: `Frame` один на обидва. Тест ловить не
//! «впало», а «намалювало не те» — випадок, у якому вікно відкривається,
//! програма не падає, і все одно нічого не працює.

use engine::frame;
use engine::gpu::Gpu;
use engine::shot::{self, Shot};

const SIZE: u32 = 128;

/// Наскільки очікуваний канал має переважати решту.
///
/// Перевіряється переважання, а не близькість до чистого кольору: трикутник
/// інтерполює вершинні кольори, тож усередині вони змішані, і будь-який поріг
/// «схоже на червоний» був би підгонкою під конкретні координати.
const DOMINANCE: u8 = 60;

fn take() -> Option<Shot> {
    // На машині без жодного адаптера перевіряти нічого. Пропуск гучний і
    // названий: мовчазний пропуск — це зелений тест, який нічого не робить.
    let Ok(gpu) = Gpu::new(wgpu::Instance::default(), None) else {
        eprintln!("ПРОПУЩЕНО: немає адаптера wgpu (немає драйвера або GPU)");
        return None;
    };

    Some(shot::take(&gpu, SIZE, SIZE).expect("кадр мав намалюватися"))
}

#[test]
fn the_background_stays_the_colour_we_asked_for() {
    let Some(taken) = take() else { return };

    assert_eq!(taken.width, SIZE);
    assert_eq!(
        taken.pixels.len(),
        (SIZE * SIZE * 4) as usize,
        "доповнення рядків не зрізалося"
    );

    // Верхні кути — поза трикутником за будь-якої розумної геометрії.
    for (x, y) in [(1, 1), (SIZE - 2, 1)] {
        let pixel = taken.pixel(x, y);
        assert_eq!(
            [pixel[0], pixel[1], pixel[2]],
            frame::CLEAR_BYTES,
            "піксель ({x}, {y}) мав лишитися фоном"
        );
        assert_eq!(pixel[3], 255, "піксель ({x}, {y}) не непрозорий");
    }
}

/// Трикутник намальовано, і намальовано правильним боком.
///
/// Точки навмисно всередині, а не на вершинах: пікселя точно на вершині
/// растеризатор законно не зафарбовує, і перевірка падала б на цілком
/// правильному рендері. Ця помилка вже траплялася на розвідці P1.
#[test]
fn the_triangle_is_drawn_the_right_way_up() {
    let Some(taken) = take() else { return };

    for (label, x, y, channel) in [
        ("верхівка — червона", SIZE / 2, SIZE / 3, 0),
        ("лівий низ — зелений", SIZE / 3, (SIZE * 5) / 7, 1),
        ("правий низ — синій", (SIZE * 2) / 3, (SIZE * 5) / 7, 2),
    ] {
        let pixel = taken.pixel(x, y);
        let rgb = [pixel[0], pixel[1], pixel[2]];
        let mine = rgb[channel];

        let dominates = rgb
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != channel)
            .all(|(_, &other)| mine > other && mine - other >= DOMINANCE);

        assert!(
            dominates,
            "{label}: канал {channel} не переважає в ({x}, {y}), маємо {rgb:?}"
        );
    }
}
