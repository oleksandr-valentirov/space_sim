//! Перевірка кроку D1: cargo рахує ті самі біти, що й make.
//!
//! Це і є весь сенс D1. Перенести збірку C у `build.rs` неважко; важко
//! помітити, що після переносу числа змінилися. Розбіжність у прапорцях не
//! падає й не попереджає — вона тихо дає інші останні біти, і виявляється аж
//! тоді, коли щось не сходиться, а причину шукають у фізиці.
//!
//! Тому звірка йде з `core/scenario/golden.txt` — тим самим закоміченим
//! еталоном, з яким звіряється `make determinism` і чотири джоби CI. Не з
//! «виводом make, порахованим щойно»: еталон один на всіх, інакше перевірка
//! порівнювала б дві збірки одна з одною й обидві могли б бути неправильні.
//!
//! Сценарії запускаються з кореня репозиторію, бо читають `data/fixture/`.

use std::path::Path;
use std::process::Command;

const CFLAGS: &str = env!("CORE_CFLAGS");
const SCENARIO_DIR: &str = env!("CORE_SCENARIO_DIR");
const REPO_ROOT: &str = env!("CORE_REPO_ROOT");

/// Запускає всі сценарії в порядку імен і збирає їхній вивід — рівно те, що
/// робить ціль `$(ACTUAL)` у `Makefile`.
fn run_scenarios() -> String {
    let dir = Path::new(SCENARIO_DIR);

    let mut binaries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("немає {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.is_file())
        .collect();
    binaries.sort();

    assert!(
        !binaries.is_empty(),
        "у {} немає сценаріїв — порожня перевірка мовчки 'проходить', \
         тому це провал",
        dir.display()
    );

    let mut output = String::new();

    for binary in binaries {
        let result = Command::new(&binary)
            .current_dir(REPO_ROOT)
            .output()
            .unwrap_or_else(|e| panic!("не запускається {}: {e}", binary.display()));

        assert!(
            result.status.success(),
            "{} завершився з {}:\n{}",
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
        .unwrap_or_else(|e| panic!("немає {}: {e}", golden_path.display()));

    let actual = run_scenarios();

    if actual == golden {
        return;
    }

    // Розбіжність — це саме той випадок, коли повідомлення тесту вирішує,
    // скільки триватиме пошук. Розвилка з ROADMAP D1 починається з прапорців,
    // тож вони тут же, поруч із різницею, а не в іншому логі.
    let mut report = String::new();
    report.push_str("хеші cargo НЕ збіглися з core/scenario/golden.txt\n\n");
    report.push_str(&format!("прапорці cargo:  {CFLAGS}\n"));
    report.push_str("прапорці make:   make flags\n");
    report.push_str("повний виклик:   CC_ENABLE_DEBUG_OUTPUT=1 cargo build -vv\n\n");

    let expected: Vec<&str> = golden.lines().collect();
    let got: Vec<&str> = actual.lines().collect();

    for i in 0..expected.len().max(got.len()) {
        let a = expected.get(i).copied().unwrap_or("<немає рядка>");
        let b = got.get(i).copied().unwrap_or("<немає рядка>");
        let mark = if a == b { "  " } else { "->" };
        report.push_str(&format!("{mark} еталон: {a}\n{mark} cargo:  {b}\n"));
    }

    report.push_str(
        "\nЯкщо різниця в одному сценарії — ділити його навпіл (ROADMAP C5).\n\
         Якщо в усіх — майже напевно прапорці.\n",
    );

    panic!("{report}");
}

/// Ті самі два твердження, що охороняє `Makefile`, але на боці cargo.
///
/// Дублювання свідоме: перевірка в `build.rs` ловить зламане читання файлу,
/// а ця — випадок, коли `build.rs` колись перепишуть і ця гарантія тихо
/// зникне. Вона коштує мікросекунду й тримає інваріант із CLAUDE.md, а не
/// дисципліну.
#[test]
fn flags_carry_the_determinism_guarantees() {
    let flags: Vec<&str> = CFLAGS.split_whitespace().collect();

    assert!(
        flags.contains(&"-ffp-contract=off"),
        "без -ffp-contract=off компілятор зливає множення й додавання у FMA: \
         той самий код дає різні біти на різних платформах (PROJECT.md §4).\n\
         прапорці: {CFLAGS}"
    );

    for forbidden in ["-ffast-math", "-Ofast", "-funsafe-math-optimizations"] {
        assert!(
            !flags.contains(&forbidden),
            "{forbidden} у прапорцях ядра. Ніколи, за жодних обставин — \
             CLAUDE.md, інваріант 2.\nпрапорці: {CFLAGS}"
        );
    }

    assert!(
        !flags.contains(&"-O0"),
        "при -O0 gcc кличе sqrt як функцію libm, а сценарії свідомо \
         лінкуються без -lm і не злінкуються. Для дебагу беріть -O1 -g: \
         результат бітово той самий (ROADMAP C5).\nпрапорці: {CFLAGS}"
    );
}
