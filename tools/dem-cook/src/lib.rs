//! Читач LOLA GDR: сира сітка висот Місяця (ROADMAP-PLANETS.md, R5a).
//!
//! Та сама форма кроку, що K5a з коефіцієнтами GRAIL: спершу дані в
//! репозиторії й читач із оракулом, і лише потім щось із ними робиться.
//! Оракул тут навмисно **не наш**: етикетка PDS3 поруч із файлом друкує
//! `MINIMUM` і `MAXIMUM` у тих самих одиницях, у яких лежать самі дані, тож
//! читач має відтворити опубліковані числа з сирих байтів. Це ловить одразу
//! три класичні помилки, і жодну з них не видно оком у картинці:
//!
//! 1. **переплутаний порядок байтів** — LOLA пише `LSB_INTEGER`, і на
//!    big-endian машині чи при наївному `from_be_bytes` мінімум і максимум
//!    поїдуть у тисячі кілометрів;
//! 2. **забутий масштаб** — висоти зберігаються цілими в **пів**метра
//!    (`SCALING_FACTOR = 0.5`), тож без нього рельєф удвічі вищий;
//! 3. **зсув на пів пікселя** — сітка **пікселе-реєстрована**, і зміщення на
//!    півклітинки дає карту, яка виглядає правильно й стоїть не там.
//!
//! Третю оракул min/max сам не ловить, і тому для неї стоїть окрема перевірка:
//! відомі координати кількох кратерів мусять дати відомі глибини.
//!
//! ## Чого тут немає
//!
//! **Розбору PDS3 у загальному вигляді.** З етикетки читаються рівно ті
//! ключі, від яких залежить арифметика, а решта лишається текстом для людини.
//! Повний розбір формату — це бібліотека, а не двадцять рядків, і жодного
//! з її решти можливостей тут ніхто не покличе.

pub mod cook;

use std::collections::HashMap;
use std::path::Path;

/// Сітка висот у простій циліндричній проєкції.
///
/// Поля названі так само, як ключі етикетки, і це навмисно: між файлом
/// джерела й цією структурою не мусить бути перекладу, у якому можна
/// помилитись.
#[derive(Clone, Debug)]
pub struct Grid {
    /// Скільки відліків по довготі (`LINE_SAMPLES`).
    pub samples: usize,
    /// Скільки рядків по широті (`LINES`).
    pub lines: usize,
    /// Множник із цілого в метри (`SCALING_FACTOR`).
    pub scale_m: f64,
    /// Опорний радіус, від якого відлічуються висоти (`OFFSET`), метри.
    pub reference_m: f64,
    /// Відліків на градус (`MAP_RESOLUTION`).
    pub per_degree: f64,
    /// Метрів на піксель за етикеткою (`MAP_SCALE`).
    pub metres_per_pixel: f64,
    /// Опубліковані межі в цілих одиницях — оракул, а не дані.
    pub published: (i32, i32),
    /// Самі відліки, рядок за рядком з півночі на південь.
    pub raw: Vec<i16>,
}

/// Значення ключа етикетки — усе, що після `=` до кінця рядка.
fn label_values(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        // Ключі етикетки — великими, з підкресленнями. Усе інше — проза
        // всередині `DESCRIPTION`, і брати її за ключ не можна.
        if key.is_empty()
            || !key
                .chars()
                .all(|c| c.is_ascii_uppercase() || c == '_' || c == '^' || c.is_ascii_digit())
        {
            continue;
        }
        // Перший запис виграє: та сама назва трапляється і в `OBJECT`, і
        // в коментарі-прикладі нижче.
        out.entry(key.to_string())
            .or_insert_with(|| value.trim().to_string());
    }
    out
}

/// Число з поля етикетки: `21008`, `0.5`, `1737400.`, `7580.84 <m/pix>`.
fn number(values: &HashMap<String, String>, key: &str) -> Result<f64, String> {
    let raw = values
        .get(key)
        .ok_or_else(|| format!("в етикетці немає {key}"))?;
    let head = raw.split_whitespace().next().unwrap_or("");
    head.trim_end_matches('.')
        .parse::<f64>()
        .map_err(|e| format!("{key} = {raw:?}: {e}"))
}

