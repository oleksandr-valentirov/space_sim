//! Кукер поверхні: командний рядок (R5b; етап T, T2d).
//!
//! Сама робота — у [`dem_cook::cook`]; тут лише розбір аргументів. Розділено
//! не з любові до шарів: тест не може покликати функцію з бінарника, а
//! детермінізм виходу треба перевіряти саме викликом, двічі.
//!
//!     cargo run -p dem-cook              висоти:  data/lola  → assets/moon.dem
//!     cargo run -p dem-cook -- --colour  колір:   data/wac   → assets/moon.col

use dem_cook::cook::{cook, cook_colour};
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

fn main() {
    let mut colour = false;
    let mut source: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut levels: Option<u32> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--colour" => colour = true,
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
    let (default_source, default_out, default_levels) = if colour {
        (
            "data/wac/wac_global_016p.img",
            "assets/moon.col",
            DEFAULT_COLOUR_LEVELS,
        )
    } else {
        ("data/lola/ldem_4.img", "assets/moon.dem", DEFAULT_LEVELS)
    };
    let source = source.unwrap_or_else(|| PathBuf::from(default_source));
    let out = out.unwrap_or_else(|| PathBuf::from(default_out));
    let levels = levels.unwrap_or(default_levels);

    let result = if colour {
        cook_colour(&source, &out, levels)
    } else {
        cook(&source, &out, levels)
    };

    match result {
        Ok(report) => println!("{report}"),
        Err(message) => {
            eprintln!("кукер не впорався: {message}");
            std::process::exit(1);
        }
    }
}
