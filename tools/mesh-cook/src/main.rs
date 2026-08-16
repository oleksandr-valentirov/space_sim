//! Mesh cooker: command line (ROADMAP, T5d2).
//!
//!     cargo run -p mesh-cook              assets-src/ship.gltf → assets/ship.mesh
//!     cargo run -p mesh-cook -- --source X --out Y

use std::path::PathBuf;

fn main() {
    let mut source: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--source" => source = Some(args.next().expect("--source wants a path").into()),
            "--out" => out = Some(args.next().expect("--out wants a path").into()),
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
    }

    let source = source.unwrap_or_else(|| PathBuf::from("assets-src/ship.gltf"));
    let out = out.unwrap_or_else(|| PathBuf::from("assets/ship.mesh"));

    let cooked = match mesh_cook::cook(&source) {
        Ok(cooked) => cooked,
        Err(message) => {
            eprintln!("{}: {message}", source.display());
            std::process::exit(1);
        }
    };

    if let Some(folder) = out.parent() {
        std::fs::create_dir_all(folder).expect("output directory");
    }
    std::fs::write(&out, cooked.model.to_bytes()).expect("writing the asset");

    println!("{} → {}", source.display(), out.display());
    println!("  vertices: {}", cooked.model.mesh.positions.len());
    println!("  triangles: {}", cooked.model.mesh.indices.len() / 3);
    println!("  indices: componentType {}", cooked.index_component);
    let mut palette: Vec<[u32; 3]> = cooked
        .model
        .paint
        .iter()
        .map(|c| c.map(f32::to_bits))
        .collect();
    palette.sort_unstable();
    palette.dedup();
    println!("  paint: {} colours in the palette", palette.len());
    println!("  length: {:.6} m", cooked.model.height_m);
    println!("  extent: {:.6} of height", cooked.model.extent);
    println!("  volume: {:.6} m^3", cooked.volume_m3);
}
