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

use core_rs::{Event, Integrator, PropConfig, Propagator, State, Stm, Stop, STM_LEN};

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
        density_scale: 1.0,
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
        .run(
            &start,
            None,
            VESSEL_T0 + 0.5 * DAY,
            &[],
            &mut samples,
            &mut step,
        )
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
            density_scale: 1.0,
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
                std::ptr::null(),
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
        .run(&start, None, VESSEL_T0 + 0.5 * DAY, &[], &mut [], &mut step)
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
        .run(
            &start,
            None,
            VESSEL_T0 + 4.0 * DAY,
            &events,
            &mut [],
            &mut step,
        )
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

/// Висота доходить як висота, а не як відстань (ROADMAP K7c).
///
/// Оракул тут — сама пара подій, без жодного числа про Землю в тесті. Та
/// сама цифра, подана як висота і як відстань, зупиняє апарат на двох різних
/// радіусах, і **різниця між ними і є радіус тіла**. Тесту лишається сказати,
/// що це справді радіус Землі — з точністю до сотні кілометрів, тобто на
/// рівні факту про світ, а не на рівні числа з ассета.
///
/// Якби варіант зліпився з `Event::Distance`, різниця була б нулем.
#[test]
fn altitude_is_measured_from_the_surface() {
    let eph = Arc::new(load());
    let start = vessel(&eph);
    let mut prop = Propagator::new(eph.clone(), config()).unwrap();

    const METRES: f64 = 30_000.0e3;

    let radius_at = |run: &core_rs::Run| {
        let earth = eph.body_state(EARTH, run.final_state.t).unwrap();
        let dx = run.final_state.r.x - earth.r.x;
        let dy = run.final_state.r.y - earth.r.y;
        let dz = run.final_state.r.z - earth.r.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    };

    let mut step = 0.0;
    let high = prop
        .run(
            &start,
            None,
            VESSEL_T0 + 4.0 * DAY,
            &[Event::Altitude {
                body: EARTH,
                metres: METRES,
            }],
            &mut [],
            &mut step,
        )
        .expect("прогін має пройти");
    assert_eq!(high.stop, Stop::Event(0));

    let mut step = 0.0;
    let low = prop
        .run(
            &start,
            None,
            VESSEL_T0 + 4.0 * DAY,
            &[Event::Distance {
                body: EARTH,
                metres: METRES,
            }],
            &mut [],
            &mut step,
        )
        .expect("прогін має пройти");
    assert_eq!(low.stop, Stop::Event(0));

    // Висоту перетнуто раніше: вона лежить далі від центра.
    assert!(high.final_state.t < low.final_state.t);

    let radius = radius_at(&high) - radius_at(&low);
    assert!(
        radius > 6.3e6 && radius < 6.4e6,
        "різниця подій має бути радіусом Землі, а вийшло {radius} м"
    );
}

/// Від'ємна висота — це помилка знака у викликача, і межа каже про це.
#[test]
fn a_negative_altitude_is_refused() {
    let eph = Arc::new(load());
    let start = vessel(&eph);
    let mut prop = Propagator::new(eph, config()).unwrap();

    let mut step = 0.0;
    let err = prop
        .run(
            &start,
            None,
            VESSEL_T0 + DAY,
            &[Event::Altitude {
                body: EARTH,
                metres: -1.0,
            }],
            &mut [],
            &mut step,
        )
        .expect_err("від'ємна висота має бути відхилена");
    assert!(matches!(err, CoreError::InvalidArg), "{err:?}");
}

/// Нульовий множник густини не читається як одиниця (ROADMAP K7c).
#[test]
fn a_zero_density_scale_is_refused() {
    let eph = Arc::new(load());
    let mut cfg = config();
    cfg.density_scale = 0.0;

    let err = Propagator::new(eph, cfg).expect_err("нуль має бути відхилений");
    assert!(matches!(err, CoreError::InvalidArg), "{err:?}");
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
        .run(&start, None, t_end, &[], &mut [], &mut step)
        .expect("один прогін");

    let mut legs = Propagator::new(eph, config()).unwrap();
    let mut piece = [State::default(); 4];
    let mut leg_step = 0.0;
    let mut state = start;
    let mut n_legs = 0;

    loop {
        let run = legs
            .run(&state, None, t_end, &[], &mut piece, &mut leg_step)
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
        prop.run(&start, None, 200.0 * DAY, &[], &mut [], &mut step)
            .err(),
        Some(CoreError::InvalidArg)
    );

    // І контекст не отруєний: наступний прогін у межах ассета проходить.
    let mut step = 0.0;
    assert!(prop
        .run(&start, None, VESSEL_T0 + 3600.0, &[], &mut [], &mut step)
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
        let _ = prop.run(
            &start,
            None,
            VESSEL_T0 + 600.0,
            &[],
            &mut samples,
            &mut step,
        );
    }
}

