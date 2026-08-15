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
    eph_body_mu, eph_body_orientation, eph_body_radius, eph_body_state, eph_free, eph_load,
    porkchop_compute_eph, prop_create, prop_free, prop_run, prop_run_stm, CoreEvent, CoreResult,
    EphemerisCtx, PorkchopPoint, PropConfig, PropagatorCtx, Quat, State, CORE_ERR_BUFFER_TOO_SMALL,
    CORE_ERR_INVALID_ARG, CORE_EVENT_PERIAPSIS, CORE_INTEG_DOP853, CORE_OK, CORE_STOP_EVENT,
};

const ORACLE: &str = env!("CORE_ORACLE");
const ORACLE_PLANNING: &str = env!("CORE_ORACLE_PLANNING");
const REPO_ROOT: &str = env!("CORE_REPO_ROOT");

const ASSET: &str = "data/fixture/earth_moon.eph";
const DAY: f64 = 86400.0;

/// Один рядок виводу оракула: тег і числа після нього.
#[derive(Clone)]
struct Record {
    tag: String,
    values: Vec<f64>,
}

impl Record {
    fn state(&self, from: usize) -> State {
        State {
            t: self.values[from],
            r: core_sys::Vec3d {
                x: self.values[from + 1],
                y: self.values[from + 2],
                z: self.values[from + 3],
            },
            v: core_sys::Vec3d {
                x: self.values[from + 4],
                y: self.values[from + 5],
                z: self.values[from + 6],
            },
        }
    }
}

/// Бітова звірка двох станів із зрозумілим повідомленням.
///
/// Порівнюються біти, а не значення: різниця тут — це розкладка структур або
/// типи в декларації, і жоден допуск про неї нічого не скаже.
fn same_bits(from_c: &State, from_rust: &State, what: &str) {
    let c = [
        from_c.t, from_c.r.x, from_c.r.y, from_c.r.z, from_c.v.x, from_c.v.y, from_c.v.z,
    ];
    let rust = [
        from_rust.t,
        from_rust.r.x,
        from_rust.r.y,
        from_rust.r.z,
        from_rust.v.x,
        from_rust.v.y,
        from_rust.v.z,
    ];

    for (i, (&a, &b)) in c.iter().zip(rust.iter()).enumerate() {
        assert_eq!(
            a.to_bits(),
            b.to_bits(),
            "{what}, компонента {i}: C дало {a:.17e}, Rust {b:.17e}.\n\
             Це розкладка структур або типи в декларації, а не фізика."
        );
    }
}

/// Запускає оракул і розбирає його вивід.
///
/// `%.17g` однозначно відновлює double, а парсер Rust коректно заокруглює,
/// тож текст посередині нічого не втрачає — порівнювати можна побітово.
fn oracle_records() -> Vec<Record> {
    records_from(ORACLE)
}

/// Те саме для будь-якого оракула. Їх два, і другий не примха: оракул
/// планування лінкується з `-lm`, а цей — навмисно без неї (build.rs).
fn records_from(oracle: &str) -> Vec<Record> {
    let output = Command::new(oracle)
        .current_dir(REPO_ROOT)
        .output()
        .unwrap_or_else(|e| panic!("не запускається {oracle}: {e}"));

    assert!(
        output.status.success(),
        "оракул завершився з {}:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let text = String::from_utf8(output.stdout).expect("оракул видав не UTF-8");
    let mut records = Vec::new();

    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        assert!(fields.len() > 1, "неочікуваний рядок оракула: {line}");

        let values = fields[1..]
            .iter()
            .map(|f| {
                f.parse()
                    .unwrap_or_else(|e| panic!("не число в '{line}': {e}"))
            })
            .collect();

        records.push(Record {
            tag: fields[0].to_string(),
            values,
        });
    }

    assert!(
        !records.is_empty(),
        "оракул {oracle} нічого не вивів — порожня звірка мовчки \
         'проходить', тому це провал"
    );

    records
}

