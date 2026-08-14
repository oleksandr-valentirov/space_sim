//! Безпечна обгортка над числовим ядром (ROADMAP D3, PROJECT.md §5).
//!
//! **Це єдине місце в проєкті з нашим `unsafe`** (CLAUDE.md, інваріант 1).
//! Сторонні `-sys` крейти — виняток; наш код більше ніде його не пише. Тому
//! цей файл має лишатися маленьким і нудним: усе, що можна зробити зовні
//! в безпечному Rust, робиться зовні.
//!
//! Обіцянка обгортки не в тому, що помилок немає, а в тому, що двох
//! конкретних помилок не можна зробити навіть навмисно:
//!
//! - **подвійне звільнення** — `eph_free` не експортується, а поле з
//!   вказівником приватне. Звільнення відбувається рівно один раз, у `Drop`.
//! - **використання після звільнення** — `Ephemeris` не `Copy` і не `Clone`,
//!   тож після `drop` значення переміщене, і компілятор не дасть його
//!   торкнутися.
//!
//! Обидві обіцянки перевіряються `compile_fail`-доктестами нижче, а не
//! коментарем.
//!
//! ## Стиль
//!
//! Свідомо простий (CLAUDE.md): конкретні типи замість дженериків, `&Path`
//! замість `impl AsRef<Path>`, ніяких лайфтаймів у структурах. `State` і
//! `Vec3d` реекспортуються з `core-sys` як є — це прості структури з `double`,
//! і шар перетворення додав би роботу без жодної нової гарантії.

use std::ffi::CString;
use std::fmt;
use std::path::Path;
use std::sync::Arc;

pub use core_sys::{State, Vec3d};

/// Помилка ядра.
///
/// `Unknown` існує не для повноти. `core-sys` віддає код повернення як
/// `c_int`, бо Rust-енум зі значенням поза переліком — невизначена поведінка;
/// перетворення на цей тип і є те місце, де невідоме значення стає видимою
/// помилкою замість тихого пошкодження.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreError {
    /// Наданий буфер замалий; C повертає потрібну кількість окремо.
    BufferTooSmall,
    /// Ітерація не збіглася до заданого допуску.
    ToleranceNotMet,
    /// Некоректний аргумент: невідоме тіло, час поза проміжком ассета.
    InvalidArg,
    /// Код, якого немає в `CoreResult`. Означає розсинхрон між C і межею.
    Unknown(i32),
    /// Шлях не вдалося передати в C: не UTF-8 або містить `\0` усередині.
    /// Помилка нашого боку, ядро її не бачило.
    BadPath,
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::BufferTooSmall => write!(f, "буфер замалий"),
            CoreError::ToleranceNotMet => write!(f, "не збіглося до допуску"),
            CoreError::InvalidArg => write!(f, "некоректний аргумент"),
            CoreError::Unknown(code) => {
                write!(f, "невідомий код ядра {code} — межа розійшлася з C")
            }
            CoreError::BadPath => write!(f, "шлях не UTF-8 або містить \\0"),
        }
    }
}

impl std::error::Error for CoreError {}

pub type Result<T> = std::result::Result<T, CoreError>;

fn check(code: core_sys::CoreResult) -> Result<()> {
    match code {
        core_sys::CORE_OK => Ok(()),
        core_sys::CORE_ERR_BUFFER_TOO_SMALL => Err(CoreError::BufferTooSmall),
        core_sys::CORE_ERR_TOLERANCE_NOT_MET => Err(CoreError::ToleranceNotMet),
        core_sys::CORE_ERR_INVALID_ARG => Err(CoreError::InvalidArg),
        other => Err(CoreError::Unknown(other)),
    }
}

