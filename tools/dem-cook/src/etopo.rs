//! Читач ETOPO 2022: форма Землі з батиметрією (етап T, крок T7b).
//!
//! Третє джерело поверхні поруч із LOLA й LROC WAC, і геометрія сітки в нього
//! **та сама**: проста циліндрична проєкція, пікселе-реєстрована, 60 відліків
//! на градус. Тому реєстрація й білінійна вибірка беруться з
//! [`crate::index_of`] і [`crate::bilinear`], а не пишуться втретє.
//!
//! ## Що тут своє, а що чуже
//!
//! Контейнер — **GeoTIFF**, тобто вперше в кукері формат, у якому дані
//! стиснені. Розпаковує їх крейт `tiff` (Deflate з floating-point
//! предиктором, тайли 256×256), і це рівно те, чого «Чого НЕ робимо» вимагає:
//! стиснення й декодери ми не пишемо. Своє тут — **інтерпретація тегів**:
//! GeoTIFF описує прив'язку до глобуса трьома масивами чисел, і жодна
//! бібліотека не скаже, чи означають вони те, чого чекає кукер.
//!
//! ## Три перевірки, кожна ловить помилку, якої не видно на картинці
//!
//! 1. **реєстрація.** `RasterType = PixelIsArea` (GeoKey 1025 = 1), тобто
//!    піксель накриває комірку, а не стоїть у вузлі. Зсув на півклітинки —
//!    0.93 км — рухає берегову лінію рівно там, де колір міняється стрибком;
//! 2. **прив'язка.** `ModelPixelScale` мусить бути 1/60 градуса по обох осях,
//!    а `ModelTiepoint` — класти піксель `(0, 0)` у `(−180°, +90°)`. Продукт
//!    з іншим кутом читався б без жодної помилки й давав би перевернуту або
//!    зсунуту Землю;
//! 3. **порожні пікселі.** `GDAL_NODATA = −99999`, і поводимось із ними так
//!    само, як з `CORE_NULL` у WAC: **рахуємо й падаємо**, якщо трапився хоч
//!    один. Виміряно на всьому продукті — їх немає жодного, тож правила
//!    заповнення тут немає навмисно: воно було б здогадом про дані, яких ми
//!    не бачили.
//!
//! ## Чому відліки стають цілими метрами
//!
//! Джерело — `float32` над геоїдом EGM2008, діапазон −10 752 … +8157 м. Тайл
//! рельєфу зберігає `i16` (R5c), тож масштаб 1 м покриває весь діапазон із
//! запасом, а дробові метри однаково нижчі за квант формату. Округлення тут
//! **одне**, а не два: сітка одразу лежить у тих одиницях, у яких її запише
//! кукер.
//!
//! ⚠ Висоти відлічені від **геоїда**, а гра малює сферу радіусом 6 371 010 м
//! (`reference_m`). Різниця — геоїдна хвиля ±100 м і сплюснутість 21 км — не
//! додається: етап T свідомо лишив сферу. Наслідок названий чесно: висоти
//! правильні відносно поверхні води, а не відносно центра Землі.

use std::path::Path;

use tiff::decoder::{Decoder, DecodingResult, Limits};
use tiff::tags::Tag;

/// Опорний радіус, від якого кукер відлічує висоти Землі, метри.
///
/// Той самий, що несе ассет ефемериди (`ephemeris.h`: середній радіус
/// 6 371 010 м проти екваторіального 6 378 137), і це не збіг, а вимога: тіло
/// в кадрі малюється сферою саме цього радіуса, тож будь-яке інше число тут
/// підняло б або втопило всю поверхню разом.
pub const REFERENCE_M: f64 = 6_371_010.0;

/// Значення «даних немає» з тега `GDAL_NODATA`.
const NODATA: f32 = -99999.0;

/// Тег GeoTIFF: розмір пікселя в одиницях координатної системи.
const TAG_PIXEL_SCALE: Tag = Tag::Unknown(33550);
/// Тег GeoTIFF: прив'язка пікселя до координат.
const TAG_TIEPOINT: Tag = Tag::Unknown(33922);
/// Тег GeoTIFF: словник ключів, серед них реєстрація й датум.
const TAG_GEO_KEYS: Tag = Tag::Unknown(34735);

