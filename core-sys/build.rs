//! Збірка числового ядра на C через крейт `cc` (ROADMAP D1).
//!
//! Це закриває борг, записаний ще в A1: ті самі `.c` файли тепер збирає і
//! `Makefile`, і `cargo`, і прапорці мусять збігатися **бітово**. Розбіжність
//! тут не падає й не попереджає — вона тихо змінює числа, і ловиться аж на
//! звірці хешів десь через тиждень (PROJECT.md §4).
//!
//! Тому прапорці не задані ні тут, ні в `Makefile`: обидва читають
//! `core/cflags.txt`. Це не зручність, а єдине джерело істини.
//!
//! ## Що саме збирається
//!
//! 1. `core/*.c` → `libcore.a` у `OUT_DIR`, лінкується в крейт. Рантаймова
//!    зона: без `libm`, дозволені лише `+ - * /` і `sqrt`.
//! 2. `core/scenario/*.c` → виконувані файли, теж у `OUT_DIR`. Вони і є
//!    перевіркою D1: `tests/determinism.rs` запускає їх і звіряє вивід із
//!    `core/scenario/golden.txt` — тим самим еталоном, з яким звіряється
//!    `make determinism`. Збіг означає, що cargo дав ті самі біти.
//!
//! Сценарії лінкуються з **тими самими** об'єктними файлами, що підуть у
//! Rust, а не перезбираються окремо. Інакше перевірялися б прапорці, а не
//! бібліотека, яку насправді використає крейт.
//!
//! ## Чому `no_default_flags`
//!
//! `cc` за замовчуванням додає свої `-O`, `-g` і `-W` за профілем cargo. У
//! debug це `-O0` — а `core/cflags.txt` прямо каже, що при `-O0` gcc кличе
//! `sqrt` як функцію `libm`, і сценарії, які свідомо лінкуються без `-lm`,
//! не злінкуються.
//!
//! **Виміряно, cc 1.4.2:** `no_default_flags(true)` прибирає не все. Фактичний
//! виклик — `cc -I core -Wall -Wextra <наші прапорці> -fPIC -o … -c …`, тобто
//! `-Wall -Wextra` крейт вставляє все одно, і вставляє їх ПЕРЕД нашими. На
//! числа це не впливає (вони й так є в `cflags.txt`), але висновок ширший:
//! повного контролю над командною строкою `no_default_flags` не дає, і
//! покладатися на нього як на гарантію не можна.
//!
//! Гарантією лишається `tests/determinism.rs`: будь-який прапорець, який
//! `cc` колись вставить і який змінить арифметику, зсуне хеші й буде
//! спійманий там. Прапорці — перше місце, куди дивитися; хеші — те, що
//! справді перевіряється.
//!
//! Побачити фактичний виклик: `CC_ENABLE_DEBUG_OUTPUT=1 cargo build -vv`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.parent().expect("core-sys має лежати в репозиторії");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let core_dir = root.join("core");
    let flags = read_flags(&core_dir.join("cflags.txt"));

    let compiler = build_library(&core_dir, &flags);
    let scenarios = build_scenarios(&compiler, &core_dir, &out_dir, &flags);

    // Оракул для tests/ffi.rs (ROADMAP D2). Лежить у крейті, а не в
    // core/scenario/, бо там він змінив би golden.txt: це риштування межі,
    // а не сценарій детермінізму.
    let oracle = link(
        &compiler,
        &flags,
        &core_dir,
        &manifest.join("oracle.c"),
        &out_dir.join(format!("oracle{}", exe_suffix())),
        &out_dir.join("libcore.a"),
    );

    watch(&core_dir);
    println!("cargo:rerun-if-changed=oracle.c");

    // Тест не має вгадувати, де що лежить, і не має другої копії прапорців.
    println!("cargo:rustc-env=CORE_CFLAGS={}", flags.join(" "));
    println!("cargo:rustc-env=CORE_SCENARIO_DIR={}", scenarios.display());
    println!("cargo:rustc-env=CORE_ORACLE={}", oracle.display());
    println!("cargo:rustc-env=CORE_REPO_ROOT={}", root.display());
}

/// Читає `core/cflags.txt` так само, як це робить `Makefile`: знімає
/// коментарі, склеює решту.
///
/// Дві перевірки нижче дослівно повторюють ті, що вже є в `Makefile`, і
/// повторюють свідомо. Вони ловлять різні речі: порожній список — зламане
/// читання файлу, відсутній `-ffp-contract=off` — правку самого файлу, яка
/// забрала прапорець не подумавши. Обидві помилки без цієї перевірки
/// проявилися б однаково: збірка мовчки пішла б з дефолтними прапорцями
/// компілятора, а впала б звірка хешів — за кілометр від причини.
fn read_flags(path: &Path) -> Vec<String> {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("не читається {}: {e}", path.display()));

    let flags: Vec<String> = text
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .flat_map(|line| line.split_whitespace())
        .map(str::to_string)
        .collect();

    if flags.is_empty() {
        panic!(
            "прапорці не витягнулися з {}. Збірка з дефолтними прапорцями \
             компілятора порушила б детермінізм, тому це помилка, а не \
             попередження.",
            path.display()
        );
    }

    if !flags.iter().any(|f| f == "-ffp-contract=off") {
        panic!(
            "у прапорцях немає -ffp-contract=off. Без нього компілятор зливає \
             множення й додавання у FMA, і той самий код дає різні біти на \
             різних платформах — PROJECT.md §4."
        );
    }

    flags
}