/// Завантажена ефемерида. Звільняється сама.
///
/// ```no_run
/// use core_rs::Ephemeris;
/// use std::path::Path;
///
/// let eph = Ephemeris::load(Path::new("data/fixture/earth_moon.eph"))?;
/// let moon = eph.body_state(4, 0.0)?;
/// println!("{:?}", moon.r);
/// # Ok::<(), core_rs::CoreError>(())
/// ```
///
/// Використання після звільнення не компілюється:
///
/// ```compile_fail
/// use core_rs::Ephemeris;
/// use std::path::Path;
///
/// let eph = Ephemeris::load(Path::new("data/fixture/earth_moon.eph")).unwrap();
/// drop(eph);
/// let _ = eph.body_state(4, 0.0);
/// ```
///
/// Звільнити двічі теж не вийде — `eph_free` просто нема звідки взяти, а поле
/// з вказівником приватне:
///
/// ```compile_fail
/// use core_rs::Ephemeris;
/// use std::path::Path;
///
/// let eph = Ephemeris::load(Path::new("data/fixture/earth_moon.eph")).unwrap();
/// let _ = eph.ctx;
/// ```
pub struct Ephemeris {
    ctx: *mut core_sys::EphemerisCtx,
}

// Читання ефемериди не має спільного мутабельного стану, і це перевірено, а
// не припущено: `eph_body_state` бере `const EphemerisCtx*`, торкається лише
// полів контексту й купи, до якої той володіє, і не має ні статиків, ні кешу
// (`core/ephemeris.c`, `core/cheb.c`). Контекст після `eph_load` не
// змінюється взагалі.
//
// Тому вказівник можна переносити між потоками (`Send`), а `&Ephemeris` —
// читати з кількох одночасно (`Sync`). Це знадобиться, щойно фізика поїде у
// свій потік (PROJECT.md §6), і краще, щоб обіцянку писав той, хто щойно
// прочитав C, ніж той, кому вона зрештою знадобиться.
unsafe impl Send for Ephemeris {}
unsafe impl Sync for Ephemeris {}

impl Ephemeris {
    /// Читає скукований ассет.
    pub fn load(path: &Path) -> Result<Ephemeris> {
        let text = path.to_str().ok_or(CoreError::BadPath)?;
        let c_path = CString::new(text).map_err(|_| CoreError::BadPath)?;

        let mut ctx: *mut core_sys::EphemerisCtx = std::ptr::null_mut();

        // SAFETY: c_path — валідний C-рядок, живий до кінця виклику. `ctx` —
        // валідне місце під один вказівник. C записує його лише при CORE_OK.
        let code = unsafe { core_sys::eph_load(c_path.as_ptr(), &mut ctx) };
        check(code)?;

        // Захист від контракту, якого C не порушує, але міг би: CORE_OK і
        // NULL разом. Без цієї перевірки помилка проявилася б розіменуванням
        // нуля десь далі, вже без сліду причини.
        if ctx.is_null() {
            return Err(CoreError::Unknown(core_sys::CORE_OK));
        }

        Ok(Ephemeris { ctx })
    }

    /// Положення й швидкість тіла в момент `t` (секунди від епохи ассета).
    ///
    /// Час поза проміжком ассета — [`CoreError::InvalidArg`], а не
    /// екстраполяція: продовження чебишевської підгонки за її межі дає
    /// впевнену дурницю.
    pub fn body_state(&self, body: i32, t: f64) -> Result<State> {
        let mut state = State::default();

        // SAFETY: self.ctx отримано з eph_load і ще не звільнено — звільняє
        // лише Drop, а самоволодіння гарантує, що Drop ще не був. `state` —
        // валідне місце під State.
        let code = unsafe { core_sys::eph_body_state(self.ctx, body, t, &mut state) };
        check(code)?;

        Ok(state)
    }

