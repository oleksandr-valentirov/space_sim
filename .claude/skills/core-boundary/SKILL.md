---
name: core-boundary
description: Межа C↔Rust — /core-sys (сирі FFI-декларації) і /core-rs (безпечна обгортка, єдине місце з нашим unsafe). Завантажуй перед тим, як додавати нову функцію на межу, чіпати build.rs, або писати будь-що в /core-rs. Пояснює точний контракт, чому unsafe тут дозволений і як він доводиться безпечним.
---

# core-boundary

Етапи D і H (ROADMAP.md) закриті: `core-sys/build.rs` збирає `/core` крейтом
`cc`, функції межі оголошені вручну й звірені з C бітово, `core-rs` дає
RAII + `Result`, і рушій уже цим користується (`engine::live`, H5). Це **єдине місце в усьому проєкті з нашим
`unsafe`** (CLAUDE.md, інваріант 1; сторонні `-sys`-крейти — виняток).

## Поточний стан межі — не плутати зі скетчем із PROJECT.md §5

PROJECT.md §5 описує **цільовий** C API (~20 функцій) — це план, не поточний
стан. **Реально оголошено й обгорнуто на сьогодні сім функцій:**

```rust
// core-sys/src/lib.rs — сирі декларації, extern "C", без жодного unsafe-блоку
pub fn eph_load(path: *const c_char, out: *mut *mut EphemerisCtx) -> CoreResult;
pub fn eph_free(ctx: *mut EphemerisCtx);
pub fn eph_body_state(ctx: *const EphemerisCtx, body: c_int, t: f64, out: *mut State) -> CoreResult;

// ROADMAP H3. Типи поруч: PropagatorCtx (непрозорий), PropConfig, CoreEvent,
// і три переліки як c_int з константами — з тієї ж причини, що CoreResult.
pub fn prop_create(eph: *const EphemerisCtx, cfg: *const PropConfig,
                   out: *mut *mut PropagatorCtx) -> CoreResult;
pub fn prop_free(p: *mut PropagatorCtx);
pub fn prop_run(/* 12 аргументів: буфер від Rust, out_cap/out_count,
                   out_final, out_stop, out_event, in_out_step */) -> CoreResult;

// ROADMAP K8c. Та сама інтеграція з матрицею переходу; out_stm — 36 f64
// від Rust. Траєкторія БІТОВО та сама, що в prop_run (контролер кроку в
// dop853.c читає лише блок 0) — виміряно на всіх трьох рівнях.
pub fn prop_run_stm(p: *mut PropagatorCtx, initial: *const State, t_end: f64,
                    out_final: *mut State, out_stm: *mut f64,
                    in_out_step: *mut f64) -> CoreResult;
```

```rust
// core-rs/src/lib.rs — безпечна обгортка
pub struct Ephemeris { /* приватний *mut EphemerisCtx */ }
impl Ephemeris {
    pub fn load(path: &Path) -> Result<Ephemeris>;
    pub fn body_state(&self, body: i32, t: f64) -> Result<State>;
}
// Drop викликає eph_free рівно раз; unsafe impl Send + Sync (обґрунтування —
// коментар над імпл-блоком: eph_body_state бере const-вказівник, немає
// статиків і кешу в ephemeris.c/cheb.c).

pub struct Propagator { /* Arc<Ephemeris> + приватний *mut PropagatorCtx */ }
impl Propagator {
    pub fn new(eph: Arc<Ephemeris>, cfg: PropConfig) -> Result<Propagator>;
    pub fn run(&mut self, initial: &State, t_end: f64, events: &[Event],
               samples: &mut [State], step: &mut f64) -> Result<Run>;
    pub fn run_stm(&mut self, initial: &State, t_end: f64,
                   step: &mut f64) -> Result<(State, Stm)>;   // K8c
}
// Stm — обгортка над [f64; 36] з get(row, col), а не голий масив: транспонована
// матриця переходу цілком правдоподібна, і помилка проявилась би як дивна
// корекція, не як падіння.
// Ефемерида — Arc, НЕ лайфтайм (CLAUDE.md: жодних лайфтаймів у структурах);
// Send є, Sync свідомо немає — контекст у C несе липкий прапорець помилки.
```

Три речі про `Propagator::run`, які легко зламати назад:

- **Порожній зріз `samples` = «без семплування»**, і це перекладається
  явно: порожній зріз у Rust — вирівняний висячий вказівник, а не нуль, і
  C вважає буфер без місця помилкою викликача.
