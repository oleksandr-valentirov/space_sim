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
стан. **Реально оголошено й обгорнуто на сьогодні вісімнадцять функцій**
(вісім нижче плюс `eph_body_radius` (U2a), `eph_body_mu` і
`porkchop_compute_eph` (U5a), `eph_body_orientation` (R1c), чотири CR3BP і дві
фреймові (U6b2, U6b1) — `eph_body_radius` і `eph_body_mu` повертають `double`
без коду помилки, бо це читання поля контексту):

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
// ROADMAP K6b: другий аргумент — `*const VesselParams` (маса, площа, Cr).
// NULL = безмасова пробна частинка, тобто бітово те, що було до K6b.
pub fn prop_run(/* 13 аргументів: vessel, буфер від Rust, out_cap/out_count,
                   out_final, out_stop, out_event, in_out_step */) -> CoreResult;

// ROADMAP K8c. Та сама інтеграція з матрицею переходу; out_stm — 36 f64
// від Rust. Траєкторія БІТОВО та сама, що в prop_run (контролер кроку в
// dop853.c читає лише блок 0) — виміряно на всіх трьох рівнях.
pub fn prop_run_stm(p: *mut PropagatorCtx, initial: *const State,
                    vessel: *const VesselParams, t_end: f64,
                    out_final: *mut State, out_stm: *mut f64,
                    in_out_step: *mut f64) -> CoreResult;

// ROADMAP L3 (борг D1). ЄДИНА функція межі поза зоною детермінізму, і єдина,
// що бере структуру ЗА ЗНАЧЕННЯМ (Vec3d — 24 байти, тобто через пам'ять на
// всіх наших ABI). Живе в окремій libcore_planning.a і має ВЛАСНИЙ оракул.
pub fn lambert_solve(r1: Vec3d, r2: Vec3d, dt: f64, mu: f64,
                     prograde: c_int, n_revs: c_int,
                     v1_out: *mut Vec3d, v2_out: *mut Vec3d) -> CoreResult;
```

```rust
// Пізніші додатки. Перші дві — читання поля: нуль означає «ассет не каже»
// і для тіла без величини, і для невідомого індексу.
pub fn eph_body_radius(ctx: *const EphemerisCtx, body: c_int) -> f64;
pub fn eph_body_mu(ctx: *const EphemerisCtx, body: c_int) -> f64;

// R1c. Тут, на відміну від двох вище, є ЧАС — а отже й спосіб не вдатися.
// Quat оголошений з `w` першим (core/quat.h); оракул друкує всі чотири
// компоненти окремо, бо переставлений `w` — теж коректне обертання.
pub fn eph_body_orientation(ctx: *const EphemerisCtx, body: c_int, t: f64,
                            out: *mut Quat) -> CoreResult;

// U5a, у libcore_planning.a разом із lambert_solve. Обгортка над
// porkchop_compute, яка сама подає eph_body_state замість колбеків.
// УВАГА: після U5b у неї немає викликача в грі (борг D10) — гра розгортає
// сітку в Rust над lambert_solve, бо їй потрібен переліт ВІД АПАРАТА і в
// координатах центрального тіла.
pub fn porkchop_compute_eph(/* eph, depart_body, arrive_body, mu, prograde,
                               дві сітки, буфер від Rust, out_count */) -> CoreResult;
```

```rust
// U6b1, U6b2: CR3BP і синодичний фрейм. Усі — БЕЗРОЗМІРНІ одиниці, і це
// угода, якої не перевіряє компілятор: метри тут дадуть число, схоже на
// правду. Тому в core-rs/tests/cr3bp.rs поруч стоять числа ЗЗОВНІ.
pub fn cr3bp_mu(gm_primary: f64, gm_secondary: f64) -> f64;
pub fn cr3bp_jacobi(r: Vec3d, v: Vec3d, mu: f64) -> f64;   // Vec3d за значенням
pub fn cr3bp_lagrange(mu: f64, point: c_int, out: *mut Vec3d) -> CoreResult;
// TOLERANCE_NOT_MET тут — ВІДПОВІДЬ (променю нема де перетнути криву), не збій.
pub fn cr3bp_zvc_radius(mu: f64, c: f64, from: Vec3d, dir_unit: Vec3d,
                        r_max: f64, r_out: *mut f64) -> CoreResult;

// SynodicFrame — найбільша структура межі (6×Vec3d + 5×double), і заповнює
// її C: помилка в розкладці дає не дивне число, а запис за межі.
pub fn frame_synodic(eph: *const EphemerisCtx, primary: c_int, secondary: c_int,
                     t: f64, out: *mut SynodicFrame) -> CoreResult;
