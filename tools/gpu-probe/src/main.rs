//! Розвідка P0: чи є bindless у wgpu на наших цілях (ROADMAP, етап E).
//!
//! Питання не академічне. PROJECT.md §7 забороняє «спочатку класично, потім
//! перепишу»: спосіб прив'язки ресурсів вирішує устрій усього рендера, і
//! міняти його потім — це переписати frame graph, кукер ассетів і шейдери
//! разом. Тому відповідь потрібна **до** того, як щось намальовано.
//!
//! Зонд нічого не малює й нічого не створює. Він перелічує адаптери й читає
//! те, що вони самі про себе кажуть: фічі й ліміти. Пристрій не запитується
//! свідомо — `adapter.features()` показує, що **можна** попросити, а це і є
//! питання. Створення пристрою додало б причин впасти, не додавши відповіді.
//!
//!     cargo run -p gpu-probe
//!
//! Пише таблицю в stdout і `build/csv/gpu_features.csv` — той самий шлях, що
//! й у експортерів ядра, щоб результат розвідки лежав поруч із рештою
//! виміряного.

use std::fs;
use std::io::Write;
use std::path::Path;

use wgpu::{Features, FeaturesWGPU};

const CSV_PATH: &str = "build/csv/gpu_features.csv";

/// Фічі, від яких залежить рішення. Не «усі, що є» — усі є в логах нижче, а
/// тут ті, без яких bindless не буде.
///
/// Що кожна означає для нас:
///
/// - масиви прив'язок — узагалі можливість дати шейдеру масив ресурсів
///   замість одного;
/// - неоднорідна індексація — індекс, порахований у шейдері, а не однаковий
///   для всієї хвилі. Без неї масив є, але користі з нього мало: індекс
///   мусить бути константою рівня draw call;
/// - часткова зв'язаність — дозвіл лишати дірки в масиві. Без неї весь масив
///   треба заповнювати щокадру, а це і є та ціна, заради уникнення якої
///   bindless беруть.
const NEEDED: &[(&str, FeaturesWGPU)] = &[
    ("масив текстур", FeaturesWGPU::TEXTURE_BINDING_ARRAY),
    ("масив буферів", FeaturesWGPU::BUFFER_BINDING_ARRAY),
    (
        "масив storage-ресурсів",
        FeaturesWGPU::STORAGE_RESOURCE_BINDING_ARRAY,
    ),
    (
        "неоднорідна індексація (текстури й storage-буфери)",
        FeaturesWGPU::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    ),
    (
        "неоднорідна індексація (storage-текстури)",
        FeaturesWGPU::STORAGE_TEXTURE_ARRAY_NON_UNIFORM_INDEXING,
    ),
    (
        "часткова зв'язаність",
        FeaturesWGPU::PARTIALLY_BOUND_BINDING_ARRAY,
    ),
];

struct Row {
    backend: String,
    name: String,
    device_type: String,
    driver: String,
    supported: Vec<bool>,
    max_elements: u32,
    max_samplers: u32,
}

fn main() {
    let instance = wgpu::Instance::default();
    let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));

    if adapters.is_empty() {
        eprintln!("жодного адаптера. Немає драйвера або немає доступу до GPU.");
        std::process::exit(1);
    }

    let mut rows = Vec::new();

    for adapter in &adapters {
        let info = adapter.get_info();
        let features = adapter.features();
        let limits = adapter.limits();

        rows.push(Row {
            backend: format!("{:?}", info.backend),
            name: info.name.clone(),
            device_type: format!("{:?}", info.device_type),
            driver: if info.driver_info.is_empty() {
                info.driver.clone()
            } else {
                format!("{} {}", info.driver, info.driver_info)
            },
            supported: NEEDED
                .iter()
                .map(|(_, flag)| features.contains(Features::from(*flag)))
                .collect(),
            max_elements: limits.max_binding_array_elements_per_shader_stage,
            max_samplers: limits.max_binding_array_sampler_elements_per_shader_stage,
        });
    }

    print_table(&rows);
    print_verdict(&rows);

    if let Err(e) = write_csv(&rows) {
        eprintln!("CSV не записався: {e}");
        std::process::exit(1);
    }
}

fn print_table(rows: &[Row]) {
    println!("Адаптери на цій машині\n");

    for row in rows {
        println!("{} — {} ({})", row.backend, row.name, row.device_type);
        println!("  драйвер: {}", row.driver);

        for ((label, _), &ok) in NEEDED.iter().zip(row.supported.iter()) {
            println!("  [{}] {}", if ok { "+" } else { " " }, label);
        }

        // Ліміт нуль означає, що масивів прив'язок немає взагалі, а не що
        // вони безрозмірні. Пишемо словом, бо «0» тут читається навпаки.
        println!("  елементів у масиві: {}", describe_limit(row.max_elements));
        println!("  семплерів у масиві: {}", describe_limit(row.max_samplers));
        println!();
    }
}

fn describe_limit(value: u32) -> String {
    if value == 0 {
        "0 (масивів прив'язок немає)".to_string()
    } else {
        value.to_string()
    }
}

/// Висновок, а не лише дані. Розвідка існує, щоб прийняти рішення, і
/// рішення має бути видно з виводу, а не виводитися щоразу заново.
fn print_verdict(rows: &[Row]) {
    let full: Vec<&Row> = rows
        .iter()
        .filter(|row| row.supported.iter().all(|&ok| ok))
        .collect();

    println!("Висновок\n");

    if full.is_empty() {
        println!("  Жоден адаптер не дає повного набору. Розвилка ROADMAP P0:");
        println!("  (а) звузити цілі, (б) тонка абстракція із запасним шляхом.");
        return;
    }

    println!("  Повний набір мають {} з {}:", full.len(), rows.len());
    for row in &full {
        println!("    {} — {}", row.backend, row.name);
    }

    let backends: Vec<&str> = rows
        .iter()
        .filter(|row| !row.supported.iter().all(|&ok| ok))
        .map(|row| row.backend.as_str())
        .collect();

    if !backends.is_empty() {
        println!();
        println!("  Не мають — на бекендах: {}", dedup(&backends).join(", "));
        println!("  Це і є та межа, всередині якої доведеться лишитися.");
    }
}

fn dedup(items: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in items {
        if !out.iter().any(|seen| seen == item) {
            out.push((*item).to_string());
        }
    }
    out
}

fn write_csv(rows: &[Row]) -> std::io::Result<()> {
    let path = Path::new(CSV_PATH);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = fs::File::create(path)?;

    write!(file, "backend,adapter,device_type,driver")?;
    for (label, _) in NEEDED {
        write!(file, ",{label}")?;
    }
    writeln!(
        file,
        ",max_binding_array_elements,max_binding_array_samplers"
    )?;

    for row in rows {
        // Кома в назві адаптера чи драйвера зсунула б усі наступні колонки.
        write!(
            file,
            "{},{},{},{}",
            row.backend,
            quote(&row.name),
            row.device_type,
            quote(&row.driver)
        )?;
        for &ok in &row.supported {
            write!(file, ",{}", u8::from(ok))?;
        }
        writeln!(file, ",{},{}", row.max_elements, row.max_samplers)?;
    }

    println!("CSV: {CSV_PATH}");
    Ok(())
}

fn quote(text: &str) -> String {
    if text.contains(',') || text.contains('"') {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}
