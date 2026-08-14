//! Сирі FFI-декларації числового ядра (ROADMAP D2, PROJECT.md §5).
//!
//! Написані **вручну**, не bindgen. Межа мала — ціль ~20 функцій, — а bindgen
//! тягне залежність від libclang і генерує те, чого ніхто не читає. Тут кожен
//! рядок має бути прочитаний очима, бо це єдине місце, де помилка не
//! діагностується: переплутані поля структури чи не той цілий тип не падають,
//! вони повертають правдоподібні числа.
//!
//! **Тут немає жодного `unsafe`-блоку** — лише оголошення. Наш `unsafe` живе
//! в одному місці, і це `core-rs` (CLAUDE.md, інваріант 1). Викликати щось
//! звідси напряму з `engine` чи `game` — помилка архітектури, а не скорочення.
//!
//! Безпечної обгортки, RAII й `Result` тут теж немає: це D3.

#![no_std]

use core::ffi::{c_char, c_int, c_long};

/// Код повернення. У C це `enum CoreResult`, тут — ціле число.
///
/// Не `#[repr(C)] enum`, і це не спрощення, а коректність: якщо C коли-небудь
/// поверне значення поза переліком, Rust-енум із таким значенням — це
/// невизначена поведінка, а не помилка, яку видно. Сирий шар віддає рівно те,
/// що сказав C; перетворення на справжній `enum` із гілкою «щось інше» — робота
/// `core-rs`, де для цього є місце.
pub type CoreResult = c_int;

pub const CORE_OK: CoreResult = 0;
pub const CORE_ERR_BUFFER_TOO_SMALL: CoreResult = 1;
pub const CORE_ERR_TOLERANCE_NOT_MET: CoreResult = 2;
pub const CORE_ERR_INVALID_ARG: CoreResult = 3;

/// `Vec3d` з `core/vec3.h`. Метри або метри за секунду.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3d {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// `State` з `core/core.h`: баріцентрична інерціальна система, метри, м/с,
/// `t` — секунди від епохи завантаженої ефемериди.
///
/// Порядок полів — частина контракту межі, а не деталь. `core.h` прямо каже,
/// що це має лишатися простою структурою з `double` без сюрпризів з
/// вирівнюванням, саме щоб її можна було оголосити ось так.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct State {
    pub r: Vec3d,
    pub v: Vec3d,
    pub t: f64,
}

/// `VesselParams` з `core/core.h` (ROADMAP K6b, K7b).
///
/// Гравітації вона не потрібна — апарат там безмасова пробна частинка, і це
/// не наближення, а поділ, на якому стоїть архітектура. Тиску світла —
/// потрібна: прискорення від нього масштабується на `Cr·A/m`, тобто на
/// властивість самого апарата. Опору — так само, на `Cd·A/m`.
///
/// `cd` чекав саме на K7b і атмосферу, яка його читає: до неї це було б
/// поле, яке викликач заповнює, а ядро ігнорує, і ніщо про це не скаже.
///
/// **Порядок полів — контракт**, і саме тут його найлегше зламати тихо:
/// переставлені `cr` і `cd` дали б цілком правдоподібну траєкторію, лише не
/// ту. Тому `core-sys/oracle.c` жене ланку з ненульовими обома, а
/// `tests/ffi.rs` звіряє біти.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VesselParams {
    pub mass_kg: f64,
    pub area_m2: f64,
    pub cr: f64,
    pub cd: f64,
}

/// Непрозорий хендл ефемериди.
///
/// Порожнє приватне поле — навмисно: воно робить тип неконструйованим ззовні
/// крейта, тож єдиний спосіб отримати `*mut EphemerisCtx` — це `eph_load`.
/// Устрій структури живе в `core/ephemeris.c` і Rust про нього не знає й
/// знати не повинен (PROJECT.md §5, правило 2).
#[repr(C)]
pub struct EphemerisCtx {
    _opaque: [u8; 0],
}

/// Непрозорий хендл пропагатора (`core/prop.h`, ROADMAP H3).
///
/// Позичає ефемериду й не володіє нею: контекст ефемериди мусить пережити
/// кожен пропагатор, збудований на ньому. У `core-rs` це не обіцянка, а тип —
/// обгортка тримає `Arc`.
#[repr(C)]
pub struct PropagatorCtx {
    _opaque: [u8; 0],
}

/// Вибір інтегратора. `CoreIntegrator` у C — теж `enum`, тобто `int`.
pub type CoreIntegrator = c_int;

pub const CORE_INTEG_DOP853: CoreIntegrator = 0;
pub const CORE_INTEG_RKN: CoreIntegrator = 1;

/// Чому прогін скінчився. Значення поза переліком тут так само неприпустимі
/// для Rust-енума, як і в `CoreResult`, тож це ціле число.
pub type CoreStopReason = c_int;

pub const CORE_STOP_T_END: CoreStopReason = 0;
pub const CORE_STOP_BUFFER_FULL: CoreStopReason = 1;
pub const CORE_STOP_EVENT: CoreStopReason = 2;

pub type CoreEventKind = c_int;

pub const CORE_EVENT_PERIAPSIS: CoreEventKind = 0;
pub const CORE_EVENT_APOAPSIS: CoreEventKind = 1;
pub const CORE_EVENT_DISTANCE: CoreEventKind = 2;

