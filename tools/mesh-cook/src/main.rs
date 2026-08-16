//! Кукер мешів: командний рядок (ROADMAP, T5d2).
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
            "--source" => source = Some(args.next().expect("--source хоче шлях").into()),
            "--out" => out = Some(args.next().expect("--out хоче шлях").into()),
            other => {
                eprintln!("невідомий аргумент {other}");
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
        std::fs::create_dir_all(folder).expect("каталог виходу");
    }
    std::fs::write(&out, cooked.model.to_bytes()).expect("запис ассета");

    println!("{} → {}", source.display(), out.display());
    println!("  вершин: {}", cooked.model.mesh.positions.len());
    println!("  трикутників: {}", cooked.model.mesh.indices.len() / 3);
    println!("  індекси: componentType {}", cooked.index_component);
    println!("  довжина: {:.6} м", cooked.model.height_m);
    println!("  extent: {:.6} висоти", cooked.model.extent);
    println!("  об'єм: {:.6} м³", cooked.volume_m3);
}
