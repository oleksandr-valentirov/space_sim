//! Кукер рельєфу: LOLA → тайли кубосфери (ROADMAP-PLANETS.md, R5b).
//!
//! Перший інструмент ассет-пайплайну на Rust. Єдиний кукер до нього — на C
//! (`make cook`, ефемерида), і форма кроку та сама: офлайн, власний формат,
//! версія в заголовку, детермінований вихід.
//!
//! ## Що робить, у трьох рядках
//!
//! Для кожного патча кожного рівня піраміди бере напрямок кожного вузла
//! сітки (`cubesphere::Patch::vertex` на одиничній сфері), питає в LOLA
//! висоту в цьому напрямку й кладе її цілим у пів метра — тими самими
//! одиницями, у яких її зберігає джерело.
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

use crate::Grid;
use engine::cubesphere::{Edge, Patch, FACES, SIDE};
use engine::tiles::{Terrain, HALO, STORED};
use std::path::Path;

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
                            let metres = match direction(&patch, a, b) {
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

    Terrain::build(levels, grid.reference_m, scale, &grids)
}

/// Напрямок вузла тайла, включно з ореолом (R7b).
///
/// Усередині сітки (`0..=SIDE`) це просто вершина патча. Поза нею — вершина
/// **сусіда**, знайдена через [`Patch::halo_node`], а не продовження власної
/// параметризації: за ребром куба міняється грань, а з нею й варп.
///
/// `None` — кут ореолу, де сусіда через ребро немає взагалі.
fn direction(patch: &Patch, a: isize, b: isize) -> Option<[f64; 3]> {
    let inside = |v: isize| (0..=SIDE as isize).contains(&v);
    let edge_of = |v: isize| {
        if v < 0 {
            Some(true)
        } else if v > SIDE as isize {
            Some(false)
        } else {
            None
        }
    };

    let (edge, along) = match (edge_of(a), edge_of(b)) {
        (None, None) => return Some(patch.vertex(a as usize, b as usize, 1.0)),
        (Some(low), None) => (if low { Edge::AMin } else { Edge::AMax }, b as usize),
        (None, Some(low)) => (if low { Edge::BMin } else { Edge::BMax }, a as usize),
        // Обидві координати за краєм — це кут, а не ребро.
        (Some(_), Some(_)) => return None,
    };
    debug_assert!(inside(if edge_of(a).is_some() { b } else { a }));

    let (there, na, nb) = patch.halo_node(edge, along);
    Some(there.vertex(na, nb, 1.0))
}

/// Метри → одиниці зберігання, з насиченням замість загортання.
///
/// Загортання тут було б найгіршим із можливих: гора на 33 км перетворилась
/// би на западину, і виглядало б це правдоподібно.
fn quantise(metres: f64, scale: f64) -> i16 {
    let units = (metres / scale).round();
    units.clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}
