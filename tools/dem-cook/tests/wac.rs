//! Читач мозаїки LROC WAC (етап T, крок T2b).
//!
//! Оракули поділені за тим, що кожен може зловити **сам**, і за тим, що для
//! кожного мусить лежати на диску:
//!
//! 1. **етикетка** — числа, від яких залежить уся арифметика. Перевіряється
//!    окремо від пікселів, бо в git лежить рівно вона (66 МБ мозаїки — ні,
//!    Q5), і бо жодна помилка в ній не видна на картинці;
//! 2. **зроблена руками мозаїка** — реєстрація, порядок байтів, білінійна
//!    вага й відмова від спеціальних значень, на файлі, кожен відлік якого
//!    відомий наперед. Ці оракули біжать завжди, джерело їм не потрібне;
//! 3. **самі дані** — орієнтація карти, і вона розпадається на **два**
//!    твердження, а не одне. Широту ловить «моря темніші за материки»;
//!    довготу воно **не ловить** (виміряно), і для неї є окрема пара точок.
//!    Обидва пропускаються без джерела — і кажуть, чого бракує.

use dem_cook::albedo::{Albedo, Header};
use std::path::{Path, PathBuf};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/wac")
        .join(name)
}

/// Сама мозаїка, або `None` — тоді тест каже, чого бракує, і не падає (Q5).
fn mosaic() -> Option<Albedo> {
    let path = data("wac_global_016p.img");
    match Albedo::read(&path) {
        Ok(map) => Some(map),
        Err(_) => {
            eprintln!(
                "ПРОПУЩЕНО: немає {}. Як покласти назад — data/wac/README.md",
                path.display()
            );
            None
        }
    }
}

/// Середня відбивна здатність по квадрату 5°×5° навколо точки.
///
/// Середнє, а не піксель: мозаїка знята при кутах падіння 53–70°, тож тінь
/// одного кратера темніша за будь-яке море, і поодинокий відлік нічого не
/// каже про те, що під ним.
fn box_mean(map: &Albedo, lat: f64, lon: f64) -> f64 {
    let degrees = std::f64::consts::PI / 180.0;
    let half = 2.5;
    let steps = 20;
    let mut sum = 0.0;
    for a in 0..steps {
        for b in 0..steps {
            let dl = -half + 2.0 * half * (f64::from(a) + 0.5) / f64::from(steps);
            let ds = -half + 2.0 * half * (f64::from(b) + 0.5) / f64::from(steps);
            sum += map.sample((lat + dl) * degrees, (lon + ds) * degrees);
        }
    }
    sum / f64::from(steps * steps)
}

/// Етикетка дає рівно ті числа, на яких стоїть читач.
///
/// Останнє твердження — не переказ етикетки, а звірка **двох її полів між
/// собою**: `MAP_SCALE` мусить дорівнювати довжині кола Місяця, поділеній на
/// кількість пікселів по екватору. Це і є перевірка `MAP_RESOLUTION`, якої
/// саме по собі не існує: помилка в ньому вдвічі зсунула б усю карту й не
/// зачепила б жодного іншого поля.
#[test]
fn the_label_gives_the_numbers_the_reader_stands_on() {
    let bytes = std::fs::read(data("wac_global_016p.lbl")).expect("етикетка лежить у git");
    let header = Header::parse(&bytes).expect("етикетка мала прочитатися");

    println!(
        "  {}×{} відліків, {} пікс/градус, {:.2} м/піксель; пікселі з байта {}",
        header.samples,
        header.lines,
        header.per_degree,
        header.metres_per_pixel,
        header.data_offset
    );

    assert_eq!(header.samples, 5760);
    assert_eq!(header.lines, 2880);
    assert_eq!(header.per_degree, 16.0);
    // `^IMAGE = 2` записів по 23040 байтів: пікселі починаються рівно там, де
    // закінчується єдиний запис етикетки. Нуль тут означав би, що читач узяв
    // текст етикетки за перший рядок картинки.
    assert_eq!(header.data_offset, 23_040);
    assert_eq!(bytes.len(), header.data_offset);

    // Довжина кола на 1737.4 км радіуса, поділена на 5760 пікселів екватора.
    let moon_radius_m = 1_737_400.0;
    let along_equator = 2.0 * std::f64::consts::PI * moon_radius_m / header.samples as f64;
    let error = (header.metres_per_pixel - along_equator).abs() / along_equator;
    assert!(
        error < 1e-3,
        "MAP_SCALE {:.3} м/піксель проти {along_equator:.3} з геометрії — розбіжність {:.1}%",
        header.metres_per_pixel,
        error * 100.0
    );
}