- **`step` треба повертати назад тим самим.** Викинути його — інша
  траєкторія й усемеро більше кроків (виміряно, `core/test/test_prop.c`).
- **`Event` — енум, а не структура з `param`**: перицентр із відстанню тут
  неможливо навіть написати.

Усе інше з `core/*.h` (`cr3bp_*`, `halo_correct`, `shoot_multiple`,
`station_keep`, `lambert_solve`, `porkchop_compute`, `target_hit`,
`eph_body_harmonics`, ...)
**існує в C, але не має FFI-декларації**. Якщо задача вимагає викликати щось
із них із Rust — це нова робота на межі, а не пошук наявної функції.

## Правила, за якими додається нова функція на межу

1. **Вручну, не bindgen** (`core-sys/Cargo.toml` не має build-dep на
   `bindgen`). Ціль — щоб кожен рядок був прочитаний очима: тут помилка
   (переплутане поле, не той цілий тип) не падає, вона повертає
   правдоподібне число.
2. **`core-sys` — тільки декларації.** Жодного `unsafe`-блоку в цьому
   крейті; типи `#[repr(C)]`, результат — сире `c_int` (не Rust `enum`:
   значення поза переліком з C — UB для енуму, а не помилка, яку видно).
3. **`core-rs` — де живе `unsafe`.** Кожен виклик обгорнутий, із
   `// SAFETY:`-коментарем, що саме гарантує безпеку (валідність
   вказівника, час життя, відсутність double-free). Дивись існуючий
   `Ephemeris::load`/`body_state`/`Drop` як зразок стилю.
4. **C не виділяє буфери з даними** (CLAUDE.md інваріант 6) — Rust дає
   буфер і розмір, C заповнює й повертає фактичну кількість. Контексти —
   виняток: непрозорі хендли, пари `create`/`free`.
5. **Жодних колбеків з C у Rust** (інваріант 7). Як це виглядає на практиці —
   готовий прецедент у `prop_run`: подія описується даними (`CoreEvent`),
   пошук кореня живе в C, керування повертається рівно на події. Колбек
   усередині C при цьому є (`StepObserver` у `dop853_integrate_obs`) — межу
   він не перетинає.
6. Подвійне звільнення й use-after-free мають бути **неможливими за
   конструкцією типів**, не лише «ми обіцяємо їх не робити». Приклад —
   `Ephemeris` не `Copy`/`Clone`, `eph_free` не експортується з `core-rs`.
   Це перевіряється `compile_fail`-доктестами в `core-rs/src/lib.rs`, не
   коментарем.

## Збірка

`core-sys/build.rs` і `core/Makefile` читають **той самий**
[core/cflags.txt](../../core/cflags.txt) і більше нізвідки — розбіжність
між ними тихо ламає детермінізм (PROJECT.md §4). `-fPIC` — єдиний
прапорець поза цим файлом (Windows-виняток), бо на арифметику не впливає.
`links = "core"` у `core-sys/Cargo.toml` не дає злінкувати бібліотеку
двічі й передає метадані (`DEP_CORE_*`) далі по графу.

```sh
cargo test -p core-sys       # тести FFI + determinism.rs проти core/scenario/golden.txt
cargo test -p core-rs        # тести обгортки, включно з compile_fail
cargo test -p engine --test live  # межа з боку споживача (H5)
cargo run -q --example flags # прапорці з боку cargo — звірка з `make flags`
```

**Оракул — `core-sys/oracle.c`**, і кожна нова функція межі має туди
дописатися: він друкує `%.17g` того, що дає C, а `tests/ffi.rs` звіряє біти
того ж виклику через FFI. Звірка має зуби — переставлені місцями `tol_m` і
`h_max_s` у декларації `PropConfig` валять тест одразу (перевірено).

## Коли оновлювати цей скіл

- З'явилася нова функція в `core-sys/src/lib.rs` або новий тип у
  `core-rs/src/lib.rs` — онови розділ «Поточний стан межі».
- Змінився принцип роботи `build.rs` (наприклад, зникло читання
  `cflags.txt`, або з'явився CMake — CLAUDE.md наразі це забороняє,
  якщо стан зміниться, він зміниться свідомо й помітно).
- Почалась робота над наступним шматком C API з PROJECT.md §5
  (`lambert_solve`, `porkchop_compute`, `target_hit`, `eph_body_rotation`) — перенеси відповідний рядок із «не має декларації»
  в список реалізованого й додай виклик в оракул.
