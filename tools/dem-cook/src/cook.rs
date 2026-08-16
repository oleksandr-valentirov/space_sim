//! Кукер поверхні: LOLA і LROC WAC → тайли кубосфери (R5b; етап T, T2d).
//!
//! Перший інструмент ассет-пайплайну на Rust. Єдиний кукер до нього — на C
//! (`make cook`, ефемерида), і форма кроку та сама: офлайн, власний формат,
//! версія в заголовку, детермінований вихід.
//!
//! ## Що робить, у трьох рядках
//!
//! Для кожного патча кожного рівня піраміди бере напрямок кожного вузла
//! сітки (`cubesphere::Patch::vertex` на одиничній сфері), питає в джерела
//! значення в цьому напрямку й кладе його цілим: висоту — у пів метра, тими
//! самими одиницями, у яких її зберігає LOLA; колір — у частках [`SCALE`].
//!
//! ## Два джерела, один обхід
//!
//! Обхід спільний навмисно (`tiles::node_direction`): **вузол кольору мусить
//! лежати рівно там, де вузол висоти**. Інакше колір і рельєф зсунулись би один відносно
//! одного на пів вузла, і виглядало б це як помилка джерела, а не як два різні
//! обходи. Глибина пірамід при цьому різна (5 проти 6) — і саме тому вона
//! параметр, а не константа обходу.
//!
//! ## Чому вихід детермінований, і чому це не випадковість
//!
//! Порядок обходу сталий, вершини патча бітово однакові з обох боків ребра
//! (R2b), а вибірка з сітки — чиста функція від напрямку. Отже той самий
//! вхід дає той самий байт, і SHA файлу стабільний між прогонами. Це
//! перевіряється, а не проголошується: `tools/dem-cook/tests/cook.rs`.
//!
//! `libm` тут дозволений без застережень — правило 4 етапу R: кукер це офлайн
//! і CPU поза інтегратором.

use crate::albedo::Albedo;
use crate::bmng::{self, Mosaic};
use crate::etopo::{self, Relief};
use crate::Grid;
use engine::cubesphere::{Patch, FACES, SIDE};
use engine::tiles::{self, node_direction, Colour, Terrain, HALO, NODES, STORED};
use std::path::Path;

/// Чому дорівнює відлік 255 у колірному тайлі — відбивна здатність.
///
/// Стала, а не перцентиль, порахований на льоту, і причина в тому, що вихід
/// кукера мусить бути передбачуваним: число, обране з самих даних, тихо
/// змінило б **усі** байти ассета від правки в одному кратері. Виміряне
/// підґрунтя — розподіл мозаїки WAC: медіана 0.044, p99.9 = 0.197, хвіст до
/// 0.599 — це 0.09% пікселів (`engine::tiles::Colour`).
///
/// Скільки вузлів насправді насичилось, кукер друкує в звіті — тобто вибір
/// перевіряється числом, а не лишається здогадом у коді.
pub const SCALE: f32 = 0.25;

/// Скукувати тайлсет із сітки LOLA.
pub fn cook(source: &Path, out: &Path, levels: u32) -> Result<String, String> {
    let grid = Grid::read(source)?;
    let terrain = build(&grid, levels);
    let bytes = terrain.to_bytes();

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;

    Ok(format!(
        "{} — {} рівнів, {} тайлів, {:.1} МіБ; найнижча точка {:.1} м",
        out.display(),
        levels,
        Terrain::count(levels),
        bytes.len() as f64 / (1024.0 * 1024.0),
        terrain.lowest_m()
    ))
}

/// Піраміда тайлів із сітки — без запису на диск, щоб тест міг звірити двічі.
pub fn build(grid: &Grid, levels: u32) -> Terrain {
    // Одиниці зберігання ті самі, що в джерела: перекладати їх означало б
    // округлити двічі там, де досить нуля разів.
    let scale = grid.scale_m as f32;
    // Місяць моря не має, і сентинел каже це прямо: правило матеріалу
    // працює на ньому скрізь, як і до T7f.
    Terrain::build(
        levels,
        grid.reference_m,
        scale,
        tiles::NO_SEA,
        &height_grids(grid, levels),
    )
}