    /// Середній радіус тіла, метри; нуль, якщо ассет не каже (ROADMAP U2a).
    ///
    /// Це та сама сфера, від якої міряють висоту атмосфера (K7a) і подія
    /// [`Event::Altitude`] (K7c), — а не еталонний радіус гармонік, який у
    /// Землі інше число. Перший викликач — HUD: висота над поверхнею без
    /// радіуса не рахується, а брати його з рендера означало б, що гра міряє
    /// висоту від намальованої сфери.
    ///
    /// Не `Result`, бо в C це читання поля: невідоме тіло дає той самий нуль,
    /// що й тіло без розміру, і це рішення, а не недогляд — обидва випадки
    /// означають «розміру немає», і два різні відповіді довелося б розрізняти
    /// кожному викликачеві.
    pub fn body_radius(&self, body: i32) -> f64 {
        // SAFETY: той самий контекст, що в body_state — отриманий з eph_load,
        // ще не звільнений (звільняє лише Drop). C читає поле й нічого не
        // пише, тож вихідного місця тут не потрібно взагалі.
        unsafe { core_sys::eph_body_radius(self.ctx, body) }
    }
}

impl Drop for Ephemeris {
    fn drop(&mut self) {
        // SAFETY: вказівник отримано з eph_load, звільняється рівно тут і
        // рівно раз — поле приватне, тип не Copy і не Clone, іншого шляху до
        // eph_free немає.
        unsafe { core_sys::eph_free(self.ctx) };
    }
}

impl fmt::Debug for Ephemeris {
    /// Без адреси всередині: вона нічого не пояснює й шумить у логах.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Ephemeris")
    }
}

// ---------------------------------------------------------------------------
// Пропагатор (ROADMAP H4)
// ---------------------------------------------------------------------------

/// Який інтегратор рахує.
///
/// `Rkn` оголошений, але ядро його ще не має: `Propagator::new` з ним поверне
/// [`CoreError::InvalidArg`]. Це не забутий шматок, а те, чого вимагає
/// PROJECT.md §4 — поле вибору інтегратора існує з першого дня, щоб додати RKN
/// колись означало змінити виклик, а не переписати шар.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Integrator {
    Dop853,
    Rkn,
}

/// Налаштування пропагатора.
#[derive(Debug, Clone, Copy)]
pub struct PropConfig {
    pub integrator: Integrator,
    /// Допуск по позиції в метрах — абсолютний, не відносний.
    pub tol_m: f64,
    /// Стеля кроку в секундах. Задавайте її: з нулем стелю обирає інтегратор
    /// за довжиною ланки, і тоді зшитий прогін лишає по собі інший крок, ніж
    /// безперервний (`core/prop.h`, виміряно).
    pub h_max_s: f64,
    /// Ліміт кроків на **один виклик** `run`. 0 — типовий ліміт ядра.
    pub max_steps: i64,
    /// Множник густини повітря для всіх апаратів цього пропагатора
    /// (ROADMAP K7c). Одиниця — профіль, який несе ассет.
    ///
    /// Тут, а не у [`VesselParams`], бо описує повітря, а не корабель: два
    /// апарати на одній ланці летять крізь ту саму атмосферу. Стала на ланку,
    /// а не функція часу: сонячний цикл — синусоїда з періодом в одинадцять
    /// років, `libm` у циклі інтегрування заборонений, і де наступний максимум
    /// — все одно ніхто не скаже. Тож множник рахує гра (майбутня галочка
    /// «космічна погода»), а ядро його лише застосовує.
    ///
    /// **Нуль неприпустимий** — `new` поверне [`CoreError::InvalidArg`].
    /// Ядро свідомо не читає його як одиницю: незаданe поле має падати гучно
    /// (`core/prop.h`).
    pub density_scale: f64,
}

impl Default for PropConfig {
    /// Метр допуску й година стелі — тобто числа, з якими вже рахували
    /// фікстуру (`data/fixture/README.md`), а не круглі значення з повітря.
    fn default() -> Self {
        PropConfig {
            integrator: Integrator::Dop853,
            tol_m: 1.0,
            h_max_s: 3600.0,
            max_steps: 0,
            // Профіль ассета як він є. Множник з'явиться тоді, коли гра
            // дасть гравцеві вимикач сонячної активності.
            density_scale: 1.0,
        }
    }
}