/// Те, що читач бере з заголовка, перш ніж торкнутись пікселів.
///
/// Окремим типом з тієї ж причини, що [`crate::albedo::Header`]: у git лежить
/// рівно заголовок (`data/etopo/etopo_2022_60s_surface.lbl`, 32 КіБ), а сам
/// продукт на 466 МБ — ні (Q5). Отже розбір мусить бути викличним **без
/// даних**, інакше перевіряти його не було б чим.
#[derive(Clone, Debug)]
pub struct Header {
    /// Скільки відліків по довготі.
    pub samples: usize,
    /// Скільки рядків по широті.
    pub lines: usize,
    /// Відліків на градус — обернене до `ModelPixelScale`.
    pub per_degree: f64,
    /// Куди прив'язаний кут пікселя `(0, 0)`: довгота й широта, градуси.
    pub corner_deg: (f64, f64),
}

impl Header {
    /// Прочитати заголовок — і з продукту, і з етикетки в git.
    ///
    /// Працює на обох, бо `Decoder::new` читає лише IFD: пікселі лишаються
    /// незайманими доти, доки їх не попросять. Саме тому етикеткою може бути
    /// голова файлу, а не окремий опис.
    pub fn read(path: &Path) -> Result<Header, String> {
        let mut decoder = open(path)?;
        Header::from_decoder(&mut decoder)
    }

    fn from_decoder(
        decoder: &mut Decoder<std::io::BufReader<std::fs::File>>,
    ) -> Result<Header, String> {
        let (width, height) = decoder
            .dimensions()
            .map_err(|e| format!("розміри GeoTIFF: {e}"))?;

        let scale = doubles(decoder, TAG_PIXEL_SCALE, "ModelPixelScale")?;
        if scale.len() < 2 {
            return Err(format!(
                "ModelPixelScale має {} чисел замість 3",
                scale.len()
            ));
        }
        // Крок по довготі й широті мусить бути однаковий: сітка квадратна в
        // градусах, і саме на цьому стоїть один `per_degree` замість двох.
        if (scale[0] - scale[1]).abs() > 1e-12 {
            return Err(format!(
                "ModelPixelScale не квадратний: {} проти {} градуса",
                scale[0], scale[1]
            ));
        }
        if scale[0] <= 0.0 {
            return Err(format!("ModelPixelScale = {} градуса", scale[0]));
        }

        let tie = doubles(decoder, TAG_TIEPOINT, "ModelTiepoint")?;
        if tie.len() < 6 {
            return Err(format!("ModelTiepoint має {} чисел замість 6", tie.len()));
        }
        // Перша трійка — піксель, друга — його координати. Кукер уміє читати
        // лише прив'язку до кута растра; будь-яка інша означає інший продукт.
        if tie[0] != 0.0 || tie[1] != 0.0 {
            return Err(format!(
                "ModelTiepoint прив'язує піксель ({}, {}), а не кут растра",
                tie[0], tie[1]
            ));
        }

        // Реєстрація: 1 — PixelIsArea, 2 — PixelIsPoint. Різниця в пів пікселя,
        // тобто 0.93 км на екваторі, і жодна картинка її не покаже.
        let keys = shorts(decoder, TAG_GEO_KEYS, "GeoKeyDirectory")?;
        match geo_key(&keys, 1025) {
            Some(1) => {}
            Some(other) => {
                return Err(format!(
                    "RasterType = {other}: сітка вузлова, а читач рахує \
                     пікселе-реєстровану"
                ))
            }
            None => return Err("у GeoKeyDirectory немає RasterType".to_string()),
        }

        Ok(Header {
            samples: width as usize,
            lines: height as usize,
            per_degree: 1.0 / scale[0],
            corner_deg: (tie[3], tie[4]),
        })
    }

