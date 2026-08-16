//! P0 reconnaissance: does wgpu have bindless on our targets (stage E).
//!
//! The question is not academic. PROJECT.md §7 forbids "classic first, rewrite
//! later": how resources are bound decides the structure of the whole
//! renderer, and changing it afterwards means rewriting the frame graph, the
//! asset cooker and the shaders together. So the answer is needed **before**
//! anything is drawn.
//!
//! The probe draws nothing and creates nothing. It enumerates adapters and
//! reads what they say about themselves: features and limits. No device is
//! requested, deliberately -- `adapter.features()` shows what **can** be
//! asked for, and that is the question. Creating a device would add reasons to
//! fail without adding an answer.
//!
//!     cargo run -p gpu-probe
//!
//! Writes a table to stdout and to `build/csv/gpu_features.csv` -- the same
//! path the core's exporters use, so the reconnaissance result sits beside the
//! rest of what has been measured.

use std::fs;
use std::io::Write;
use std::path::Path;

use wgpu::{Features, FeaturesWGPU};

const CSV_PATH: &str = "build/csv/gpu_features.csv";

/// The features the decision depends on. Not "all of them" -- all of them are
/// in the logs below; these are the ones without which there is no bindless.
///
/// What each means for us:
///
/// - binding arrays -- the possibility of handing the shader an array of
///   resources instead of one;
/// - non-uniform indexing -- an index computed in the shader rather than the
///   same across the whole wave. Without it the array exists but is of little
///   use: the index must be a draw-call-level constant;
/// - partial binding -- permission to leave holes in the array. Without it the
///   whole array must be filled every frame, which is the very cost bindless
///   is taken to avoid.
const NEEDED: &[(&str, FeaturesWGPU)] = &[
    ("texture array", FeaturesWGPU::TEXTURE_BINDING_ARRAY),
    ("buffer array", FeaturesWGPU::BUFFER_BINDING_ARRAY),
    (
        "storage resource array",
        FeaturesWGPU::STORAGE_RESOURCE_BINDING_ARRAY,
    ),
    (
        "non-uniform indexing (textures and storage buffers)",
        FeaturesWGPU::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    ),
    (
        "non-uniform indexing (storage textures)",
        FeaturesWGPU::STORAGE_TEXTURE_ARRAY_NON_UNIFORM_INDEXING,
    ),
    (
        "partial binding",
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
        eprintln!("no adapters. No driver, or no access to a GPU.");
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
        eprintln!("CSV was not written: {e}");
        std::process::exit(1);
    }
}

fn print_table(rows: &[Row]) {
    println!("Adapters on this machine\n");

    for row in rows {
        println!("{} — {} ({})", row.backend, row.name, row.device_type);
        println!("  driver: {}", row.driver);

        for ((label, _), &ok) in NEEDED.iter().zip(row.supported.iter()) {
            println!("  [{}] {}", if ok { "+" } else { " " }, label);
        }

        // A limit of zero means there are no binding arrays at all, not that
        // they are unbounded. Spelled out in words, because "0" reads the
        // other way round here.
        println!("  elements in array: {}", describe_limit(row.max_elements));
        println!("  samplers in array: {}", describe_limit(row.max_samplers));
        println!();
    }
}

fn describe_limit(value: u32) -> String {
    if value == 0 {
        "0 (no binding arrays)".to_string()
    } else {
        value.to_string()
    }
}

/// A conclusion, not only data. Reconnaissance exists to make a decision, and
/// the decision must be visible in the output rather than re-derived each
/// time.
fn print_verdict(rows: &[Row]) {
    let full: Vec<&Row> = rows
        .iter()
        .filter(|row| row.supported.iter().all(|&ok| ok))
        .collect();

    println!("Conclusion\n");

    if full.is_empty() {
        println!("  No adapter offers the full set. The ROADMAP P0 fork:");
        println!("  (a) narrow the targets, (b) a thin abstraction with a fallback.");
        return;
    }

    println!("  {} of {} have the full set:", full.len(), rows.len());
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
        println!("  Missing on backends: {}", dedup(&backends).join(", "));
        println!("  That is the boundary we will have to stay inside.");
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
        // A comma in an adapter or driver name would shift every later
        // column.
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