/// Сітка, зібрана руками: кожен відлік дорівнює своєму номеру рядка.
///
/// Файл із вбудованою етикеткою, як у справжнього продукту, але 8×4 відліки й
/// значення, які відомі наперед. Такий оракул ловить те, чого не ловить
/// жодна перевірка на справжніх даних: `at` за межами сітки, реєстрацію на
/// півклітинки й білінійну вагу — на справжній мозаїці всі три дали б
/// правдоподібні числа.
fn hand_made(values: &[f32], samples: usize, lines: usize) -> Vec<u8> {
    // Запис навмисно **не** дорівнює рядку картинки, хоч у справжнього
    // продукту він дорівнює: тоді зсув до пікселів справді рахується з двох
    // полів етикетки, а не збігається з чимось, що читач і так знає.
    let record = 1024;
    let label = format!(
        "PDS_VERSION_ID = PDS3\r\n\
         RECORD_TYPE   = FIXED_LENGTH\r\n\
         RECORD_BYTES  = {record}\r\n\
         LABEL_RECORDS = 1\r\n\
         ^IMAGE        = 2\r\n\
         OBJECT = IMAGE_MAP_PROJECTION\r\n\
         MAP_PROJECTION_TYPE = EQUIRECTANGULAR\r\n\
         MAP_RESOLUTION = {res} <PIX/DEG>\r\n\
         MAP_SCALE = 1.0 <METERS/PIXEL>\r\n\
         END_OBJECT = IMAGE_MAP_PROJECTION\r\n\
         OBJECT = IMAGE\r\n\
         LINES = {lines}\r\n\
         LINE_SAMPLES = {samples}\r\n\
         SAMPLE_TYPE = PC_REAL\r\n\
         SAMPLE_BITS = 32\r\n\
         BANDS = 1\r\n\
         END_OBJECT = IMAGE\r\n\
         END\r\n",
        res = samples as f64 / 360.0,
    );
    assert!(label.len() <= record, "етикетка не влізла в запис");

    let mut bytes = label.into_bytes();
    bytes.resize(record, 0);
    for v in values {
        bytes.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    bytes
}

fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, bytes).expect("тимчасовий файл мав записатися");
    path
}

#[test]
fn a_hand_made_mosaic_reads_back_exactly() {
    let (samples, lines) = (8usize, 4usize);
    // Значення = номер рядка: тоді вибірка по широті мусить дати саму широту,
    // а вибірка по довготі — не зрушити нічого.
    let values: Vec<f32> = (0..lines)
        .flat_map(|line| (0..samples).map(move |_| line as f32))
        .collect();
    let path = write_temp(
        "space_sim_wac_rows.img",
        &hand_made(&values, samples, lines),
    );
    let map = Albedo::read(&path).expect("рукотворна мозаїка мала прочитатися");
    std::fs::remove_file(&path).ok();

    assert_eq!((map.samples, map.lines), (samples, lines));
    assert_eq!(map.per_degree, samples as f64 / 360.0);
    assert_eq!(map.measured(), (0.0, (lines - 1) as f32));

    // Центри рядків: широта центра рядка `l` — це `90 − (l + 0.5)/per_degree`
    // градусів, і там вибірка мусить дати рівно `l`, без інтерполяції.
    let degrees = std::f64::consts::PI / 180.0;
    for line in 0..lines {
        let lat = (90.0 - (line as f64 + 0.5) / map.per_degree) * degrees;
        let got = map.sample(lat, 0.0);
        assert!(
            (got - line as f64).abs() < 1e-9,
            "центр рядка {line} дав {got}, а мусив дати {line}"
        );
    }

    // Рівно між центрами двох рядків — половина. Це і є перевірка ваги: зсув
    // на півклітинки зробив би тут ціле число.
    let between = (90.0 - 1.0 / map.per_degree) * degrees;
    let got = map.sample(between, 0.0);
    assert!(
        (got - 0.5).abs() < 1e-9,
        "між центрами рядків 0 і 1 вибірка дала {got}, а мусила 0.5"
    );

    // Довгота загортається, широта затискається — обидва краї сітки.
    assert_eq!(map.at(0, samples as i64), map.at(0, 0));
    assert_eq!(map.at(-1, 0), map.at(0, 0));
    assert_eq!(map.at(lines as i64, 0), map.at(lines as i64 - 1, 0));
}