    /// Чи прив'язана сітка так, як чекає кукер: північно-західний кут світу.
    ///
    /// Продукт, що починається деінде, читався б без єдиної помилки й давав
    /// би зсунуту Землю — тобто помилку, яку видно лише поруч із берегом.
    pub fn covers_globe(&self) -> bool {
        let span_lon = self.samples as f64 / self.per_degree;
        let span_lat = self.lines as f64 / self.per_degree;
        (self.corner_deg.0 + 180.0).abs() < 1e-9
            && (self.corner_deg.1 - 90.0).abs() < 1e-9
            && (span_lon - 360.0).abs() < 1e-9
            && (span_lat - 180.0).abs() < 1e-9
    }
}

/// Сітка висот Землі в простій циліндричній проєкції, цілі метри.
#[derive(Clone, Debug)]
pub struct Relief {
    /// Скільки відліків по довготі.
    pub samples: usize,
    /// Скільки рядків по широті.
    pub lines: usize,
    /// Відліків на градус.
    pub per_degree: f64,
    /// Самі відліки, рядок за рядком з півночі на південь, метри.
    pub raw: Vec<i16>,
}

impl Relief {
    /// Прочитати продукт цілком.
    pub fn read(path: &Path) -> Result<Relief, String> {
        let mut decoder = open(path)?;
        let header = Header::from_decoder(&mut decoder)?;
        if !header.covers_globe() {
            return Err(format!(
                "сітка {}×{} з кутом {:?} не накриває глобус",
                header.samples, header.lines, header.corner_deg
            ));
        }

        // Ліміти крейта розраховані на картинки для екрана; тут 933 МБ
        // відліків, і це нормальний розмір джерела, а не ознака поламаного
        // файлу. Кукер офлайновий, пам'ять у нього є.
        let image = decoder
            .read_image()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let DecodingResult::F32(values) = image else {
            return Err("очікували float32 — інший тип відліку тут не читається".to_string());
        };
        if values.len() != header.samples * header.lines {
            return Err(format!(
                "{} відліків замість {}×{}",
                values.len(),
                header.samples,
                header.lines
            ));
        }

        // Правила заповнення немає навмисно (див. вступ модуля): продукт з
        // дірками — це інший продукт, і кукати його мовчки не можна.
        let empty = values.iter().filter(|&&v| v == NODATA).count();
        if empty > 0 {
            return Err(format!(
                "{empty} відліків = {NODATA} (GDAL_NODATA); правила заповнення \
                 в кукера немає"
            ));
        }

        let raw = values
            .iter()
            .map(|&v| quantise(f64::from(v)))
            .collect::<Vec<i16>>();
        drop(values);

        Ok(Relief {
            samples: header.samples,
            lines: header.lines,
            per_degree: header.per_degree,
            raw,
        })
    }

    /// Відлік сітки. Індекси загортаються по довготі й затискаються по
    /// широті — саме так поводиться сама сфера.
    pub fn at(&self, line: i64, sample: i64) -> i16 {
        let line = line.clamp(0, self.lines as i64 - 1) as usize;
        let sample = sample.rem_euclid(self.samples as i64) as usize;
        self.raw[line * self.samples + sample]
    }

    /// Висота відліку над опорною сферою, метри.
    pub fn height_m(&self, line: i64, sample: i64) -> f64 {
        f64::from(self.at(line, sample))
    }

    /// Висота в довільній точці, білінійно між чотирма відліками.
    ///
    /// ⚠ **Перший стовпець тут — 180° західної, а не нульовий меридіан.** У
    /// LOLA й WAC сітка починається з 0° (`index_of` це й припускає), в ETOPO
    /// — з −180°, і саме це каже `ModelTiepoint`. Тому довгота приїжджає в
    /// спільну реєстрацію зсунутою на π: без зсуву карта читалася б без
    /// жодної помилки й стояла б на пів глобуса не там — тобто помилка,
    /// схожа на правильну Землю, поки не глянеш, де океан.
    pub fn sample_m(&self, lat: f64, lon: f64) -> f64 {
        crate::bilinear(
            self.per_degree,
            lat,
            lon + std::f64::consts::PI,
            |line, sample| self.height_m(line, sample),
        )
    }

    /// Висота в напрямку `direction` (не обов'язково одиничному), метри.
    pub fn sample_direction_m(&self, direction: [f64; 3]) -> f64 {
        let (lat, lon) = crate::lat_lon(direction);
        self.sample_m(lat, lon)
    }

