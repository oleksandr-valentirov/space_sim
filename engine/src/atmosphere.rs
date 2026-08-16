//! Повітря на CPU: та сама фізика, що в `shaders/sky.slang`, але в `f64` і
//! без GPU (ROADMAP-ATMOSPHERE.md, S2).
//!
//! ## Навіщо друга копія
//!
//! Правило 2 етапу S вимагає від кожного LUT **числа**, а не «схоже на небо».
//! Число мусить прийти звідкись, і взяти його з того самого шейдера не можна:
//! так перевіряється лише те, що GPU вміє додавати. Тому тут лежить дослівний
//! двійник — рівно та сама конструкція, що вже двічі виправдала себе в
//! проєкті: `engine::cull` проти `cull.slang`, `Terrain::height_m` проти
//! `sample_height`.
//!
//! ## На чому стоїть сам двійник
//!
//! Двійник — теж чисельне інтегрування, тож його теж треба чимось пришпилити,
//! інакше дві однакові помилки збіглися б і назвалися перевіркою. Пришпилює
//! його **замкнена форма**: для вертикального променя оптична товща крізь
//! експоненційну атмосферу дорівнює `β·H·(exp(−h₀/H) − exp(−h₁/H))`, а крізь
//! трикутний озоновий шар — інтегралу трапеції, теж у замкненому вигляді.
//! Отже ланцюг такий:
//!
//! 1. замкнена форма ⇄ [`optical_depth`] — юніт-тест, без GPU;
//! 2. [`optical_depth`] ⇄ таблиця на GPU — тест на пристрої, на десятках
//!    висот і кутів (`engine/tests/atmosphere.rs`).
//!
//! Жодна ланка не перевіряє себе сама.
//!
//! ## Геометрія
//!
//! Скрізь однакова пара `(r, mu)`: `r` — відстань від **центра тіла**, `mu` —
//! косинус кута між напрямком променя й зенітом (напрямком від центра). Це та
//! сама параметризація, що в статті, і саме в ній таблиця пропускання має
//! перший стовпець рівно вертикальним — на чому й стоїть замкнена форма.

use crate::scene::Atmosphere;

/// Ширина таблиці пропускання — вісь `mu`.
///
/// Мусить збігатися з `TRANSMITTANCE_WIDTH` у `shaders/sky.slang`; спільної
/// константи між Rust і Slang не існує, тож звіряє їх тест
/// `engine::tests::atmosphere`, який греппить файл шейдера. Той самий прийом,
/// що з `SIDE` у патчів (R6a).
pub const TRANSMITTANCE_WIDTH: u32 = 256;
/// Висота таблиці пропускання — вісь `r`.
pub const TRANSMITTANCE_HEIGHT: u32 = 64;

/// Скільки кроків робить чисельне інтегрування **тут**.
///
/// Учетверо більше, ніж у шейдері (там 500), і різниця навмисна: два
/// обчислення з однаковою сіткою мали б і однакову похибку дискретизації,
/// тобто збігалися б навіть тоді, коли обидва неправильні. Виміряно на
/// найгіршому промені таблиці: 500 кроків дають 3.6·10⁻⁵, 2048 — 2.1·10⁻⁶,
/// тобто оракул точніший за перевіряне на порядок із гаком. Оракул має право
/// коштувати дорого — він біжить у тесті, а не в кадрі.
pub const ORACLE_STEPS: usize = 2048;

/// Густина трьох компонент повітря на висоті `h` метрів над поверхнею:
/// `[Релей, Мі, озон]`, безрозмірна частка приземного значення.
///
/// Релей і Мі — експоненти зі своїми висотами шкали. Озон — трикутник, як у
/// статті: він не спадає з висотою взагалі, а має шар на ~25 км, і саме тому
/// небо в зеніті синє, а не фіолетове.
pub fn density(air: &Atmosphere, h: f64) -> [f64; 3] {
    let rayleigh = (-h / f64::from(air.rayleigh_height_m)).exp();
    let mie = (-h / f64::from(air.mie_height_m)).exp();
    let centre = f64::from(air.ozone_centre_m);
    let width = f64::from(air.ozone_width_m);
    let ozone = (1.0 - (h - centre).abs() / width).max(0.0);
    [rayleigh, mie, ozone]
}

