//! Розподіл нахилу в тайлсеті рельєфу (етап T, кроки T4c і T7f).
//!
//! Існує заради одного числа — [`engine::material::SLOPE_REF`], нахилу, на
//! якому підсвітка схилу виходить на повну. Взяти його з фізики не можна:
//! кут природного укосу реголіту стосується **місцевого** схилу, а
//! `Terrain::slope_at` міряє нахил на базі найдрібнішого вузла піраміди —
//! 5330 м на Місяці. Різниця виявилась більш ніж удвічі, і поставлений «з
//! фізики» поріг вимикав правило на 999 вузлах з 1000.
//!
//! **Друга таблиця — про рівень патча, і вона з'явилась не заради ассета, а
//! заради артефакту** (T7f): на Землі з висоти 10⁶ м кадр вийшов смугастим по
//! патчах. Освітлення тут ні до чого — нормаль у шейдері це нормаль сфери, —
//! отже яскравість може відрізнятися лише множником матеріалу, а той читає
//! нахил. Тож питання ставиться прямо: чи залежить нахил від того, патч
//! якого рівня його спитав.
//!
//!     cargo run --release -p engine --example slope_histogram [асет]

use engine::cubesphere::{Patch, FACES, SIDE};
use engine::{demo, material, tiles};

/// Квантилі, які друкуються обома таблицями.
const QUANTILES: [f64; 7] = [0.5, 0.75, 0.9, 0.95, 0.99, 0.999, 1.0];

fn main() -> Result<(), String> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| demo::TERRAIN_ASSET.to_string());
    let bytes = std::fs::read(&path)
        .map_err(|e| format!("{path}: {e}\nполікувати: make cook-dem або make cook-earth"))?;
    let terrain = tiles::Terrain::from_bytes(&bytes)?;
    println!(
        "{path}: {} рівнів, крок {:.0} м",
        terrain.levels,
        terrain.step_m()
    );

    // Найглибший рівень: там нахил міряється на найкоротшій базі, яка в ассеті
    // взагалі є, і саме його читає кадр зблизька.
    let deepest = terrain.levels.saturating_sub(1);
    let values = sample(&terrain, deepest);
    let at = |q: f64| values[((values.len() - 1) as f64 * q) as usize];
    println!("вузлів {}", values.len());
    for q in QUANTILES {
        let slope = at(q);
        println!(
            "  {:>5.1}% : нахил {slope:.4} ({:.2}°), множник {:.3}",
            q * 100.0,
            slope.atan().to_degrees(),
            material::tint(slope, 0.0)
        );
    }
    println!("SLOPE_REF зараз {:.3}", material::SLOPE_REF);

    // ── Чи однаковий нахил на всіх рівнях патча ────────────────────────────
    //
    // `step_m` обіцяє, що ні: крок центральної різниці взято на найдрібнішому
    // вузлі піраміди, а `delta_nodes` перераховує його в координати тайла,
    // який патч читає, — тобто **відстань** та сама на будь-якому рівні.
    // Але дані на грубому тайлі інші: він рідша вибірка тієї самої поверхні.
    // Ця таблиця й каже, наскільки «інші» — у нахилі й одразу в множнику,
    // тобто в тому, що видно оком.
    println!();
    println!("нахил за рівнем патча (той самий крок у метрах, різні дані):");
    println!("  рівень |  медіана |  90% |  множник медіани");
    for level in 0..=deepest {
        let values = sample(&terrain, level);
        let at = |q: f64| values[((values.len() - 1) as f64 * q) as usize];
        println!(
            "  {level:>6} | {:>8.4} | {:>6.4} | {:.4}",
            at(0.5),
            at(0.9),
            material::tint(at(0.5), 0.0)
        );
    }
    Ok(())
}

/// Нахил у вузлах патчів заданого рівня — розріджено й відсортовано.
///
/// Розрідження одне на всі рівні (кожен другий патч, кожен четвертий вузол),
/// тож на грубих рівнях вибірка виходить меншою — і це чесно: там патчів
/// стільки й є.
fn sample(terrain: &tiles::Terrain, level: u32) -> Vec<f64> {
    let side = 1u32 << level;
    let step = if side >= 2 { 2 } else { 1 };
    let mut values = Vec::new();
    for face in 0..FACES {
        for i in (0..side).step_by(step) {
            for j in (0..side).step_by(step) {
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
    values
}
