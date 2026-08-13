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
| valgrind | перевірка пам'яті на межі (ROADMAP D3) | ні, це робить CI | `valgrind` |

```sh
sudo apt install build-essential binutils python3-matplotlib valgrind
```

Rust — не з apt: див. «Версія Rust» нижче. Версія в репозиторіях Ubuntu вже
застара для того, що знадобиться на етапі E.

Python-залежності продубльовано машинно-читно у
[scripts/requirements.txt](scripts/requirements.txt) — якщо ставите не через
apt, а у venv.

### Версія Rust

Проєкт стоїть на **rustup, канал stable** — це закріплено в
[rust-toolchain.toml](rust-toolchain.toml), і rustup підхоплює його сам.

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

`rust-version` у `Cargo.toml` — **1.87**, і цього вимагає `wgpu` 30, який
приїхав із розвідкою P0. До неї поріг був 1.75 (упирався не в мову, а в
cargo: таблиця `[lints]` з'явилася в 1.74).

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
make csv         # вивід ядра у build/csv/*.csv
make plots       # графіки з CSV у build/plots/*.png (потрібен matplotlib)
make flags       # прапорці компіляції ядра
make cook        # перегенерувати ассет-фікстуру (робити свідомо!)

cargo test       # межа C↔Rust: збірка ядра крейтом cc, FFI, обгортка
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

`engine/`, `game/` і `tools/` поки порожні — вони з'являються за порядком
кроків у ROADMAP.md, а не наперед.
