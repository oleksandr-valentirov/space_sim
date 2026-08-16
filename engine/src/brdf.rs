//! Матеріал корпусу: Cook-Torrance з GGX (ROADMAP, T5c).
//!
//! Дослівний двійник `ship.slang`, рівно як [`crate::atmosphere`] проти
//! `sky.slang` і [`crate::cull`] проти `cull.slang`. Оракул той самий: числа з
//! обох боків мусять збігтися, і це перевіряє тест.
//!
//! **Аналітика, а не семплювання, і саме тому це оракул.** GGX має замкнену
//! форму, тож двійник дає число без експозиції, тонмапера й будь-яких
//! налаштувань вигляду — отже розбіжність означає помилку, а не інші
//! налаштування. Порівняння з рендером у Blender такої властивості не має
//! (ROADMAP, T5).
//!
//! ## Формулювання — Karis 2013 / Filament, і кожен вибір має ціну
//!
//! - **`α = roughness²`.** Не сам `roughness`: художній параметр мусить бути
//!   рівномірним на око, а не в математиці, і квадрат — та угода, яку розуміють
//!   і Blender, і glTF. Оскільки з Blender параметри й приїдуть (T5d), взяти
//!   іншу означало б тихо перефарбувати кожен імпортований матеріал.
//! - **Smith з кореляцією висот**, і одразу поділений на `4(n·l)(n·v)`. Окремо
//!   G і окремий знаменник дають нуль на нуль на дотичних кутах — той самий
//!   клас, що `max(x, 1e-30)` у повітрі.
//! - **`F0 = 0.04` для діелектрика.** Це не «магічне 4%», а нормальне
//!   відбиття для показника заломлення ~1.5, тобто для фарби, скла й пластику.
//!   Метал бере `F0` з базового кольору й не має дифузного члена взагалі.

/// Нормальне відбиття діелектрика — показник заломлення близько 1.5.
pub const DIELECTRIC_F0: f64 = 0.04;

/// Найменша шорсткість, нижче якої відблиск стає дельта-функцією.
///
/// ⚠ Не косметика: при `α → 0` знаменник `D` прямує до нуля в одній точці, і
/// на `f32` це нескінченність в одному пікселі й нуль у сусідньому. Межа
/// поставлена так, щоб пік `D` лишався в межах `f32` з великим запасом.
pub const MIN_ROUGHNESS: f64 = 0.045;

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalise(v: [f64; 3]) -> [f64; 3] {
    let n = dot(v, v).sqrt();
    [v[0] / n, v[1] / n, v[2] / n]
}

/// Розподіл нормалей мікрограней, GGX / Trowbridge-Reitz.
pub fn distribution(n_dot_h: f64, roughness: f64) -> f64 {
    let a = (roughness.max(MIN_ROUGHNESS)).powi(2);
    let a2 = a * a;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    a2 / (std::f64::consts::PI * d * d)
}

/// Видимість Сміта з кореляцією висот, **уже поділена** на `4(n·l)(n·v)`.
pub fn visibility(n_dot_v: f64, n_dot_l: f64, roughness: f64) -> f64 {
    let a2 = (roughness.max(MIN_ROUGHNESS)).powi(2).powi(2);
    let v = n_dot_l * (n_dot_v * n_dot_v * (1.0 - a2) + a2).sqrt();
    let l = n_dot_v * (n_dot_l * n_dot_l * (1.0 - a2) + a2).sqrt();
    0.5 / (v + l).max(1e-30)
}

/// Френель за Шліком.
pub fn fresnel(f0: f64, v_dot_h: f64) -> f64 {
    f0 + (1.0 - f0) * (1.0 - v_dot_h).clamp(0.0, 1.0).powi(5)
}

