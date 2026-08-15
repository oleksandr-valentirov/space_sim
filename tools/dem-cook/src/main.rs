//! Кукер рельєфу: командний рядок (ROADMAP-PLANETS.md, R5b).
//!
//! Сама робота — у [`dem_cook::cook`]; тут лише розбір аргументів. Розділено
//! не з любові до шарів: тест не може покликати функцію з бінарника, а
//! детермінізм виходу треба перевіряти саме викликом, двічі.

use dem_cook::cook::cook;
use std::path::PathBuf;

/// Скільки рівнів піраміди кукати за замовчуванням.
///
/// Виміряне число, не смак. LDEM_4 дає 7581 м на відлік; клітинка патча
/// рівня `L` на Місяці — `(π·R/2) / (SIDE·2^L)`, тобто 85 км на рівні 0 і
/// 5.3 км на рівні 4. Тобто рівень 4 уже дрібніший за джерело, а рівень 5
/// не приніс би жодного нового числа — лише вчетверо більше файлу.
const DEFAULT_LEVELS: u32 = 5;

fn main() {
    let mut source = PathBuf::from("data/lola/ldem_4.img");
    let mut out = PathBuf::from("assets/moon.dem");
    let mut levels = DEFAULT_LEVELS;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--source" => source = args.next().expect("--source хоче шлях").into(),
            "--out" => out = args.next().expect("--out хоче шлях").into(),
            "--levels" => {
                levels = args
                    .next()
                    .expect("--levels хоче число")
                    .parse()
                    .expect("--levels хоче число")
            }
            other => {
                eprintln!("невідомий аргумент {other}");
                std::process::exit(2);
            }
        }
    }

    match cook(&source, &out, levels) {
        Ok(report) => println!("{report}"),
        Err(message) => {
            eprintln!("кукер не впорався: {message}");
            std::process::exit(1);
        }
    }
}
