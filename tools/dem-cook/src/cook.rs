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
use engine::cubesphere::{Patch, FACES, SIDE};
use engine::tiles::{Terrain, NODES};
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
                    let mut tile = Vec::with_capacity(NODES * NODES);
                    for a in 0..=SIDE {
                        for b in 0..=SIDE {
                            // Одинична сфера: висота залежить від напрямку, а
                            // не від радіуса, і напрямок тут бітово той самий,
                            // що в сусіднього патча на спільному ребрі.
                            let metres = grid.sample_direction_m(patch.vertex(a, b, 1.0));
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

/// Метри → одиниці зберігання, з насиченням замість загортання.
///
/// Загортання тут було б найгіршим із можливих: гора на 33 км перетворилась
/// би на западину, і виглядало б це правдоподібно.
fn quantise(metres: f64, scale: f64) -> i16 {
    let units = (metres / scale).round();
    units.clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}
