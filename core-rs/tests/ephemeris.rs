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
        assert_eq!(
            got.to_bits(),
            first.to_bits(),
            "паралельне читання розійшлося"
        );
    }
}

// ---------------------------------------------------------------------------
// Пропагатор (ROADMAP H4)
// ---------------------------------------------------------------------------

use std::sync::Arc;

use core_rs::{Event, Integrator, PropConfig, Propagator, State, Stop};

const VESSEL_T0: f64 = DAY;
const VESSEL_DX: f64 = 42_164.0e3;
const VESSEL_VY: f64 = 1967.84;
const VESSEL_VZ: f64 = 1475.88;

const EARTH: i32 = 3;

/// Той самий апарат, що в `core-sys/oracle.c`: витягнута навколоземна орбіта,
/// задана числами, з перицентром, який є що шукати.
fn vessel(eph: &Ephemeris) -> State {
    let earth = eph
        .body_state(EARTH, VESSEL_T0)
        .expect("Земля в межах ассета");

    let mut s = State {
        r: earth.r,
        v: earth.v,
        t: VESSEL_T0,
    };
    s.r.x += VESSEL_DX;
    s.v.y += VESSEL_VY;
    s.v.z += VESSEL_VZ;
    s
}

fn config() -> PropConfig {
    PropConfig {
        integrator: Integrator::Dop853,
        tol_m: 1e-2,
        h_max_s: 1800.0,
        max_steps: 0,
    }
}

fn same_bits(a: &State, b: &State) -> bool {
    a.r.x.to_bits() == b.r.x.to_bits()
        && a.r.y.to_bits() == b.r.y.to_bits()
        && a.r.z.to_bits() == b.r.z.to_bits()
        && a.v.x.to_bits() == b.v.x.to_bits()
        && a.v.y.to_bits() == b.v.y.to_bits()
        && a.v.z.to_bits() == b.v.z.to_bits()
        && a.t.to_bits() == b.t.to_bits()
}

/// Обгортка пропагатора не змінює жодного біта проти сирого виклику.
///
/// Та сама вимога, що й до `Ephemeris` вище, і та сама причина: `core-sys`
/// уже звірений з C оракулом, тож ланцюжок C → core-sys → core-rs замикається
/// цим тестом.
#[test]
fn propagation_matches_the_raw_call_bit_for_bit() {
    const CAP: usize = 64;

    let eph = Arc::new(load());
    let start = vessel(&eph);

    let mut prop = Propagator::new(eph.clone(), config()).expect("пропагатор має створитися");

    let mut samples = vec![State::default(); CAP];
    let mut step = 0.0;
    let run = prop
        .run(&start, VESSEL_T0 + 0.5 * DAY, &[], &mut samples, &mut step)
        .expect("прогін має пройти");

    // Сирий шлях, той самий буфер, ті самі числа.
    let mut raw_samples = vec![State::default(); CAP];
    let mut raw_count: usize = 0;
    let mut raw_final = State::default();
    let mut raw_stop: core_sys::CoreStopReason = -1;
    let mut raw_event: i32 = -1;
    let mut raw_step = 0.0;

    unsafe {
        let raw_cfg = core_sys::PropConfig {
            integrator: core_sys::CORE_INTEG_DOP853,
            tol_m: 1e-2,
            h_max_s: 1800.0,
            max_steps: 0,
        };
        let mut raw_eph: *mut core_sys::EphemerisCtx = std::ptr::null_mut();
        let path = std::ffi::CString::new(fixture().to_str().unwrap()).unwrap();
        assert_eq!(
            core_sys::eph_load(path.as_ptr(), &mut raw_eph),
            core_sys::CORE_OK
        );

        let mut raw_prop: *mut core_sys::PropagatorCtx = std::ptr::null_mut();
        assert_eq!(
            core_sys::prop_create(raw_eph, &raw_cfg, &mut raw_prop),
            core_sys::CORE_OK
        );

        assert_eq!(
            core_sys::prop_run(
                raw_prop,
                &start,
                VESSEL_T0 + 0.5 * DAY,
                std::ptr::null(),
                0,
                raw_samples.as_mut_ptr(),
                CAP,
                &mut raw_count,
                &mut raw_final,
                &mut raw_stop,
                &mut raw_event,
                &mut raw_step,
            ),
            core_sys::CORE_OK
        );

        core_sys::prop_free(raw_prop);
        core_sys::eph_free(raw_eph);
    }

    assert_eq!(run.filled, raw_count, "кількість семплів");
    assert!(
        run.filled > 0,
        "прогін без жодного семпла нічого не доводить"
    );
    for (i, (safe, raw)) in samples[..run.filled]
        .iter()
        .zip(raw_samples[..raw_count].iter())
        .enumerate()
    {
        assert!(
            same_bits(safe, raw),
            "семпл {i} розійшовся з сирим викликом"
        );
    }
    assert!(same_bits(&run.final_state, &raw_final), "кінцевий стан");
    assert_eq!(step.to_bits(), raw_step.to_bits(), "перенесений крок");
}