/// Подія, на якій прогін зупиняється.
///
/// Енум замість структури з полем `param`, яке для двох видів із трьох нічого
/// не означає: тут неможливо задати перицентр із відстанню, бо такого варіанту
/// просто немає.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// Найближча точка до тіла.
    Periapsis { body: i32 },
    /// Найдальша.
    Apoapsis { body: i32 },
    /// Задана відстань від **центра** тіла, в обидва боки. Сфера впливу або
    /// кільце зустрічі — це саме відстань від центра, до поверхні вона
    /// стосунку не має.
    Distance { body: i32, metres: f64 },
    /// Задана висота над **поверхнею** тіла, в обидва боки (ROADMAP K7c).
    ///
    /// Поверхня — середній радіус з ассета, тобто та сама сфера, від якої
    /// міряє висоти атмосфера. Тіло, розміру якого ассет не називає, дає
    /// [`CoreError::InvalidArg`] при озброєнні: радіус нуль мовчки зробив би
    /// із цього [`Event::Distance`]. Нуль як висота дозволений — це поверхня.
    Altitude { body: i32, metres: f64 },
}

impl Event {
    fn raw(&self) -> core_sys::CoreEvent {
        match *self {
            Event::Periapsis { body } => core_sys::CoreEvent {
                kind: core_sys::CORE_EVENT_PERIAPSIS,
                body_id: body,
                param: 0.0,
            },
            Event::Apoapsis { body } => core_sys::CoreEvent {
                kind: core_sys::CORE_EVENT_APOAPSIS,
                body_id: body,
                param: 0.0,
            },
            Event::Distance { body, metres } => core_sys::CoreEvent {
                kind: core_sys::CORE_EVENT_DISTANCE,
                body_id: body,
                param: metres,
            },
            Event::Altitude { body, metres } => core_sys::CoreEvent {
                kind: core_sys::CORE_EVENT_ALTITUDE,
                body_id: body,
                param: metres,
            },
        }
    }
}

/// Чому прогін спинився.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// Дійшов до `t_end`.
    ReachedEnd,
    /// Скінчився буфер під семпли. Продовжуйте з `final_state` і тим самим
    /// кроком — це буде та сама траєкторія, бітово.
    BufferFull,
    /// Спрацювала подія з таким індексом у переданому зрізі.
    Event(usize),
}

/// Скільки чисел у матриці переходу: 6×6 (`STM_SIZE` у `core/stm.h`).
pub const STM_LEN: usize = 36;

/// Матриця переходу стану, рядково-мажорна 6×6 (ROADMAP K8).
///
/// Обгортка навколо масиву, а не голий `[f64; 36]`, рівно з однієї причини:
/// щоб `phi[(i, j)]` читалося як «рядок i, стовпець j» і не було місця, де
/// хтось перепише індекс навпаки. Транспонована матриця переходу — цілком
/// правдоподібна матриця, і помилка проявилась би як дивна корекція, а не
/// як падіння.
#[derive(Debug, Clone, Copy)]
pub struct Stm(pub [f64; STM_LEN]);

impl Stm {
    /// ∂y_i(t_end) / ∂y_j(t0), стан у порядку `(x, y, z, vx, vy, vz)`.
    pub fn get(&self, row: usize, col: usize) -> f64 {
        assert!(row < 6 && col < 6, "STM 6x6, а просять ({row}, {col})");
        self.0[row * 6 + col]
    }

    /// Сирі 36 чисел у тому ж порядку, в якому їх дав C.
    pub fn as_slice(&self) -> &[f64] {
        &self.0
    }
}