/// Сітки висот **з ореолом** — рівно те, що приймає `Terrain::build`.
///
/// Окремо від [`build`] не заради структури, а заради оракула: з версії 4
/// формату ореол у файл не потрапляє (нахил запечений, градієнт переїхав у
/// записувач), тож перевірити «ореол — це справді вузол сусіда» можна лише
/// тут, до того як його з'їдять. Перевіряє це
/// `tests/cook.rs::the_halo_holds_the_neighbours_own_node`.
pub fn height_grids(grid: &Grid, levels: u32) -> Vec<Vec<i16>> {
    let mut grids = Vec::with_capacity(Terrain::count(levels));
    for level in 0..levels {
        let side = 1u32 << level;
        for face in 0..FACES {
            for i in 0..side {
                for j in 0..side {
                    let patch = Patch { face, level, i, j };
                    let mut tile = Vec::with_capacity(STORED * STORED);
                    for a in 0..STORED as isize {
                        for b in 0..STORED as isize {
                            let (a, b) = (a - HALO as isize, b - HALO as isize);
                            // Одинична сфера: висота залежить від напрямку, а
                            // не від радіуса, і напрямок тут бітово той самий,
                            // що в сусіднього патча на спільному ребрі.
                            let metres = match node_direction(&patch, a, b) {
                                Some(unit) => grid.sample_direction_m(unit),
                                // Кут ореолу: сусіда через ребро там немає, і
                                // ніхто його не читає (`engine::tiles`).
                                None => 0.0,
                            };
                            tile.push(quantise(metres, grid.scale_m));
                        }
                    }
                    grids.push(tile);
                }
            }
        }
    }
    grids
}

/// Скукувати тайлсет висот Землі з ETOPO.
///
/// Окремо від [`cook`], а не прапорцем усередині: спільним у них лишається
/// обхід (`tiles::node_direction`) і формат, а джерела різні в усьому — одиниці,
/// опорний радіус, ланцюг, реєстрація довготи.
pub fn cook_earth(source: &Path, out: &Path, levels: u32) -> Result<String, String> {
    let relief = Relief::read(source)?;
    let terrain = build_earth(&relief, levels);
    let bytes = terrain.to_bytes();

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;

    Ok(format!(
        "{} — {} рівнів, {} тайлів, {:.1} МіБ; найнижча точка {:.1} м, суші {:.2}%",
        out.display(),
        levels,
        Terrain::count(levels),
        bytes.len() as f64 / (1024.0 * 1024.0),
        terrain.lowest_m(),
        100.0 * relief.land_fraction(),
    ))
}

/// Піраміда висот Землі — без запису на диск, щоб тест міг звірити двічі.
pub fn build_earth(relief: &Relief, levels: u32) -> Terrain {
    let chain = relief.chain();
    let rads = chain.iter().map(Relief::pixel_rad).collect::<Vec<f64>>();

    let mut grids = Vec::with_capacity(Terrain::count(levels));
    for level in 0..levels {
        let source = &chain[source_for(&rads, level)];
        let side = 1u32 << level;
        for face in 0..FACES {
            for i in 0..side {
                for j in 0..side {
                    let patch = Patch { face, level, i, j };
                    let mut tile = Vec::with_capacity(STORED * STORED);
                    for a in 0..STORED as isize {
                        for b in 0..STORED as isize {
                            let (a, b) = (a - HALO as isize, b - HALO as isize);
                            let metres = match node_direction(&patch, a, b) {
                                Some(unit) => source.sample_direction_m(unit),
                                // Кут ореолу: сусіда через ребро там немає, і
                                // ніхто його не читає (`engine::tiles`).
                                None => 0.0,
                            };
                            // Одиниця зберігання — метр, тобто та сама, у якій
                            // сітка вже лежить; округлення тут друге й останнє.
                            tile.push(quantise(metres, 1.0));
                        }
                    }
                    grids.push(tile);
                }
            }
        }
    }

    // Рівень моря — рівно нуль, і це не домовленість: ETOPO міряє висоти від
    // геоїда, а одиниця зберігання тут метр. Тобто «нижче нуля» в цьому
    // тайлсеті означає «під водою» за побудовою джерела, а не за нашим
    // вибором порога. Виміряно на скукованому ассеті: нижче нуля 72.0%
    // вузлів, при справжній частці океану 71%.
    Terrain::build(levels, etopo::REFERENCE_M, 1.0, 0.0, &grids)
}

