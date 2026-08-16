//! Кукер висот Землі: ETOPO → тайлсет кубосфери (етап T, крок T7d).
//!
//! Оракули тут не повторюють `cook.rs` (Місяць): формат, ореол і зшивання
//! рівнів уже доведені там і від джерела не залежать. Доводиться те, що в
//! Землі **інше**:
//!
//! 1. **ланцюг** — джерело вп'ятеро дрібніше за вузол найглибшого рівня і в
//!    тридцять тисяч разів дрібніше за вузол нульового, тож грубий рівень
//!    мусить усереднювати, а не брати піксель;
//! 2. **берегова лінія** — те, заради чого крок узагалі є: знак висоти в
//!    тайлі мусить збігатися зі знаком у джерелі, у координатах;
//! 3. **опорний радіус і одиниці** — метр і 6 371 010 м, а не пів метра й
//!    місячний радіус.
//!
//! Усе, що потребує самого продукту, пропускається без нього.

use dem_cook::cook::build_earth;
use dem_cook::etopo::{Relief, REFERENCE_M};
use engine::cubesphere::{self, Patch, SIDE};
use std::path::{Path, PathBuf};

fn source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/etopo/etopo_2022_60s_surface.tif")
}

/// Сітка ETOPO, або `None` — тоді тест каже, чого бракує, і не падає (Q5).
fn relief() -> Option<Relief> {
    match Relief::read(&source()) {
        Ok(grid) => Some(grid),
        Err(_) => {
            eprintln!(
                "ПРОПУЩЕНО: немає {}. Як покласти назад — data/etopo/README.md",
                source().display()
            );
            None
        }
    }
}

/// Два прогони кукера дають байт у байт те саме.
///
/// Кукається дві піраміди на два рівні, а не на шість: детермінізм не
/// залежить від глибини, а тест не має права коштувати хвилини.
#[test]
fn cooking_twice_gives_the_same_bytes() {
    let Some(grid) = relief() else { return };

    let first = build_earth(&grid, 2).to_bytes();
    let second = build_earth(&grid, 2).to_bytes();

    assert_eq!(first, second);
}

/// Одиниці й опорний радіус — Землі, а не Місяця.
///
/// Дрібниця, яку легко не помітити й неможливо побачити в кадрі: рельєф з
/// місячним масштабом 0.5 просто вдвічі нижчий, а з місячним радіусом —
/// поверхня, втоплена на чотири з половиною тисячі кілометрів.
#[test]
fn the_asset_carries_earths_own_numbers() {
    let Some(grid) = relief() else { return };

    let terrain = build_earth(&grid, 2);

    assert_eq!(terrain.scale_m, 1.0);
    assert_eq!(terrain.reference_m, REFERENCE_M);
}

/// Кожен вузол тайла — те саме число, що дає джерело, прочитане іншим шляхом.
///
/// Іншим шляхом: тайл читається через `Terrain::node`, а джерело — через
/// `sample_direction_m` у напрямку тієї самої вершини патча. Збігтися вони
/// мусять точно, бо між ними лише округлення до метра, яке робить обидва.
#[test]
fn every_node_is_the_source_read_a_second_way() {
    let Some(grid) = relief() else { return };

    let levels = 2;
    let terrain = build_earth(&grid, levels);
    let chain = grid.chain();
    // Найглибший рівень піраміди читає ту сітку, яку йому дав ланцюг; для
    // рівня 1 це не сама ETOPO, і брати тут `grid` було б перевіркою іншого.
    let rads = chain.iter().map(Relief::pixel_rad).collect::<Vec<f64>>();
    let source = &chain[dem_cook::cook::source_for(&rads, levels - 1)];

    let patch = Patch {
        face: 2,
        level: levels - 1,
        i: 1,
        j: 0,
    };
    let index = terrain.index(&patch).expect("патч у піраміді");
    for a in (0..=SIDE).step_by(7) {
        for b in (0..=SIDE).step_by(7) {
            let unit = patch.vertex(a, b, 1.0);
            let expect = source.sample_direction_m(unit).round();
            let got = f64::from(terrain.node(index, a as i32, b as i32));
            assert_eq!(got, expect, "вузол ({a}, {b})");
        }
    }
}