/// Коефіцієнт ослаблення на висоті `h`, 1/м, по RGB.
///
/// Ослаблення — це розсіювання **плюс** поглинання: промінь втрачає фотон і
/// тоді, коли той полетів убік, і тоді, коли його з'їли. Релей не поглинає
/// зовсім, Мі поглинає більше, ніж розсіює, озон лише поглинає.
pub fn extinction(air: &Atmosphere, h: f64) -> [f64; 3] {
    let [d_rayleigh, d_mie, d_ozone] = density(air, h);
    let mie = f64::from(air.mie_scattering) + f64::from(air.mie_absorption);
    let mut out = [0.0; 3];
    for (channel, value) in out.iter_mut().enumerate() {
        *value = f64::from(air.rayleigh_scattering[channel]) * d_rayleigh
            + mie * d_mie
            + f64::from(air.ozone_absorption[channel]) * d_ozone;
    }
    out
}

/// Скільки метрів від точки `(r, mu)` до верхньої межі повітря.
///
/// Корінь квадратного рівняння `|r·zenith + d·dir|² = top²`. Другий корінь
/// від'ємний завжди, коли точка всередині повітря, тож вибору тут немає.
pub fn distance_to_top(r: f64, mu: f64, top: f64) -> f64 {
    let discriminant = r * r * (mu * mu - 1.0) + top * top;
    (-r * mu + discriminant.max(0.0).sqrt()).max(0.0)
}

/// Скільки метрів до поверхні, або `None`, якщо промінь її не зустріне.
///
/// Промінь, спрямований угору (`mu ≥ 0`), поверхні не бачить ніколи — і
/// перевіряти це окремо треба, бо дискримінант там теж буває додатний: то
/// друга, «задня» точка перетину, якої попереду немає.
pub fn distance_to_ground(r: f64, mu: f64, bottom: f64) -> Option<f64> {
    let discriminant = r * r * (mu * mu - 1.0) + bottom * bottom;
    if mu >= 0.0 || discriminant < 0.0 {
        return None;
    }
    Some(-r * mu - discriminant.sqrt())
}

/// Скільки метрів промінь `(r, mu)` іде повітрям: до поверхні або до верхньої
/// межі, залежно від того, що ближче.
///
/// Для променів **таблиці пропускання** цією функцією користуватися не можна,
/// і це не смак — див. [`optical_depth_to_top`].
pub fn span_in_air(air: &Atmosphere, bottom: f64, r: f64, mu: f64) -> f64 {
    let top = distance_to_top(r, mu, air.top_m);
    match distance_to_ground(r, mu, bottom) {
        Some(ground) => ground.min(top),
        None => top,
    }
}

/// Оптична товща на відрізку `span` уздовж променя `(r, mu)`, по RGB.
///
/// Правило середньої точки, `steps` кроків. Не Сімпсон: підінтегральна
/// функція біля горизонту міняється на порядки на довжині кроку, і виграш
/// вищого порядку там уявний, а ціна — реальна.
pub fn optical_depth(
    air: &Atmosphere,
    bottom: f64,
    r: f64,
    mu: f64,
    span: f64,
    steps: usize,
) -> [f64; 3] {
    let step = span / steps as f64;

    let mut out = [0.0; 3];
    for k in 0..steps {
        let d = (k as f64 + 0.5) * step;
        // Висота середини кроку: теорема косинусів у трикутнику
        // центр-точка-середина.
        let h = (r * r + d * d + 2.0 * r * d * mu).max(0.0).sqrt() - bottom;
        let e = extinction(air, h);
        for (value, add) in out.iter_mut().zip(e.iter()) {
            *value += add * step;
        }
    }
    out
}

