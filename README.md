# space_sim

Космічний симулятор зі справжньою N-body орбітальною механікою. Числове ядро
на C, оркестрація та рендер на Rust + wgpu.

- **Архітектурні рішення** — [PROJECT.md](PROJECT.md). Джерело істини.
- **Послідовність робіт і виміряні результати** — [ROADMAP.md](ROADMAP.md).
- **Правила для агента** — [CLAUDE.md](CLAUDE.md).

Цей файл — лише про те, що треба встановити й що чим запускається.

---

## Залежності

Розділені за тим, **що зламається без них**. Обов'язкове — те, без чого не
проходить `make test` або `cargo test`; решта потрібна для окремих цілей.

| | Навіщо | Обов'язково | Ubuntu / Debian |
|---|---|---|---|
| C11-компілятор | ядро, `make` і `cargo` | так | `build-essential` або `clang` |
| GNU make | збірка ядра | так | `build-essential` |
| binutils (`nm`, `ar`) | «поліція libm», статичні бібліотеки | так | `binutils` |
| rustup (stable) | межа C↔Rust, з M1 | так | див. «Версія Rust» нижче |
| clippy, rustfmt | лінт і форматування, у CI обидва — гейт | для розробки | ставить rustup за `rust-toolchain.toml` |
| python3 | скрипти в `scripts/` | для `make plots` і завантаження даних | є в системі |
| matplotlib | графіки з CSV | для `make plots` | `python3-matplotlib` |
| valgrind | `make valgrind`: читання неініціалізованої пам'яті, чого ASan не бачить | для гейта перед пушем | `valgrind` |
| Slang | компілятор шейдерів; WGSL комітяться, тож потрібен лише коли їх правиш | ні | `sh scripts/fetch_slang.sh` |

```sh
sudo apt install build-essential binutils python3-matplotlib valgrind
```

Rust — не з apt: див. «Версія Rust» нижче. Версія в репозиторіях Ubuntu вже
застара для наших залежностей.

Python-залежності продубльовано машинно-читно у
[scripts/requirements.txt](scripts/requirements.txt) — якщо ставите не через
apt, а у venv.

### Версія Rust

Проєкт стоїть на **rustup, канал stable** — це закріплено в
[rust-toolchain.toml](rust-toolchain.toml), і rustup підхоплює його сам.

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

`rust-version` у `Cargo.toml` — **1.95**, і його задає `egui` 0.36 (до нього
поріг тримав `wgpu` 30 на 1.87, а ще раніше — cargo з таблицею `[lints]` на
1.75). Поріг тут завжди чужий: його ставить найвимогливіша залежність, а не
наш код.

Старий cargo проєкт уже не збере — але **повідомлення буде оманливе**.
Перевірено на 1.75: він падає не на `rust-version`, а раніше, спіткнувшись
про транзитивну залежність з `edition2024`, і каже саме про неї:

```
feature `edition2024` is required ... not stabilized in this version of Cargo (1.75.0)
```

Якщо побачили це — питання не в залежності, а в тому, що збирає не той
`cargo`. Дивіться попередження нижче.

> ⚠ **Якщо стоять обидва — з apt і з rustup — перевірте, чий cargo у вас
> у PATH.** `rust-toolchain.toml` читає лише rustup; системний cargo його не
> бачить і збиратиме своєю версією, мовчки.
>
> ```sh
> cargo --version           # чий саме
> which -a cargo            # де вони обидва
> ```
>
> Щоб завжди брався rustup, у `~/.bashrc`:
>
> ```sh
> . "$HOME/.cargo/env"
> ```
>
> Без цього рядка `~/.cargo/bin` не потрапляє в PATH, і `cargo` лишається
> системним.

---

## Що чим запускається

```sh
make test        # усе про C: «поліція libm», юніт-тести, звірка хешів
make asan        # ті самі юніт-тести під ASan+UBSan (~5 хв)
make valgrind    # ті самі юніт-тести під valgrind (~6 хв)
make csv         # вивід ядра у build/csv/*.csv
make plots       # графіки з CSV у build/plots/*.png (потрібен matplotlib)
make bench       # інтегратор, силова модель, пропускна здатність (скіл perf-probe)
make flags       # прапорці компіляції ядра
make cook        # перегенерувати ассет-фікстуру (робити свідомо!)
make cook-dem    # скукувати тайли рельєфу з data/lola у assets/

cargo test --workspace   # межа C↔Rust, рушій, гра
cargo run -p game        # сама гра: вікно
cargo clippy --all-targets -- -D warnings
cargo run -q --example flags    # ті самі прапорці, з боку cargo
```

Обидві збірки читають прапорці з `core/cflags.txt` і більше нізвідки. Що вони
дають однакові числа — перевіряє `core-sys/tests/determinism.rs` проти
`core/scenario/golden.txt`, того самого еталона, з яким звіряється `make`.

Усі команди запускаються **з кореня репозиторію**: тести й експортери читають
`data/`.

---

## Структура

```
core/        C. Числове ядро. Юніт-тести, сценарії детермінізму, експорт CSV.
core-sys/    Rust. Збірка ядра через cc, сирі FFI-декларації.
core-rs/     Rust. Безпечна обгортка. Єдине місце з нашим unsafe.
engine/      Rust. Рендер, frame graph, ассети, ввід, звук.
game/        Rust. Стан гри, планувальник місій, UI, сейви.
tools/       Rust. Asset cooker (офлайн).
scripts/     Python. Завантаження опорних даних, графіки.
data/        Опорні дані JPL і закомічена ассет-фікстура.
```

`tools/` містить кукер тайлів рельєфу (`dem-cook`, з'явився разом із першим
тайлом DEM, а не наперед) і зонди `gpu-probe` та `slang-probe`.
