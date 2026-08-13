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
