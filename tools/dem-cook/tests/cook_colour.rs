//! Кукер кольору: той самий вхід — той самий байт, і той самий вузол (T2d).
//!
//! Форма оракулів та сама, що в кукера висот (`cook.rs`), але **трьом із
//! чотирьох джерело не потрібне**: `Albedo` — це прості поля, тож сітку можна
//! скласти руками. Це навмисно, а не зручність. Мозаїка WAC у git не лежить
//! (Q5), і кукер, за яким стежили б лише перевірки, що без неї пропускаються,
//! був би не стережений ніде, крім однієї машини.
//!
//! 1. **стабільність** — два прогони дають байт у байт те саме;
//! 2. **два шляхи, одне число** — колір у тайлі й колір, прочитаний із
//!    джерела за широтою й довготою, збігаються. Шляхи справді різні: кукер
//!    іде через `Patch::vertex` і `sample_direction`, тест — через явний
//!    переклад напрямку в кути;
//! 3. **шва немає** — вузол на спільному ребрі двох патчів несе той самий
//!    байт в обох тайлах, а ореол одного дорівнює сітці сусіда. Це та сама
//!    властивість, на якій стоїть рельєф (R2b, R7b), і кольору вона потрібна
//!    не менше: різниця в один байт на ребрі — це видима лінія;
//! 4. **шкала не з'їла контраст** — єдине твердження, якому потрібна справжня
//!    мозаїка, і єдине, яке взагалі питає про вибір `SCALE`: три попередні
//!    пройшли б і при шкалі, у якій море й материк різняться на п'ять
//!    одиниць з 255.

use dem_cook::albedo::Albedo;
use dem_cook::cook::{build_colour, SCALE};
use engine::cubesphere::{Patch, FACES, SIDE};
use engine::tiles;

const LEVELS: u32 = 3;

/// Сітка, у якій відбивна здатність — плавна функція позиції.
///
/// Плавна навмисно: сходинка дала б ті самі байти по обидва боки ребра просто
/// тому, що там усе однакове, і третій оракул став би порожнім. Період
/// підібраний так, щоб на одну грань куба припадало кілька хвиль — тоді
/// сусідні вузли справді різні.
fn painted() -> Albedo {
    let (samples, lines) = (720usize, 360usize);
    let per_degree = 2.0;
    let mut raw = Vec::with_capacity(samples * lines);
    for line in 0..lines {
        for sample in 0..samples {
            let lat = 90.0 - (line as f64 + 0.5) / per_degree;
            let lon = (sample as f64 + 0.5) / per_degree;
            let radians = std::f64::consts::PI / 180.0;
            // Діапазон 0.02 … 0.18 — той самий, у якому живе справжня мозаїка,
            // тож квантування тут таке саме грубе, як у бойовому ассеті.
            let wave = (3.0 * lon * radians).sin() * (2.0 * lat * radians).cos();
            raw.push((0.1 + 0.08 * wave) as f32);
        }
    }
    Albedo {
        samples,
        lines,
        per_degree,
        raw,
    }
}

/// Два прогони кукера дають байт у байт те саме.
#[test]
fn cooking_twice_gives_the_same_bytes() {
    let map = painted();
    let (first, saturated) = build_colour(&map, LEVELS);
    let (second, again) = build_colour(&map, LEVELS);

    assert_eq!(saturated, again);
    assert_eq!(
        first.to_bytes(),
        second.to_bytes(),
        "два прогони кукера розійшлися"
    );
    // Фікстура не доходить до шкали: насичення тут означало б, що тест міряє
    // затиснення, а не квантування.
    assert_eq!(saturated, 0, "фікстура насичилась — {SCALE} замала");
}

/// Колір у тайлі дорівнює кольору джерела в тому самому напрямку.
#[test]
fn every_node_is_the_source_read_a_second_way() {
    let map = painted();
    let (colour, _) = build_colour(&map, LEVELS);

    let mut checked = 0;
    for level in 0..LEVELS {
        let side = 1u32 << level;
        for face in 0..FACES {
            for i in (0..side).step_by(3) {
                for j in (0..side).step_by(3) {
                    let patch = Patch { face, level, i, j };
                    let index = tiles::index(LEVELS, &patch).expect("тайл є");
                    for (a, b) in [(0usize, 0usize), (1, 7), (SIDE / 2, SIDE / 3), (SIDE, SIDE)] {
                        let unit = colour.node(index, a as i32, b as i32, 0);

                        // Другий шлях: напрямок → кути → сітка, без жодного
                        // виклику з кукера.
                        let [x, y, z] = patch.vertex(a, b, 1.0);
                        let flat = (x * x + y * y).sqrt();
                        let want = map.sample(z.atan2(flat), y.atan2(x));
                        let want = (want / f64::from(SCALE) * 255.0).round() as u8;

                        assert_eq!(
                            unit, want,
                            "патч {patch:?}, вузол ({a}, {b}): у тайлі {unit}, з джерела {want}"
                        );
                        checked += 1;
                    }
                }
            }
        }
    }
    assert!(checked > 100, "перевірено лише {checked} вузлів");
}