/// `run_stm` (ROADMAP K8): матриця приходить, і траєкторія при цьому та сама.
///
/// Друге твердження — головне. Матриця варта чогось лише тоді, коли вона
/// належить траєкторії, якою апарат справді летить; звірка бітова, бо
/// «приблизно та сама» тут нічого не значила б.
#[test]
fn the_stm_run_is_the_same_trajectory() {
    let eph = Arc::new(load());
    let start = vessel(&eph);
    let mut prop = Propagator::new(eph, config()).unwrap();

    let t_end = VESSEL_T0 + 0.25 * DAY;

    let mut plain_step = 0.0;
    let plain = prop
        .run(&start, None, t_end, &[], &mut [], &mut plain_step)
        .unwrap();

    let mut stm_step = 0.0;
    let (final_state, phi) = prop.run_stm(&start, None, t_end, &mut stm_step).unwrap();

    for (a, b) in [
        (final_state.r.x, plain.final_state.r.x),
        (final_state.r.y, plain.final_state.r.y),
        (final_state.r.z, plain.final_state.r.z),
        (final_state.v.x, plain.final_state.v.x),
        (final_state.v.y, plain.final_state.v.y),
        (final_state.v.z, plain.final_state.v.z),
    ] {
        assert_eq!(a.to_bits(), b.to_bits(), "траєкторія мусить бути та сама");
    }
    assert_eq!(
        stm_step.to_bits(),
        plain_step.to_bits(),
        "крок, який лишається на наступну ланку, теж"
    );

    // Матриця осмислена: не одинична, не порожня, і індексується так, як
    // обіцяно. Без цього все вище звірялося б із нулями.
    assert_eq!(phi.as_slice().len(), STM_LEN);
    let off_diagonal: f64 = (0..6)
        .flat_map(|i| (0..6).map(move |j| (i, j)))
        .filter(|(i, j)| i != j)
        .map(|(i, j)| phi.get(i, j).abs())
        .sum();
    assert!(off_diagonal > 1.0, "STM виглядає одиничною");

    // get(row, col) читає той самий елемент, що й сирий зріз - інакше
    // транспонування пройшло б непоміченим.
    for i in 0..6 {
        for j in 0..6 {
            assert_eq!(phi.get(i, j).to_bits(), phi.as_slice()[i * 6 + j].to_bits());
        }
    }
}

/// Та сама відмова, що в `run`: за межами ассета це помилка, а не матриця
/// для апарата, який не відчув тяжіння.
#[test]
fn an_stm_run_past_the_asset_is_an_error() {
    let eph = Arc::new(load());
    let start = vessel(&eph);
    let mut prop = Propagator::new(eph, config()).unwrap();

    let mut step = 0.0;
    assert_eq!(
        prop.run_stm(&start, None, 200.0 * DAY, &mut step).err(),
        Some(CoreError::InvalidArg)
    );
}

/// `Stm::get` поза 6×6 — це помилка програміста, і вона мусить бути гучною.
#[test]
#[should_panic(expected = "STM 6x6")]
fn indexing_the_stm_out_of_range_panics() {
    let phi = Stm([0.0; STM_LEN]);
    let _ = phi.get(6, 0);
}

/// Тиск сонячного світла через обгортку (ROADMAP K6b).
///
/// Фізику міряє `core/test/test_srp.c`; тут перевіряється переклад
/// `Option<&VesselParams>` у вказівник, і три твердження, кожне з яких
/// ламається окремо:
///
/// - `None` і апарат без площі — це те саме, **бітово**: усе, що літало до
///   K6b, летить так само;
/// - апарат із площею летить інакше, і на скільки саме — видно;
/// - `run_stm` несе той самий апарат, тобто матриця належить траєкторії, а
///   не сусідній (це K8c, перевірене ще раз там, де його найлегше втратити).
#[test]
fn a_vessel_with_area_feels_the_sun() {
    let eph = Arc::new(load());
    let start = vessel(&eph);
    let mut prop = Propagator::new(eph, config()).unwrap();

    let t_end = VESSEL_T0 + 0.5 * DAY;

    let bare = core_rs::VesselParams {
        mass_kg: 1000.0,
        area_m2: 0.0,
        cr: 1.3,
        cd: 0.0,
    };
    let sail = core_rs::VesselParams {
        mass_kg: 1000.0,
        area_m2: 20.0,
        cr: 1.3,
        cd: 0.0,
    };

    let mut step = 0.0;
    let none = prop
        .run(&start, None, t_end, &[], &mut [], &mut step)
        .unwrap();

    let mut step_bare = 0.0;
    let zero_area = prop
        .run(&start, Some(&bare), t_end, &[], &mut [], &mut step_bare)
        .unwrap();
    assert!(
        same_bits(&none.final_state, &zero_area.final_state),
        "апарат без площі — це та сама пробна частинка"
    );
    assert_eq!(step.to_bits(), step_bare.to_bits(), "і той самий крок");

    let mut step_sail = 0.0;
    let lit = prop
        .run(&start, Some(&sail), t_end, &[], &mut [], &mut step_sail)
        .unwrap();

    let moved = ((lit.final_state.r.x - none.final_state.r.x).powi(2)
        + (lit.final_state.r.y - none.final_state.r.y).powi(2)
        + (lit.final_state.r.z - none.final_state.r.z).powi(2))
    .sqrt();
    println!("  пів доби під SRP зрушили апарат на {moved:.4} м");
    assert!(
        moved > 1.0,
        "площа мала змінити траєкторію, а зрушила {moved} м"
    );

    let mut stm_step = 0.0;
    let (stm_final, _) = prop
        .run_stm(&start, Some(&sail), t_end, &mut stm_step)
        .unwrap();
    assert!(
        same_bits(&stm_final, &lit.final_state),
        "матриця мусить належати траєкторії, яку апарат справді летить"
    );
}
