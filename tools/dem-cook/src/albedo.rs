//! Читач глобальної мозаїки LROC WAC: відбивна здатність Місяця (етап T, T2b).
//!
//! Друге джерело поверхні поруч із LOLA, і форма кроку та сама, що в R5a:
//! спершу дані на диску й читач із оракулом, і лише потім щось із ними
//! робиться. Геометрія сітки спільна з висотами — та сама проста циліндрична
//! проєкція, та сама пікселе-реєстрація, — тож реєстрація живе в
//! [`crate::index_of`], а не в двох копіях.
//!
//! ## Три відмінності від LOLA, і кожна ламала б читання мовчки
//!
//! 1. **Етикетка вбудована, а не окрема.** Файла `.LBL` поруч не існує (сервер
//!    віддає на нього 404): PDS3-заголовок лежить у голові самого `.IMG`, і
//!    де закінчується він і починаються пікселі, каже сам заголовок —
//!    `^IMAGE = 2` записів по `RECORD_BYTES`. Прочитати файл із нульового
//!    зсуву означає взяти текст етикетки за перший рядок картинки;
//! 2. **відліки — дійсні числа, а не цілі.** `SAMPLE_TYPE = PC_REAL`,
//!    `SAMPLE_BITS = 32`, і значення це відбивна здатність (0.02 у морях,
//!    0.05 у материках), а не одиниці зберігання з масштабом;
//! 3. **у форматі є спеціальні значення** — `CORE_NULL` і чотири насичення,
//!    задані бітовими патернами (`16#FF7FFFFB#` і сусідні). Як `f32` це
//!    −3.4·10³⁸, тобто число, яке білінійна вибірка розмаже по чотирьох
//!    вузлах, а квантування затисне в нуль. Виглядало б це як чорна пляма
//!    правильної форми.
//!
//! ## Що робить читач із порожніми пікселями: нічого, і голосно
//!
//! Правила заповнення тут немає, і заводити його наперед не можна — воно було
//! б здогадом про дані, яких ми не бачили. Виміряно натомість: у
//! `WAC_GLOBAL_E000N1800_016P` **жодного** спеціального значення немає,
//! усі 16 588 800 відліків справжні. Тому читач їх **рахує й падає**, якщо
//! хоч один трапився: продукт із дірками — це інший продукт, і мовчки кукати
//! його не можна.

use std::path::Path;

use crate::{label_values, number};

/// Скільки байтів голови файлу читаються як текст етикетки.
///
/// Це не `RECORD_BYTES` — його ще треба звідкись узяти, і взяти можна лише з
/// самої етикетки. Курка з яйцем розв'язується так: беремо свідомо більше, ніж
/// етикетка може бути (реальна — 4 КБ тексту в записі на 23 КБ), читаємо з
/// цього ключі, і **аж потім** довіряємо їхнім числам.
const LABEL_PROBE_BYTES: usize = 64 * 1024;

/// Спеціальні значення PDS3: порожньо й чотири насичення.
///
/// Патерни, а не числа з плаваючою комою: порівнювати `f32` на рівність із
/// −3.4·10³⁸ можна, але біти кажуть те саме без жодного питання про
/// округлення, і саме бітами їх задає етикетка.
const SPECIAL: [u32; 5] = [
    0xFF7F_FFFB,
    0xFF7F_FFFC,
    0xFF7F_FFFD,
    0xFF7F_FFFE,
    0xFF7F_FFFF,
];

/// Сітка відбивної здатності в простій циліндричній проєкції.
#[derive(Clone, Debug)]
pub struct Albedo {
    /// Скільки відліків по довготі (`LINE_SAMPLES`).
    pub samples: usize,
    /// Скільки рядків по широті (`LINES`).
    pub lines: usize,
    /// Відліків на градус (`MAP_RESOLUTION`).
    pub per_degree: f64,
    /// Самі відліки, рядок за рядком з півночі на південь.
    pub raw: Vec<f32>,
}

/// Те, що читач бере з етикетки, перш ніж торкнутись пікселів.
///
/// Окремим типом, бо етикетка перевіряється **окремо від картинки**: у git
/// лежить голова продукту (`data/wac/wac_global_016p.lbl`), тобто саме ці
/// 23 КБ, а сам файл на 66 МБ — ні (Q5). Отже розбір заголовка мусить бути
/// викличним без даних, інакше перевіряти його не було б чим.
#[derive(Clone, Debug)]
pub struct Header {
    pub samples: usize,
    pub lines: usize,
    pub per_degree: f64,
    /// Метрів на піксель за етикеткою (`MAP_SCALE`).
    pub metres_per_pixel: f64,
    /// З якого байта файлу починаються пікселі.
    pub data_offset: usize,
}