/// Грубий рівень читає усереднену сітку ланцюга, а не саму ETOPO (T3c).
///
/// ⚠ **Два оракули, які тут напрошуються, обидва не працюють**, і це варто
/// знати наперед:
///
/// - *дисперсія сусідів*: на рівні 0 вузол накриває 312 км, і сусідні вузли
///   законно різняться на чотири кілометри — шельф проти океанічного дна.
///   Виміряно 3924 м, і це правда про Землю;
/// - *близькість до площинного середнього*: саме «середнє» доводиться
///   оцінювати вибіркою, і при 11×11 точках його власний шум (±360 м) більший
///   за різницю, яку він мав би показати. Виміряно: 290 м проти 239 м, тобто
///   оракул відповідає на своє питання шумом.
///
/// Працює натомість інваріант самого ланцюга, і він точний: рівень 0 мусить
/// брати **не нульову** сітку, і вузол тайла мусить бітово дорівнювати
/// вибірці саме з неї.
#[test]
fn a_coarse_level_reads_a_reduced_grid() {
    let Some(grid) = relief() else { return };

    let chain = grid.chain();
    let rads = chain.iter().map(Relief::pixel_rad).collect::<Vec<f64>>();
    let chosen = dem_cook::cook::source_for(&rads, 0);
    assert!(
        chosen > 0,
        "рівень 0 читає саму ETOPO — ланцюг не дійшов до 312-кілометрового вузла"
    );

    // І та сітка справді грубіша за вузол не більш ніж на крок ланцюга:
    // грубіша дала б рівню 0 менше деталі, ніж він здатен нести.
    let node_rad = std::f64::consts::FRAC_PI_2 / SIDE as f64;
    assert!(chain[chosen].pixel_rad() <= node_rad);
    assert!(chain[chosen + 1].pixel_rad() > node_rad);

    let terrain = build_earth(&grid, 1);
    let patch = Patch {
        face: 0,
        level: 0,
        i: 0,
        j: 0,
    };
    let index = terrain.index(&patch).expect("патч у піраміді");
    for a in (0..=SIDE).step_by(7) {
        for b in (0..=SIDE).step_by(7) {
            let unit = cubesphere::vertex(
                patch.face,
                cubesphere::parameter(a, SIDE, true),
                cubesphere::parameter(b, SIDE, true),
                1.0,
            );
            let expect = chain[chosen].sample_direction_m(unit).round();
            let got = f64::from(terrain.node(index, a as i32, b as i32));
            assert_eq!(got, expect, "вузол ({a}, {b})");
        }
    }
}

/// Берегова лінія в тайлсеті стоїть там, де вона в джерела.
///
/// Це та перевірка, заради якої крок і робився (T7). Знак висоти, а не саме
/// значення: між тайлом і джерелом стоїть ланцюг, тобто числа різняться
/// законно — а от суша, що стала морем, означала б зсунуту сітку.
///
/// Точки взяті по обидва боки берега й у глибині обох середовищ, включно з
/// внутрішнім морем (Каспій) — тим випадком, який ловить дзеркальну довготу.
#[test]
fn the_coastline_lands_where_the_source_has_it() {
    let Some(grid) = relief() else { return };

    let terrain = build_earth(&grid, 6);
    let degrees = std::f64::consts::PI / 180.0;

    for (name, lat, lon, land) in [
        ("Сахара", 23.0, 13.0, true),
        ("Тибет", 32.0, 88.0, true),
        ("Амазонія", -3.0, -60.0, true),
        ("Антарктида", -80.0, 0.0, true),
        ("центр Тихого океану", 0.0, -140.0, false),
        ("Атлантика", 30.0, -40.0, false),
        ("Каспій", 42.0, 51.0, false),
        ("Північний Льодовитий", 89.0, 0.0, false),
    ] {
        let (lat, lon) = (lat * degrees, lon * degrees);
        let unit = [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()];

        // Від напрямку до вузла тайла: грань і місце на ній дає `locate`,
        // а патч найглибшого рівня — це просто ціла частина місця в сітці.
        let place = cubesphere::locate(unit);
        let nodes = Patch::face_nodes(5);
        let (u, v) = (place.s * nodes as f64, place.t * nodes as f64);
        let patch = Patch {
            face: place.face,
            level: 5,
            i: (u as usize / SIDE).min((1 << 5) - 1) as u32,
            j: (v as usize / SIDE).min((1 << 5) - 1) as u32,
        };
        let height = terrain.height_m(&patch, u as usize % SIDE, v as usize % SIDE);

        assert_eq!(
            height >= 0.0,
            land,
            "{name}: тайлсет дає {height:.0} м, джерело — {:.0} м",
            grid.sample_direction_m(unit)
        );
    }
}