pub fn frame_from_inertial(f: *const SynodicFrame, input: *const State, out: *mut State);
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
    pub fn run(&mut self, initial: &State, vessel: Option<&VesselParams>,
               t_end: f64, events: &[Event], samples: &mut [State],
               step: &mut f64) -> Result<Run>;
    pub fn run_stm(&mut self, initial: &State, vessel: Option<&VesselParams>,
                   t_end: f64, step: &mut f64) -> Result<(State, Stm)>;  // K8c
}
// Stm — обгортка над [f64; 36] з get(row, col), а не голий масив: транспонована
// матриця переходу цілком правдоподібна, і помилка проявилась би як дивна
// корекція, не як падіння.

pub fn lambert_solve(r1: Vec3d, r2: Vec3d, dt: f64, mu: f64,
                     prograde: bool, n_revs: i32) -> Result<(Vec3d, Vec3d)>;
// Вільна функція, не метод: контексту тут немає, як і алокацій — отже нема й
// пари create/free. prograde як bool, бо в C це прапорець, а не число.
// Ефемерида — Arc, НЕ лайфтайм (CLAUDE.md: жодних лайфтаймів у структурах);
// Send є, Sync свідомо немає — контекст у C несе липкий прапорець помилки.
```

Чотири речі про `Propagator::run`, які легко зламати назад:

- **`vessel: None` — це не «за замовчуванням», а «безмасова пробна
  частинка»** (K6b), і воно бітово дорівнює тому, що було до K6b.
  Апарат передається **на прогін**, не в конфігурацію: маса змінюється
  при горінні, а `/game` тримає один пропагатор на всі апарати.

- **Порожній зріз `samples` = «без семплування»**, і це перекладається
  явно: порожній зріз у Rust — вирівняний висячий вказівник, а не нуль, і
  C вважає буфер без місця помилкою викликача.
- **`step` треба повертати назад тим самим.** Викинути його — інша
  траєкторія й усемеро більше кроків (виміряно, `core/test/test_prop.c`).
- **`Event` — енум, а не структура з `param`**: перицентр із відстанню тут
  неможливо навіть написати.

Усе інше з `core/*.h` (`cr3bp_*`, `halo_correct`, `shoot_multiple`,
`station_keep`, `porkchop_compute`, `target_hit`,
`eph_body_harmonics`, `eph_body_radius`, `eph_body_flux`, ...)
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

**Оракулів два, і другий не примха.** `core-sys/oracle.c` лінкується
**без `-lm`** — саме лінкування є перевіркою того, що в рантаймову зону не
просочилася тригонометрія. `core-sys/oracle_planning.c` лінкується **з
`-lm`** проти `libcore_planning.a`, бо `core/planning/` кличе libm свідомо
(PROJECT.md §4: межа детермінізму проходить по пропагації, не по плануванню).
Дописати Ламберта в перший означало б зняти ту перевірку заради зручності.

Те саме на рівні бібліотек: `build.rs` збирає **дві** — `libcore.a` з
`core/*.c` і `libcore_planning.a` з `core/planning/*.c`. Сценарії детермінізму
лінкуються тільки з першою, і так має лишитися.

**Кожна нова функція межі має дописатися у свій оракул:** він друкує `%.17g` того, що дає C, а `tests/ffi.rs` звіряє біти
того ж виклику через FFI.

**Але оракул не перевіряє змісту, і на це вже двічі наступили.** Звірка
порівнює межу саму з собою, тож однакова помилка з обох боків скорочується:
спряжений кватерніон збігається в усьому, крім знака трьох компонент
(R1c), а сітка porkchop у баріцентричних координатах збігалася з
`lambert_solve` у тих самих баріцентричних координатах, хоч дуга будувалася
навколо початку координат із `mu` Землі (U5b). Отже кожна функція, у якої є
**угода** — система координат, напрямок обертання, порядок компонент, —
потребує ще й числа ззовні: опублікованої сталої, іншого методу, іншої
фізики. Приклади в `core-rs/tests/ephemeris.rs` (RA нульового меридіана
280.194°) і `game/tests/porkchop.rs` (кеплерівське просування). Звірка має зуби — переставлені місцями `tol_m` і
`h_max_s` у декларації `PropConfig` валять тест одразу (перевірено).
Те саме правило стосується **нового аргументу**, не лише функції: після
K6b оракул жене ту саму ланку з апаратом, який має площу, бо переставлені
`area_m2` і `cr` дали б цілком правдоподібну траєкторію, лише не ту.

## Коли оновлювати цей скіл

- З'явилася нова функція в `core-sys/src/lib.rs` або новий тип у
  `core-rs/src/lib.rs` — онови розділ «Поточний стан межі».
- Змінився принцип роботи `build.rs` (наприклад, зникло читання
  `cflags.txt`, або з'явився CMake — CLAUDE.md наразі це забороняє,
  якщо стан зміниться, він зміниться свідомо й помітно).
- Почалась робота над наступним шматком C API з PROJECT.md §5
  (`porkchop_compute`, `target_hit`, `eph_body_rotation`) — перенеси
  відповідний рядок із «не має декларації» в список реалізованого й додай
  виклик у той оракул, до якого функція належить за libm.
