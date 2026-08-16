//! Читач мозаїки Blue Marble (етап T, крок T7c).
//!
//! Головний оракул тут **не власний**, і це навмисно: колір сам про себе
//! нічого не доводить («схоже на Землю» — не перевірка). Доводить його
//! сусідній продукт — там, де ETOPO каже воду, мозаїка мусить бути синьою.
//! Одна перевірка ловить і зсув на пів пікселя, і перевернутий рядок, і
//! переплутані канали, і невірний початок довготи.
//!
//! Решта оракулів тримає те, чого збіг масок не бачить: геометрію сітки
//! (порівнюється з етикеткою ETOPO, тобто з файлом у git), простір відліків
//! (sRGB туди й назад) і ланцюг грубіших сіток.

use dem_cook::bmng::{self, Mosaic};
use dem_cook::etopo::{Header, Relief};
use std::path::{Path, PathBuf};

fn data(dir: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data")
        .join(dir)
        .join(name)
}

/// Сама мозаїка, або `None` — тоді тест каже, чого бракує, і не падає (Q5).
fn mosaic() -> Option<Mosaic> {
    let path = data("bmng", "world.topo.bathy.200407.jpg");
    match Mosaic::read(&path) {
        Ok(map) => Some(map),
        Err(_) => {
            eprintln!(
                "ПРОПУЩЕНО: немає {}. Як покласти назад — data/bmng/README.md",
                path.display()
            );
            None
        }
    }
}

/// sRGB туди й назад — на всіх 256 входах, бо їх рівно 256.
///
/// Перевірка не декодера, а **нашої** пари перетворень: колір іде з файлу в
/// лінійне світло й повертається в байт тайла, і якщо ці дві функції не
/// обернені, поверхня Землі поїде в яскравості цілком — рівно, тобто
/// непомітно, поки не поставити поруч джерело.
#[test]
fn srgb_round_trips_on_every_byte() {
    let table = (0..=255u8).map(|b| {
        let x = f64::from(b) / 255.0;
        let linear = if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        };
        (b, bmng::to_srgb(linear))
    });

    for (before, after) in table {
        assert_eq!(before, after, "{before} → лінійне → {after}");
    }
}

/// Геометрія сітки мусить збігатися з ETOPO — саме на цьому стоїть вибір пари.
///
/// Порівняння з **етикеткою**, а не з другим продуктом: етикетка лежить у
/// git, тобто цей бік перевірки є завжди.
#[test]
fn the_grid_matches_the_dem() {
    let Some(map) = mosaic() else { return };
    let dem = Header::read(&data("etopo", "etopo_2022_60s_surface.lbl")).expect("етикетка в git");

    assert_eq!(map.samples, dem.samples);
    assert_eq!(map.lines, dem.lines);
    assert!(
        (map.per_degree - dem.per_degree).abs() < 1e-9,
        "{} відліків на градус проти {}",
        map.per_degree,
        dem.per_degree
    );
}

/// Названі точки: океан темний і синій, пустеля світла й тепла, лід білий.
///
/// Числа — з незалежного читача (Python + PIL), який пройшов **той самий**
/// шлях: sRGB у лінійне, білінійна вага в лінійному, назад у байт. Порівняння
/// в байтах, бо в лінійних одиницях їх нема з чим звірити оком.
///
/// ⚠ Порівнювати з відліком **пікселя** тут не можна: білінійна вага бере
/// чотирьох сусідів, і на Сахарі це різниця в два байти з 197. Оракул мусить
/// повторювати арифметику, а не лише джерело.
#[test]
fn named_points_match_an_independent_reader() {
    let Some(map) = mosaic() else { return };
    let degrees = std::f64::consts::PI / 180.0;

    for (name, lat, lon, expect) in [
        ("центр Тихого океану", 0.0, -140.0, [5u8, 16, 43]),
        ("Сахара", 23.0, 13.0, [198, 158, 110]),
        ("Амазонія", -3.0, -60.0, [87, 90, 56]),
        ("Гренландія", 72.0, -40.0, [252, 254, 253]),
    ] {
        let got = map.sample(lat * degrees, lon * degrees);
        for channel in 0..3 {
            let byte = bmng::to_srgb(got[channel]);
            assert!(
                byte.abs_diff(expect[channel]) <= 1,
                "{name}, канал {channel}: {byte} проти {}",
                expect[channel]
            );
        }
    }
}