impl Header {
    /// Розібрати вбудовану етикетку з голови файлу.
    pub fn parse(head: &[u8]) -> Result<Header, String> {
        let probe = &head[..head.len().min(LABEL_PROBE_BYTES)];
        // `from_utf8_lossy`, а не `from_utf8`: хвіст запису добитий нулями, а
        // за етикеткою можуть початися вже й самі пікселі. Текст, який нас
        // цікавить, — ASCII на початку, і псувати його це не може.
        let text = String::from_utf8_lossy(probe);
        let values = label_values(&text);

        let kind = values
            .get("SAMPLE_TYPE")
            .map(String::as_str)
            .unwrap_or_default();
        if kind != "PC_REAL" {
            return Err(format!(
                "очікували PC_REAL — інший тип відліку тут не читається, \
                 а етикетка каже {kind:?}"
            ));
        }
        let bits = number(&values, "SAMPLE_BITS")?;
        if bits != 32.0 {
            return Err(format!("очікували 32 біти на відлік, етикетка каже {bits}"));
        }
        // Один канал — це не спрощення, а сам продукт: WAC GLOBAL знятий одним
        // фільтром (643 нм). Мозаїка з кількома смугами лежала б інакше
        // (`BAND_STORAGE_TYPE`), і читати її цим кодом не можна.
        let bands = number(&values, "BANDS")?;
        if bands != 1.0 {
            return Err(format!("очікували один канал, етикетка каже {bands}"));
        }
        let projection = values
            .get("MAP_PROJECTION_TYPE")
            .map(String::as_str)
            .unwrap_or_default();
        if projection != "EQUIRECTANGULAR" {
            return Err(format!(
                "очікували EQUIRECTANGULAR — реєстрація читача виведена саме з \
                 неї, а етикетка каже {projection:?}"
            ));
        }

        let record_bytes = number(&values, "RECORD_BYTES")? as usize;
        let first_record = number(&values, "^IMAGE")? as usize;
        if first_record == 0 {
            return Err("^IMAGE = 0 — записи PDS3 нумеруються з одиниці".to_string());
        }

        Ok(Header {
            samples: number(&values, "LINE_SAMPLES")? as usize,
            lines: number(&values, "LINES")? as usize,
            per_degree: number(&values, "MAP_RESOLUTION")?,
            metres_per_pixel: number(&values, "MAP_SCALE")?,
            data_offset: (first_record - 1) * record_bytes,
        })
    }
}

impl Albedo {
    /// Прочитати мозаїку: етикетка з голови того самого файлу, далі пікселі.
    pub fn read(img: &Path) -> Result<Albedo, String> {
        let bytes = std::fs::read(img).map_err(|e| format!("{}: {e}", img.display()))?;
        let header = Header::parse(&bytes)?;

        let wanted = header.samples * header.lines * 4;
        let end = header.data_offset + wanted;
        if bytes.len() < end {
            return Err(format!(
                "{}: {} байтів, а етикетка обіцяє {end} = {} + {}×{}×4",
                img.display(),
                bytes.len(),
                header.data_offset,
                header.samples,
                header.lines
            ));
        }

        let mut raw = Vec::with_capacity(header.samples * header.lines);
        let mut specials = 0usize;
        for chunk in bytes[header.data_offset..end].chunks_exact(4) {
            let word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            if SPECIAL.contains(&word) {
                specials += 1;
            }
            raw.push(f32::from_bits(word));
        }
        if specials > 0 {
            return Err(format!(
                "{}: {specials} спеціальних значень PDS3 (порожньо або насичення). \
                 Правила заповнення в читача немає навмисно — цей продукт їх не має",
                img.display()
            ));
        }

        Ok(Albedo {
            samples: header.samples,
            lines: header.lines,
            per_degree: header.per_degree,
            raw,
        })
    }

    /// Відлік сітки. Індекси загортаються по довготі й затискаються по
    /// широті — саме так поводиться сама сфера.
    pub fn at(&self, line: i64, sample: i64) -> f32 {
        let line = line.clamp(0, self.lines as i64 - 1) as usize;
        let sample = sample.rem_euclid(self.samples as i64) as usize;
        self.raw[line * self.samples + sample]
    }

    /// Відбивна здатність у довільній точці, білінійно між чотирма відліками.
    pub fn sample(&self, lat: f64, lon: f64) -> f64 {
        crate::bilinear(self.per_degree, lat, lon, |line, sample| {
            f64::from(self.at(line, sample))
        })
    }

    /// Відбивна здатність у напрямку `direction` (не обов'язково одиничному).
    pub fn sample_direction(&self, direction: [f64; 3]) -> f64 {
        let (lat, lon) = crate::lat_lon(direction);
        self.sample(lat, lon)
    }

    /// Межі, пораховані з самих даних.
    pub fn measured(&self) -> (f32, f32) {
        let mut low = f32::MAX;
        let mut high = f32::MIN;
        for &v in &self.raw {
            low = low.min(v);
            high = high.max(v);
        }
        (low, high)
    }
}
