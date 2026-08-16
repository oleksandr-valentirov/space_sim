//! Читач мозаїки Blue Marble Next Generation: колір Землі (етап T, крок T7c).
//!
//! Четверте джерело поверхні, і геометрія в нього та сама, що в ETOPO
//! ([`crate::etopo`]): проста циліндрична проєкція, пікселе-реєстрована,
//! 60 відліків на градус, перший стовпець — 180° західної. Збіг сіток
//! **пікселе-в-піксель** і був причиною взяти саме цю пару продуктів (T7):
//! вузол кольору лягає рівно на вузол висоти, тож берегова лінія не може
//! розійтися сама з собою.
//!
//! ## Що тут чуже, а що своє
//!
//! JPEG розбирає крейт `jpeg-decoder` — декодери зображень ми не пишемо
//! (CLAUDE.md, «Чого НЕ робимо»). Своє — те, чого декодер не знає: як ці
//! байти прив'язані до глобуса і в якому вони просторі.
//!
//! ## Простір: sRGB у файлі, лінійний у пам'яті
//!
//! Мозаїка кодована **sRGB** — це картинка для ока, а не поле фізичних
//! величин. Кадр же працює в лінійному світлі (T5c), і будь-яке усереднення
//! — білінійна вага, ланцюг грубіших сіток — має сенс лише в лінійному:
//! середнє двох sRGB-байтів це не колір суміші, а колір «між ними на око».
//! Тому читач декодує один раз, при завантаженні, і далі всі числа тут
//! лінійні.
//!
//! Ціна названа: `float32` замість байта — це 2.8 ГБ на нульовий рівень і
//! ~3.7 ГБ на весь ланцюг. Кукер офлайновий, і платити пам'яттю тут дешевше,
//! ніж округлювати сім разів поспіль.
//!
//! ⚠ **Це не альбедо.** Мозаїка зібрана з MODIS і відретушована для ока: у
//! ній лишились тіні схилів і сліди атмосферної корекції. Фізичного
//! відбиття, як у WAC, вона не обіцяє — і саме тому колірний тайл Землі несе
//! «колір поверхні», а не «відбивну здатність».

use std::path::Path;

/// Скільки каналів несе мозаїка.
pub const CHANNELS: usize = 3;

/// Сітка кольору в простій циліндричній проєкції, **лінійне** світло.
#[derive(Clone, Debug)]
pub struct Mosaic {
    /// Скільки відліків по довготі.
    pub samples: usize,
    /// Скільки рядків по широті.
    pub lines: usize,
    /// Відліків на градус.
    pub per_degree: f64,
    /// Самі відліки: `CHANNELS` підряд на піксель, рядок за рядком з півночі
    /// на південь, кожен від 0 до 1 у лінійному світлі.
    pub raw: Vec<f32>,
}

impl Mosaic {
    /// Прочитати мозаїку цілком.
    pub fn read(path: &Path) -> Result<Mosaic, String> {
        let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut decoder = jpeg_decoder::Decoder::new(std::io::BufReader::new(file));
        let pixels = decoder
            .decode()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let info = decoder
            .info()
            .ok_or_else(|| format!("{}: JPEG без заголовка", path.display()))?;

        if info.pixel_format != jpeg_decoder::PixelFormat::RGB24 {
            return Err(format!(
                "{}: очікували три канали по вісім біт, а там {:?}",
                path.display(),
                info.pixel_format
            ));
        }

        let (samples, lines) = (info.width as usize, info.height as usize);
        // Мозаїка мусить накривати глобус, і накривати його рівно: два до
        // одного. Продукт-плитка (BMNG роздається ще й вісьмома шматками)
        // читався б без помилки й давав би восьму частину світу на всю Землю.
        if samples != 2 * lines {
            return Err(format!(
                "{}: {samples}×{lines} — це не глобус, у якого довгота вдвічі \
                 довша за широту",
                path.display()
            ));
        }
        if pixels.len() != samples * lines * CHANNELS {
            return Err(format!(
                "{}: {} байтів замість {samples}×{lines}×{CHANNELS}",
                path.display(),
                pixels.len()
            ));
        }

        let table = srgb_table();
        let raw = pixels.iter().map(|&b| table[b as usize]).collect();

        Ok(Mosaic {
            samples,
            lines,
            per_degree: samples as f64 / 360.0,
            raw,
        })
    }

    /// Відлік сітки, лінійний. Індекси загортаються по довготі й затискаються
    /// по широті — саме так поводиться сама сфера.
    pub fn at(&self, line: i64, sample: i64, channel: usize) -> f32 {
        let line = line.clamp(0, self.lines as i64 - 1) as usize;
        let sample = sample.rem_euclid(self.samples as i64) as usize;
        self.raw[(line * self.samples + sample) * CHANNELS + channel]
    }