/// Рядки одного тегу, у порядку виводу. Клонує замість позичання — межа тут
/// не в продуктивності, а в тому, щоб код читався (CLAUDE.md, стиль Rust).
fn tagged(records: &[Record], tag: &str) -> Vec<Record> {
    records.iter().filter(|r| r.tag == tag).cloned().collect()
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
    let records = oracle_records();
    let samples = tagged(&records, "eph");
    assert!(!samples.is_empty(), "оракул не дав жодного рядка eph");

    unsafe {
        let ctx = load_fixture();

        for sample in &samples {
            let body = sample.values[0] as i32;
            let t = sample.values[1];

            let mut state = State::default();
            let result = eph_body_state(ctx, body, t, &mut state);
            assert_eq!(result, CORE_OK, "тіло {body} у момент {t}");

            let mut expected = sample.state(1);
            // Час у рядку — той, про який просили; решта з C.
            expected.t = t;
            same_bits(&expected, &state, &format!("тіло {body}, момент {t}"));
        }

        eph_free(ctx);
    }
}

/// Орієнтація звіряється бітово, всі чотири компоненти (R1c).
///
/// Найлегша помилка тут — не арифметична, а домовленісна: половина світу
/// пише кватерніон як `(x, y, z, w)`, і переставлений `w` лишається цілком
/// правильним обертанням, просто не тим. Ні код повернення, ні довжина
/// (вона одинична за будь-якої перестановки) цього не покажуть — лише
/// покомпонентна звірка з C.
///
/// Друге, що тут закріплюється: тіло без моделі обертання віддає **одиничний**
/// кватерніон і `CORE_OK`. «Не змодельовано» не має права з часом перетворитися
/// на «не вдалося»: у фікстурі таких тіл вісім із десяти.
#[test]
fn orientations_match_the_c_oracle_bit_for_bit() {
    let records = oracle_records();
    let quats = tagged(&records, "quat");
    assert!(!quats.is_empty(), "оракул не дав жодного рядка quat");

    unsafe {
        let ctx = load_fixture();

        let mut turning = 0;
        for record in &quats {
            let body = record.values[0] as i32;
            let t = record.values[1];

            let mut got = Quat::default();
            let result = eph_body_orientation(ctx, body, t, &mut got);
            assert_eq!(result, CORE_OK, "тіло {body} у момент {t}");

            for (k, (name, expected)) in [
                ("w", record.values[2]),
                ("x", record.values[3]),
                ("y", record.values[4]),
                ("z", record.values[5]),
            ]
            .iter()
            .enumerate()
            {
                let component = [got.w, got.x, got.y, got.z][k];
                assert_eq!(
                    component.to_bits(),
                    expected.to_bits(),
                    "тіло {body}, момент {t}, компонента {name}: C дав \
                     {expected}, а межа {component}"
                );
            }

            // Одиничний кватерніон — це «не обертається»; звірка самих
            // одиничних пройшла б і на функції, яка нічого не читає.
            if got != Quat::default() {
                turning += 1;
            }
        }

        assert!(
            turning > 0,
            "усі кватерніони одиничні — звірка нічого не перевірила"
        );

        eph_free(ctx);
    }
}