/// Спеціальне значення PDS3 зупиняє читання, а не їде далі числом.
///
/// Перевірка існує тому, що мовчазний шлях тут виглядав би нормально:
/// −3.4·10³⁸ у білінійній вибірці дає чорну пляму правильної форми, і жоден
/// інший оракул про неї не спитає.
#[test]
fn a_special_value_stops_the_reader() {
    let (samples, lines) = (8usize, 4usize);
    let mut values = vec![0.5f32; samples * lines];
    values[13] = f32::from_bits(0xFF7F_FFFB);
    let path = write_temp(
        "space_sim_wac_null.img",
        &hand_made(&values, samples, lines),
    );
    let result = Albedo::read(&path);
    std::fs::remove_file(&path).ok();

    let message = result.expect_err("читач мусив відмовитись").to_string();
    assert!(
        message.contains("1 спеціальних значень"),
        "не те повідомлення: {message}"
    );
}

/// Маріа темніші за материки — і саме там, де вони справді є.
///
/// Оракул, який питає про **орієнтацію**: карта, перевернута по широті, має ті
/// самі розміри, той самий діапазон і ту саму етикетку. Числа — середні по
/// квадрату 5°×5°, а не окремі пікселі: мозаїка знята при великих кутах
/// падіння, тож окремий піксель у тіні кратера темніший за будь-яке море.
///
/// ⚠ **Знака довготи це твердження не ловить, і це виміряно, а не здогад.**
/// Перевернутий знак пройшов усі чотири перевірки цього файлу, бо моря
/// видимого боку розкидані майже симетрично щодо нульового меридіана: дзеркало
/// відображає море в море, а зворотний бік — сам у себе. Для знака є окрема
/// перевірка нижче.
#[test]
fn the_maria_are_darker_than_the_highlands() {
    let Some(map) = mosaic() else {
        return;
    };

    let maria = [
        ("Ясності", 28.0, 17.5),
        ("Дощів", 35.0, 345.0),
        ("Океан Бур", 18.0, 303.0),
        ("Спокою", 8.0, 31.0),
        ("Криз", 17.0, 59.0),
    ];
    let highlands = [
        ("південніше Птолемея", -20.0, 355.0),
        ("зворотний бік, −10°", -10.0, 180.0),
        ("зворотний бік, +10°", 10.0, 200.0),
        ("зворотний бік, −25°", -25.0, 150.0),
    ];

    let mut darkest_highland = f64::MAX;
    let mut brightest_mare = f64::MIN;
    for (name, lat, lon) in maria {
        let value = box_mean(&map, lat, lon);
        println!("  море {name}: {value:.4}");
        brightest_mare = brightest_mare.max(value);
    }
    for (name, lat, lon) in highlands {
        let value = box_mean(&map, lat, lon);
        println!("  материк {name}: {value:.4}");
        darkest_highland = darkest_highland.min(value);
    }

    // Розрив, а не просто нерівність: виміряно 0.0267 проти 0.0456, тобто
    // в 1.7 раза. Множник 1.3 лишає запас на вибір точок і водночас падає
    // від будь-якого перевороту карти — там числа міняються місцями.
    assert!(
        darkest_highland > 1.3 * brightest_mare,
        "найсвітліше море {brightest_mare:.4} і найтемніший материк \
         {darkest_highland:.4} не розділені — карта лежить не тим боком"
    );
}

/// Схід — це схід: дзеркало по довготі ламає карту, і ось точка, яка це бачить.
///
/// Ця перевірка з'явилася тому, що попередня знака довготи **не ловила**, і це
/// було виміряно: перевернутий знак пройшов усі чотири тести. Причина —
/// симетрія самого Місяця, а не слабкість оракула: моря видимого боку лежать
/// майже симетрично щодо нульового меридіана, зворотний бік дзеркалиться сам у
/// себе, тож пари «море проти материка» дзеркало переставляє одна в одну.
///
/// Розрізняє їх пара, у якої дзеркальні точки належать до **різних** класів, і
/// знайдена вона перебором по всій карті, а не з голови. Найкраща виявилася
/// пам'ятною: **Море Спокою (10° пн., 20° сх.)** і його дзеркало —
/// **Коперник (10° пн., 20° зх.)**, кратер зі світлою системою променів.
/// Виміряно: 0.0207 проти 0.0466, тобто перевернутий знак поміняв би темне зі
/// світлим удвічі.
#[test]
fn east_is_east_and_the_mirror_of_a_mare_is_a_bright_crater() {
    let Some(map) = mosaic() else {
        return;
    };

    let mare = box_mean(&map, 10.0, 20.0);
    let crater = box_mean(&map, 10.0, -20.0);
    println!("  Море Спокою (20° сх.): {mare:.4}; Коперник (20° зх.): {crater:.4}");

    assert!(
        crater > 1.5 * mare,
        "20° східної ({mare:.4}) і 20° західної ({crater:.4}) не розділені — \
         знак довготи або нульовий меридіан не той"
    );
}
