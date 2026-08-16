//! Build of the C numeric core through the `cc` crate (ROADMAP D1).
//!
//! This closes the debt recorded back in A1: the same `.c` files are now built
//! by both `Makefile` and `cargo`, and the flags must match **bitwise**. A
//! divergence here neither fails nor warns -- it quietly changes the numbers,
//! and is caught at the hash comparison about a week later (PROJECT.md
//! section 4).
//!
//! So the flags are set neither here nor in `Makefile`: both read
//! `core/cflags.txt`. That is a single source of truth, not a convenience.
//!
//! ## What is built
//!
//! 1. `core/*.c` -> `libcore.a` in `OUT_DIR`, linked into the crate. Runtime
//!    zone: no `libm`, only `+ - * /` and `sqrt` allowed.
//! 2. `core/scenario/*.c` -> executables, also in `OUT_DIR`. They are the D1
//!    check: `tests/determinism.rs` runs them and compares the output against
//!    `core/scenario/golden.txt` -- the same golden file `make determinism`
//!    uses. Agreement means cargo produced the same bits.
//! 3. `core/planning/*.c` -> a **separate** `libcore_planning.a` (ROADMAP L3).
//!
//! The scenarios link against the **same** object files that go into Rust
//! rather than being rebuilt separately. Otherwise the check would cover the
//! flags, not the library the crate actually uses.
//!
//! ## Why planning is its own library
//!
//! `core/planning/` calls `libm` freely and deliberately: the determinism
//! boundary runs along propagation, not planning (PROJECT.md section 4), which
//! is why `Makefile` keeps it in a separate `libcore_planning.a`. Same here,
//! and not for symmetry with `Makefile`.
//!
//! The reason is concrete. The determinism scenarios and the oracle link
//! against `libcore.a` **without `-lm`**, and that is a check, not decoration:
//! if trigonometry ever seeps into the runtime zone, the link fails here and
//! now. Throwing `lambert.c` into the same archive would bring `acos`, `sinh`
//! and `cosh` into it -- and although GNU ld extracts only the object files it
//! needs, so it would still link, the claim "this archive contains no libm"
//! would stop being true. A check that no longer forbids anything is worse
//! than no check.
//!
//! Second consequence of the same: the planning oracle is a **separate binary
//! with `-lm`**. The existing `oracle.c` links without libm on purpose, and
//! merging the two would lose exactly the claim it exists to make.
//!
//! ## Why `no_default_flags`
//!
//! By default `cc` adds its own `-O`, `-g` and `-W` per the cargo profile. In
//! debug that is `-O0` -- and `core/cflags.txt` says outright that at `-O0`
//! gcc calls `sqrt` as a `libm` function, so the scenarios, which deliberately
//! link without `-lm`, fail to link.
//!
//! **Measured, cc 1.4.2:** `no_default_flags(true)` does not remove
//! everything. The actual invocation is `cc -I core -Wall -Wextra <our flags>
//! -fPIC -o ... -c ...` -- the crate inserts `-Wall -Wextra` anyway, and
//! inserts them BEFORE ours. That does not affect the numbers (they are in
//! `cflags.txt` already), but the wider conclusion is: `no_default_flags` does
//! not give full control of the command line and cannot be relied on as a
//! guarantee.
//!
//! The guarantee remains `tests/determinism.rs`: any flag `cc` ever inserts
//! that changes the arithmetic will shift the hashes and be caught there.
//! Flags are the first place to look; hashes are what is actually checked.
//!
//! See the real invocation with: `CC_ENABLE_DEBUG_OUTPUT=1 cargo build -vv`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest
        .parent()
        .expect("core-sys must live inside the repository");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let core_dir = root.join("core");
    let flags = read_flags(&core_dir.join("cflags.txt"));

    let compiler = build_library(&core_dir, &flags);
    let scenarios = build_scenarios(&compiler, &core_dir, &out_dir, &flags);
    build_planning(&core_dir, &flags);

    // Oracle for tests/ffi.rs (ROADMAP D2). It lives in the crate rather than
    // in core/scenario/, where it would change golden.txt: this is scaffolding
    // for the boundary, not a determinism scenario.
    //
    // Without `-lm`, like the scenarios: see the comment at the top.
    let oracle = link(
        &compiler,
        &flags,
        std::slice::from_ref(&core_dir),
        &manifest.join("oracle.c"),
        &out_dir.join(format!("oracle{}", exe_suffix())),
        &[out_dir.join("libcore.a")],
        &[],
    );

    // The planning oracle (ROADMAP L3) is a second binary precisely because it
    // needs `-lm`. The first does not have it and will not get it.
    let oracle_planning = link(
        &compiler,
        &flags,
        &[core_dir.clone(), core_dir.join("planning")],
        &manifest.join("oracle_planning.c"),
        &out_dir.join(format!("oracle_planning{}", exe_suffix())),
        &[
            out_dir.join("libcore_planning.a"),
            out_dir.join("libcore.a"),
        ],
        &["-lm"],
    );

    watch(&core_dir);
    println!("cargo:rerun-if-changed=oracle.c");
    println!("cargo:rerun-if-changed=oracle_planning.c");

    // Planning calls libm, and the crate linking it must have it. On glibc
    // 2.34+ it is merged into libc and this line changes nothing; on older
    // glibc and on musl it does. windows-gnu has the maths in its CRT.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        println!("cargo:rustc-link-lib=m");
    }

    // The test should not guess where things are, nor hold a second copy of
    // the flags.
    println!("cargo:rustc-env=CORE_CFLAGS={}", flags.join(" "));
    println!("cargo:rustc-env=CORE_SCENARIO_DIR={}", scenarios.display());
    println!("cargo:rustc-env=CORE_ORACLE={}", oracle.display());
    println!(
        "cargo:rustc-env=CORE_ORACLE_PLANNING={}",
        oracle_planning.display()
    );
    println!("cargo:rustc-env=CORE_REPO_ROOT={}", root.display());
}

