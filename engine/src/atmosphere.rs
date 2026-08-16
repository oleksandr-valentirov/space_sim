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
    // Під поверхнею повітря немає. Затискання тут не косметика: промінь, що
    // пірнув під землю, дав би `exp(+796)`, тобто нескінченність, а вона на
    // нульовому кроці дає NaN. Спіймано на S3, у шейдері.
    let h = h.max(0.0);
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

/// `r² − bottom²` — квадрат відстані від точки до дотику з поверхнею.
///
/// **Це число, а не радіус, є природною змінною всієї геометрії тут**, і на цьому
/// етап S спіткнувся двічі. Обидві відстані — до поверхні й до верхньої межі —
/// виражаються через нього без жодного віднімання великих чисел, а сам радіус у
/// них входить лише множником. Хто знає його точніше (параметризація таблиці,
/// висота над поверхнею), той і мусить його передати: у `f32` при `r ≈ 6.4·10⁶`
/// різниця квадратів має одиницю останнього розряду 4·10⁶, тобто біля дотику
/// їй дає знак округлення.
pub fn rho_squared(r: f64, bottom: f64) -> f64 {
    (r * r - bottom * bottom).max(0.0)
}

/// Те саме для верхньої межі: `top² − bottom²`, у вигляді добутку.
///
/// Добутком, а не різницею квадратів: `(top − bottom)·(top + bottom)` — це сто
/// кілометрів на тринадцять тисяч, тобто жодного скорочення.
pub fn shell_squared(air: &Atmosphere, bottom: f64) -> f64 {
    (air.top_m - bottom) * (air.top_m + bottom)
}

/// Скільки метрів від точки `(r, mu)` до верхньої межі повітря.
///
/// `rho2` — [`rho_squared`] цієї точки, `shell2` — [`shell_squared`] атмосфери.
/// Корінь квадратного рівняння `|r·zenith + d·dir|² = top²`, переписаний через
/// них: `d = −r·mu + √(r²mu² + shell2 − rho2)`. Другий корінь від'ємний завжди,
/// коли точка всередині повітря, тож вибору тут немає.
pub fn distance_to_top(r: f64, mu: f64, rho2: f64, shell2: f64) -> f64 {
    let discriminant = r * r * mu * mu + (shell2 - rho2);
    (-r * mu + discriminant.max(0.0).sqrt()).max(0.0)
}

/// Скільки метрів до поверхні, або `None`, якщо промінь її не зустріне.
///
/// Промінь, спрямований угору (`mu ≥ 0`), поверхні не бачить ніколи — і
/// перевіряти це окремо треба, бо дискримінант там теж буває додатний: то
/// друга, «задня» точка перетину, якої попереду немає.
pub fn distance_to_ground(r: f64, mu: f64, rho2: f64) -> Option<f64> {
    let discriminant = r * r * mu * mu - rho2;
    if mu >= 0.0 || discriminant < 0.0 {
        return None;
    }
    // `max(0)` — не косметика. Промінь, що йде вниз із самої поверхні, має
    // `rho² = 0`, і різниця `−r·mu − √(r²mu²)` у `f32` виходить то трохи
    // додатною, то трохи від'ємною. Від'ємну викликач читає як «поверхні
    // попереду немає» й веде промінь крізь планету. Спіймано на S3: рядок 0
    // таблиці розсіювання виходив утричі яскравішим за двійник.
    Some((-r * mu - discriminant.sqrt()).max(0.0))
}

