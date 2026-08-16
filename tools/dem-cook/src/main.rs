//! Кукер поверхні: командний рядок (R5b; етап T, T2d).
//!
//! Сама робота — у [`dem_cook::cook`]; тут лише розбір аргументів. Розділено
//! не з любові до шарів: тест не може покликати функцію з бінарника, а
//! детермінізм виходу треба перевіряти саме викликом, двічі.
//!
//!     cargo run -p dem-cook                       data/lola  → assets/moon.dem
//!     cargo run -p dem-cook -- --colour           data/wac   → assets/moon.col
//!     cargo run -p dem-cook -- --body earth       data/etopo → assets/earth.dem
//!     cargo run -p dem-cook -- --body earth --colour  data/bmng → assets/earth.col
//!
//! Тіло — окремий прапорець, а не окремий бінарник: спільного в них рівно
//! стільки, скільки й мало б бути — обхід кубосфери й формат тайла.

use dem_cook::cook::{cook, cook_colour, cook_earth, cook_earth_colour};
use std::path::PathBuf;

/// Скільки рівнів піраміди висот кукати за замовчуванням.
///
/// Виміряне число, не смак. LDEM_4 дає 7581 м на відлік; клітинка патча
/// рівня `L` на Місяці — `(π·R/2) / (SIDE·2^L)`, тобто 85 км на рівні 0 і
/// 5.3 км на рівні 4. Тобто рівень 4 уже дрібніший за джерело, а рівень 5
/// не приніс би жодного нового числа — лише вчетверо більше файлу.
const DEFAULT_LEVELS: u32 = 5;

/// Скільки рівнів піраміди кольору — на один більше, і теж виміряне (T2a).
///
/// Джерело вдвічі дрібніше за LOLA (1.9 км проти 7.6 км на піксель), тож
/// шостий рівень має що взяти: 3.8 км на вузол. Сьомий коштував би 256 МіБ
/// відеопам'яті проти 32 і вчетверо довшого завантаження, а екрана не досягає
/// однаково — той розрив закриває правило матеріалу (T4).
const DEFAULT_COLOUR_LEVELS: u32 = 6;

/// Скільки рівнів піраміди в Землі — шість, і теж виміряне (T7).
///
/// Вузол рівня 6 накриває 9.77 км Землі, тобто вп'ятеро грубіше за джерело
/// (1.85 км) — сітку кукер усереднює ланцюгом. Сьомий рівень коштував би
/// 384 МіБ відеопам'яті проти 96 і 1.67 мс кадру проти 0.42 (борг D19), а
/// екрана не досягає однаково: при камері на 100 км вузол шостого рівня — це
/// 61 екранний піксель.
const DEFAULT_EARTH_LEVELS: u32 = 6;

fn main() {
    let mut colour = false;
    let mut earth = false;
    let mut source: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut levels: Option<u32> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--colour" => colour = true,
            "--body" => match args.next().as_deref() {
                Some("earth") => earth = true,
                Some("moon") => earth = false,
                other => {
                    eprintln!("--body хоче moon або earth, а не {other:?}");
                    std::process::exit(2);
                }
            },
            "--source" => source = Some(args.next().expect("--source хоче шлях").into()),
            "--out" => out = Some(args.next().expect("--out хоче шлях").into()),
            "--levels" => {
                levels = Some(
                    args.next()
                        .expect("--levels хоче число")
                        .parse()
                        .expect("--levels хоче число"),
                )
            }
            other => {
                eprintln!("невідомий аргумент {other}");
                std::process::exit(2);
            }
        }
    }

    // Замовчування залежать від того, що кукається: джерела різні, глибини
    // пірамід різні, і плутати їх мовчки не можна.
    let (default_source, default_out, default_levels) = match (earth, colour) {
        (false, false) => ("data/lola/ldem_4.img", "assets/moon.dem", DEFAULT_LEVELS),
        (false, true) => (
            "data/wac/wac_global_016p.img",
            "assets/moon.col",
            DEFAULT_COLOUR_LEVELS,
        ),
        (true, false) => (
            "data/etopo/etopo_2022_60s_surface.tif",
            "assets/earth.dem",
            DEFAULT_EARTH_LEVELS,
        ),
        (true, true) => (
            "data/bmng/world.topo.bathy.200407.jpg",
            "assets/earth.col",
            DEFAULT_EARTH_LEVELS,
        ),
    };
    let source = source.unwrap_or_else(|| PathBuf::from(default_source));
    let out = out.unwrap_or_else(|| PathBuf::from(default_out));
    let levels = levels.unwrap_or(default_levels);

    let result = match (earth, colour) {
        (false, false) => cook(&source, &out, levels),
        (false, true) => cook_colour(&source, &out, levels),
        (true, false) => cook_earth(&source, &out, levels),
        (true, true) => cook_earth_colour(&source, &out, levels),
    };

    match result {
        Ok(report) => println!("{report}"),
        Err(message) => {
            eprintln!("кукер не впорався: {message}");
            std::process::exit(1);
        }
    }
}