/// Скукувати колірний тайлсет із мозаїки WAC.
pub fn cook_colour(source: &Path, out: &Path, levels: u32) -> Result<String, String> {
    let map = Albedo::read(source)?;
    let (colour, saturated) = build_colour(&map, levels);
    let bytes = colour.to_bytes();

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;

    let nodes = tiles::count(levels) * NODES * NODES;
    Ok(format!(
        "{} — {levels} рівнів, {} тайлів, {:.1} МіБ; шкала {SCALE}, насичено \
         {saturated} вузлів з {nodes} ({:.4}%)",
        out.display(),
        tiles::count(levels),
        bytes.len() as f64 / (1024.0 * 1024.0),
        100.0 * saturated as f64 / nodes as f64,
    ))
}

/// Піраміда колірних тайлів — без запису на диск, щоб тест міг звірити двічі.
///
/// Разом із нею — скільки вузлів упритул до [`SCALE`]: це і є ціна вибору
/// шкали, і платиться вона в тих самих байтах, що й сам ассет.
pub fn build_colour(map: &Albedo, levels: u32) -> (Colour, usize) {
    let chain = map.chain();
    let rads = chain.iter().map(Albedo::pixel_rad).collect::<Vec<f64>>();
    let mut saturated = 0usize;
    let mut grids = Vec::with_capacity(tiles::count(levels));
    for level in 0..levels {
        let source = &chain[source_for(&rads, level)];
        let side = 1u32 << level;
        for face in 0..FACES {
            for i in 0..side {
                for j in 0..side {
                    let patch = Patch { face, level, i, j };
                    let mut tile = Vec::with_capacity(NODES * NODES);
                    for a in 0..NODES {
                        for b in 0..NODES {
                            // Ореолу колірний тайл не несе (W4): градієнта в
                            // кольору немає, а вибірка на краю патча має на
                            // ньому вагу нуль.
                            let value = source.sample_direction(patch.vertex(a, b, 1.0));
                            let unit = quantise_colour(value);
                            if unit == u8::MAX {
                                saturated += 1;
                            }
                            tile.push(unit);
                        }
                    }
                    grids.push(tile);
                }
            }
        }
    }
    (Colour::build(levels, 1, SCALE, false, &grids), saturated)
}

/// Скукувати колірний тайлсет Землі з мозаїки BMNG.
pub fn cook_earth_colour(source: &Path, out: &Path, levels: u32) -> Result<String, String> {
    let map = Mosaic::read(source)?;
    let colour = build_earth_colour(&map, levels);
    let bytes = colour.to_bytes();

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;

    // Два середні поруч, і це не звіт, а звірка (T7h). Перше пораховане по
    // сітці широта-довгота джерела, друге — по найгрубішому рівню піраміди
    // кубосфери, тобто зовсім іншим шляхом і після трьох перетворень. Друге з
    // них і читає рушій, коли будує таблицю неба.
    let mean = map.mean();
    let ours = colour.mean();
    Ok(format!(
        "{} — {levels} рівнів, {} тайлів, {:.1} МіБ, чотири канали sRGB; \
         середній колір мозаїки {:.4} {:.4} {:.4}, піраміди {:.4} {:.4} {:.4}",
        out.display(),
        tiles::count(levels),
        bytes.len() as f64 / (1024.0 * 1024.0),
        mean[0],
        mean[1],
        mean[2],
        ours[0],
        ours[1],
        ours[2],
    ))
}