/// Скільки метрів промінь `(r, mu)` іде повітрям: до поверхні або до верхньої
/// межі, залежно від того, що ближче.
///
/// Для променів **таблиці пропускання** цією функцією користуватися не можна,
/// і це не смак — див. [`optical_depth_to_top`].
pub fn span_in_air(air: &Atmosphere, bottom: f64, r: f64, mu: f64) -> f64 {
    let rho2 = rho_squared(r, bottom);
    let top = distance_to_top(r, mu, rho2, shell_squared(air, bottom));
    match distance_to_ground(r, mu, rho2) {
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
    let span = distance_to_top(r, mu, rho_squared(r, bottom), shell_squared(air, bottom));
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
    let rho2 = rho_squared(r, bottom);
    let rho = rho2.sqrt();
    let d = distance_to_top(r, mu, rho2, h * h);
    let d_min = top - r;
    let d_max = rho + h;
    let u = if d_max > d_min {
        ((d - d_min) / (d_max - d_min)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (u, (rho / h).clamp(0.0, 1.0))
}

/// Сторона таблиці багаторазового розсіювання (S3).
///
/// Мусить збігатися з `MULTISCATTER_SIZE` у `shaders/sky.slang`.
pub const MULTISCATTER_SIZE: u32 = 32;

/// Скільки напрямків обходить інтегрування по сфері — 8×8.
///
/// Сітка, а не випадкові напрямки: таблиця, яку не можна відтворити, не може
/// бути й оракулом. Той самий набір будує CPU-двійник, і саме тому їх узагалі
/// можна покласти поруч.
pub const MULTISCATTER_DIRECTIONS: u32 = 64;

/// Скільки кроків робить промінь усередині одного напрямку.
pub const MULTISCATTER_STEPS: u32 = 20;

/// Точка `(r, mu_s)` за одиничними координатами таблиці розсіювання.
///
/// Параметризація тут проста — лінійна по обох осях, — і це не лінощі: у
/// таблиці пропускання нелінійність існувала заради дотичного променя, тобто
/// заради того, щоб різкий край горизонту не розмазався по текселю. Тут
/// різкого краю немає взагалі: багаторазове розсіювання — це те, що лишилося
/// після усереднення по всій сфері напрямків.
pub fn multiscatter_uv(air: &Atmosphere, bottom: f64, u: f64, v: f64) -> (f64, f64) {
    let mu_s = (u * 2.0 - 1.0).clamp(-1.0, 1.0);
    let r = bottom + v * (air.top_m - bottom);
    (r, mu_s)
}

/// Таблиця пропускання в пам'яті — дзеркало тієї, що лежить на GPU.
///
/// Потрібна тому, що двійник багаторазового розсіювання (S3) читає пропускання
/// **мільйон разів**, і рахувати його щоразу інтегруванням означало б тест,
/// який ніхто не запустить. Шейдер робить рівно те саме — читає таблицю
/// білінійно, — тож двійник тут ще й ближчий до нього, а не далі.
pub struct Table {
    pub width: u32,
    pub height: u32,
    values: Vec<[f64; 3]>,
}

impl Table {
    /// Побудувати таблицю пропускання так само, як це робить `sky.slang`.
    pub fn transmittance(air: &Atmosphere, bottom: f64, steps: usize) -> Table {
        let width = TRANSMITTANCE_WIDTH;
        let height = TRANSMITTANCE_HEIGHT;
        let mut values = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                let u = f64::from(x) / f64::from(width - 1);
                let v = f64::from(y) / f64::from(height - 1);
                let (r, mu) = uv_to_r_mu(air, bottom, u, v);
                values.push(super::atmosphere::transmittance(air, bottom, r, mu, steps));
            }
        }
        Table {
            width,
            height,
            values,
        }
    }

    /// Побудувати таблицю багаторазового розсіювання — двійник
    /// `multiscatter_main` (S3).
    ///
    /// Тільки `ψ`; `f` тут не зберігається, бо його читає лише перевірка
    /// збіжності, а вона дивиться в таблицю на GPU.
    pub fn multiscatter(air: &Atmosphere, bottom: f64, transmittance: &Table) -> Table {
        let size = MULTISCATTER_SIZE;
        let mut values = Vec::with_capacity((size * size) as usize);
        for y in 0..size {
            for x in 0..size {
                let u = f64::from(x) / f64::from(size - 1);
                let v = f64::from(y) / f64::from(size - 1);
                let (r, mu_s) = multiscatter_uv(air, bottom, u, v);
                let (psi, _) = multiple_scattering(air, bottom, transmittance, r, mu_s);
                values.push(psi);
            }
        }
        Table {
            width: size,
            height: size,
            values,
        }
    }

    /// Значення за **одиничними** координатами — білінійно, як `SampleLevel` у
    /// шейдері.
    ///
    /// Одиничними, а не текстурними: параметризація — справа викликача, і саме
    /// тому одна таблиця обслуговує три різні (S2, S3, S4).
    pub fn sample_unit(&self, u: f64, v: f64) -> [f64; 3] {
        // З одиничного діапазону в координату текстури, звідти в індекс
        // текселя. `− 0.5`, бо тексель `k` живе в координаті `(k + 0.5)/розмір`.
        let x = unit_to_texture(u, self.width) * f64::from(self.width) - 0.5;
        let y = unit_to_texture(v, self.height) * f64::from(self.height) - 0.5;
        self.bilinear(x, y)
    }

    /// Пропускання від `(r, mu)` до верхньої межі.
    pub fn transmittance_at(&self, air: &Atmosphere, bottom: f64, r: f64, mu: f64) -> [f64; 3] {
        let (u, v) = r_mu_to_uv(air, bottom, r, mu);
        self.sample_unit(u, v)
    }

    /// Багаторазове розсіювання в точці `(r, mu_s)`.
    pub fn multiscatter_at(&self, air: &Atmosphere, bottom: f64, r: f64, mu_s: f64) -> [f64; 3] {
        let u = (mu_s * 0.5 + 0.5).clamp(0.0, 1.0);
        let v = ((r - bottom) / (air.top_m - bottom)).clamp(0.0, 1.0);
        self.sample_unit(u, v)
    }

    fn bilinear(&self, x: f64, y: f64) -> [f64; 3] {
        let clamp_index = |value: f64, size: u32| -> (usize, usize, f64) {
            let floor = value.floor();
            let t = value - floor;
            let lo = (floor as i64).clamp(0, i64::from(size) - 1) as usize;
            let hi = (floor as i64 + 1).clamp(0, i64::from(size) - 1) as usize;
            (lo, hi, t.clamp(0.0, 1.0))
        };
        let (x0, x1, tx) = clamp_index(x, self.width);
        let (y0, y1, ty) = clamp_index(y, self.height);

        let at = |x: usize, y: usize| self.values[y * self.width as usize + x];
        let mut out = [0.0; 3];
        for (channel, value) in out.iter_mut().enumerate() {
            let top = at(x0, y0)[channel] * (1.0 - tx) + at(x1, y0)[channel] * tx;
            let bottom = at(x0, y1)[channel] * (1.0 - tx) + at(x1, y1)[channel] * tx;
            *value = top * (1.0 - ty) + bottom * ty;
        }
        out
    }
}

/// Напрямок номер `k` з рівномірної сітки 8×8 на сфері.
///
/// Виписано так само, як у шейдері, включно з порядком: `k / 8` іде по азимуту,
/// `k % 8` — по полярному куту. Порядок сам по собі на результат не впливає
/// (сума комутативна), але двійник, який обходить сферу іншою сіткою, вже не
/// двійник.
pub fn sphere_direction(k: u32) -> [f64; 3] {
    let i = 0.5 + f64::from(k / 8);
    let j = 0.5 + f64::from(k % 8);
    let theta = 2.0 * std::f64::consts::PI * i / 8.0;
    let phi = (1.0 - 2.0 * j / 8.0).acos();
    [phi.sin() * theta.cos(), phi.sin() * theta.sin(), phi.cos()]
}

/// Розсіювання (без поглинання) на висоті `h`, 1/м по RGB.
///
/// Відрізняється від [`extinction`] рівно тим, що не рахує з'їденого: озон не
/// розсіює зовсім, Мі розсіює менше, ніж гасить. Саме це число стоїть у
/// джерелі розсіювання — фотон, якого поглинули, у небо не летить.
pub fn scattering(air: &Atmosphere, h: f64) -> [f64; 3] {
    let [d_rayleigh, d_mie, _] = density(air, h);
    let mie = f64::from(air.mie_scattering) * d_mie;
    let mut out = [0.0; 3];
    for (channel, value) in out.iter_mut().enumerate() {
        *value = f64::from(air.rayleigh_scattering[channel]) * d_rayleigh + mie;
    }
    out
}

/// Багаторазове розсіювання в точці `(r, mu_s)` — двійник `multiscatter_main`.
///
/// Повертає пару: `ψ` (внесок другого й наступних порядків, усереднений по
/// сфері напрямків, на одиницю освітленості Сонця) і `f` — частку, яку одне
/// розсіювання повертає назад у ту саму точку.
///
/// ## Звідки береться `ψ = L₂ / (1 − f)`
///
/// Означення тут самоузгоджене, і його варто прочитати перед правкою.
///
/// `ψ` — **середня по сфері яскравість** у точці від порядків 2 і вище. Тоді
/// джерело ізотропного розсіювання в точці дорівнює `σ_s · ψ`, бо
/// `∫ L·(1/4π) dω` і є середнє.
///
/// `f` рахується так: покласти, що середовище світиться рівномірно з
/// яскравістю 1, і подивитися, яка середня яскравість повернеться в точку
/// після **одного** розсіювання. Це лінійний оператор, тож наступний порядок
/// дає `f` від попереднього, а сума — геометрична прогресія. Звідси й
/// `1 / (1 − f)`, і вимога `f < 1`, яку тест і перевіряє: якщо вона не
/// виконується, ряд не збігається, а «енергія росте» перестає бути метафорою.
///
/// ## Чого тут свідомо немає
///
/// **Відбиття від поверхні.** У статті воно є, і воно справжнє: над снігом
/// небо світліше. Але кольору поверхні в [`crate::scene`] немає взагалі, а
/// вигаданий дав би небу відтінок, якого в грі нема звідки взяти. Отже
/// альбедо нуль — і це рішення, а не пропуск.
pub fn multiple_scattering(
    air: &Atmosphere,
    bottom: f64,
    table: &Table,
    r: f64,
    mu_s: f64,
) -> ([f64; 3], [f64; 3]) {
    // Сонце в площині xz, точка на осі z: `up = (0, 0, 1)`.
    let sun = [(1.0 - mu_s * mu_s).max(0.0).sqrt(), 0.0, mu_s];

    let shell2 = shell_squared(air, bottom);
    // `rho²` точки, з якої все починається. **Через висоту, а не через різницю
    // квадратів радіусів**: на рівні поверхні друге дорівнює нулю, і в `f32`
    // йому дає знак округлення — промінь униз тоді не зупиняється на землі, а
    // йде крізь планету. Спіймано на S3: рядок 0 таблиці розходився з двійником
    // удвічі, решта — на 0.05%.
    let altitude = r - bottom;
    let rho2 = altitude * (2.0 * bottom + altitude);

    let mut second = [0.0; 3];
    let mut fraction = [0.0; 3];

    for k in 0..MULTISCATTER_DIRECTIONS {
        let w = sphere_direction(k);
        let mu = w[2];
        let mut span = distance_to_top(r, mu, rho2, shell2);
        if let Some(ground) = distance_to_ground(r, mu, rho2) {
            span = span.min(ground);
        }
        let step = span / f64::from(MULTISCATTER_STEPS);

        let mut throughput = [1.0; 3];
        for s in 0..MULTISCATTER_STEPS {
            let t = (f64::from(s) + 0.5) * step;
            // Точка семпла: `p + t·w` при `p = (0, 0, r)`.
            let point = [t * w[0], t * w[1], r + t * w[2]];
            // `rho²` семпла — теж без різниці квадратів:
            // `|p + t·w|² − bottom² = rho² + 2·t·r·mu + t²`.
            let rho2_here = (rho2 + 2.0 * t * r * mu + t * t).max(0.0);
            let radius = (rho2_here + bottom * bottom).max(0.0).sqrt();
            // Висота — з того самого `rho²`: `rho² = (radius − bottom)(radius + bottom)`,
            // а сума великих чисел скорочення не має.
            let h = rho2_here / (radius + bottom);
            let mu_s_here =
                (point[0] * sun[0] + point[1] * sun[1] + point[2] * sun[2]) / radius.max(1.0);

            // Тінь планети: Сонце під горизонтом цієї точки — світла немає
            // взагалі, і саме звідси береться нічний бік.
            let lit = distance_to_ground(radius, mu_s_here, rho2_here).is_none();
            let to_sun = if lit {
                table.transmittance_at(air, bottom, radius, mu_s_here)
            } else {
                [0.0; 3]
            };

            let sigma_s = scattering(air, h);
            let sigma_e = extinction(air, h);

            for channel in 0..3 {
                // Порожнє повітря: і джерело, і ослаблення нулі, тобто внесок
                // нульовий. Ділити тут не можна — 0/0.
                if sigma_e[channel] <= 0.0 {
                    continue;
                }
                let step_transmittance = (-sigma_e[channel] * step).exp();
                // Точний інтеграл джерела на кроці, а не «значення в середині
                // × довжину»: на верхніх кроках промінь гасне в межах одного
                // кроку, і різниця там не косметична.
                let integrate =
                    |source: f64| source * (1.0 - step_transmittance) / sigma_e[channel];

                // Друге розсіювання: у точку з напрямку `w` приходить те, що
                // розсіялося з прямого сонячного світла. Фазова функція
                // рівномірна — це і є наближення статті.
                let uniform_phase = 1.0 / (4.0 * std::f64::consts::PI);
                second[channel] += throughput[channel]
                    * integrate(sigma_s[channel] * to_sun[channel] * uniform_phase);
                // Частка: те саме, але середовище світиться одиницею з усіх
                // боків, тож джерело — просто `σ_s` (інтеграл рівномірної
                // яскравості по сфері з рівномірною фазою дає одиницю).
                fraction[channel] += throughput[channel] * integrate(sigma_s[channel]);

                throughput[channel] *= step_transmittance;
            }
        }
    }

    // Середнє по сфері: `(4π/N)` тілесного кута на напрямок, поділене на `4π`
    // самого усереднення. Лишається `1/N`.
    let mut psi = [0.0; 3];
    for channel in 0..3 {
        second[channel] /= f64::from(MULTISCATTER_DIRECTIONS);
        fraction[channel] /= f64::from(MULTISCATTER_DIRECTIONS);
        psi[channel] = second[channel] / (1.0 - fraction[channel]).max(1.0e-6);
    }
    (psi, fraction)
}

/// Ширина таблиці неба — азимут відносно Сонця (S4).
pub const SKYVIEW_WIDTH: u32 = 192;
/// Висота таблиці неба — зенітний кут погляду.
pub const SKYVIEW_HEIGHT: u32 = 108;
/// Скільки кроків робить промінь таблиці неба.
pub const SKYVIEW_STEPS: u32 = 32;

/// Напрямок погляду за одиничними координатами таблиці неба.
///
/// Повертає `(mu_v, cos_azimuth)`: косинус зенітного кута погляду й косинус
/// азимутального кута між поглядом і Сонцем.
///
/// ## Чому обидві осі нелінійні
///
/// **По зеніту** — бо горизонт різкий, а решта неба ні. Половина висоти
/// таблиці витрачається на півсферу над горизонтом, половина на ту, що під
/// ним, і всередині кожної половини крок згущується саме до горизонту
/// (квадратний корінь). Лінійна шкала розмазала б смугу заходу по одному
/// текселю з ста восьми.
///
/// **По азимуту** — бо Сонце мале, а фазова функція Мі гостра: більшість
/// зміни кольору відбувається в кількох градусах від світила. Квадрат
/// стискає далекий від Сонця бік і розтягує ближній.
///
/// Границя півсфер — не екватор, а **горизонт цієї висоти**: з десяти
/// кілометрів він нижчий за геометричну горизонталь, і таблиця, побудована на
/// екваторі, мала б розрив у видимому місці.
pub fn skyview_uv(bottom: f64, r: f64, u: f64, v: f64) -> (f64, f64) {
    let rho2 = rho_squared(r, bottom);
    // Кут від надира до горизонту; зенітний кут горизонту — `π − beta`.
    let beta = (rho2.sqrt() / r).clamp(-1.0, 1.0).acos();
    let zenith_horizon = std::f64::consts::PI - beta;

    let zenith = if v < 0.5 {
        let c = 1.0 - 2.0 * v;
        zenith_horizon * (1.0 - c * c)
    } else {
        let c = 2.0 * v - 1.0;
        zenith_horizon + beta * c * c
    };
    // `1 − 2u²` — обернене до `u = √((1 − cos)/2)`. При `u = 0` погляд у бік
    // Сонця, при `u = 1` — від нього.
    (zenith.cos(), 1.0 - 2.0 * u * u)
}

/// Обернене до [`skyview_uv`]: координати таблиці за напрямком погляду.
pub fn skyview_coords(bottom: f64, r: f64, mu_v: f64, cos_azimuth: f64) -> (f64, f64) {
    let rho2 = rho_squared(r, bottom);
    let beta = (rho2.sqrt() / r).clamp(-1.0, 1.0).acos();
    let zenith_horizon = std::f64::consts::PI - beta;
    let zenith = mu_v.clamp(-1.0, 1.0).acos();

    let v = if zenith <= zenith_horizon {
        let c = if zenith_horizon > 0.0 {
            1.0 - (1.0 - zenith / zenith_horizon).max(0.0).sqrt()
        } else {
            0.0
        };
        c * 0.5
    } else {
        let c = if beta > 0.0 {
            ((zenith - zenith_horizon) / beta).clamp(0.0, 1.0).sqrt()
        } else {
            0.0
        };
        0.5 + c * 0.5
    };
    let u = ((1.0 - cos_azimuth) * 0.5).max(0.0).sqrt();
    (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0))
}