/// Оптична товща від `(r, mu)` **до верхньої межі**, без огляду на поверхню.
///
/// Саме це лежить у таблиці пропускання, і «без огляду на поверхню» тут не
/// спрощення, а вимога. Параметризація таблиці накриває рівно ті напрямки, які
/// верхньої межі досягають; крайній стовпець — промінь, дотичний до поверхні.
/// Питати такий промінь, чи він зустріне поверхню, не можна взагалі: відповідь
/// стоїть на різниці `r²−bottom²`, яка в дотику дорівнює нулю, і знак їй дає
/// похибка округлення. Одного разу це вже коштувало вдесятеро коротшого шляху
/// (виявлено на S2, у `f32` на GPU — там та сама різниця дає ±1.5 км).
pub fn optical_depth_to_top(
    air: &Atmosphere,
    bottom: f64,
    r: f64,
    mu: f64,
    steps: usize,
) -> [f64; 3] {
    let span = distance_to_top(r, mu, air.top_m);
    optical_depth(air, bottom, r, mu, span, steps)
}

/// Пропускання від `(r, mu)` до верхньої межі — `exp(−оптична товща)`.
pub fn transmittance(air: &Atmosphere, bottom: f64, r: f64, mu: f64, steps: usize) -> [f64; 3] {
    let depth = optical_depth_to_top(air, bottom, r, mu, steps);
    [(-depth[0]).exp(), (-depth[1]).exp(), (-depth[2]).exp()]
}

/// Оптична товща **вертикального** променя вгору — у замкненій формі.
///
/// Те, чим пришпилюється [`optical_depth`]. Для експоненційного шару це
/// `β·H·(exp(−h₀/H) − exp(−h₁/H))`, для трикутного озонового — різниця
/// первісних трикутника, теж елементарна.
///
/// Тільки вгору й тільки вертикально: сферична геометрія під кутом замкненої
/// форми не має взагалі (це функція Чепмена, а вона не елементарна). Одного
/// напрямку досить — таблиця пропускання влаштована так, що її перший
/// стовпець і є цей промінь, тож замкнена форма накриває всі 64 висоти.
pub fn vertical_optical_depth(air: &Atmosphere, bottom: f64, r: f64) -> [f64; 3] {
    let h0 = r - bottom;
    let h1 = air.top_m - bottom;

    let exponential = |scale: f64| scale * ((-h0 / scale).exp() - (-h1 / scale).exp());
    let rayleigh = exponential(f64::from(air.rayleigh_height_m));
    let mie = exponential(f64::from(air.mie_height_m));
    let ozone = triangle_integral(
        f64::from(air.ozone_centre_m),
        f64::from(air.ozone_width_m),
        h1,
    ) - triangle_integral(
        f64::from(air.ozone_centre_m),
        f64::from(air.ozone_width_m),
        h0,
    );

    let mie_extinction = f64::from(air.mie_scattering) + f64::from(air.mie_absorption);
    let mut out = [0.0; 3];
    for (channel, value) in out.iter_mut().enumerate() {
        *value = f64::from(air.rayleigh_scattering[channel]) * rayleigh
            + mie_extinction * mie
            + f64::from(air.ozone_absorption[channel]) * ozone;
    }
    out
}

/// Первісна трикутного профілю озону: `∫₀^h max(0, 1 − |z − centre| / width) dz`.
fn triangle_integral(centre: f64, width: f64, h: f64) -> f64 {
    if h <= centre - width {
        0.0
    } else if h <= centre {
        let t = h - (centre - width);
        t * t / (2.0 * width)
    } else if h <= centre + width {
        let s = h - centre;
        width / 2.0 + s - s * s / (2.0 * width)
    } else {
        width
    }
}