/// Reads `core/cflags.txt` exactly as `Makefile` does: strip comments, join
/// the rest.
///
/// The two checks below repeat those in `Makefile` verbatim, and deliberately.
/// They catch different things: an empty list means a broken file read, a
/// missing `-ffp-contract=off` means an edit to the file itself that dropped
/// the flag without thinking. Without this check both would look identical:
/// the build would quietly proceed with the compiler's default flags and the
/// hash comparison would fail a mile from the cause.
fn read_flags(path: &Path) -> Vec<String> {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

    let flags: Vec<String> = text
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .flat_map(|line| line.split_whitespace())
        .map(str::to_string)
        .collect();

    if flags.is_empty() {
        panic!(
            "could not extract flags from {}. Building with the compiler's \
             default flags would break determinism, so this is an error, not \
             a warning.",
            path.display()
        );
    }

    if !flags.iter().any(|f| f == "-ffp-contract=off") {
        panic!(
            "flags do not contain -ffp-contract=off. Without it the compiler \
             fuses multiply and add into FMA, and the same code gives \
             different bits on different platforms -- PROJECT.md section 4."
        );
    }

    flags
}

/// `core/*.c` -> `libcore.a`. Returns the compiler used, so the scenarios are
/// built by the same one rather than whatever `cc` picks a second time.
fn build_library(core_dir: &Path, flags: &[String]) -> cc::Tool {
    let mut build = cc::Build::new();
    build.no_default_flags(true);

    for flag in flags {
        build.flag(flag);
    }

    // The only flag outside cflags.txt, and it cannot live there: that file is
    // a flat list while this one depends on the platform. Rust executables on
    // Linux are PIE by default, so objects without -fPIC will not link.
    //
    // Does not affect bitwise accuracy: -fPIC changes addressing, not
    // arithmetic. And that is not an assumption -- the scenarios below are
    // built from these very objects, and their hashes are compared against a
    // golden file produced by the `make` build WITHOUT -fPIC. So the test
    // checks this claim too, and at D1 it held: every hash matched.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        build.flag("-fPIC");
    }

    let tool = build.get_compiler();
    if tool.is_like_msvc() {
        panic!(
            "core/cflags.txt is written for gcc/clang, but this is MSVC. On \
             Windows use the x86_64-pc-windows-gnu toolchain -- that is what \
             the windows-mingw CI job builds with, through MSYS2. Matching \
             MSVC flags is a separate debt, ROADMAP A1."
        );
    }

    build.include(core_dir);
    for src in sources(core_dir) {
        build.file(src);
    }

    build.compile("core");
    tool
}