/// Фазова функція Релея: `3/(16π)·(1 + cos²θ)`.
///
/// Симетрична вперед-назад, і саме тому небо світле й позаду спостерігача, а
/// не лише навколо Сонця.
pub fn rayleigh_phase(cos_theta: f64) -> f64 {
    3.0 / (16.0 * std::f64::consts::PI) * (1.0 + cos_theta * cos_theta)
}

/// Фазова функція Мі — Хеньї-Ґрінстайна з параметром `g`.
///
/// Гостро вперед: `g = 0.8` означає, що аерозоль розсіює переважно в бік
/// продовження променя. Звідси й ореол навколо Сонця, і те, що серпанок видно
/// проти світла, а не за ним.
pub fn mie_phase(cos_theta: f64, g: f64) -> f64 {
    let denominator = 1.0 + g * g - 2.0 * g * cos_theta;
    (1.0 - g * g) / (4.0 * std::f64::consts::PI * denominator.max(1.0e-6).powf(1.5))
}

/// Повітря разом з обома сталими таблицями — усе, чим рахується небо.
///
/// Структура, а не чотири окремі аргументи: без неї [`Model::sky_view`] брав би
/// вісім, і жоден із них не можна було б переплутати лише на око. Полів рівно
/// стільки, скільки читається (CLAUDE.md), і власних, без лайфтаймів — таблиця
/// коштує пів мегабайта, а будується раз на тест.
pub struct Model {
    pub air: Atmosphere,
    pub bottom: f64,
    pub transmittance: Table,
    pub multiscatter: Table,
}