/// Точка `(r, mu)` за координатами текселя таблиці пропускання.
///
/// Параметризація зі статті, і в ній важлива рівно одна властивість, на якій
/// стоїть уся перевірка кроку: при `u = 0` виходить `mu = 1`, тобто промінь
/// строго вгору. Це не збіг — `u` міряє довжину променя від найкоротшої
/// (вертикальної, `top − r`) до найдовшої (дотичної до поверхні), а найкоротша
/// і є вертикаль.
pub fn uv_to_r_mu(air: &Atmosphere, bottom: f64, u: f64, v: f64) -> (f64, f64) {
    let top = air.top_m;
    // Довжина дотичної до поверхні з верхньої межі — природна одиниця
    // «горизонтального» розміру атмосфери.
    let h = (top * top - bottom * bottom).sqrt();
    let rho = h * v;
    let r = (rho * rho + bottom * bottom).sqrt();

    let d_min = top - r;
    let d_max = rho + h;
    let d = d_min + u * (d_max - d_min);
    let mu = if d == 0.0 {
        1.0
    } else {
        ((h * h - rho * rho - d * d) / (2.0 * r * d)).clamp(-1.0, 1.0)
    };
    (r, mu)
}

/// Координата текстури, у якій лежить одиничне значення `u`.
///
/// Кінці одиничного діапазону сідають у **центри крайніх текселів**, а не на
/// краї текстури. Без цього `u = 0` (вертикаль) не лежало б у таблиці зовсім, а
/// білінійна вибірка на кінцях зазирала б за край. Прийом стандартний
/// (Bruneton), і саме він робить перший стовпець таблиці перевірюваним
/// замкненою формою.
pub fn unit_to_texture(u: f64, size: u32) -> f64 {
    let n = f64::from(size);
    0.5 / n + u * (1.0 - 1.0 / n)
}