/// Апарат так, як його бачить модель сил (ROADMAP K6b, K7b, `core/core.h`).
///
/// Гравітації вона не потрібна: там апарат — безмасова пробна частинка, і це
/// поділ, на якому стоїть архітектура, а не наближення. Тиску сонячного
/// світла — потрібна, бо прискорення від нього масштабується на `Cr·A/m`.
/// Опору — на `Cd·A/m`.
///
/// Передається **на кожен прогін**, а не в конфігурацію пропагатора: маса
/// змінюється при горінні, а `/game` тримає один пропагатор на всі апарати
/// (`game/src/world.rs`) — апарат у конфігурації зробив би з них один
/// корабель із кількома траєкторіями.
///
/// **Одна площа на дві сили.** Переріз, підставлений Сонцю, і переріз,
/// підставлений повітрю, — тут одне число: розділити їх означало б
/// моделювати орієнтацію, а орієнтація це локальний рівень (PROJECT.md §4),
/// не цей.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VesselParams {
    pub mass_kg: f64,
    /// Площа перерізу, спільна для світла й повітря, м².
    pub area_m2: f64,
    /// 1 — повне поглинання, 2 — дзеркало; реальні апарати біля 1.3.
    pub cr: f64,
    /// Коефіцієнт опору; для тупого тіла на низькій орбіті близько 2.2.
    pub cd: f64,
}

impl VesselParams {
    fn raw(&self) -> core_sys::VesselParams {
        core_sys::VesselParams {
            mass_kg: self.mass_kg,
            area_m2: self.area_m2,
            cr: self.cr,
            cd: self.cd,
        }
    }
}

/// Що дав один виклик [`Propagator::run`].
#[derive(Debug, Clone, Copy)]
pub struct Run {
    /// Скільки семплів записано на початок переданого зрізу.
    pub filled: usize,
    /// Стан, у якому прогін спинився. Саме з нього продовжувати.
    pub final_state: State,
    pub stop: Stop,
}

/// Пропагатор апарата в полі всіх тіл ассета.
///
/// Тримає [`Arc`] на ефемериду, а не позичену ссилку: контекст у C зберігає
/// сирий вказівник на неї, тож вона мусить пережити пропагатор. Лайфтайм у
/// структурі виразив би те саме й заразив би ним усе, що пропагатор
/// зберігає (CLAUDE.md: жодних лайфтаймів у структурах).
///
/// ```no_run
/// use core_rs::{Ephemeris, Event, PropConfig, Propagator, State};
/// use std::path::Path;
/// use std::sync::Arc;
///
/// let eph = Arc::new(Ephemeris::load(Path::new("data/fixture/earth_moon.eph"))?);
/// let mut prop = Propagator::new(eph.clone(), PropConfig::default())?;
///
/// let mut samples = vec![State::default(); 256];
/// let mut step = 0.0;
/// let vessel = State::default();
///
/// let run = prop.run(&vessel, None, 86_400.0, &[Event::Periapsis { body: 3 }],
///                    &mut samples, &mut step)?;
/// println!("{:?} після {} семплів", run.stop, run.filled);
/// # Ok::<(), core_rs::CoreError>(())
/// ```
///
/// Використання після звільнення не компілюється:
///
/// ```compile_fail
/// use core_rs::{Ephemeris, PropConfig, Propagator, State};
/// use std::path::Path;
/// use std::sync::Arc;
///
/// let eph = Arc::new(Ephemeris::load(Path::new("data/fixture/earth_moon.eph")).unwrap());
/// let mut prop = Propagator::new(eph, PropConfig::default()).unwrap();
/// drop(prop);
/// let mut step = 0.0;
/// let _ = prop.run(&State::default(), None, 1.0, &[], &mut [], &mut step);
/// ```
///
/// Звільнити двічі теж нема чим — `prop_free` не реекспортується, поле
/// приватне:
///
/// ```compile_fail
/// use core_rs::{Ephemeris, PropConfig, Propagator};
/// use std::path::Path;
/// use std::sync::Arc;
///
/// let eph = Arc::new(Ephemeris::load(Path::new("data/fixture/earth_moon.eph")).unwrap());
/// let prop = Propagator::new(eph, PropConfig::default()).unwrap();
/// let _ = prop.ctx;
/// ```
pub struct Propagator {
    // Тримає ефемериду живою. Читається лише в Drop-порядку — поле мусить
    // існувати, а не використовуватись.
    _eph: Arc<Ephemeris>,
    ctx: *mut core_sys::PropagatorCtx,
}