/// Скільки подій `prop_run` бере за раз (`PROP_MAX_EVENTS`).
pub const PROP_MAX_EVENTS: usize = 8;

/// `PropConfig` з `core/prop.h`.
///
/// `max_steps` — `c_long`, бо в C це `long`: на Linux і macOS це 64 біти, на
/// Windows 32. `c_long` іде за платформою так само, тож обидва боки згодні.
/// Це межа кількості кроків, а не арифметика, тож різна ширина на різних
/// платформах детермінізму не торкається.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PropConfig {
    pub integrator: CoreIntegrator,
    pub tol_m: f64,
    pub h_max_s: f64,
    pub max_steps: c_long,
}

/// `CoreEvent` з `core/prop.h`: подія, описана даними.
///
/// Саме така структура — `enum`, `int`, `double` поспіль — і є місцем, де
/// вирівнювання розходиться тихо, тож у `tests/ffi.rs` прогін з озброєною
/// подією звіряється з C бітово.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CoreEvent {
    pub kind: CoreEventKind,
    pub body_id: c_int,
    pub param: f64,
}

extern "C" {
    /// Завантажує скукований ассет. `path` має бути C-рядком із `\0`.
    ///
    /// Одна з двох алокуючих функцій усього API. Пара до [`eph_free`], і
    /// порушення цієї пари — єдина витік-небезпека межі; за конструкцією типів
    /// вона стає неможливою в `core-rs` (D3).
    pub fn eph_load(path: *const c_char, out: *mut *mut EphemerisCtx) -> CoreResult;

    /// Звільняє контекст. `NULL` допустимий.
    pub fn eph_free(ctx: *mut EphemerisCtx);

    /// Положення й швидкість тіла в момент `t`.
    ///
    /// Повертає [`CORE_ERR_INVALID_ARG`] для невідомого тіла або часу поза
    /// проміжком ассета: екстраполяція чебишевської підгонки дає впевнену
    /// дурницю, тож вихід за межі — це подія, про яку викликач має почути.
    pub fn eph_body_state(
        ctx: *const EphemerisCtx,
        body: c_int,
        t: f64,
        out: *mut State,
    ) -> CoreResult;

    /// Створює пропагатор над ефемеридою. Друга (і остання) алокуюча пара
    /// межі; `prop_free(NULL)` дозволений.
    pub fn prop_create(
        eph: *const EphemerisCtx,
        cfg: *const PropConfig,
        out: *mut *mut PropagatorCtx,
    ) -> CoreResult;

    /// Звільняє контекст. `NULL` допустимий.
    pub fn prop_free(p: *mut PropagatorCtx);

    /// Інтегрує апарат від `initial` до `t_end`, до першої озброєної події
    /// або поки не заповниться `out_states`.
    ///
    /// Буфер дає **Rust**, а C лише заповнює його й повертає фактичну
    /// кількість (PROJECT.md §5, правило 1) — тому питання «хто звільняє»
    /// не виникає взагалі.
    ///
    /// `in_out_step` несе крок інтегратора між викликами. Нуль на першому
    /// виклику означає «обери сам»; далі туди слід повертати те, що функція
    /// там лишила, — інакше траєкторія буде інша, і це виміряно
    /// (`core/test/test_prop.c`).
    #[allow(clippy::too_many_arguments)]
    pub fn prop_run(
        p: *mut PropagatorCtx,
        initial: *const State,
        vessel: *const VesselParams,
        t_end: f64,
        events: *const CoreEvent,
        n_events: usize,
        out_states: *mut State,
        out_cap: usize,
        out_count: *mut usize,
        out_final: *mut State,
        out_stop: *mut CoreStopReason,
        out_event: *mut c_int,
        in_out_step: *mut f64,
    ) -> CoreResult;

    /// Те саме інтегрування, але несе матрицю переходу стану (ROADMAP K8).
    ///
    /// `out_stm` — рядково-мажорна 6×6, тобто рівно `STM_SIZE` = 36 `f64`;
    /// порядок стану `(x, y, z, vx, vy, vz)`. Буфер дає Rust, як і скрізь на
    /// цій межі.
    ///
    /// **Траєкторія бітово та сама, що в `prop_run`** — не «в межах
    /// допуску»: контролер кроку в `core/dop853.c` читає лише блок 0, тож
    /// шість варіаційних блоків їдуть тією ж послідовністю кроків, не
    /// впливаючи на неї. Виміряно в `core/test/test_prop.c`. Це CLAUDE.md
    /// інваріант 5 у місці, де його найлегше втратити.
    ///
    /// Подій і буфера семплів тут немає навмисно: питання «куди дійде
    /// зміна початкового стану до `t_end`» стосується однієї ланки з двома
    /// кінцями, а подія обірвала б її там, де викликач не просив.
    ///
    /// `vessel` — як у `prop_run`: null означає безмасову пробну частинку.
    pub fn prop_run_stm(
        p: *mut PropagatorCtx,
        initial: *const State,
        vessel: *const VesselParams,
        t_end: f64,
        out_final: *mut State,
        out_stm: *mut f64,
        in_out_step: *mut f64,
    ) -> CoreResult;
}