impl Model {
    /// Побудувати обидві сталі таблиці. `steps` — скільки кроків на промінь у
    /// таблиці пропускання; 500 дає те саме, що шейдер.
    pub fn build(air: &Atmosphere, bottom: f64, steps: usize) -> Model {
        let transmittance = Table::transmittance(air, bottom, steps);
        let multiscatter = Table::multiscatter(air, bottom, &transmittance);
        Model {
            air: *air,
            bottom,
            transmittance,
            multiscatter,
        }
    }

    /// Розсіяне світло вздовж променя — двійник `skyview_main` (S4).
    ///
    /// Система координат локальна: `up = (0, 0, 1)`, Сонце в площині `xz`.
    /// Погляд задається парою `(mu_v, cos_azimuth)`, тобто рівно тим, що лежить
    /// в осях таблиці, — і це не втрата: фізика залежить лише від
    /// `dot(погляд, Сонце)` і зенітних кутів обох, а знак азимута в них не
    /// входить.
    ///
    /// Промінь зупиняється на поверхні й **нічого до неї не додає**: поверхню
    /// малює кадр, а не таблиця неба. Те, що видно крізь повітря, — це вже
    /// аеральна перспектива, і вона окремий крок (S5).
    pub fn sky_view(&self, r: f64, mu_s: f64, mu_v: f64, cos_azimuth: f64) -> [f64; 3] {
        let air = &self.air;
        let bottom = self.bottom;
        let transmittance = &self.transmittance;
        let multiscatter = &self.multiscatter;
        let sun = [(1.0 - mu_s * mu_s).max(0.0).sqrt(), 0.0, mu_s];
        let sin_v = (1.0 - mu_v * mu_v).max(0.0).sqrt();
        let sin_azimuth = (1.0 - cos_azimuth * cos_azimuth).max(0.0).sqrt();
        let w = [sin_v * cos_azimuth, sin_v * sin_azimuth, mu_v];

        // Кут розсіювання сталий уздовж променя: обидва напрямки нерухомі.
        let cos_theta = w[0] * sun[0] + w[1] * sun[1] + w[2] * sun[2];
        let phase_r = rayleigh_phase(cos_theta);
        let phase_m = mie_phase(cos_theta, f64::from(air.mie_g));

        let rho2 = rho_squared(r, bottom);
        let mut span = distance_to_top(r, mu_v, rho2, shell_squared(air, bottom));
        if let Some(ground) = distance_to_ground(r, mu_v, rho2) {
            span = span.min(ground);
        }
        let step = span / f64::from(SKYVIEW_STEPS);

        let mut throughput = [1.0; 3];
        let mut light = [0.0; 3];
        for s in 0..SKYVIEW_STEPS {
            let t = (f64::from(s) + 0.5) * step;
            let point = [t * w[0], t * w[1], r + t * w[2]];
            let rho2_here = (rho2 + 2.0 * t * r * mu_v + t * t).max(0.0);
            let radius = (rho2_here + bottom * bottom).max(0.0).sqrt();
            let h = rho2_here / (radius + bottom);
            let mu_s_here =
                (point[0] * sun[0] + point[1] * sun[1] + point[2] * sun[2]) / radius.max(1.0);

            let lit = distance_to_ground(radius, mu_s_here, rho2_here).is_none();
            let to_sun = if lit {
                transmittance.transmittance_at(air, bottom, radius, mu_s_here)
            } else {
                [0.0; 3]
            };
            let psi = multiscatter.multiscatter_at(air, bottom, radius, mu_s_here);

            let [d_rayleigh, d_mie, _] = density(air, h);
            let sigma_e = extinction(air, h);

            for channel in 0..3 {
                if sigma_e[channel] <= 0.0 {
                    continue;
                }
                let sigma_r = f64::from(air.rayleigh_scattering[channel]) * d_rayleigh;
                let sigma_m = f64::from(air.mie_scattering) * d_mie;
                // Пряме світло — з власною фазовою функцією кожної компоненти;
                // багаторазове — вже усереднене по сфері, тож фази не має.
                let source = (sigma_r * phase_r + sigma_m * phase_m) * to_sun[channel]
                    + (sigma_r + sigma_m) * psi[channel];

                let step_transmittance = (-sigma_e[channel] * step).exp();
                light[channel] +=
                    throughput[channel] * source * (1.0 - step_transmittance) / sigma_e[channel];
                throughput[channel] *= step_transmittance;
            }
        }
        light
    }
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
        let rho2 = rho_squared(r, BOTTOM);
        assert!(distance_to_ground(r, 0.5, rho2).is_none());
        assert!(distance_to_ground(r, 0.0, rho2).is_none());
        // Строго вниз: до поверхні рівно висота.
        let down = distance_to_ground(r, -1.0, rho2).expect("вниз поверхня є");
        assert!((down - 50_000.0).abs() < 1.0e-6, "{down}");
        // Ковзний промінь трохи нижче дотичної поверхні таки досягає.
        let grazing = -rho2.sqrt() / r;
        assert!(distance_to_ground(r, grazing - 1.0e-6, rho2).is_some());
    }

    /// Промінь униз із самої поверхні нікуди не йде — і це нуль, а не «немає».
    ///
    /// Найдрібніше з тверджень цього модуля й найдорожче з них: у `f32` цей
    /// самий вираз без затискання виходив від'ємним, викликач читав його як
    /// «поверхні попереду немає», і промінь ішов крізь планету. Рядок 0
    /// таблиці розсіювання виходив утричі яскравішим (S3).
    #[test]
    fn a_ray_leaving_the_surface_downwards_travels_nowhere() {
        for mu in [-1.0, -0.5, -0.001] {
            let d = distance_to_ground(BOTTOM, mu, 0.0).expect("поверхня прямо тут");
            assert_eq!(d, 0.0, "mu = {mu}");
        }
    }

    /// `rho²` через висоту й через різницю квадратів — те саме число.
    ///
    /// У `f64` різниця квадратів іще працює, тож тут перевіряється не точність,
    /// а те, що обидві формули описують одну величину: саме на цьому стоїть
    /// право шейдера рахувати її дешевшим способом.
    #[test]
    fn rho_squared_is_the_same_whether_it_comes_from_height_or_from_radii() {
        for altitude in [0.0, 10.0, 1_000.0, 100_000.0] {
            let r = BOTTOM + altitude;
            let by_height = altitude * (2.0 * BOTTOM + altitude);
            let by_radii = rho_squared(r, BOTTOM);
            let scale = by_height.max(1.0);
            assert!(
                (by_height - by_radii).abs() / scale < 1.0e-9,
                "висота {altitude}: {by_height} проти {by_radii}"
            );
        }
        // Те саме для оболонки.
        let air = air();
        let shell = shell_squared(&air, BOTTOM);
        assert!((shell - (air.top_m * air.top_m - BOTTOM * BOTTOM)).abs() / shell < 1.0e-9);
    }
}
