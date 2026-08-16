//! Розподіл нахилу в тайлсеті рельєфу (етап T, крок T4c).
//!
//! Існує заради одного числа — [`engine::material::SLOPE_REF`], нахилу, на
//! якому підсвітка схилу виходить на повну. Взяти його з фізики не можна:
//! кут природного укосу реголіту стосується **місцевого** схилу, а
//! `Terrain::slope_at` міряє нахил на базі найдрібнішого вузла піраміди —
//! 5330 м на Місяці. Різниця виявилась більш ніж удвічі, і поставлений «з
//! фізики» поріг вимикав правило на 999 вузлах з 1000.
//!
//! Тому число береться з розподілу, а розподіл — звідси. T7 приведе Землю з
//! іншим DEM та іншою базою, і тоді це доведеться перерахувати.
//!
//! cargo run --release -p engine --example slope_histogram

use engine::cubesphere::{Patch, FACES, SIDE};
use engine::{demo, material, tiles};

fn main() -> Result<(), String> {
    let bytes = std::fs::read(demo::TERRAIN_ASSET)
        .map_err(|e| format!("{}: {e}\nполікувати: make cook-dem", demo::TERRAIN_ASSET))?;
    let terrain = tiles::Terrain::from_bytes(&bytes)?;
    println!(
        "{}: {} рівнів, крок {:.0} м",
        demo::TERRAIN_ASSET,
        terrain.levels,
        terrain.step_m()
    );

    // Найглибший рівень: там нахил міряється на найкоротшій базі, яка в ассеті
    // взагалі є, і саме його читає кадр зблизька.
    let level = terrain.levels.saturating_sub(1);
    let side = 1u32 << level;
    let mut values = Vec::new();
    for face in 0..FACES {
        for i in (0..side).step_by(2) {
            for j in (0..side).step_by(2) {
                let patch = Patch { face, level, i, j };
                for a in (0..=SIDE).step_by(4) {
                    for b in (0..=SIDE).step_by(4) {
                        values.push(terrain.slope_at(&patch, a, b));
                    }
                }
            }
        }
    }
    values.sort_by(|a, b| a.partial_cmp(b).expect("нахил не буває NaN"));

    let at = |q: f64| values[((values.len() - 1) as f64 * q) as usize];
    println!("вузлів {}", values.len());
    for q in [0.5, 0.75, 0.9, 0.95, 0.99, 0.999, 1.0] {
        let slope = at(q);
        println!(
            "  {:>5.1}% : нахил {slope:.4} ({:.2}°), множник {:.3}",
            q * 100.0,
            slope.atan().to_degrees(),
            material::tint(slope, 0.0)
        );
    }
    println!("SLOPE_REF зараз {:.3}", material::SLOPE_REF);
    Ok(())
}
