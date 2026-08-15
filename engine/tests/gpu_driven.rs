//! Вершинна стадія розкладає список трикутників так само, як це робив
//! індексний буфер (ROADMAP-PLANETS.md, R6a).
//!
//! ## Навіщо цей тест існує
//!
//! До R6a зшивання рівнів жило в `cubesphere::indices`: шістнадцять індексних
//! наборів, виклик малювання на патч. Після R6a та сама підміна робиться
//! арифметикою у вершинному шейдері, і кадр малюється одним викликом на тіло.
//!
//! Отже одне правило тепер записане **двічі** — у Rust і в Slang. Це рівно та
//! ситуація, у якій два записи розходяться на четвертій правці, і єдине, що
//! від цього рятує, — сторож, який зіставляє їх напряму.
//!
//! Тут відтворено арифметику шейдера **дослівно**, у тих самих цілих, і
//! звірено з `cubesphere::indices` для всіх шістнадцяти масок. Це не «те саме,
//! написане двічі»: ліва частина — переклад Slang рядок у рядок, права —
//! незалежна реалізація через таблицю вузлів. Збігтися вони можуть лише якщо
//! обидві правильні.
//!
//! Знімок цього не ловить, і це виміряно: `--shot` після R6a бітово той самий,
//! що до нього, — але сцена зондів рушія має п'ять патчів, і **жодного зшитого
//! ребра**. Тобто бітова рівність кадру доводить розкладку трикутників і
//! нічого не каже про підміну вузлів.

use engine::cubesphere::{self, SIDE};

/// Переклад `node_of` зі `shaders/patch.slang` рядок у рядок.
///
/// Свідомо незграбний: `u32`, ділення з остачею, ті самі імена. Якщо колись
/// захочеться написати це «гарніше» — саме тоді він і перестане бути звіркою
/// з шейдером.
fn node_of(vertex: u32, mask: u32) -> u32 {
    const SIDE_U: u32 = SIDE as u32;
    const NODES: u32 = SIDE_U + 1;

    let triangle = vertex / 3;
    let corner = vertex % 3;
    let cell = triangle / 2;
    let half = triangle % 2;

    let mut a = cell / SIDE_U;
    let mut b = cell % SIDE_U;

    let first = [(0u32, 0u32), (1, 0), (0, 1)];
    let second = [(0u32, 1u32), (1, 0), (1, 1)];
    let step = if half == 0 {
        first[corner as usize]
    } else {
        second[corner as usize]
    };
    a += step.0;
    b += step.1;

    let odd_on_b = a % 2 == 1 && ((b == 0 && mask & 4 != 0) || (b == SIDE_U && mask & 8 != 0));
    let odd_on_a = b % 2 == 1 && ((a == 0 && mask & 1 != 0) || (a == SIDE_U && mask & 2 != 0));
    if odd_on_b {
        a -= 1;
    }
    if odd_on_a {
        b -= 1;
    }

    a * NODES + b
}

/// Для всіх шістнадцяти масок арифметика шейдера дає той самий список вузлів,
/// що й індексний буфер — вершина за вершиною, у тому самому порядку.
#[test]
fn the_shader_walks_the_same_triangles_as_the_index_buffer() {
    let count = SIDE * SIDE * 6;
    for mask in 0..16u8 {
        let expected = cubesphere::indices(mask);
        assert_eq!(expected.len(), count);

        for (vertex, &wanted) in expected.iter().enumerate() {
            let by_shader = node_of(vertex as u32, u32::from(mask));
            assert_eq!(
                by_shader, wanted,
                "маска {mask:04b}, вершина {vertex}: шейдер дає вузол \
                 {by_shader}, індексний буфер — {wanted}"
            );
        }
    }
    println!("  {count} вершин × 16 масок збіглися до одного вузла");
}

/// Сітка в шейдері й сітка в коді — те саме число.
///
/// `SIDE` записаний і в `cubesphere`, і в `shaders/patch.slang` як
/// `static const uint SIDE = 32`. Спільної константи між Rust і Slang не
/// існує, тож лишається сторож — і саме тому він дивиться в **файл шейдера**,
/// а не повторює число.
#[test]
fn the_shader_and_the_code_agree_on_the_patch_size() {
    let source = include_str!("../shaders/patch.slang");
    let wanted = format!("static const uint SIDE = {SIDE};");
    assert!(
        source.contains(&wanted),
        "у shaders/patch.slang немає рядка «{wanted}» — сітка розійшлася з \
         cubesphere::SIDE, і кадр малюватиме інші трикутники"
    );
}
