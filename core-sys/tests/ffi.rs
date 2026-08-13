//! Перевірка кроку D2: декларації межі описують саме те, що є в C.
//!
//! Помилка в FFI не падає. Переплутані поля `State`, `int` замість `size_t`,
//! `*mut` там, де C очікує `*const` — усе це компілюється й повертає числа,
//! які виглядають як координати. Тому перевірка тут бітова, а не «в межах
//! допуску»: та сама функція, той самий ассет, ті самі моменти часу, викликані
//! з C і з Rust, мусять дати **однакові біти**. Будь-яка розбіжність у
//! розкладці зіпсує їх до невпізнання.
//!
//! Оракул на C — `core-sys/oracle.c`, збирається в `build.rs`.
//!
//! `unsafe` тут є, і це не порушення інваріанта з CLAUDE.md: правило про
//! «наш `unsafe` лише в core-rs» стосується коду, який ми постачаємо. Тест
//! сирого шару інакше написати не можна — він саме про те, що виклик через
//! межу коректний.

use std::ffi::CString;
use std::path::Path;
use std::process::Command;

use core_sys::{
    eph_body_state, eph_free, eph_load, CoreResult, EphemerisCtx, State, CORE_ERR_INVALID_ARG,
    CORE_OK,
};

const ORACLE: &str = env!("CORE_ORACLE");
const REPO_ROOT: &str = env!("CORE_REPO_ROOT");

const ASSET: &str = "data/fixture/earth_moon.eph";
const DAY: f64 = 86400.0;

/// Один рядок виводу оракула: тіло, час і шість компонент стану.
struct Sample {
    body: i32,
    t: f64,
    values: [f64; 6],
}

/// Запускає оракул і розбирає його вивід.
///
/// `%.17g` однозначно відновлює double, а парсер Rust коректно заокруглює,
/// тож текст посередині нічого не втрачає — порівнювати можна побітово.
fn oracle_samples() -> Vec<Sample> {
    let output = Command::new(ORACLE)
        .current_dir(REPO_ROOT)
        .output()
        .unwrap_or_else(|e| panic!("не запускається {ORACLE}: {e}"));

    assert!(
        output.status.success(),
        "оракул завершився з {}:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let text = String::from_utf8(output.stdout).expect("оракул видав не UTF-8");
    let mut samples = Vec::new();

    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(fields.len(), 8, "неочікуваний рядок оракула: {line}");

        let number = |i: usize| -> f64 {
            fields[i]
                .parse()
                .unwrap_or_else(|e| panic!("не число в '{line}', поле {i}: {e}"))
        };

        let mut values = [0.0; 6];
        for (i, value) in values.iter_mut().enumerate() {
            *value = number(2 + i);
        }

        samples.push(Sample {
            body: fields[0].parse().expect("тіло не ціле"),
            t: number(1),
            values,
        });
    }

    assert!(
        !samples.is_empty(),
        "оракул нічого не вивів — порожня звірка мовчки 'проходить', \
         тому це провал"
    );

    samples
}

/// Завантажує фікстуру. Викликач зобов'язаний віддати результат у `eph_free` —
/// саме та зобов'язаність, яку D3 зробить неможливо порушити.
///
/// # Safety
///
/// Повертає сирий вказівник, дійсний до `eph_free`.
unsafe fn load_fixture() -> *mut EphemerisCtx {
    let path = Path::new(REPO_ROOT).join(ASSET);
    let c_path = CString::new(path.to_str().expect("шлях не UTF-8")).expect("шлях із \\0");

    let mut ctx: *mut EphemerisCtx = std::ptr::null_mut();
    let result: CoreResult = eph_load(c_path.as_ptr(), &mut ctx);

    assert_eq!(result, CORE_OK, "eph_load не прочитав {}", path.display());
    assert!(!ctx.is_null(), "eph_load повернув CORE_OK і NULL");

    ctx
}

#[test]
fn states_match_the_c_oracle_bit_for_bit() {
    let samples = oracle_samples();

    unsafe {
        let ctx = load_fixture();

        for sample in &samples {
            let mut state = State::default();
            let result = eph_body_state(ctx, sample.body, sample.t, &mut state);
            assert_eq!(
                result, CORE_OK,
                "тіло {} у момент {}",
                sample.body, sample.t
            );

            let got = [
                state.r.x, state.r.y, state.r.z, state.v.x, state.v.y, state.v.z,
            ];

            for (i, (&from_c, &from_rust)) in sample.values.iter().zip(got.iter()).enumerate() {
                assert_eq!(
                    from_c.to_bits(),
                    from_rust.to_bits(),
                    "тіло {}, момент {}, компонента {i}: C дало {from_c:.17e}, \
                     Rust {from_rust:.17e}.\nЦе розкладка структур або типи \
                     в декларації, а не фізика.",
                    sample.body,
                    sample.t
                );
            }

            // Час у структурі — теж поле, і теж може з'їхати, якщо порядок
            // полів State оголошено неправильно.
            assert_eq!(state.t.to_bits(), sample.t.to_bits(), "поле t зсунулося");
        }

        eph_free(ctx);
    }
}

/// Помилки мусять доходити як помилки, а не як нулі.
///
/// Це друга половина контракту: якщо код повернення читається неправильно,
/// виклик поза проміжком ассета виглядатиме як успіх зі станом, набитим
/// сміттям, — і це найгірший можливий результат, бо траєкторія вийде
/// правдоподібна.
#[test]
fn out_of_range_is_reported_not_extrapolated() {
    unsafe {
        let ctx = load_fixture();
        let mut state = State::default();

        // Фікстура покриває 120 діб від J2000 (data/fixture/README.md).
        for (label, body, t) in [
            ("час до початку", 0, -DAY),
            ("час після кінця", 0, 200.0 * DAY),
            ("від'ємний індекс тіла", -1, 0.0),
            ("індекс поза списком", 999, 0.0),
        ] {
            let result = eph_body_state(ctx, body, t, &mut state);
            assert_eq!(
                result, CORE_ERR_INVALID_ARG,
                "{label}: очікували CORE_ERR_INVALID_ARG, отримали {result}"
            );
        }

        // А те, що всередині проміжку, має проходити — інакше попередня
        // перевірка «проходила б» і на зламаному коді повернення.
        assert_eq!(
            eph_body_state(ctx, 0, 0.0, &mut state),
            CORE_OK,
            "початок проміжку мав прочитатися"
        );

        eph_free(ctx);
    }
}

/// `eph_free(NULL)` дозволений — так каже `core/ephemeris.h`.
///
/// Дрібниця, але D3 буде на неї спиратися: RAII-обгортка звільняє в `Drop`
/// беззастережно, і якщо ця обіцянка неправдива, воно впаде не тут, а десь
/// у грі при вивантаженні сцени.
#[test]
fn freeing_null_is_allowed() {
    unsafe {
        eph_free(std::ptr::null_mut());
    }
}