// Пропагатор можна віддати іншому потоку — саме це й станеться, щойно фізика
// поїде у свій (PROJECT.md §6). Він володіє своїм контекстом, а ефемерида, на
// яку той дивиться, вже `Sync` (обґрунтування вище).
//
// `Sync` НЕ оголошений, і це не забутий рядок: контекст усередині C несе
// липкий прапорець помилки, який `prop_run` скидає на початку кожного
// прогону. Два потоки з `&Propagator` не могли б навіть покликати `run` —
// вона бере `&mut self`, — але заявляти безпеку, якої ніхто не перевіряв,
// немає навіщо. Один потік — один пропагатор.
unsafe impl Send for Propagator {}

impl Propagator {
    pub fn new(eph: Arc<Ephemeris>, cfg: PropConfig) -> Result<Propagator> {
        let raw = core_sys::PropConfig {
            integrator: match cfg.integrator {
                Integrator::Dop853 => core_sys::CORE_INTEG_DOP853,
                Integrator::Rkn => core_sys::CORE_INTEG_RKN,
            },
            tol_m: cfg.tol_m,
            h_max_s: cfg.h_max_s,
            max_steps: cfg.max_steps as std::ffi::c_long,
            density_scale: cfg.density_scale,
        };

        let mut ctx: *mut core_sys::PropagatorCtx = std::ptr::null_mut();

        // SAFETY: eph.ctx отримано з eph_load і живий — `Arc` нижче тримає
        // його щонайменше стільки ж, скільки цей пропагатор. `raw` живе до
        // кінця виклику, C його лише читає. `ctx` — валідне місце під один
        // вказівник, C пише туди лише при CORE_OK.
        let code = unsafe { core_sys::prop_create(eph.ctx, &raw, &mut ctx) };
        check(code)?;

        if ctx.is_null() {
            return Err(CoreError::Unknown(core_sys::CORE_OK));
        }

        Ok(Propagator { _eph: eph, ctx })
    }