    /// Межі, пораховані з самих даних, метри.
    pub fn measured(&self) -> (i16, i16) {
        let mut low = i16::MAX;
        let mut high = i16::MIN;
        for &v in &self.raw {
            low = low.min(v);
            high = high.max(v);
        }
        (low, high)
    }

    /// Частка суші (`h ≥ 0`) на сфері, зважена `cos(широта)`.
    ///
    /// Головний оракул читача, і саме тому він живе тут, а не в тесті: одне
    /// число ловить увесь клас помилок геометрії. Справжня частка — 29.2%;
    /// зсув на пів пікселя, перевернутий порядок рядків чи хибно розібраний
    /// предиктор рухають його на відсотки, а поламана сітка — на десятки.
    ///
    /// Вага `cos(широта)`, а не рахунок пікселів: у циліндричній проєкції
    /// полярний рядок накриває в сотні разів меншу площу, ніж екваторіальний,
    /// і без ваги Антарктида важила б як Африка.
    pub fn land_fraction(&self) -> f64 {
        let degrees = std::f64::consts::PI / 180.0;
        let mut land = 0.0;
        let mut total = 0.0;
        for line in 0..self.lines {
            let lat = 90.0 - (line as f64 + 0.5) * 180.0 / self.lines as f64;
            let weight = (lat * degrees).cos();
            let row = &self.raw[line * self.samples..(line + 1) * self.samples];
            let above = row.iter().filter(|&&v| v >= 0).count();
            land += weight * above as f64;
            total += weight * self.samples as f64;
        }
        land / total
    }

    /// Кут, який накриває один піксель сітки, радіани.
    ///
    /// Те саме, що [`crate::albedo::Albedo::pixel_rad`], і потрібне тому
    /// самому — вибору сітки під рівень піраміди (T3c, `cook::source_for`).
    pub fn pixel_rad(&self) -> f64 {
        std::f64::consts::PI / (180.0 * self.per_degree)
    }
}

/// Метри з плаваючою комою → цілі метри, з насиченням замість загортання.
///
/// Загортання тут було б найгіршим із можливих: западина на 40 км стала б
/// горою, і виглядало б це правдоподібно. Виміряний діапазон продукту
/// (−10 752 … +8157) не наближається до межі, тож насичення — це запобіжник
/// на чужий продукт, а не робочий шлях.
fn quantise(metres: f64) -> i16 {
    metres
        .round()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

fn open(path: &Path) -> Result<Decoder<std::io::BufReader<std::fs::File>>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let decoder = Decoder::new(std::io::BufReader::new(file))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(decoder.with_limits(Limits::unlimited()))
}

fn doubles(
    decoder: &mut Decoder<std::io::BufReader<std::fs::File>>,
    tag: Tag,
    name: &str,
) -> Result<Vec<f64>, String> {
    decoder
        .get_tag_f64_vec(tag)
        .map_err(|e| format!("тег {name}: {e}"))
}

fn shorts(
    decoder: &mut Decoder<std::io::BufReader<std::fs::File>>,
    tag: Tag,
    name: &str,
) -> Result<Vec<u16>, String> {
    decoder
        .get_tag_u16_vec(tag)
        .map_err(|e| format!("тег {name}: {e}"))
}

/// Значення ключа з `GeoKeyDirectory`.
///
/// Директорія — це масив `u16` четвірками: чотири числа заголовка, далі по
/// чотири на ключ (номер, куди веде значення, скільки його, саме значення).
/// Нас цікавлять лише ключі, що лежать у самій директорії (`location == 0`);
/// решта посилається на інші теги, і жодного такого кукер не читає.
fn geo_key(keys: &[u16], wanted: u16) -> Option<u16> {
    if keys.len() < 4 {
        return None;
    }
    let count = keys[3] as usize;
    for k in 0..count {
        let at = 4 + k * 4;
        if at + 3 >= keys.len() {
            break;
        }
        if keys[at] == wanted && keys[at + 1] == 0 {
            return Some(keys[at + 3]);
        }
    }
    None
}
