//! The D1 check: cargo computes the same bits as make.
//!
//! This is the whole point of D1. Moving the C build into `build.rs` is not
//! hard; noticing that the numbers changed afterwards is. A flag divergence
//! neither fails nor warns -- it quietly gives different last bits, and
//! surfaces only when something does not add up and the cause is sought in the
//! physics.
//!
//! So the comparison is against `core/scenario/golden.txt`, the same committed
//! golden file `make determinism` and the four CI jobs use. Not against
//! "make's output computed just now": there is one golden file for all,
//! otherwise the check would compare two builds with each other and both could
//! be wrong.
//!
//! The scenarios run from the repository root, because they read
//! `data/fixture/`.

use std::path::Path;
use std::process::Command;

const CFLAGS: &str = env!("CORE_CFLAGS");
const SCENARIO_DIR: &str = env!("CORE_SCENARIO_DIR");
const REPO_ROOT: &str = env!("CORE_REPO_ROOT");

/// Runs every scenario in name order and collects their output -- exactly what
/// the `$(ACTUAL)` target in `Makefile` does.
fn run_scenarios() -> String {
    let dir = Path::new(SCENARIO_DIR);

    let mut binaries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("missing {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.is_file())
        .collect();
    binaries.sort();

    assert!(
        !binaries.is_empty(),
        "no scenarios in {} -- an empty check silently 'passes', so this is \
         a failure",
        dir.display()
    );

    let mut output = String::new();

    for binary in binaries {
        let result = Command::new(&binary)
            .current_dir(REPO_ROOT)
            .output()
            .unwrap_or_else(|e| panic!("cannot run {}: {e}", binary.display()));

        assert!(
            result.status.success(),
            "{} exited with {}:\n{}",
            binary.display(),
            result.status,
            String::from_utf8_lossy(&result.stderr)
        );

        output.push_str(&String::from_utf8_lossy(&result.stdout));
    }

    output
}

#[test]
fn hashes_match_the_committed_golden() {
    let golden_path = Path::new(REPO_ROOT).join("core/scenario/golden.txt");
    let golden = std::fs::read_to_string(&golden_path)
        .unwrap_or_else(|e| panic!("missing {}: {e}", golden_path.display()));

    let actual = run_scenarios();

    if actual == golden {
        return;
    }

    // A divergence is exactly the case where the test message decides how long
    // the search takes. The ROADMAP D1 fork starts with the flags, so they are
    // right here next to the difference rather than in another log.
    let mut report = String::new();
    report.push_str("cargo hashes did NOT match core/scenario/golden.txt\n\n");
    report.push_str(&format!("cargo flags:   {CFLAGS}\n"));
    report.push_str("make flags:    make flags\n");
    report.push_str("full command:  CC_ENABLE_DEBUG_OUTPUT=1 cargo build -vv\n\n");

    let expected: Vec<&str> = golden.lines().collect();
    let got: Vec<&str> = actual.lines().collect();

    for i in 0..expected.len().max(got.len()) {
        let a = expected.get(i).copied().unwrap_or("<no such line>");
        let b = got.get(i).copied().unwrap_or("<no such line>");
        let mark = if a == b { "  " } else { "->" };
        report.push_str(&format!("{mark} golden: {a}\n{mark} cargo:  {b}\n"));
    }

    report.push_str(
        "\nIf one scenario differs, bisect it (ROADMAP C5).\n\
         If all of them do, it is almost certainly the flags.\n",
    );

    panic!("{report}");
}

/// The same two claims `Makefile` guards, on the cargo side.
///
/// The duplication is deliberate: the check in `build.rs` catches a broken
/// file read, while this one catches the case where `build.rs` is rewritten
/// someday and that guarantee quietly disappears. It costs a microsecond and
/// rests on a CLAUDE.md invariant rather than on discipline.
#[test]
fn flags_carry_the_determinism_guarantees() {
    let flags: Vec<&str> = CFLAGS.split_whitespace().collect();

    assert!(
        flags.contains(&"-ffp-contract=off"),
        "without -ffp-contract=off the compiler fuses multiply and add into \
         FMA: the same code gives different bits on different platforms \
         (PROJECT.md §4).\nflags: {CFLAGS}"
    );

    for forbidden in ["-ffast-math", "-Ofast", "-funsafe-math-optimizations"] {
        assert!(
            !flags.contains(&forbidden),
            "{forbidden} in the core flags. Never, under any circumstances \
             -- CLAUDE.md, invariant 2.\nflags: {CFLAGS}"
        );
    }

    assert!(
        !flags.contains(&"-O0"),
        "at -O0 gcc calls sqrt as a libm function, while the scenarios \
         deliberately link without -lm and fail to link. For debugging use \
         -O1 -g: the result is bit-identical (ROADMAP C5).\nflags: {CFLAGS}"
    );
}
