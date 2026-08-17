//! Star cooker: command line (ROADMAP, stage Z, Z2).
//!
//!     cargo run -p star-cook              data/bsc5/catalog → assets/stars.cat
//!     cargo run -p star-cook -- --source X --out Y
//!
//! The source is not in git and never will be (Q5, 2026-08-16); fetching it is
//! debt D18. Until that debt is paid the file is put there by hand, and the
//! error below says where from.

use std::path::PathBuf;

const SOURCE: &str = "data/bsc5/catalog";
const OUT: &str = "assets/stars.cat";

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

    let source = source.unwrap_or_else(|| PathBuf::from(SOURCE));
    let out = out.unwrap_or_else(|| PathBuf::from(OUT));

    let bytes = match std::fs::read(&source) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("{}: {e}", source.display());
            eprintln!("the Yale Bright Star Catalogue is not in git (Q5). To fetch it:");
            eprintln!("  mkdir -p data/bsc5 && curl -sSL -o data/bsc5/catalog.gz \\");
            eprintln!("    https://cdsarc.cds.unistra.fr/ftp/V/50/catalog.gz");
            eprintln!("  gunzip -c data/bsc5/catalog.gz > data/bsc5/catalog");
            std::process::exit(1);
        }
    };

    // Lossy rather than strict: the table is ASCII by its own specification,
    // and a stray byte in a spectral type is not a reason to refuse a sky. The
    // fields this cooker reads are digits and signs, and those do not survive
    // as replacement characters -- a damaged one fails to parse and drops its
    // star, loudly enough in the count below.
    let text = String::from_utf8_lossy(&bytes);

    let cooked = match star_cook::cook(&text) {
        Ok(cooked) => cooked,
        Err(message) => {
            eprintln!("{}: {message}", source.display());
            std::process::exit(1);
        }
    };

    let lines = text.lines().count();
    let brightest = cooked
        .stars
        .iter()
        .fold(f32::INFINITY, |best, star| best.min(star.magnitude));
    let faintest = cooked
        .stars
        .iter()
        .fold(f32::NEG_INFINITY, |worst, star| worst.max(star.magnitude));

    if let Some(dir) = out.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("{}: {e}", dir.display());
            std::process::exit(1);
        }
    }
    let bytes = cooked.to_bytes();
    if let Err(e) = std::fs::write(&out, &bytes) {
        eprintln!("{}: {e}", out.display());
        std::process::exit(1);
    }

    // The skipped count is printed rather than hidden: the catalogue really
    // does carry withdrawn entries, and a reader who does not know that would
    // read the difference as a parsing failure.
    println!(
        "{}: {} stars from {lines} lines ({} skipped), magnitudes {brightest:.2} to {faintest:.2}, {} KiB",
        out.display(),
        cooked.stars.len(),
        lines - cooked.stars.len(),
        bytes.len() / 1024,
    );
}