impl Grid {
    /// Прочитати пару «етикетка + дані».
    ///
    /// Шлях указує на `.img`; етикетка шукається поруч із тим самим іменем і
    /// розширенням `.lbl`. Так їх і публікують, і так само вони лежать у нас.
    pub fn read(img: &Path) -> Result<Grid, String> {
        let lbl = img.with_extension("lbl");
        let text = std::fs::read_to_string(&lbl).map_err(|e| format!("{}: {e}", lbl.display()))?;
        let values = label_values(&text);

        let bits = number(&values, "SAMPLE_BITS")?;
        if bits != 16.0 {
            return Err(format!(
                "очікували 16 біт на відлік, а етикетка каже {bits}"
            ));
        }
        let kind = values
            .get("SAMPLE_TYPE")
            .map(String::as_str)
            .unwrap_or_default();
        if kind != "LSB_INTEGER" {
            return Err(format!(
                "очікували LSB_INTEGER — інший порядок байтів тут не читається, \
                 а етикетка каже {kind:?}"
            ));
        }

        let samples = number(&values, "LINE_SAMPLES")? as usize;
        let lines = number(&values, "LINES")? as usize;

        let bytes = std::fs::read(img).map_err(|e| format!("{}: {e}", img.display()))?;
        let wanted = samples * lines * 2;
        if bytes.len() != wanted {
            return Err(format!(
                "{}: {} байтів замість {wanted} = {samples}×{lines}×2",
                img.display(),
                bytes.len()
            ));
        }

        let raw = bytes
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();

        Ok(Grid {
            samples,
            lines,
            scale_m: number(&values, "SCALING_FACTOR")?,
            reference_m: number(&values, "OFFSET")?,
            per_degree: number(&values, "MAP_RESOLUTION")?,
            metres_per_pixel: number(&values, "MAP_SCALE")?,
            published: (
                number(&values, "MINIMUM")? as i32,
                number(&values, "MAXIMUM")? as i32,
            ),
            raw,
        })
    }

    /// Ціле значення відліку. Індекси загортаються по довготі й
    /// затискаються по широті — саме так поводиться сама сфера.
    pub fn at(&self, line: i64, sample: i64) -> i16 {
        let line = line.clamp(0, self.lines as i64 - 1) as usize;
        let sample = sample.rem_euclid(self.samples as i64) as usize;
        self.raw[line * self.samples + sample]
    }

    /// Висота відліку над опорним радіусом, метри.
    pub fn height_m(&self, line: i64, sample: i64) -> f64 {
        f64::from(self.at(line, sample)) * self.scale_m
    }

    /// Межі, пораховані з самих даних, у цілих одиницях.
    pub fn measured(&self) -> (i32, i32) {
        let mut low = i32::MAX;
        let mut high = i32::MIN;
        for &v in &self.raw {
            low = low.min(i32::from(v));
            high = high.max(i32::from(v));
        }
        (low, high)
    }

    /// Дробові індекси відліку для широти й довготи, **радіани**.
    ///
    /// Сітка пікселе-реєстрована: центр першого відліку лежить не на краю
    /// діапазону, а на півклітинки всередині. Звідси `− 0.5` в обох
    /// формулах — той самий зсув, який етикетка називає
    /// `LINE_PROJECTION_OFFSET = 359.5` і `SAMPLE_PROJECTION_OFFSET = 719.5`.
    /// Забути його означає зсунути всю карту на 3.8 км.
    pub fn index_of(&self, lat: f64, lon: f64) -> (f64, f64) {
        let degrees = 180.0 / std::f64::consts::PI;
        let line = (90.0 - lat * degrees) * self.per_degree - 0.5;
        let sample = (lon * degrees).rem_euclid(360.0) * self.per_degree - 0.5;
        (line, sample)
    }

    /// Висота в довільній точці, білінійно між чотирма відліками.
    ///
    /// Білінійно, а не найближчим: тайл кубосфери падає на цю сітку під
    /// довільним кутом, і сходинки найближчого сусіда стали б видимими
    /// рівно там, де тайл дрібніший за клітинку джерела.
    pub fn sample_m(&self, lat: f64, lon: f64) -> f64 {
        let (line, sample) = self.index_of(lat, lon);
        let (l0, s0) = (line.floor(), sample.floor());
        let (tl, ts) = (line - l0, sample - s0);
        let (l0, s0) = (l0 as i64, s0 as i64);

        let h = |dl: i64, ds: i64| self.height_m(l0 + dl, s0 + ds);
        let top = h(0, 0) * (1.0 - ts) + h(0, 1) * ts;
        let bottom = h(1, 0) * (1.0 - ts) + h(1, 1) * ts;
        top * (1.0 - tl) + bottom * tl
    }

    /// Висота в напрямку `direction` (не обов'язково одиничному), метри.
    ///
    /// Напрямок, а не пара кутів: кубосфера оперує напрямками, і переклад
    /// у широту-довготу мусить жити в одному місці — тут, де поруч стоїть
    /// сама сітка.
    pub fn sample_direction_m(&self, direction: [f64; 3]) -> f64 {
        let [x, y, z] = direction;
        let flat = (x * x + y * y).sqrt();
        self.sample_m(z.atan2(flat), y.atan2(x))
    }
}
