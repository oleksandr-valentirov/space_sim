//! Перевірка кроку D3: обгортка нічого не змінює й нічого не тече.
//!
//! Два різні твердження, і перевіряються вони по-різному.
//!
//! **Числа.** Обгортка не має права нічого підправити по дорозі, тож те, що
//! вона віддає, звіряється з тим, що дає сирий виклик, бітово. Тест на D2 вже
//! звірив сирий шар із C — отже ланцюжок C → core-sys → core-rs замкнений.
//!
//! **Пам'ять.** Що подвійне звільнення й використання після звільнення
//! неможливі, показують `compile_fail`-доктести в `src/lib.rs`: це властивість
//! типів, її перевіряє компілятор, а не прогін. Те, що звільнення таки
//! відбувається (а не просто не падає), ловиться інструментом — див. крок
//! «Valgrind» у CI.

use std::path::{Path, PathBuf};

use core_rs::{CoreError, Ephemeris};

const DAY: f64 = 86400.0;

fn repo_root() -> PathBuf {
    // Тести cargo запускаються з кореня крейта, а ассет лежить у корені
    // репозиторію.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core-rs має лежати в репозиторії")
        .to_path_buf()
}

fn fixture() -> PathBuf {
    repo_root().join("data/fixture/earth_moon.eph")
}

fn load() -> Ephemeris {
    Ephemeris::load(&fixture()).expect("фікстура має читатися з кореня репозиторію")
}

/// Обгортка віддає рівно ті самі біти, що й сирий виклик.
///
/// Не «в межах допуску»: будь-яка різниця тут означала б, що по дорозі щось
/// сталося — перетворення типу, копія через інший шлях, — а такого шару тут
/// свідомо немає.
#[test]
fn wrapper_returns_the_same_bits_as_the_raw_call() {
    let eph = load();

    let mut raw_ctx: *mut core_sys::EphemerisCtx = std::ptr::null_mut();
    let path = fixture();
    let c_path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();

    // SAFETY: другий, незалежний контекст на той самий файл. Тест сирого шару
    // — єдине місце поза core-rs, де ми пишемо unsafe, і саме тому він тут:
    // інакше нічого було б із чим звіряти.
    unsafe {
        assert_eq!(
            core_sys::eph_load(c_path.as_ptr(), &mut raw_ctx),
            core_sys::CORE_OK
        );
    }

    for body in [0, 3, 4] {
        for t in [0.0, 30.0 * DAY, 119.0 * DAY] {
            let safe = eph.body_state(body, t).expect("момент усередині проміжку");

            let mut raw = core_sys::State::default();
            // SAFETY: raw_ctx щойно завантажено й ще не звільнено.
            let code = unsafe { core_sys::eph_body_state(raw_ctx, body, t, &mut raw) };
            assert_eq!(code, core_sys::CORE_OK);

            for (i, (a, b)) in [
                (safe.r.x, raw.r.x),
                (safe.r.y, raw.r.y),
                (safe.r.z, raw.r.z),
                (safe.v.x, raw.v.x),
                (safe.v.y, raw.v.y),
                (safe.v.z, raw.v.z),
                (safe.t, raw.t),
            ]
            .iter()
            .enumerate()
            {
                assert_eq!(
                    a.to_bits(),
                    b.to_bits(),
                    "тіло {body}, момент {t}, компонента {i}: обгортка змінила число"
                );
            }
        }
    }

    // SAFETY: контекст ще живий, звільняється рівно раз.
    unsafe { core_sys::eph_free(raw_ctx) };
}

/// Коди повернення C стають типізованими помилками, а не зникають.
#[test]
fn errors_arrive_as_errors() {
    let eph = load();

    for (label, body, t) in [
        ("час до початку", 0, -DAY),
        ("час після кінця", 0, 200.0 * DAY),
        ("від'ємне тіло", -1, 0.0),
        ("тіло поза списком", 999, 0.0),
    ] {
        assert_eq!(
            eph.body_state(body, t),
            Err(CoreError::InvalidArg),
            "{label} мав дати InvalidArg"
        );
    }

    // І навпаки: всередині проміжку — успіх. Без цього попередня перевірка
    // «проходила б» і на обгортці, яка повертає InvalidArg завжди.
    assert!(eph.body_state(0, 0.0).is_ok());
}

#[test]
fn a_missing_file_is_an_error_not_a_panic() {
    let missing = repo_root().join("data/fixture/немає-такого.eph");
    assert!(matches!(
        Ephemeris::load(&missing),
        Err(CoreError::InvalidArg)
    ));
}

/// Шлях із `\0` усередині не може стати C-рядком. Це помилка нашого боку —
/// ядро її не бачить, — і вона мусить бути окремою, а не вдавати помилку ядра.
#[test]
fn a_path_with_a_nul_is_rejected_before_c_sees_it() {
    // matches!, а не assert_eq!: `Ephemeris` навмисно не PartialEq — його
    // рівність нічого не означала б, бо це володіння хендлом, а не значення.
    let bad = Path::new("data/fixture/earth\0moon.eph");
    assert!(matches!(Ephemeris::load(bad), Err(CoreError::BadPath)));
}

/// Багато завантажень і звільнень поспіль.
///
/// Сам по собі тест нічого не доводить — він пройде і з витоком. Він існує,
/// щоб дати Valgrind у CI що виміряти: витік на одному завантаженні легко
/// не помітити, витік на п'ятдесяти — ні.
#[test]
fn loading_and_dropping_repeatedly_is_clean() {
    for _ in 0..50 {
        let eph = load();
        assert!(eph.body_state(4, 0.0).is_ok());
    }
}

/// `Send` і `Sync` — обіцянка, обґрунтована читанням C. Ось її вжиток.
#[test]
fn the_handle_can_be_shared_between_threads() {
    use std::sync::Arc;

    let eph = Arc::new(load());
    let mut handles = Vec::new();

    for _ in 0..4 {
        let shared = Arc::clone(&eph);
        handles.push(std::thread::spawn(move || {
            shared.body_state(4, 0.0).map(|s| s.r.x)
        }));
    }

    let first = eph.body_state(4, 0.0).unwrap().r.x;
    for handle in handles {
        let got = handle.join().expect("потік не мав панікувати").unwrap();
        assert_eq!(got.to_bits(), first.to_bits(), "паралельне читання розійшлося");
    }
}