/// Зворотне до [`uv_to_r_mu`]. Потрібне тому, хто читає таблицю, а не тому,
/// хто її пише.
pub fn r_mu_to_uv(air: &Atmosphere, bottom: f64, r: f64, mu: f64) -> (f64, f64) {
    let top = air.top_m;
    let h = (top * top - bottom * bottom).sqrt();
    let rho = (r * r - bottom * bottom).max(0.0).sqrt();
    let d = distance_to_top(r, mu, top);
    let d_min = top - r;
    let d_max = rho + h;
    let u = if d_max > d_min {
        ((d - d_min) / (d_max - d_min)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (u, (rho / h).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTTOM: f64 = 6_371_000.0;

    fn air() -> Atmosphere {
        Atmosphere::EARTH
    }

    /// Ланка 1 ланцюга: чисельне інтегрування збігається із замкненою формою.
    ///
    /// Вертикальний промінь — єдиний напрямок, у якому замкнена форма існує,
    /// і саме тому таблиця влаштована так, щоб він у ній був. Перевіряється на
    /// всіх висотах шару, а не в одній точці: помилка в густині озону видна
    /// лише там, де озон є.
    #[test]
    fn the_numeric_integral_matches_the_closed_form_going_straight_up() {
        let air = air();
        let thickness = air.top_m - BOTTOM;
        let mut worst: f64 = 0.0;
        for k in 0..=20 {
            let r = BOTTOM + thickness * f64::from(k) / 20.0;
            let numeric = optical_depth_to_top(&air, BOTTOM, r, 1.0, ORACLE_STEPS);
            let closed = vertical_optical_depth(&air, BOTTOM, r);
            for channel in 0..3 {
                // Відносна похибка: біля верхньої межі обидва числа малі, і
                // абсолютна нічого не сказала б.
                let scale = closed[channel].abs().max(1.0e-12);
                worst = worst.max((numeric[channel] - closed[channel]).abs() / scale);
            }
        }
        assert!(
            worst < 1.0e-3,
            "найгірша відносна розбіжність {worst}, а мала б бути кроком інтегрування"
        );
    }

    /// Та сама перевірка, але для точки НАД шаром озону.
    ///
    /// Окремо, бо тут первісна трикутника входить обома гілками — і саме на
    /// стику гілок помилка в ній була б непомітна в тесті вище.
    #[test]
    fn the_ozone_layer_integrates_through_its_own_peak() {
        let air = air();
        // 25 км — рівно центр шару, тобто злам профілю.
        let numeric = optical_depth_to_top(&air, BOTTOM, BOTTOM + 25_000.0, 1.0, ORACLE_STEPS);
        let closed = vertical_optical_depth(&air, BOTTOM, BOTTOM + 25_000.0);
        for channel in 0..3 {
            let relative = (numeric[channel] - closed[channel]).abs() / closed[channel];
            assert!(relative < 1.0e-3, "канал {channel}: розбіжність {relative}");
        }
    }

    /// Перший стовпець таблиці — вертикаль. На цьому стоїть увесь оракул S2.
    #[test]
    fn the_first_column_of_the_table_looks_straight_up() {
        let air = air();
        for k in 0..=8 {
            let v = f64::from(k) / 8.0;
            let (r, mu) = uv_to_r_mu(&air, BOTTOM, 0.0, v);
            assert!((mu - 1.0).abs() < 1.0e-9, "v = {v}: mu = {mu}");
            assert!(r >= BOTTOM - 1.0 && r <= air.top_m + 1.0, "r = {r}");
        }
    }

    /// Останній стовпець — дотична до поверхні: промінь, що ковзає горизонтом.
    #[test]
    fn the_last_column_grazes_the_ground() {
        let air = air();
        for k in 1..=8 {
            let v = f64::from(k) / 8.0;
            let (r, mu) = uv_to_r_mu(&air, BOTTOM, 1.0, v);
            // Дотична з висоти r має `mu = −√(r² − bottom²)/r`.
            let expected = -(r * r - BOTTOM * BOTTOM).sqrt() / r;
            assert!(
                (mu - expected).abs() < 1.0e-9,
                "v = {v}: {mu} проти {expected}"
            );
        }
    }

    /// Пряме й зворотне перетворення дають ту саму точку.
    ///
    /// Це не тавтологія: зворотне рахується іншою формулою (через
    /// [`distance_to_top`]), і саме воно читатиме таблицю в кадрі.
    #[test]
    fn the_parametrisation_survives_a_round_trip() {
        let air = air();
        let mut worst: f64 = 0.0;
        for i in 0..16 {
            for j in 0..16 {
                let u = (f64::from(i) + 0.5) / 16.0;
                let v = (f64::from(j) + 0.5) / 16.0;
                let (r, mu) = uv_to_r_mu(&air, BOTTOM, u, v);
                let (u2, v2) = r_mu_to_uv(&air, BOTTOM, r, mu);
                worst = worst.max((u - u2).abs()).max((v - v2).abs());
            }
        }
        assert!(worst < 1.0e-6, "найгірше розходження {worst}");
    }

    /// Промінь угору поверхні не бачить, промінь униз — бачить.
    #[test]
    fn only_a_ray_pointing_down_can_meet_the_ground() {
        let r = BOTTOM + 50_000.0;
        assert!(distance_to_ground(r, 0.5, BOTTOM).is_none());
        assert!(distance_to_ground(r, 0.0, BOTTOM).is_none());
        // Строго вниз: до поверхні рівно висота.
        let down = distance_to_ground(r, -1.0, BOTTOM).expect("вниз поверхня є");
        assert!((down - 50_000.0).abs() < 1.0e-6, "{down}");
        // Ковзний промінь трохи нижче дотичної поверхні таки досягає.
        let grazing = -(r * r - BOTTOM * BOTTOM).sqrt() / r;
        assert!(distance_to_ground(r, grazing - 1.0e-6, BOTTOM).is_some());
    }
}