    /// Інтегрує від `initial` до `t_end`, до першої події або поки не
    /// скінчиться `samples`.
    ///
    /// `samples` може бути порожнім — тоді прогін іде без семплування й
    /// зупиняється лише на `t_end` або на події. Це та сама інтеграція, крок
    /// у крок, і саме тому фізика й лінія прогнозу можуть ділити один шлях
    /// (CLAUDE.md, інваріант 5).
    ///
    /// `step` несе крок інтегратора між викликами: 0 на першому, далі —
    /// значення, яке лишив попередній. Він входить у сейв (PROJECT.md §4), і
    /// це не формальність: викинути його коштує сімдесятикратної роботи й
    /// іншої траєкторії (`core/test/test_prop.c`).
    ///
    /// `vessel` — `None` для безмасової пробної частинки, тобто рівно те, що
    /// цей виклик робив до K6b. З `Some` до сил додається тиск сонячного
    /// світла з моделлю тіні.
    pub fn run(
        &mut self,
        initial: &State,
        vessel: Option<&VesselParams>,
        t_end: f64,
        events: &[Event],
        samples: &mut [State],
        step: &mut f64,
    ) -> Result<Run> {
        let raw_events: Vec<core_sys::CoreEvent> = events.iter().map(|e| e.raw()).collect();
        let raw_vessel = vessel.map(|v| v.raw());

        let mut count: usize = 0;
        let mut final_state = State::default();
        let mut stop: core_sys::CoreStopReason = -1;
        let mut event: std::ffi::c_int = -1;

        // Порожній зріз у Rust — це НЕ нульовий вказівник, а вирівняний
        // «висячий», і C розрізняє ці випадки: буфер без місця він вважає
        // помилкою викликача, бо той крутився б у циклі без поступу. Тож
        // порожність перекладається явно.
        let (out_ptr, out_cap) = if samples.is_empty() {
            (std::ptr::null_mut(), 0)
        } else {
            (samples.as_mut_ptr(), samples.len())
        };

        let events_ptr = if raw_events.is_empty() {
            std::ptr::null()
        } else {
            raw_events.as_ptr()
        };

        // `None` перекладається в нульовий вказівник, і C читає це як
        // «безмасова пробна частинка» — той самий прогін, що був до K6b,
        // біт у біт.
        let vessel_ptr = raw_vessel
            .as_ref()
            .map_or(std::ptr::null(), |v| v as *const _);

        // SAFETY: self.ctx отримано з prop_create і ще не звільнено (звільняє
        // лише Drop, а `&mut self` доводить, що його не було). `initial` і
        // `raw_events` живі до кінця виклику й лише читаються. Буфер має рівно
        // `out_cap` елементів `State`, і C обіцяє не писати далі — рівно тому
        // місткість передається поруч із ним. `raw_vessel` живе на стеку до
        // кінця виклику й лише читається, а null там — легальне значення,
        // не помилка. Решта вказівників — місця під по одному значенню на
        // стеку.
        let code = unsafe {
            core_sys::prop_run(
                self.ctx,
                initial,
                vessel_ptr,
                t_end,
                events_ptr,
                raw_events.len(),
                out_ptr,
                out_cap,
                &mut count,
                &mut final_state,
                &mut stop,
                &mut event,
                step,
            )
        };
        check(code)?;

        let stop = match stop {
            core_sys::CORE_STOP_T_END => Stop::ReachedEnd,
            core_sys::CORE_STOP_BUFFER_FULL => Stop::BufferFull,
            core_sys::CORE_STOP_EVENT => {
                // Індекс приходить із C і вказує в зріз, який дав викликач.
                // Перевіряємо, а не довіряємо: з нього збираються зрізати.
                if event < 0 || (event as usize) >= events.len() {
                    return Err(CoreError::Unknown(event));
                }
                Stop::Event(event as usize)
            }
            other => return Err(CoreError::Unknown(other)),
        };

        Ok(Run {
            filled: count,
            final_state,
            stop,
        })
    }

    /// Та сама інтеграція, що `run`, але несе матрицю переходу стану
    /// (ROADMAP K8).
    ///
    /// Повертає кінцевий стан і 6×6 рядково-мажорну Φ = ∂y(t_end)/∂y(initial)
    /// зі станом у порядку `(x, y, z, vx, vy, vz)`. Це те, що просить
    /// диференціальна корекція M3 і чим рухається коваріація в M6.
    ///
    /// **Траєкторія бітово та сама, що дав би `run`** з тими самими
    /// аргументами й тим самим `step` — не «в межах допуску». Контролер
    /// кроку в C читає лише опорний блок, тож шість варіаційних блоків
    /// їдуть тією ж послідовністю кроків, не голосуючи за неї
    /// (`core/test/test_prop.c` це міряє). Планувальник, який виправляє
    /// маневр матрицею від трохи іншої траєкторії, цілив би туди, де
    /// апарата немає.
    ///
    /// Подій тут немає навмисно: питання стосується однієї ланки з двома
    /// кінцями, і подія обірвала б її там, де викликач не просив. Хто хоче
    /// обох — спершу `run`, щоб знайти подію, потім це на знайденій ланці.
    pub fn run_stm(
        &mut self,
        initial: &State,
        vessel: Option<&VesselParams>,
        t_end: f64,
        step: &mut f64,
    ) -> Result<(State, Stm)> {
        let mut final_state = State::default();
        let mut phi = [0.0f64; STM_LEN];
        let raw_vessel = vessel.map(|v| v.raw());
        let vessel_ptr = raw_vessel
            .as_ref()
            .map_or(std::ptr::null(), |v| v as *const _);

        // SAFETY: self.ctx отримано з prop_create і ще не звільнено (звільняє
        // лише Drop, а `&mut self` доводить, що його не було). `initial` живий
        // до кінця виклику й лише читається. `phi` — рівно STM_LEN значень
        // поспіль, стільки ж, скільки C оголосив у `out_stm[36]`; решта
        // вказівників — місця під по одному значенню на стеку.
        let code = unsafe {
            core_sys::prop_run_stm(
                self.ctx,
                initial,
                vessel_ptr,
                t_end,
                &mut final_state,
                phi.as_mut_ptr(),
                step,
            )
        };
        check(code)?;

        Ok((final_state, Stm(phi)))
    }
}