/// Порожній зріз означає «без семплування», а не «буфер уже повний».
///
/// Різниця не косметична: порожній зріз у Rust — це вирівняний висячий
/// вказівник, не нуль, і якби він поїхав у C як буфер, прогін зупинявся б
/// одразу, без поступу, і викликач крутився б у циклі назавжди.
#[test]
fn an_empty_slice_means_no_sampling() {
    let eph = Arc::new(load());
    let start = vessel(&eph);
    let mut prop = Propagator::new(eph, config()).unwrap();

    let mut step = 0.0;
    let run = prop
        .run(&start, VESSEL_T0 + 0.5 * DAY, &[], &mut [], &mut step)
        .expect("прогін без семплів має пройти");

    assert_eq!(run.filled, 0);
    assert_eq!(run.stop, Stop::ReachedEnd);
    assert!(run.final_state.t > start.t, "час мусив просунутися");
}

/// Подія доходить як подія, з індексом у переданий зріз.
#[test]
fn events_come_back_as_events() {
    let eph = Arc::new(load());
    let start = vessel(&eph);
    let mut prop = Propagator::new(eph.clone(), config()).unwrap();

    let events = [
        Event::Apoapsis { body: EARTH },
        Event::Periapsis { body: EARTH },
    ];

    let mut step = 0.0;
    let run = prop
        .run(&start, VESSEL_T0 + 4.0 * DAY, &events, &mut [], &mut step)
        .expect("прогін має пройти");

    // Апарат стартує рівно в апоцентрі, тож першим має бути перицентр — це
    // індекс 1, і саме він доводить, що індекс не вигаданий і не нульовий
    // за замовчуванням.
    assert_eq!(run.stop, Stop::Event(1));

    let earth = eph.body_state(EARTH, run.final_state.t).unwrap();
    let dx = run.final_state.r.x - earth.r.x;
    let dy = run.final_state.r.y - earth.r.y;
    let dz = run.final_state.r.z - earth.r.z;
    let r = (dx * dx + dy * dy + dz * dz).sqrt();
    assert!(r < VESSEL_DX, "перицентр має бути ближче за старт: {r} м");
}

/// Прогін, порізаний буфером, — та сама траєкторія (CLAUDE.md, інваріант 5),
/// і через обгортку теж.
#[test]
fn stitched_legs_are_the_same_trajectory() {
    let eph = Arc::new(load());
    let start = vessel(&eph);
    let t_end = VESSEL_T0 + 0.5 * DAY;

    let mut whole = Propagator::new(eph.clone(), config()).unwrap();
    let mut step = 0.0;
    let single = whole
        .run(&start, t_end, &[], &mut [], &mut step)
        .expect("один прогін");

    let mut legs = Propagator::new(eph, config()).unwrap();
    let mut piece = [State::default(); 4];
    let mut leg_step = 0.0;
    let mut state = start;
    let mut n_legs = 0;

    loop {
        let run = legs
            .run(&state, t_end, &[], &mut piece, &mut leg_step)
            .expect("ланка");
        state = run.final_state;
        n_legs += 1;

        if run.stop == Stop::ReachedEnd {
            break;
        }
        assert_eq!(run.stop, Stop::BufferFull);
        assert!(n_legs < 1000, "ланки не закінчуються — немає поступу");
    }

    assert!(n_legs > 1, "буфер на чотири семпли мав розрізати прогін");
    assert!(
        same_bits(&state, &single.final_state),
        "траєкторія розійшлася"
    );
    assert_eq!(leg_step.to_bits(), step.to_bits(), "перенесений крок");
}

/// Інтегратор, якого ще немає, — помилка, а не тихе підставляння наявного.
#[test]
fn asking_for_an_integrator_that_does_not_exist_is_an_error() {
    let eph = Arc::new(load());

    let cfg = PropConfig {
        integrator: Integrator::Rkn,
        ..config()
    };
    assert_eq!(Propagator::new(eph, cfg).err(), Some(CoreError::InvalidArg));
}

/// Вихід за проміжок ассета доходить як помилка, а не як правдоподібна
/// траєкторія апарата, на який ніщо не тягне.
#[test]
fn running_past_the_asset_is_an_error() {
    let eph = Arc::new(load());
    let start = vessel(&eph);
    let mut prop = Propagator::new(eph, config()).unwrap();

    let mut step = 0.0;
    assert_eq!(
        prop.run(&start, 200.0 * DAY, &[], &mut [], &mut step).err(),
        Some(CoreError::InvalidArg)
    );

    // І контекст не отруєний: наступний прогін у межах ассета проходить.
    let mut step = 0.0;
    assert!(prop
        .run(&start, VESSEL_T0 + 3600.0, &[], &mut [], &mut step)
        .is_ok());
}

/// Пропагатори створюються й звільняються, і це те, що міряє valgrind у CI:
/// типи доводять, що звільнити двічі не можна, але не доводять, що звільнення
/// взагалі відбувається — витік не є помилкою типів.
#[test]
fn creating_and_dropping_repeatedly_is_clean() {
    let eph = Arc::new(load());

    for _ in 0..50 {
        let mut prop = Propagator::new(eph.clone(), config()).unwrap();
        let start = vessel(&eph);
        let mut step = 0.0;
        let mut samples = [State::default(); 8];
        let _ = prop.run(&start, VESSEL_T0 + 600.0, &[], &mut samples, &mut step);
    }
}