/// Спільне ребро двох патчів несе той самий байт, і ореол дорівнює сітці.
///
/// Обидва твердження про одну річ — про те, що між тайлами немає шва, — але
/// ловлять різне. Перше падає, якщо кукер зсунув координати всередині патча;
/// друге — якщо він неправильно знайшов сусіда за ребром куба, тобто саме
/// там, де міняється грань, а з нею й варп.
#[test]
fn the_shared_edge_and_the_halo_agree_with_the_neighbour() {
    let map = painted();
    let (colour, _) = build_colour(&map, LEVELS);

    let level = LEVELS - 1;
    let side = 1u32 << level;
    let mut pairs = 0;
    for face in 0..FACES {
        for i in 0..side - 1 {
            for j in 0..side {
                let left = Patch { face, level, i, j };
                let right = Patch {
                    face,
                    level,
                    i: i + 1,
                    j,
                };
                let (l, r) = (
                    tiles::index(LEVELS, &left).expect("тайл є"),
                    tiles::index(LEVELS, &right).expect("тайл є"),
                );

                for b in [0i32, 1, SIDE as i32 / 2, SIDE as i32] {
                    // Спільне ребро: останній вузол лівого — це нульовий
                    // вузол правого.
                    assert_eq!(
                        colour.node(l, SIDE as i32, b, 0),
                        colour.node(r, 0, b, 0),
                        "ребро між {left:?} і {right:?}, вузол {b}"
                    );
                    // Ореол лівого дивиться на перший внутрішній вузол правого.
                    assert_eq!(
                        colour.node(l, SIDE as i32 + 1, b, 0),
                        colour.node(r, 1, b, 0),
                        "ореол {left:?} проти сітки {right:?}, вузол {b}"
                    );
                }
                pairs += 1;
            }
        }
    }
    assert!(pairs > 0, "жодної пари сусідів не перевірено");
}

/// Квантування лишає Місяцю контраст, а не робить його рівно сірим.
///
/// Єдина перевірка тут, якій потрібне справжнє джерело, і єдина, яка питає про
/// **вибір шкали**: усе вище пройшло б і при `SCALE = 1.0`, де море й материк
/// відрізнялися б на п'ять одиниць з 255. Піраміда навмисно мілка — питання
/// про діапазон значень, а не про глибину, і 96 тайлів відповідають на нього
/// так само, як 8190.
#[test]
fn the_moon_keeps_its_contrast_through_the_quantisation() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/wac/wac_global_016p.img");
    let Ok(map) = Albedo::read(&path) else {
        eprintln!(
            "ПРОПУЩЕНО: немає {}. Як покласти назад — data/wac/README.md",
            path.display()
        );
        return;
    };

    let (colour, saturated) = build_colour(&map, 2);
    let (mut low, mut high) = (u8::MAX, u8::MIN);
    for index in 0..tiles::count(2) {
        for a in 0..=SIDE as i32 {
            for b in 0..=SIDE as i32 {
                let unit = colour.node(index, a, b, 0);
                low = low.min(unit);
                high = high.max(unit);
            }
        }
    }
    println!("  вузли {low} … {high} з 255; насичено {saturated}");

    // ⚠ Насичення є на **будь-якій** глибині, і це не властивість дрібності:
    // грубий рівень піраміди не усереднює нічого, він бере ту саму точкову
    // білінійну вибірку, просто рідше. Тому очікувати тут нуля не можна —
    // перша версія цієї перевірки очікувала, і впала на чотирьох вузлах.
    // Виміряно: 4 з 36 750 на двох рівнях (0.011%) і 552 з 10 032 750 на
    // шести (0.0055%) — обидва на порядок менші за 0.09% сирих пікселів понад
    // 0.2, бо вибірка між пікселями джерела усереднює четвірку сусідів.
    let nodes = tiles::count(2) * (SIDE + 1) * (SIDE + 1);
    assert!(
        saturated * 1000 < nodes,
        "насичено {saturated} вузлів з {nodes} — понад проміле, шкала замала"
    );
    assert!(
        high - low > 60,
        "діапазон вузлів {low}…{high} — шкала з'їла контраст поверхні"
    );
}
