---
name: core-boundary
description: Межа C↔Rust — /core-sys (сирі FFI-декларації) і /core-rs (безпечна обгортка, єдине місце з нашим unsafe). Завантажуй перед тим, як додавати нову функцію на межу, чіпати build.rs, або писати будь-що в /core-rs. Пояснює точний контракт, чому unsafe тут дозволений і як він доводиться безпечним.
---

# core-boundary

Етап D (ROADMAP.md) закритий: `core-sys/build.rs` збирає `/core` крейтом
`cc`, перші три функції межі оголошені вручну й звірені з C бітово,
`core-rs` дає RAII + `Result`. Це **єдине місце в усьому проєкті з нашим
`unsafe`** (CLAUDE.md, інваріант 1; сторонні `-sys`-крейти — виняток).

## Поточний стан межі — не плутати зі скетчем із PROJECT.md §5

PROJECT.md §5 описує **цільовий** C API (~20 функцій: `prop_run`,
`lambert_solve`, `frame_to_rotating` тощо) — це план, не поточний стан.
**Реально оголошено і обгорнуто на сьогодні лише три функції:**

```rust
// core-sys/src/lib.rs — сирі декларації, extern "C", без жодного unsafe-блоку
pub fn eph_load(path: *const c_char, out: *mut *mut EphemerisCtx) -> CoreResult;
pub fn eph_free(ctx: *mut EphemerisCtx);
pub fn eph_body_state(ctx: *const EphemerisCtx, body: c_int, t: f64, out: *mut State) -> CoreResult;
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
```

Усе інше з `core/*.h` (`cr3bp_*`, `dop853_integrate*`, `halo_correct`,
`shoot_multiple`, `station_keep`, `field_*`, ...) **існує в C, але не має
FFI-декларації**. Якщо задача вимагає викликати щось із них із Rust —
це нова робота на межі, а не пошук наявної функції.

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
5. **Жодних колбеків з C у Rust** (інваріант 7) — це вже враховано в
   дизайні `core/core.h` (події як дані), стосується майбутніх `prop_run`.
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
cargo run -q --example flags # прапорці з боку cargo — звірка з `make flags`
```

## Коли оновлювати цей скіл

- З'явилася нова функція в `core-sys/src/lib.rs` або новий тип у
  `core-rs/src/lib.rs` — онови розділ «Поточний стан межі».
- Змінився принцип роботи `build.rs` (наприклад, зникло читання
  `cflags.txt`, або з'явився CMake — CLAUDE.md наразі це забороняє,
  якщо стан зміниться, він зміниться свідомо й помітно).
- Почалась робота над наступним шматком C API з PROJECT.md §5
  (`prop_run`, `lambert_solve`) — перенеси відповідний рядок із «не має
  декларації» в таблицю реалізованого.