/// `core/planning/*.c` -> `libcore_planning.a` (ROADMAP L3).
///
/// The same flags from `cflags.txt` -- determinism is irrelevant here, but two
/// flag sets in one crate would be two sets someone eventually desynchronises.
/// There is exactly one difference from `libcore.a`, and it is in linking:
/// this archive calls `libm`, that one does not.
///
/// `-Icore/planning` is added only here: nobody in the runtime zone needs the
/// planning headers, and a path that gives nothing eventually gives a
/// surprise.
fn build_planning(core_dir: &Path, flags: &[String]) {
    let planning_dir = core_dir.join("planning");

    let mut build = cc::Build::new();
    build.no_default_flags(true);

    for flag in flags {
        build.flag(flag);
    }

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        build.flag("-fPIC");
    }

    build.include(core_dir);
    build.include(&planning_dir);
    for src in sources(&planning_dir) {
        build.file(src);
    }

    build.compile("core_planning");
}

/// `core/scenario/*.c` -> executables.
///
/// Without `libcore_offline.a` and without `-lm`, exactly as in `Makefile`:
/// linking here is itself the check that no trigonometry seeped into the
/// runtime. If it does, it fails right here rather than a week later on
/// another platform.
fn build_scenarios(tool: &cc::Tool, core_dir: &Path, out_dir: &Path, flags: &[String]) -> PathBuf {
    let scenario_dir = core_dir.join("scenario");
    let bin_dir = out_dir.join("scenario");
    fs::create_dir_all(&bin_dir).expect("cannot create the scenario directory");

    let lib = out_dir.join("libcore.a");
    let core_owned = core_dir.to_path_buf();

    for src in sources(&scenario_dir) {
        let stem = src.file_stem().unwrap().to_string_lossy().to_string();
        let exe = bin_dir.join(format!("{stem}{}", exe_suffix()));
        link(
            tool,
            flags,
            std::slice::from_ref(&core_owned),
            &src,
            &exe,
            std::slice::from_ref(&lib),
            &[],
        );
    }

    bin_dir
}

/// Links one C program against the already built archives.
///
/// **`extra` being empty is the point.** For the scenarios and for `oracle.c`
/// there is no `-lm`, so linking is itself the check that no trigonometry
/// seeped into the runtime zone: `sin` or `pow` simply find no symbol. A
/// cheaper and earlier check than the "libm police", and it holds itself up
/// without a separate script.
///
/// Exactly one caller passes a non-empty `extra` -- the planning oracle -- and
/// that is why it is a separate binary rather than another flag on the first.
fn link(
    tool: &cc::Tool,
    flags: &[String],
    includes: &[PathBuf],
    src: &Path,
    exe: &Path,
    libs: &[PathBuf],
    extra: &[&str],
) -> PathBuf {
    let mut cmd = Command::new(tool.path());
    cmd.args(flags);

    for dir in includes {
        cmd.arg("-I").arg(dir);
    }

    cmd.arg("-o").arg(exe).arg(src).args(libs).args(extra);

    let status = cmd
        .status()
        .unwrap_or_else(|e| panic!("cannot run {:?}: {e}", tool.path()));

    if !status.success() {
        panic!("{} failed to build: {cmd:?}", src.display());
    }

    exe.to_path_buf()
}

fn exe_suffix() -> &'static str {
    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("windows") => ".exe",
        _ => "",
    }
}

/// The directory's `.c` files, sorted.
///
/// Sorting matters: scenario order sets the order of the lines compared
/// against the golden file, and `read_dir` guarantees no stable order.
fn sources(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "c"))
        .collect();

    files.sort();
    files
}

/// Rebuild when any input changed. Headers too: `cc` does not track them, and
/// editing a `.h` without touching a `.c` is routine.
///
/// **Directories, not only files.** Found at ROADMAP G1: the `sc_uncertainty`
/// check passed silently against stale output as long as no already-tracked
/// file changed -- `cargo:rerun-if-changed` on individual files tells cargo to
/// rebuild when a FILE from that list changes, not when a new one is added to
/// the directory. The hole only opens where `target/` survives several commits
/// (a CI cache, or a dev session that never touched build.rs); a first `cargo
/// build` with an empty cache would pick the new file up at once. The line
/// below tells cargo to watch the directory itself, so its mtime changing
/// (a file added or removed) rebuilds as well.
fn watch(core_dir: &Path) {
    println!("cargo:rerun-if-changed=build.rs");
    println!(
        "cargo:rerun-if-changed={}",
        core_dir.join("cflags.txt").display()
    );

    for dir in [
        core_dir.to_path_buf(),
        core_dir.join("scenario"),
        core_dir.join("planning"),
    ] {
        println!("cargo:rerun-if-changed={}", dir.display());

        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for path in entries.filter_map(|e| e.ok().map(|e| e.path())) {
            if path.extension().is_some_and(|ext| ext == "c" || ext == "h") {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
}
