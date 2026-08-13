//! Halo-траєкторія з етапу C, для перевірки перетворення фрейму (ROADMAP F6).
//!
//! Дані — фікстура `data/fixture/halo_inertial.csv`, вивантажена
//! `core/export/ex_trajectory` (ROADMAP C6): та сама орбіта каталогу 1151,
//! перенесена в реальну ефемериду й зшита multiple shooting (C4). Комітиться
//! так само, як `data/fixture/earth_moon.eph`, — рушій не лінкує `core-rs`
//! (це окреме, більше рішення, не для кроку про рендер), тож дані приходять
//! готовим ассетом, а не через FFI.
//!
//! Стовпці `sx,sy,sz` — синодичні координати з `frame_from_inertial` (C,
//! `core/frame.h`), безрозмірні одиниці CR3BP. Це оракул, не вхід рендера:
//! PROJECT.md §7 вимагає рахувати те саме перетворення у вертексному
//! шейдері з позицій Землі й Місяця, а не довіряти готовому числу з CSV.
//! `engine/tests/trajectory.rs` звіряє [`rotating_position`] з цим оракулом.

const CSV: &str = include_str!("../../data/fixture/halo_inertial.csv");

/// mu_Місяць / (mu_Земля + mu_Місяць). Надрукований `make csv`
/// (`ex_cr3bp: ... mu = 0.012150585609624041`) — константа маси системи, не
/// перерахунок фізики, тож жорстко прописана тут точно так само, як
/// [`crate::sphere::EARTH_RADIUS_M`].
pub const MU: f64 = 0.012_150_585_609_624_04;

pub struct Sample {
    pub t: f64,
    pub vessel: [f64; 3],
    pub earth: [f64; 3],
    pub moon: [f64; 3],
    /// Нормаль миттєвої орбітальної площини Земля-Місяць, `d × ḋ`
    /// (`core/frame.h`, `z = h/|h|`), центральною різницею по сусідніх
    /// семплах. Не залежить від камери й від вершини корабля, тож рахується
    /// один раз при завантаженні, а не в шейдері.
    pub z_axis: [f64; 3],
    /// `sx,sy,sz` з фікстури — оракул для тесту, рушій це не використовує.
    pub synodic_reference: [f64; 3],
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(v: [f64; 3]) -> [f64; 3] {
    let len = dot(v, v).sqrt();
    [v[0] / len, v[1] / len, v[2] / len]
}

/// Той самий розрахунок, що вертексний шейдер (`trajectory.slang`) робить
/// на GPU щокадру: `origin`, ортонормований базис із `d = moon − earth`,
/// проєкція корабля на нього, у безрозмірних одиницях CR3BP (масштаб `L`,
/// як у `core/frame.h`).
pub fn rotating_position(
    vessel: [f64; 3],
    earth: [f64; 3],
    moon: [f64; 3],
    z_axis: [f64; 3],
) -> [f64; 3] {
    let d = sub(moon, earth);
    let length = dot(d, d).sqrt();
    let x_axis = [d[0] / length, d[1] / length, d[2] / length];
    let y_axis = cross(z_axis, x_axis);

    let origin = [
        earth[0] + MU * d[0],
        earth[1] + MU * d[1],
        earth[2] + MU * d[2],
    ];
    let rel = sub(vessel, origin);

    [
        dot(rel, x_axis) / length,
        dot(rel, y_axis) / length,
        dot(rel, z_axis) / length,
    ]
}

/// Читає фікстуру й довиводить `z_axis` центральною різницею.
///
/// Крайні семпли беруть різницю в один бік — половина крадеться, а не
/// зникає: перший і останній семпл усе одно потребують нормалі, а
/// однобічна різниця на щільній сітці (~2.7 год між семплами проти
/// 27-денного місячного місяця) вносить похибку, надто малу, щоб її
/// побачити на цьому масштабі.
pub fn load() -> Vec<Sample> {
    let mut lines = CSV.lines();
    lines.next(); // заголовок

    let rows: Vec<[f64; 13]> = lines
        .map(|line| {
            let mut values = [0.0; 13];
            for (slot, field) in values.iter_mut().zip(line.split(',')) {
                *slot = field.parse().expect("фікстура — валідні числа");
            }
            values
        })
        .collect();

    let d_of =
        |row: &[f64; 13]| -> [f64; 3] { [row[7] - row[4], row[8] - row[5], row[9] - row[6]] };

    let mut samples = Vec::with_capacity(rows.len());
    for i in 0..rows.len() {
        let row = &rows[i];

        let prev = if i == 0 { i } else { i - 1 };
        let next = if i + 1 == rows.len() { i } else { i + 1 };
        let d_dot = sub(d_of(&rows[next]), d_of(&rows[prev]));
        let z_axis = normalize(cross(d_of(row), d_dot));

        samples.push(Sample {
            t: row[0],
            vessel: [row[1], row[2], row[3]],
            earth: [row[4], row[5], row[6]],
            moon: [row[7], row[8], row[9]],
            z_axis,
            synodic_reference: [row[10], row[11], row[12]],
        });
    }

    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fixture_is_not_empty() {
        let samples = load();
        assert!(samples.len() > 1000, "лишилось {} семплів", samples.len());
    }

    /// Головна перевірка алгоритму, окремо від GPU: чи відтворює
    /// [`rotating_position`] той самий синодичний фрейм, що `core/frame.h`
    /// поклав у фікстуру. Тут допуск і легко звузити, якщо колись
    /// знадобиться точніша нормаль, ніж центральна різниця.
    #[test]
    fn rotating_position_matches_the_c_oracle() {
        let samples = load();
        let mut max_error = 0.0f64;

        for s in &samples {
            let computed = rotating_position(s.vessel, s.earth, s.moon, s.z_axis);
            for (c, r) in computed.iter().zip(s.synodic_reference) {
                max_error = max_error.max((c - r).abs());
            }
        }

        // Виміряно: 3.48e-7, на семплі 0 — де центральна різниця вироджена
        // в однобічну (крайня точка ряду). Запас удвічі, не на порядок:
        // тісний допуск ловить регресію в самому алгоритмі, а не лише
        // «щось зовсім зламалось».
        assert!(
            max_error < 7e-7,
            "найгірша розбіжність із оракулом: {max_error:e}"
        );
    }
}