/// Піраміда колірних тайлів Землі — без запису на диск, щоб тест звірив двічі.
///
/// ## Чотири канали, з яких несе колір три
///
/// Трибайтового формату текстури не існує ні в wgpu, ні у Vulkan (T2a), тож
/// четвертий байт є в будь-якому разі. Він заповнюється `255` і не читається
/// ніким: маска води виводиться з висоти безкоштовно (`h < 0`), а поле, яке
/// ніхто не читає, гірше за свою відсутність — тому туди й не кладеться нічого
/// «про запас».
///
/// ## Байт зберігає sRGB, а не лінійне світло
///
/// Усе всередині — і білінійна вага, і ланцюг — рахується лінійно
/// (`bmng::Mosaic`), а в тайл кладеться sRGB-кодований байт. Причина числова:
/// лінійна яскравість океану — 0.0015, тобто нуль у восьми бітах. Розкодує
/// його GPU при вибірці (`Rgba8UnormSrgb`), безкоштовно, а на CPU —
/// `Colour::reflectance`.
pub fn build_earth_colour(map: &Mosaic, levels: u32) -> Colour {
    let chain = map.chain();
    let rads = chain.iter().map(Mosaic::pixel_rad).collect::<Vec<f64>>();

    let mut grids = Vec::with_capacity(tiles::count(levels));
    for level in 0..levels {
        let source = &chain[source_for(&rads, level)];
        let side = 1u32 << level;
        for face in 0..FACES {
            for i in 0..side {
                for j in 0..side {
                    let patch = Patch { face, level, i, j };
                    let mut tile = Vec::with_capacity(NODES * NODES * 4);
                    for a in 0..NODES {
                        for b in 0..NODES {
                            let linear = source.sample_direction(patch.vertex(a, b, 1.0));
                            for value in linear {
                                tile.push(bmng::to_srgb(value));
                            }
                            tile.push(u8::MAX);
                        }
                    }
                    grids.push(tile);
                }
            }
        }
    }

    // Шкала одиниця: у байті лежить весь діапазон кольору, і стискати його
    // нема куди — на відміну від відбивної здатності Місяця, у якої 99.9%
    // відліків нижчі за 0.2 (`SCALE`).
    Colour::build(levels, 4, 1.0, true, &grids)
}

/// Яку сітку ланцюга читає рівень піраміди.
///
/// Найгрубішу з тих, чий піксель ще не більший за вузол цього рівня. Кут, а
/// не метри: вузол рівня `L` — це `(π/2) / (SIDE·2^L)` радіана незалежно від
/// радіуса тіла, а піксель сітки — `π/(180·per_degree)`. Радіус скорочується,
/// тож те саме число працює і для Місяця, і для Землі.
///
/// Дрібніша сітка дала б точкову вибірку там, де вузол накриває тисячі
/// пікселів (плямистий шум замість карти); грубіша викинула б деталь, яку
/// вузол ще здатен нести.
///
/// Параметр — самі кути, а не ланцюг: сіток тепер три різні типи (висоти
/// Місяця, мозаїка Місяця, висоти й колір Землі), а питання до них одне.
pub fn source_for(pixel_rad: &[f64], level: u32) -> usize {
    let node_rad = std::f64::consts::FRAC_PI_2 / f64::from(SIDE as u32 * (1u32 << level));
    let mut best = 0;
    for (index, rad) in pixel_rad.iter().enumerate() {
        if *rad <= node_rad {
            best = index;
        }
    }
    best
}

/// Відбивна здатність → один байт.
///
/// Затискається з обох боків, і обидва боки означають різне. Знизу — від'ємні
/// значення джерела (1.66% мозаїки): це шум фотометричної нормалізації, а нуль
/// — фізична підлога. Згори — хвіст понад [`SCALE`], і його насичення в білий
/// кукер рахує й друкує.
fn quantise_colour(value: f64) -> u8 {
    let units = (value / f64::from(SCALE) * 255.0).round();
    units.clamp(0.0, 255.0) as u8
}

/// Метри → одиниці зберігання, з насиченням замість загортання.
///
/// Загортання тут було б найгіршим із можливих: гора на 33 км перетворилась
/// би на западину, і виглядало б це правдоподібно.
fn quantise(metres: f64, scale: f64) -> i16 {
    let units = (metres / scale).round();
    units.clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}
