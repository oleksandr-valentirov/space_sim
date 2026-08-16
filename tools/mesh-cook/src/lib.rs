//! Кукер мешів: glTF з Blender → `SSMSH` (ROADMAP, T5d2).
//!
//! Робота розділена так само, як у `dem-cook`: сама куховарня — бібліотека,
//! бінарник лише розбирає аргументи. Причина не в шарах: тест не може
//! покликати функцію з бінарника.

pub mod gltf;

use engine::mesh::{self, Model};
use std::path::Path;

/// Скукувати модель: прочитати glTF, нормалізувати до одиничної висоти,
/// віддати те, що піде у файл, і числа, які має перевірити викликач.
#[derive(Debug)]
pub struct Cooked {
    pub model: Model,
    /// Габарити, опубліковані експортером у JSON акесора.
    pub published: gltf::Published,
    /// Знаковий об'єм **у метрах**, тобто до нормалізації.
    pub volume_m3: f64,
    pub index_component: u64,
}

pub fn cook(path: &Path) -> Result<Cooked, String> {
    let loaded = gltf::load(path)?;

    // Об'єм рахується **до** нормалізації: саме в метрах його дає Blender, і
    // саме там його можна звірити. Після поділу на висоту він падає в куб
    // висоти, і порівняння вимагало б ще одного множення — тобто ще одного
    // місця, де можна помилитись.
    let volume_m3 = mesh::signed_volume(&loaded.mesh);

    // Габарити з JSON перевіряються тут, а не в тесті, і це різні речі:
    // тест ловить регресію в нашому читачі, а ця перевірка — **зіпсований
    // ассет**, тобто випадок, коли `.bin` і `.gltf` розійшлися.
    let (low, high) = bounds(&loaded.mesh);
    for k in 0..3 {
        for (ours, theirs, what) in [
            (low[k], loaded.published.min[k], "min"),
            (high[k], loaded.published.max[k], "max"),
        ] {
            // Допуск від розміру моделі: числа в JSON надруковані десятковим
            // рядком, тобто вже пройшли туди-назад через текст.
            let scale = (high[k] - low[k]).abs().max(1.0);
            if (ours - theirs).abs() > 1e-6 * scale {
                return Err(format!(
                    "{what}[{k}]: у .bin {ours}, а в JSON акесора {theirs} — \
                     файли розійшлися"
                ));
            }
        }
    }

    let model = Model::from_metres(loaded.mesh, loaded.paint)?;
    Ok(Cooked {
        model,
        published: loaded.published,
        volume_m3,
        index_component: loaded.index_component,
    })
}

/// Габарити меша по кожній осі.
pub fn bounds(mesh: &engine::sphere::Mesh) -> ([f64; 3], [f64; 3]) {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for p in &mesh.positions {
        for k in 0..3 {
            low[k] = low[k].min(p[k]);
            high[k] = high[k].max(p[k]);
        }
    }
    (low, high)
}