/// `core/*.c` → `libcore.a`. Повертає компілятор, яким це зроблено, щоб
/// сценарії збиралися тим самим, а не тим, що вдруге вибере `cc`.
fn build_library(core_dir: &Path, flags: &[String]) -> cc::Tool {
    let mut build = cc::Build::new();
    build.no_default_flags(true);

    for flag in flags {
        build.flag(flag);
    }

    // Єдиний прапорець поза cflags.txt, і він там бути не може: файл — плоский
    // список, а цей залежить від платформи. Виконувані файли Rust на Linux
    // за замовчуванням PIE, тож об'єкти без -fPIC у них не злінкуються.
    //
    // На бітову точність не впливає: -fPIC змінює адресацію, а не арифметику.
    // І це не припущення — сценарії нижче збираються саме з цих об'єктів, а
    // їхні хеші звіряються з еталоном, порахованим збіркою `make` БЕЗ -fPIC.
    // Тобто тест перевіряє в тому числі й це твердження, і на D1 воно
    // підтвердилося: усі одинадцять хешів збіглися.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        build.flag("-fPIC");
    }

    let tool = build.get_compiler();
    if tool.is_like_msvc() {
        panic!(
            "core/cflags.txt написаний під gcc/clang, а тут MSVC. На Windows \
             беріть тулчейн x86_64-pc-windows-gnu — саме ним (через MSYS2) \
             збирається джоб windows-mingw у CI. Звірка прапорців MSVC — \
             окремий борг, ROADMAP A1."
        );
    }

    build.include(core_dir);
    for src in sources(core_dir) {
        build.file(src);
    }

    build.compile("core");
    tool
}

/// `core/scenario/*.c` → виконувані файли.
///
/// Без `libcore_offline.a` і без `-lm`, точно як у `Makefile`: лінкування тут
/// саме по собі є перевіркою того, що в рантайм не просочилася тригонометрія.
/// Якщо просочиться — впаде саме тут, а не через тиждень на іншій платформі.
fn build_scenarios(
    tool: &cc::Tool,
    core_dir: &Path,
    out_dir: &Path,
    flags: &[String],
) -> PathBuf {
    let scenario_dir = core_dir.join("scenario");
    let bin_dir = out_dir.join("scenario");
    fs::create_dir_all(&bin_dir).expect("не створюється каталог для сценаріїв");

    let lib = out_dir.join("libcore.a");

    for src in sources(&scenario_dir) {
        let stem = src.file_stem().unwrap().to_string_lossy().to_string();
        let exe = bin_dir.join(format!("{stem}{}", exe_suffix()));
        link(tool, flags, core_dir, &src, &exe, &lib);
    }

    bin_dir
}

/// Лінкує одну програму на C проти вже зібраної `libcore.a`.
///
/// **Без `-lm` і це головне.** Лінкування саме по собі є перевіркою того, що
/// в рантаймову зону не просочилася тригонометрія: `sin` чи `pow` тут просто
/// не знайдуть символу. Дешевша й раніша перевірка за «поліцію libm», і вона
/// тримається сама, без окремого скрипта.
fn link(
    tool: &cc::Tool,
    flags: &[String],
    core_dir: &Path,
    src: &Path,
    exe: &Path,
    lib: &Path,
) -> PathBuf {
    let mut cmd = Command::new(tool.path());
    cmd.args(flags)
        .arg("-I")
        .arg(core_dir)
        .arg("-o")
        .arg(exe)
        .arg(src)
        .arg(lib);

    let status = cmd
        .status()
        .unwrap_or_else(|e| panic!("не запускається {:?}: {e}", tool.path()));

    if !status.success() {
        panic!("{} не зібрався: {cmd:?}", src.display());
    }

    exe.to_path_buf()
}

fn exe_suffix() -> &'static str {
    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("windows") => ".exe",
        _ => "",
    }
}

/// Файли `.c` каталогу, відсортовані.
///
/// Сортування не для краси: порядок сценаріїв визначає порядок рядків, які
/// звіряються з еталоном, а `read_dir` сталого порядку не гарантує.
fn sources(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("не читається {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "c"))
        .collect();

    files.sort();
    files
}

/// Перезбирати, коли змінилося будь-що з входів. Заголовки теж: `cc` сам їх
/// не відстежує, а зміна `.h` без зміни `.c` — звичайна річ.
fn watch(core_dir: &Path) {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", core_dir.join("cflags.txt").display());

    for dir in [core_dir.to_path_buf(), core_dir.join("scenario")] {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for path in entries.filter_map(|e| e.ok().map(|e| e.path())) {
            if path
                .extension()
                .is_some_and(|ext| ext == "c" || ext == "h")
            {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
}