    /// Колір у довільній точці, білінійно між чотирма відліками.
    ///
    /// ⚠ Довгота зсунута на π з тієї ж причини, що в [`crate::etopo`]: сітка
    /// починається з −180°, а спільна реєстрація рахує від нуля.
    pub fn sample(&self, lat: f64, lon: f64) -> [f64; CHANNELS] {
        let mut out = [0.0; CHANNELS];
        for (channel, value) in out.iter_mut().enumerate() {
            *value = crate::bilinear(
                self.per_degree,
                lat,
                lon + std::f64::consts::PI,
                |line, sample| f64::from(self.at(line, sample, channel)),
            );
        }
        out
    }

    /// Колір у напрямку `direction` (не обов'язково одиничному).
    pub fn sample_direction(&self, direction: [f64; 3]) -> [f64; CHANNELS] {
        let (lat, lon) = crate::lat_lon(direction);
        self.sample(lat, lon)
    }

    /// Кутовий розмір пікселя, радіани.
    pub fn pixel_rad(&self) -> f64 {
        std::f64::consts::PI / 180.0 / self.per_degree
    }

    /// Та сама мозаїка, грубіша на крок ланцюга: кожен відлік — середнє блоку.
    ///
    /// Середнє **лінійних** значень, а не sRGB-байтів, — це й було причиною
    /// декодувати один раз при читанні.
    pub fn reduced(&self) -> Option<Mosaic> {
        let step = crate::reduce_step(self.samples, self.lines)?;
        let (samples, lines) = (self.samples / step, self.lines / step);
        let mut raw = Vec::with_capacity(samples * lines * CHANNELS);
        for line in 0..lines {
            for sample in 0..samples {
                for channel in 0..CHANNELS {
                    let mut sum = 0.0f64;
                    for dl in 0..step {
                        for ds in 0..step {
                            let l = step * line + dl;
                            let s = step * sample + ds;
                            sum += f64::from(self.raw[(l * self.samples + s) * CHANNELS + channel]);
                        }
                    }
                    raw.push((sum / (step * step) as f64) as f32);
                }
            }
        }
        Some(Mosaic {
            samples,
            lines,
            per_degree: self.per_degree / step as f64,
            raw,
        })
    }

    /// Ланцюг сіток, кожна грубіша за попередню; нульова — ця сама.
    ///
    /// Те саме й з тієї ж причини, що [`crate::albedo::Albedo::chain`]: без
    /// нього грубий рівень піраміди брав би точкову вибірку з тисячі пікселів
    /// і давав плямистий шум замість карти (T3c). На скільки грубішає кожен
    /// крок, вирішує [`crate::reduce_step`] — і не завжди вдвічі: сітка Землі
    /// ділиться надвоє лише чотири рази.
    pub fn chain(&self) -> Vec<Mosaic> {
        let mut out = vec![self.clone()];
        while let Some(next) = out.last().expect("ланцюг не порожній").reduced() {
            out.push(next);
        }
        out
    }

    /// Середній колір по всій мозаїці, зважений `cos(широта)`.
    ///
    /// Потрібен не для краси: альбедо неба (S1) бере один колір на тіло, і
    /// саме це число буде його оцінкою, поки в кадрі немає кращої.
    pub fn mean(&self) -> [f64; CHANNELS] {
        let degrees = std::f64::consts::PI / 180.0;
        let mut sum = [0.0; CHANNELS];
        let mut total = 0.0;
        for line in 0..self.lines {
            let lat = 90.0 - (line as f64 + 0.5) * 180.0 / self.lines as f64;
            let weight = (lat * degrees).cos();
            let row =
                &self.raw[line * self.samples * CHANNELS..(line + 1) * self.samples * CHANNELS];
            for pixel in row.chunks_exact(CHANNELS) {
                for (channel, value) in pixel.iter().enumerate() {
                    sum[channel] += weight * f64::from(*value);
                }
            }
            total += weight * self.samples as f64;
        }
        sum.map(|s| s / total)
    }
}

/// sRGB-байт → лінійне світло, таблицею на 256 входів.
///
/// Таблицею, а не формулою на кожен піксель: входів рівно 256, а пікселів
/// 233 мільйони. Сама формула — стандартна sRGB, з лінійною ділянкою внизу;
/// наближення `x^2.2` тут не годиться саме в темному, тобто в океані, який
/// займає дві третини кадру.
fn srgb_table() -> [f32; 256] {
    let mut table = [0.0f32; 256];
    for (index, value) in table.iter_mut().enumerate() {
        let x = index as f64 / 255.0;
        *value = if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        } as f32;
    }
    table
}

/// Лінійне світло → sRGB-байт. Обернене до [`srgb_table`].
///
/// Потрібне кукеру: тайл зберігає **байт**, і зберігати в ньому лінійне
/// значення означало б витратити всю шкалу на світлі тони — океан при 0.0015
/// лінійних отримав би нуль. Отже в тайлі знову sRGB, а розкодує його GPU
/// при вибірці, безкоштовно (`Rgba8UnormSrgb`).
pub fn to_srgb(linear: f64) -> u8 {
    let x = linear.clamp(0.0, 1.0);
    let encoded = if x <= 0.003_130_8 {
        12.92 * x
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}