/// Скільки світла йде в око з одиниці опромінення — на канал.
///
/// * `normal`, `view`, `light` — одиничні; `view` дивиться **від поверхні до
///   ока**, `light` — від поверхні до світила;
/// * `base` — базовий колір каналу, `0…1`;
/// * `roughness`, `metallic` — `0…1`.
///
/// Повертає вже помножене на `n·l`, тобто те, що йде в піксель при одиничному
/// опроміненні. Нуль, коли поверхня відвернута від світила або від ока.
pub fn radiance(
    normal: [f64; 3],
    view: [f64; 3],
    light: [f64; 3],
    base: f64,
    roughness: f64,
    metallic: f64,
) -> f64 {
    let n = normalise(normal);
    let v = normalise(view);
    let l = normalise(light);

    let n_dot_l = dot(n, l);
    let n_dot_v = dot(n, v);
    if n_dot_l <= 0.0 || n_dot_v <= 0.0 {
        return 0.0;
    }
    let h = normalise([v[0] + l[0], v[1] + l[1], v[2] + l[2]]);
    let n_dot_h = dot(n, h).clamp(0.0, 1.0);
    let v_dot_h = dot(v, h).clamp(0.0, 1.0);

    // Метал не має дифузного відбиття, а його `F0` — це базовий колір.
    let f0 = DIELECTRIC_F0 * (1.0 - metallic) + base * metallic;
    let f = fresnel(f0, v_dot_h);
    let specular = distribution(n_dot_h, roughness) * visibility(n_dot_v, n_dot_l, roughness) * f;
    let diffuse = (1.0 - f) * (1.0 - metallic) * base / std::f64::consts::PI;

    (diffuse + specular) * n_dot_l
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Дзеркальний напрямок — і тільки він — дає пік розподілу.
    #[test]
    fn the_lobe_peaks_where_the_mirror_direction_is() {
        for roughness in [0.05, 0.2, 0.5, 1.0] {
            let peak = distribution(1.0, roughness);
            for n_dot_h in [0.0, 0.3, 0.7, 0.9, 0.99] {
                let got = distribution(n_dot_h, roughness);
                assert!(
                    got <= peak,
                    "шорсткість {roughness}: при n·h = {n_dot_h} розподіл {got} > піку {peak}"
                );
            }
        }
    }

    /// Гладша поверхня дає вужчий і вищий відблиск.
    ///
    /// Це те, що взагалі робить `roughness` параметром: якби пік не залежав
    /// від нього монотонно, повзунок нічого б не означав.
    #[test]
    fn a_smoother_surface_has_a_sharper_highlight() {
        let mut previous = f64::INFINITY;
        for step in 1..20 {
            let roughness = f64::from(step) * 0.05;
            let peak = distribution(1.0, roughness);
            assert!(
                peak < previous,
                "шорсткість {roughness} дала пік {peak}, не менший за {previous}"
            );
            previous = peak;
        }
    }

    /// Розподіл нормований: інтеграл `D · (n·h)` по півсфері дорівнює одиниці.
    ///
    /// Це і є те, що робить GGX **розподілом**, а не просто дзвоном; помилка в
    /// показнику знаменника чи в `α²` ламає рівно це й більше нічого видимого.
    #[test]
    fn the_distribution_integrates_to_one() {
        for roughness in [0.1, 0.3, 0.6, 1.0] {
            // Півсфера в сферичних координатах: `∫ D cosθ sinθ dθ dφ`.
            let steps = 20_000;
            let mut sum = 0.0;
            for k in 0..steps {
                let theta = (f64::from(k) + 0.5) / f64::from(steps) * std::f64::consts::FRAC_PI_2;
                sum += distribution(theta.cos(), roughness)
                    * theta.cos()
                    * theta.sin()
                    * (std::f64::consts::FRAC_PI_2 / f64::from(steps));
            }
            let total = sum * 2.0 * std::f64::consts::PI;
            println!("  шорсткість {roughness}: інтеграл {total:.6}");
            assert!(
                (total - 1.0).abs() < 1e-3,
                "шорсткість {roughness}: інтеграл {total:.6}, а мусить бути одиниця"
            );
        }
    }

    /// Френель на дотичному куті — повне відбиття, на нормалі — `F0`.
    #[test]
    fn fresnel_goes_from_f0_to_one() {
        assert!((fresnel(DIELECTRIC_F0, 1.0) - DIELECTRIC_F0).abs() < 1e-12);
        assert!((fresnel(DIELECTRIC_F0, 0.0) - 1.0).abs() < 1e-12);
        assert!((fresnel(0.9, 0.0) - 1.0).abs() < 1e-12);
    }

    /// Взаємність Гельмгольца: поміняти око зі світилом нічого не змінює.
    ///
    /// Фізичний закон, а не властивість формули, — і саме тому це перевірка:
    /// несиметричний `visibility` (класична помилка в Сміті) ламає її, і
    /// більше нічого.
    #[test]
    fn swapping_the_eye_and_the_light_changes_nothing() {
        let normal = [0.0, 0.0, 1.0];
        let mut worst: f64 = 0.0;
        for k in 0..12 {
            for m in 0..12 {
                let a = f64::from(k) * 0.13 + 0.05;
                let b = f64::from(m) * 0.11 + 0.05;
                let view = normalise([a.sin(), 0.0, a.cos()]);
                let light = normalise([b.sin() * 0.6, b.sin() * 0.8, b.cos()]);
                for roughness in [0.1, 0.4, 0.9] {
                    let here = radiance(normal, view, light, 0.5, roughness, 0.0);
                    let there = radiance(normal, light, view, 0.5, roughness, 0.0);
                    // Множник `n·l` не симетричний, тож ділиться назад.
                    let here = here / dot(normal, light);
                    let there = there / dot(normal, view);
                    worst = worst.max((here - there).abs() / here.max(1e-12));
                }
            }
        }
        println!("  найгірша несиметрія {worst:.3e}");
        assert!(worst < 1e-12, "взаємність порушена на {worst:.3e}");
    }

    /// Метал не має дифузного відбиття, діелектрик має.
    #[test]
    fn metal_reflects_only_its_highlight() {
        let normal = [0.0, 0.0, 1.0];
        // ⚠ Пара напрямків мусить бути **далеко від дзеркальної**, і симетрія
        // тут — пастка: `v = (s, 0, c)` разом з `l = (−s, 0, c)` дає
        // `h = normalize(v + l) = n`, тобто рівно дзеркало й пік відблиску.
        // Перша редакція тесту взяла саме її й міряла метал на його максимумі.
        // Півсфера розводиться полярним кутом, не азимутом.
        let view = normalise([0.174, 0.0, 0.985]);
        let light = normalise([-0.940, 0.0, 0.342]);
        let rough = 0.35;
        let metal = radiance(normal, view, light, 0.9, rough, 1.0);
        let paint = radiance(normal, view, light, 0.9, rough, 0.0);
        println!(
            "  метал {metal:.5}, фарба {paint:.5}, у {:.1} раза",
            paint / metal
        );
        assert!(
            paint > 4.0 * metal,
            "метал {metal:.5} світиться майже як фарба {paint:.5} — дифузний \
             член не прибрано"
        );
    }

    /// Поверхня, відвернута від світила або від ока, не світиться.
    #[test]
    fn a_surface_turned_away_is_black() {
        let n = [0.0, 0.0, 1.0];
        let up = [0.0, 0.0, 1.0];
        let down = [0.0, 0.0, -1.0];
        assert_eq!(radiance(n, up, down, 0.8, 0.3, 0.0), 0.0);
        assert_eq!(radiance(n, down, up, 0.8, 0.3, 0.0), 0.0);
    }

    /// Матеріал не віддає більше, ніж отримав.
    ///
    /// Груба, але справжня межа: інтеграл вихідної яскравості по півсфері
    /// напрямків ока не може перевищити одиницю при одиничному опроміненні.
    /// Порушення тут означає, що поверхня світиться сама.
    ///
    /// ⚠ Знизу межі немає навмисно, і числа це показують: шорсткий метал
    /// віддає лише 0.32 з одиниці. Це **відома** властивість одноразового
    /// розсіяння в GGX — світло, що відбилося між мікрогранями двічі,
    /// формула не повертає взагалі, — а не помилка. Компенсація існує
    /// (Kulla-Conty), коштує ще однієї таблиці й потрібна там, де шорсткий
    /// метал несе вигляд; корпус із `roughness ≈ 0.35` втрачає одиниці
    /// відсотків, тож поки не платимо.
    #[test]
    fn the_material_never_gives_back_more_than_it_got() {
        let normal = [0.0, 0.0, 1.0];
        for roughness in [0.08, 0.3, 0.7, 1.0] {
            for metallic in [0.0, 1.0] {
                let light = normalise([0.3, 0.0, 0.954]);
                let steps = 400;
                let mut total = 0.0;
                for k in 0..steps {
                    let theta =
                        (f64::from(k) + 0.5) / f64::from(steps) * std::f64::consts::FRAC_PI_2;
                    let mut ring = 0.0;
                    let around = 200;
                    for m in 0..around {
                        let phi =
                            (f64::from(m) + 0.5) / f64::from(around) * 2.0 * std::f64::consts::PI;
                        let view = [
                            theta.sin() * phi.cos(),
                            theta.sin() * phi.sin(),
                            theta.cos(),
                        ];
                        // Вихідна яскравість без множника `n·l`, помножена на
                        // `cos` напрямку ока — це і є потік назовні.
                        ring += radiance(normal, view, light, 1.0, roughness, metallic)
                            / dot(normal, light)
                            * theta.cos();
                    }
                    total += ring / f64::from(around)
                        * 2.0
                        * std::f64::consts::PI
                        * theta.sin()
                        * (std::f64::consts::FRAC_PI_2 / f64::from(steps));
                }
                println!("  шорсткість {roughness}, метал {metallic}: віддано {total:.4}");
                assert!(
                    total <= 1.0 + 1e-3,
                    "шорсткість {roughness}, метал {metallic}: віддано {total:.4} \
                     з одиниці — поверхня світиться сама"
                );
            }
        }
    }
}