/// Радіуси теж звіряються бітово (ROADMAP U2a).
///
/// Функція повертає `double` замість коду — отже єдиний спосіб помітити, що
/// декларація розійшлася з C, це порівняти саме число. І порівнювати треба
/// **всі** тіла оракула разом із неіснуючим: розмір Землі й розмір Місяця
/// відрізняються втричі, тож зсув на одне тіло в масиві контексту дав би
/// цілком правдоподібний радіус — і невидиму помилку.
#[test]
fn radii_match_the_c_oracle_bit_for_bit() {
    let records = oracle_records();
    let radii = tagged(&records, "rad");
    assert!(!radii.is_empty(), "оракул не дав жодного рядка rad");

    unsafe {
        let ctx = load_fixture();

        let mut nonzero = 0;
        for record in &radii {
            let body = record.values[0] as i32;
            let expected = record.values[1];

            let got = eph_body_radius(ctx, body);
            assert_eq!(
                got.to_bits(),
                expected.to_bits(),
                "тіло {body}: C дав {expected}, а межа {got}"
            );

            if got != 0.0 {
                nonzero += 1;
            }
        }

        // Звірка нулів із нулями пройшла б і на функції, яка завжди повертає
        // нуль. Тіла з розміром у фікстурі є, і хоч одне з них має тут бути.
        assert!(
            nonzero > 0,
            "усі радіуси нульові — звірка нічого не перевірила"
        );

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

/// Пропагація через межу дає ті самі біти, що прямий виклик у C (ROADMAP H3).
///
/// Тут перевіряється більше, ніж одна функція. `prop_run` бере одинадцять
/// аргументів, серед них дві структури (`PropConfig` з `enum`, двома `double`
/// і `long`; `CoreEvent` з `enum`, `int` і `double`), три вихідні вказівники
/// й буфер, який дає Rust. Кожне з цього — окремий спосіб мовчки розійтися:
/// зсунуте поле, не той цілий тип, переплутаний порядок `out_cap`/`out_count`.
/// Жоден із них не падає — усі повертають числа, схожі на траєкторію.
///
/// Оракул проганяє два прогони: один до заданого часу з семплами, другий до
/// перицентра. Другий проходить саме через `CoreEvent` і через код зупинки.
#[test]
fn propagation_matches_the_c_oracle_bit_for_bit() {
    // Ті самі літерали, що в core-sys/oracle.c. Апарат заданий числами, а не
    // порахований: оракул лінкується без libm, тож sqrt там немає.
    const VESSEL_T0: f64 = DAY;
    const VESSEL_DX: f64 = 42_164.0e3;
    const VESSEL_VY: f64 = 1967.84;
    const VESSEL_VZ: f64 = 1475.88;
    // Низька орбіта для перевірки опору (ROADMAP K7b); дзеркало
    // core-sys/oracle.c, де пояснено, чому потрібна друга.
    const LEO_DX: f64 = 6_698_137.0;
    const LEO_VY: f64 = 6680.0;
    const LEO_VZ: f64 = 3860.0;
    const CAP: usize = 64;

    let records = oracle_records();
    let oracle_samples = tagged(&records, "samp");
    let oracle_runs = tagged(&records, "run");
    let oracle_ends = tagged(&records, "end");

    assert!(!oracle_samples.is_empty(), "оракул не дав жодного семпла");
    assert_eq!(oracle_runs.len(), 2, "оракул мав дати два прогони");
    assert_eq!(oracle_ends.len(), 2);

    unsafe {
        let ctx = load_fixture();

        let mut earth = State::default();
        assert_eq!(eph_body_state(ctx, 3, VESSEL_T0, &mut earth), CORE_OK);

        let mut vessel = State {
            r: earth.r,
            v: earth.v,
            t: VESSEL_T0,
        };
        vessel.r.x += VESSEL_DX;
        vessel.v.y += VESSEL_VY;
        vessel.v.z += VESSEL_VZ;

        // density_scale = 1 дзеркалить оракул: він теж будує конфігурацію з
        // одиницею, і саме тому ці два прогони можна порівнювати бітово.
        let cfg = PropConfig {
            integrator: CORE_INTEG_DOP853,
            tol_m: 1e-2,
            h_max_s: 1800.0,
            max_steps: 0,
            density_scale: 1.0,
        };

        let mut p: *mut PropagatorCtx = std::ptr::null_mut();
        assert_eq!(prop_create(ctx, &cfg, &mut p), CORE_OK);
        assert!(!p.is_null(), "prop_create повернув CORE_OK і NULL");

        // ---- Прогін перший: семпли до заданого часу.
        let mut samples = vec![State::default(); CAP];
        let mut count: usize = 0;
        let mut final_state = State::default();
        let mut stop: core_sys::CoreStopReason = -1;
        let mut event: i32 = -2;
        let mut step = 0.0f64;

        let result = prop_run(
            p,
            &vessel,
            std::ptr::null(),
            VESSEL_T0 + 0.5 * DAY,
            std::ptr::null(),
            0,
            samples.as_mut_ptr(),
            CAP,
            &mut count,
            &mut final_state,
            &mut stop,
            &mut event,
            &mut step,
        );
        assert_eq!(result, CORE_OK);

        assert_eq!(
            count,
            oracle_samples.len(),
            "кількість семплів розійшлася: буфер із Rust наповнюється не так, \
             як у C"
        );
        for (k, from_c) in oracle_samples.iter().enumerate() {
            assert_eq!(from_c.values[0] as usize, k, "порядок семплів оракула");
            same_bits(&from_c.state(1), &samples[k], &format!("семпл {k}"));
        }

        let run = &oracle_runs[0];
        assert_eq!(run.values[0] as usize, count, "out_count");
        assert_eq!(run.values[1] as i32, stop, "код зупинки");
        assert_eq!(run.values[2] as i32, event, "індекс події");
        assert_eq!(
            run.values[3].to_bits(),
            step.to_bits(),
            "перенесений крок: {} проти {}",
            run.values[3],
            step
        );
        same_bits(&oracle_ends[0].state(0), &final_state, "кінцевий стан");

        // ---- Прогін другий: зупинка на події.
        let ev = CoreEvent {
            kind: CORE_EVENT_PERIAPSIS,
            body_id: 3,
            param: 0.0,
        };

        step = 0.0;
        let result = prop_run(
            p,
            &vessel,
            std::ptr::null(),
            VESSEL_T0 + 4.0 * DAY,
            &ev,
            1,
            std::ptr::null_mut(),
            0,
            &mut count,
            &mut final_state,
            &mut stop,
            &mut event,
            &mut step,
        );
        assert_eq!(result, CORE_OK);

        // Не лише «збіглося з оракулом», а й «сталося те, про що просили»:
        // якби подія не спрацювала, обидва боки однаково дійшли б до t_end і
        // звірка мовчки пройшла б.
        assert_eq!(stop, CORE_STOP_EVENT, "подія мала зупинити прогін");
        assert_eq!(event, 0);

        let run = &oracle_runs[1];
        assert_eq!(run.values[1] as i32, stop);
        assert_eq!(run.values[2] as i32, event);
        assert_eq!(run.values[3].to_bits(), step.to_bits(), "крок після події");
        same_bits(&oracle_ends[1].state(0), &final_state, "стан на події");

        // --- prop_run_stm (ROADMAP K8) --------------------------------
        //
        // Дві різні заяви, і друга не випливає з першої: що межа доносить
        // 36 чисел матриці без перестановок, і що траєкторія при цьому
        // бітово та сама, що дав би prop_run.
        let oracle_stm_run = tagged(&records, "stmrun");
        let oracle_stm_end = tagged(&records, "stmend");
        let oracle_stm = tagged(&records, "stm");

        assert_eq!(oracle_stm_run.len(), 1);
        assert_eq!(oracle_stm_end.len(), 1);
        assert_eq!(oracle_stm.len(), 36, "матриця мусить бути 6x6");

        let mut stm_final = State::default();
        let mut phi = [0.0f64; 36];
        let mut stm_step = 0.0f64;

        let result = prop_run_stm(
            p,
            &vessel,
            std::ptr::null(),
            VESSEL_T0 + 0.5 * DAY,
            &mut stm_final,
            phi.as_mut_ptr(),
            &mut stm_step,
        );
        assert_eq!(result, CORE_OK);

        assert_eq!(
            oracle_stm_run[0].values[0].to_bits(),
            stm_step.to_bits(),
            "крок після прогону з матрицею"
        );
        same_bits(&oracle_stm_end[0].state(0), &stm_final, "кінцевий стан STM");

        // Порядок елементів — рядково-мажорний, і оракул несе індекс у
        // кожному рядку, тож перестановка рядків з стовпцями впала б тут,
        // а не проявилась як дивна корекція через півроку.
        for (k, record) in oracle_stm.iter().enumerate() {
            assert_eq!(record.values[0] as usize, k, "порядок елементів STM");
            assert_eq!(
                record.values[1].to_bits(),
                phi[k].to_bits(),
                "елемент STM {k}"
            );
        }

        // --- Апарат, що відчуває тиск світла (ROADMAP K6b) -------------
        //
        // Тут перевіряється не фізика — її міряє core/test/test_srp.c, —
        // а оголошення `VesselParams`: переставлені місцями `area_m2` і
        // `cr` дали б цілком правдоподібну траєкторію, лише не ту.
        // Оракул рахує ту саму ланку в C і друкує, що вийшло.
        let oracle_srp_run = tagged(&records, "srprun");
        let oracle_srp_end = tagged(&records, "srpend");
        assert_eq!(oracle_srp_run.len(), 1);
        assert_eq!(oracle_srp_end.len(), 1);

        let sail = core_sys::VesselParams {
            mass_kg: 1000.0,
            area_m2: 20.0,
            cr: 1.3,
            cd: 0.0,
        };

        let mut srp_final = State::default();
        step = 0.0;
        let result = prop_run(
            p,
            &vessel,
            &sail,
            VESSEL_T0 + 0.5 * DAY,
            std::ptr::null(),
            0,
            samples.as_mut_ptr(),
            CAP,
            &mut count,
            &mut srp_final,
            &mut stop,
            &mut event,
            &mut step,
        );
        assert_eq!(result, CORE_OK);

        let run = &oracle_srp_run[0];
        assert_eq!(run.values[0] as usize, count, "кількість семплів під SRP");
        assert_eq!(run.values[3].to_bits(), step.to_bits(), "крок під SRP");
        same_bits(
            &oracle_srp_end[0].state(0),
            &srp_final,
            "кінцевий стан під SRP",
        );

        // І він таки інший: якби вказівник на апарат нікуди не доходив,
        // усе вище збіглося б з оракулом, який теж нічого не відчув.
        let moved = ((srp_final.r.x - final_state.r.x).powi(2)
            + (srp_final.r.y - final_state.r.y).powi(2)
            + (srp_final.r.z - final_state.r.z).powi(2))
        .sqrt();
        assert!(
            moved > 1.0,
            "апарат з площею мав полетіти інакше, а зрушив на {moved} м"
        );

        // Матриця не одинична й не порожня — інакше все вище звірялося б із
        // нулями й проходило б на будь-якій помилці.
        let off_diagonal: f64 = (0..36)
            .filter(|k| k / 6 != k % 6)
            .map(|k| phi[k].abs())
            .sum();
        assert!(off_diagonal > 1.0, "STM виглядає одиничною: {phi:?}");

        // --- Апарат, що відчуває повітря (ROADMAP K7b) -----------------
        //
        // Та сама причина, що й для SRP вище, і на одне поле гостріша:
        // `cr` і `cd` стоять поруч, мають однаковий тип і правдоподібні
        // значення один для одного, тож переставлені місцями вони дали б
        // траєкторію, яка виглядає бездоганно. Ланка низька навмисно — на
        // геостаціонарі, де летить апарат вище, повітря немає взагалі, і
        // прогін з `cd` надрукував би те саме, що без нього.
        let oracle_drag_run = tagged(&records, "dragrun");
        let oracle_drag_end = tagged(&records, "dragend");
        assert_eq!(oracle_drag_run.len(), 1);
        assert_eq!(oracle_drag_end.len(), 1);

        let mut low = State::default();
        let result = eph_body_state(ctx, 3, VESSEL_T0, &mut low);
        assert_eq!(result, CORE_OK);
        low.r.x += LEO_DX;
        low.v.y += LEO_VY;
        low.v.z += LEO_VZ;
        low.t = VESSEL_T0;

        let blunt = core_sys::VesselParams {
            mass_kg: 1000.0,
            area_m2: 20.0,
            cr: 1.3,
            cd: 2.2,
        };

        let mut drag_final = State::default();
        step = 0.0;
        let result = prop_run(
            p,
            &low,
            &blunt,
            VESSEL_T0 + 600.0,
            std::ptr::null(),
            0,
            samples.as_mut_ptr(),
            CAP,
            &mut count,
            &mut drag_final,
            &mut stop,
            &mut event,
            &mut step,
        );
        assert_eq!(result, CORE_OK);

        let run = &oracle_drag_run[0];
        assert_eq!(
            run.values[0] as usize, count,
            "кількість семплів під опором"
        );
        assert_eq!(run.values[3].to_bits(), step.to_bits(), "крок під опором");
        same_bits(
            &oracle_drag_end[0].state(0),
            &drag_final,
            "кінцевий стан під опором",
        );

        // І опір таки щось зробив: та сама ланка без `cd` мусить прийти
        // в інше місце. Без цього все вище звірялося б із оракулом, який
        // теж пролетів крізь вакуум.
        let dry = core_sys::VesselParams { cd: 0.0, ..blunt };
        let mut dry_final = State::default();
        step = 0.0;
        let result = prop_run(
            p,
            &low,
            &dry,
            VESSEL_T0 + 600.0,
            std::ptr::null(),
            0,
            samples.as_mut_ptr(),
            CAP,
            &mut count,
            &mut dry_final,
            &mut stop,
            &mut event,
            &mut step,
        );
        assert_eq!(result, CORE_OK);

        let moved = ((drag_final.r.x - dry_final.r.x).powi(2)
            + (drag_final.r.y - dry_final.r.y).powi(2)
            + (drag_final.r.z - dry_final.r.z).powi(2))
        .sqrt();
        assert!(
            moved > 1e-3,
            "апарат з cd мав загальмувати, а розійшовся на {moved} м"
        );

        prop_free(p);
        eph_free(ctx);
    }
}

/// `prop_free(NULL)` дозволений — так каже `core/prop.h`, і H4 на це спирається.
#[test]
fn freeing_a_null_propagator_is_allowed() {
    unsafe {
        prop_free(std::ptr::null_mut());
    }
}

/// Ламберт через межу дає ті самі біти, що з C (ROADMAP L3, борг D1).
///
/// **Найтонше місце — структура за значенням.** `lambert_solve` перша на межі
/// бере `Vec3d` не вказівником: 24 байти, тобто в регістри жодного нашого ABI
/// вона не влазить і їде через пам'ять. Якби Rust і C розійшлися в тому, як
/// саме, тест не впав би з помилкою — він повернув би правдоподібні швидкості
/// для іншої геометрії. Тому звірка бітова, і тому оракул задає точку `r2` з
/// ненульовим z: помилка, яка плутає порядок полів, на площині xy має шанс
/// сховатися.
#[test]
fn lambert_matches_the_c_oracle_bit_for_bit() {
    let records = records_from(ORACLE_PLANNING);
    let solved = tagged(&records, "lam");
    assert_eq!(
        solved.len(),
        2,
        "оракул планування мав дати дві розв'язані задачі (пряму й зворотну)"
    );

    // Ті самі числа, що в core-sys/oracle_planning.c. Дублювання свідоме:
    // тест, який брав би аргументи з виводу оракула, звіряв би оракул сам із
    // собою і пройшов би навіть тоді, коли Rust передав у C зовсім інше.
    let r1 = core_sys::Vec3d {
        x: 1.4959787e11,
        y: 0.0,
        z: 0.0,
    };
    let r2 = core_sys::Vec3d {
        x: -1.9e11,
        y: 1.1e11,
        z: 8.0e9,
    };
    let mu = 1.32712440018e20;
    let dt = 2.5e7;

    for (i, prograde) in [1, 0].into_iter().enumerate() {
        let mut v1 = core_sys::Vec3d::default();
        let mut v2 = core_sys::Vec3d::default();

        let result =
            unsafe { core_sys::lambert_solve(r1, r2, dt, mu, prograde, 0, &mut v1, &mut v2) };
        assert_eq!(result, CORE_OK, "prograde = {prograde}");

        let expected = &solved[i].values;
        let got = [v1.x, v1.y, v1.z, v2.x, v2.y, v2.z];

        for (k, (&c, &rust)) in expected.iter().zip(got.iter()).enumerate() {
            assert_eq!(
                c.to_bits(),
                rust.to_bits(),
                "prograde = {prograde}, компонента {k}: C дало {c:.17e}, \
                 Rust {rust:.17e}.\n\
                 Це передача структури за значенням або порядок аргументів, \
                 а не фізика."
            );
        }
    }
}

/// Відмови Ламберта теж перетинають межу як відмови.
///
/// Дзеркало до попереднього тесту й окремий сенс: `CoreResult` оголошений як
/// `c_int` з константами саме тому, що Rust-енум зі значенням поза переліком
/// був би UB. Це має вартість лише тоді, коли значення справді звіряють.
#[test]
fn lambert_refusals_cross_the_boundary() {
    let records = records_from(ORACLE_PLANNING);
    let refused = tagged(&records, "lerr");
    assert_eq!(refused.len(), 2, "оракул мав дати дві відмови");

    let r1 = core_sys::Vec3d {
        x: 1.4959787e11,
        y: 0.0,
        z: 0.0,
    };
    let r2 = core_sys::Vec3d {
        x: -1.9e11,
        y: 1.1e11,
        z: 8.0e9,
    };
    let opposite = core_sys::Vec3d {
        x: -r1.x,
        y: -r1.y,
        z: -r1.z,
    };
    let mu = 1.32712440018e20;
    let dt = 2.5e7;

    let mut v1 = core_sys::Vec3d::default();
    let mut v2 = core_sys::Vec3d::default();

    // Багатообертовий випадок: lambert.h каже, що n_revs мусить бути 0.
    let many_revs = unsafe { core_sys::lambert_solve(r1, r2, dt, mu, 1, 1, &mut v1, &mut v2) };
    // Вироджена геометрія: r1 і r2 на одній прямій через початок.
    let collinear =
        unsafe { core_sys::lambert_solve(r1, opposite, dt, mu, 1, 0, &mut v1, &mut v2) };

    for (label, got, expected) in [
        ("n_revs = 1", many_revs, refused[0].values[0] as i32),
        ("колінеарні r1 і r2", collinear, refused[1].values[0] as i32),
    ] {
        assert_eq!(
            got, expected,
            "{label}: C повернуло {expected}, Rust побачив {got}"
        );
        assert_eq!(
            got, CORE_ERR_INVALID_ARG,
            "{label}: і це мав бути саме CORE_ERR_INVALID_ARG"
        );
    }
}

/// `mu` теж звіряється бітово — на тих самих тілах, що й радіуси.
///
/// Окремим тестом, а не рядком у попередньому: це різні поля контексту, і
/// зсув на одне тіло в масиві `mu` виглядав би як цілком розумна гравітація.
#[test]
fn gravitational_parameters_match_the_c_oracle_bit_for_bit() {
    let records = oracle_records();
    let mus = tagged(&records, "mu");
    assert!(!mus.is_empty(), "оракул не дав жодного рядка mu");

    unsafe {
        let ctx = load_fixture();

        for record in &mus {
            let body = record.values[0] as i32;
            let expected = record.values[1];
            let got = eph_body_mu(ctx, body);

            assert_eq!(
                got.to_bits(),
                expected.to_bits(),
                "тіло {body}: C дав {expected:e}, а межа {got:e}"
            );
            assert!(got > 0.0, "тіло {body} у фікстурі мусить мати масу");
        }

        assert_eq!(
            eph_body_mu(ctx, 999),
            0.0,
            "невідоме тіло — нуль, як і радіус"
        );

        eph_free(ctx);
    }
}

/// Сітка porkchop перетинає межу бітово (ROADMAP-UI.md, U5a).
///
/// Функція повертає **масив структур**, і це нове на межі: досі туди їздили
/// або скаляри, або `State`. Переставлені `t1` і `tof` дали б цілком
/// правдоподібний плот — обидва додатні, обидва в секундах, — тож звіряються
/// всі чотири поля кожної клітинки.
#[test]
fn the_porkchop_grid_matches_the_c_oracle_bit_for_bit() {
    let records = records_from(ORACLE_PLANNING);
    let cells = tagged(&records, "pork");
    assert!(!cells.is_empty(), "оракул не дав жодного рядка pork");

    unsafe {
        let ctx = load_fixture();

        const EARTH: i32 = 3;
        const MOON: i32 = 4;
        let day = 86400.0;
        let t1s = [0.0, 3.0 * day, 6.0 * day];
        let tofs = [4.0 * day, 5.0 * day];

        let mut grid = [PorkchopPoint::default(); 6];
        let mut count: usize = 0;
        let result = porkchop_compute_eph(
            ctx,
            EARTH,
            MOON,
            eph_body_mu(ctx, EARTH),
            1,
            t1s.as_ptr(),
            t1s.len(),
            tofs.as_ptr(),
            tofs.len(),
            grid.as_mut_ptr(),
            grid.len(),
            &mut count,
        );

        assert_eq!(result, CORE_OK);
        assert_eq!(count, cells.len(), "кількість клітинок розійшлася");

        for (k, cell) in cells.iter().enumerate() {
            let got = grid[k];
            for (name, from_c, from_rust) in [
                ("t1", cell.values[1], got.t1),
                ("tof", cell.values[2], got.tof),
                ("v_inf_depart", cell.values[3], got.v_inf_depart),
                ("v_inf_arrive", cell.values[4], got.v_inf_arrive),
            ] {
                assert_eq!(
                    from_c.to_bits(),
                    from_rust.to_bits(),
                    "клітинка {k}, {name}: C дав {from_c:e}, межа {from_rust:e}"
                );
            }
        }

        // Замалий буфер — це відмова з кількістю, а не тиша: та сама угода,
        // що в `prop_run`, і перевіряти її треба тут, бо саме нею викликач
        // дізнається, скільки місця просити.
        let mut one = [PorkchopPoint::default(); 1];
        let mut written: usize = 0;
        let squeezed = porkchop_compute_eph(
            ctx,
            EARTH,
            MOON,
            eph_body_mu(ctx, EARTH),
            1,
            t1s.as_ptr(),
            t1s.len(),
            tofs.as_ptr(),
            tofs.len(),
            one.as_mut_ptr(),
            one.len(),
            &mut written,
        );
        assert_eq!(squeezed, CORE_ERR_BUFFER_TOO_SMALL);
        assert_eq!(written, 1, "кількість написаного мала дійти й при відмові");

        eph_free(ctx);
    }
}