impl Drop for Propagator {
    fn drop(&mut self) {
        // SAFETY: вказівник отримано з prop_create, звільняється рівно тут і
        // рівно раз — поле приватне, тип не Copy і не Clone.
        unsafe { core_sys::prop_free(self.ctx) };
    }
}

impl fmt::Debug for Propagator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Propagator")
    }
}

/// Задача Ламберта: швидкості перельоту з `r1` у `r2` за `dt` секунд навколо
/// тіла з гравітаційним параметром `mu` (ROADMAP L3, борг D1).
///
/// Повертає `(v1, v2)` — швидкість на відльоті й на прильоті, м/с.
///
/// **Поза межею детермінізму, і це єдина така функція тут.** PROJECT.md §4:
/// симуляція заданого плану мусить збігатися біт-у-біт, а те, як гравець цей
/// план придумав, — ні. Результат Ламберта це **дані** — з них виходить
/// маневр `(час, Δv)`, і вже його виконання відтворюється точно. Тому ця
/// функція має право на `libm` (вона в окремій `libcore_planning.a`), і тому
/// її числа не входять у звірку хешів.
///
/// `prograde` — це знак z-компоненти моменту імпульсу, **не** «коротка чи
/// довга дуга». Викликач, що працює не в площині ефемериди, повертає `r1`
/// і `r2` у площину, де це справедливо, перед викликом.
///
/// # Помилки
///
/// [`CoreError::InvalidArg`] — `dt <= 0`, `mu <= 0`, `n_revs != 0` або `r1`
/// і `r2` на одній прямій через початок координат (площина перельоту, а з
/// нею й угода про напрям, там невизначені).
/// [`CoreError::ToleranceNotMet`] — Ньютон не зійшовся; `core/test/test_lambert.c`
/// записує, для яких геометрій це справді трапляється.
///
/// ```no_run
/// use core_rs::{lambert_solve, Vec3d};
///
/// let r1 = Vec3d { x: 1.4959787e11, y: 0.0, z: 0.0 };
/// let r2 = Vec3d { x: -1.9e11, y: 1.1e11, z: 8.0e9 };
/// let (v1, _v2) = lambert_solve(r1, r2, 2.5e7, 1.32712440018e20, true, 0)?;
/// println!("{v1:?}");
/// # Ok::<(), core_rs::CoreError>(())
/// ```
pub fn lambert_solve(
    r1: Vec3d,
    r2: Vec3d,
    dt: f64,
    mu: f64,
    prograde: bool,
    n_revs: i32,
) -> Result<(Vec3d, Vec3d)> {
    let mut v1 = Vec3d::default();
    let mut v2 = Vec3d::default();

    // SAFETY: обидва вихідні вказівники ведуть на локальні змінні, живі до
    // кінця виклику; вхідні структури передаються за значенням і копіюються
    // C-стороною. Функція нічого не виділяє й не зберігає — звільняти нічого,
    // а отже й пари create/free тут немає.
    let code = unsafe {
        core_sys::lambert_solve(
            r1,
            r2,
            dt,
            mu,
            i32::from(prograde),
            n_revs,
            &mut v1,
            &mut v2,
        )
    };
    check(code)?;

    Ok((v1, v2))
}