/// Головний оракул: маска води з ETOPO і синій колір мозаїки — це та сама
/// маска.
///
/// Не 100%, і не мусить бути: тінисті ліси в мозаїці теж синюваті, а мілини
/// й солончаки — ні. Виміряно на кожному двадцятому пікселі обох продуктів —
/// **97.95%** зважено `cos(широта)`; будь-який зсув сітки валить це число на
/// десятки відсотків.
#[test]
fn the_mosaic_is_blue_where_the_dem_says_water() {
    let Some(map) = mosaic() else { return };
    let Ok(dem) = Relief::read(&data("etopo", "etopo_2022_60s_surface.tif")) else {
        eprintln!("ПРОПУЩЕНО: немає ETOPO. Як покласти назад — data/etopo/README.md");
        return;
    };

    let degrees = std::f64::consts::PI / 180.0;
    let step = 20;
    let mut agree = 0.0;
    let mut total = 0.0;
    for line in (0..map.lines).step_by(step) {
        let lat = 90.0 - (line as f64 + 0.5) * 180.0 / map.lines as f64;
        let weight = (lat * degrees).cos();
        for sample in (0..map.samples).step_by(step) {
            let lon = -180.0 + (sample as f64 + 0.5) * 360.0 / map.samples as f64;
            let colour = map.sample(lat * degrees, lon * degrees);
            let water = dem.sample_m(lat * degrees, lon * degrees) < 0.0;
            let blue = colour[2] > colour[0] && colour[2] > colour[1];
            if water == blue {
                agree += weight;
            }
            total += weight;
        }
    }

    let fraction = agree / total;
    assert!(
        fraction > 0.95,
        "маски збігаються лише на {:.2}% — сітки розійшлися",
        100.0 * fraction
    );
}

/// Ланцюг грубіших сіток: доходить до вузла найгрубішого рівня піраміди й не
/// рухає середній колір по дорозі.
///
/// Перше — те, заради чого ланцюг існує: вузол рівня 0 накриває 312 км, і
/// якби ланцюг спинявся на діленні надвоє (10800 = 2⁴ · 675), найгрубіша
/// сітка була б удесятеро дрібніша за вузол, тобто рівень 0 знову брав би
/// точку з тридцяти тисяч пікселів.
///
/// Друге — те, чого ланцюг не має права зробити: середнє він зберігає, бо
/// прибирає деталь, а не міняє яскравість планети.
#[test]
fn the_chain_reaches_the_coarsest_node_and_keeps_the_mean() {
    let Some(map) = mosaic() else { return };

    let chain = map.chain();
    let node_rad = std::f64::consts::FRAC_PI_2 / 32.0;
    let coarsest = chain.last().expect("ланцюг не порожній");
    assert!(
        coarsest.pixel_rad() >= node_rad,
        "найгрубіша сітка {}×{} — {:.4} рад на піксель проти {node_rad:.4} рад вузла",
        coarsest.samples,
        coarsest.lines,
        coarsest.pixel_rad()
    );

    // Незважене середнє, а не `mean()`: рівні блоки box-фільтра зберігають
    // саме суму, тобто це **точний** інваріант, і допуск тут лише на
    // округлення `f32`. Зважене `cos(широта)` таким інваріантом не є — на
    // сітці в п'ятнадцять рядків вага центра блоку вже не дорівнює середній
    // вазі по блоку, і 0.9% на найгрубішому рівні є правдою про сферу, а не
    // помилкою ланцюга.
    let flat = |level: &Mosaic| {
        let mut sum = [0.0f64; 3];
        for pixel in level.raw.chunks_exact(3) {
            for (channel, value) in pixel.iter().enumerate() {
                sum[channel] += f64::from(*value);
            }
        }
        sum.map(|s| s / (level.samples * level.lines) as f64)
    };

    let mean = flat(&map);
    for (index, level) in chain.iter().enumerate() {
        let here = flat(level);
        for channel in 0..3 {
            assert!(
                (here[channel] - mean[channel]).abs() < 1e-6,
                "рівень {index}, канал {channel}: {} проти {}",
                here[channel],
                mean[channel]
            );
        }
    }
}
