#!/usr/bin/env python3
"""Графіки з CSV, які виводить `make csv` — поставка Milestone 0.

Навіщо взагалі. Ядро міряло себе з першого дня, але лише твердженнями й
окремими числами в тестах. Тест каже «пройдено»; він не каже, що саме
пораховано. Траєкторія, яка вкладається в допуск і при цьому летить не туди, —
річ, яка трапляється, і ловиться вона оком.

Скрипт нічого не рахує. Уся фізика лишається в C; тут тільки читання CSV
і осі. Це навмисно: якщо графік показує дурницю, дивитися треба в ядро,
а не сюди.

    make plots                      зібрати CSV і побудувати все
    python3 scripts/plot.py --csv build/csv --out build/plots
    python3 scripts/plot.py --only trajectory

Потрібен matplotlib. Він НЕ є залежністю збірки: ядро від нього не залежить,
і на CI його немає.
"""

import argparse
import csv
import sys
import textwrap
from pathlib import Path

try:
    import matplotlib
    matplotlib.use("Agg")          # без дисплея, працює і по ssh, і на CI
    import matplotlib.pyplot as plt
except ImportError:
    sys.exit("потрібен matplotlib:  python3 -m pip install matplotlib\n"
             "  (або: apt install python3-matplotlib)")


MEGAMETRE = 1.0e6
GIGAMETRE = 1.0e9
YEAR_DAYS = 365.25

# Ідентифікатори в CSV англійські, як і решта коду ядра; підписи на графіках —
# ні. Тут одне перекладається в інше, і більше ніде.
NAMES = {
    "l4_region": "околиця L4",
    "close_approach": "близький проліт",
    "ten_body": "десять тіл",
    "three_body": "три тіла (контроль)",
    "asset_3630d": "ассет, повний спан",
    "raw_tol_1m": "контроль, старий допуск 1 м",
    "raw_fixture_tol": "контроль, допуск фікстури",
}


def load(path):
    """CSV -> словник «колонка: список значень».

    Числові колонки стають float, решта лишається рядками. Тип визначається
    по колонці, а не по клітинці, — щоб колонка з іменами не перетворилася
    наполовину на числа й не поламала групування нижче.
    """
    if not path.is_file():
        sys.exit("немає %s — спершу make csv" % path)

    with open(path, newline="", encoding="utf-8") as f:
        rows = list(csv.reader(f))

    if len(rows) < 2:
        sys.exit("%s порожній — спершу make csv" % path)

    header, body = rows[0], rows[1:]
    table = {}

    for i, name in enumerate(header):
        column = [row[i] for row in body]
        try:
            table[name] = [float(v) for v in column]
        except ValueError:
            table[name] = column

    return table


def groups(table, key):
    """Розбиває таблицю на групи за значенням колонки key, зберігаючи порядок
    появи. Повертає список пар (значення, підтаблиця)."""
    order = []
    index = {}

    for i, value in enumerate(table[key]):
        if value not in index:
            index[value] = []
            order.append(value)
        index[value].append(i)

    return [(value, {name: [column[i] for i in index[value]]
                     for name, column in table.items()})
            for value in order]


def label_of(value):
    """Підпис групи: відомий ідентифікатор перекладається, ціле число йде без
    хвоста «.0», решта як є."""
    if value in NAMES:
        return NAMES[value]
    if isinstance(value, float) and value == int(value):
        return str(int(value))
    return str(value)


def log_pairs(xs, ys):
    """Пари (x, y), у яких y додатний.

    Нуль на логарифмічній осі показати не можна, а нулі тут законні: похибка
    на нульовій вибірці рівно нуль, і дрейф збереженої величини теж буває
    рівно нуль. Підмінити їх якоюсь підлогою — намалювати число, якого не
    міряли, і розтягнути вісь на двадцять порядків заради нього. Тому точка
    просто не малюється.
    """
    kept = [(x, y) for x, y in zip(xs, ys) if y > 0.0]
    return [x for x, _ in kept], [y for _, y in kept]


def caption(fig, text):
    """Підпис під усією сторінкою.

    Під осями, а не всередині: у першій версії ці пояснення лягали поверх
    кривих, які пояснювали. Рядки перегортаються по ширині фігури, інакше
    bbox_inches="tight" розтягує сторінку на ширину найдовшого речення.
    """
    width = int(fig.get_figwidth() * 11)
    wrapped = "\n".join(textwrap.fill(paragraph, width)
                        for paragraph in text.split("\n"))
    fig.text(0.5, -0.01, wrapped, ha="center", va="top", fontsize=8,
             color="0.3")


# --- CR3BP: сімейство орбіт і точки лібрації -------------------------------

def figure_cr3bp(csv_dir, out_dir):
    family = load(csv_dir / "halo_family.csv")
    points = load(csv_dir / "cr3bp_points.csv")

    named = dict(zip(points["name"],
                     zip(points["x"], points["y"], points["z"])))

    fig, axes = plt.subplots(1, 3, figsize=(15, 5))
    fig.suptitle("CR3BP Земля–Місяць: п'ять halo-орбіт каталогу JPL "
                 "навколо L2 (безрозмірні одиниці)")

    panels = ((axes[0], "x", "y", "згори (x–y)"),
              (axes[1], "x", "z", "збоку (x–z)"),
              (axes[2], "y", "z", "з торця (y–z)"))

    for ax, a, b, title in panels:
        for index, orbit in groups(family, "orbit"):
            ax.plot(orbit[a], orbit[b], linewidth=1.2,
                    label="орбіта %s" % label_of(index))

        # У проєкції з торця Місяць і обидві точки лібрації лежать на осі
        # обертання й накладаються в одну точку. Позначаємо це чесно, одним
        # маркером, замість трьох підписів один поверх одного.
        if (a, b) == ("y", "z"):
            ax.plot(0.0, 0.0, "o", color="0.35", markersize=6)
            ax.annotate("Місяць, L1, L2\n(в одній точці)", (0.0, 0.0),
                        textcoords="offset points", xytext=(8, 4),
                        fontsize=8, color="0.35")
        else:
            for name, marker, text in (("moon", "o", "Місяць"),
                                       ("l1", "+", "L1"),
                                       ("l2", "+", "L2")):
                coords = dict(zip("xyz", named[name]))
                ax.plot(coords[a], coords[b], marker, color="0.25",
                        markersize=7)
                ax.annotate(text, (coords[a], coords[b]),
                            textcoords="offset points", xytext=(6, 5),
                            fontsize=8, color="0.25")

        ax.set_xlabel(a)
        ax.set_ylabel(b)
        ax.set_title(title)
        ax.set_aspect("equal", adjustable="datalim")
        ax.grid(alpha=0.3)

    axes[0].legend(fontsize=8, loc="upper left")

    caption(fig, "Опубліковані початкові умови, проінтегровані нашим DOP853 "
                 "на повний період з допуском 1e-14. Криві замикаються — "
                 "тобто орбіта справді періодична в нашій динаміці, а не "
                 "лише в чужій таблиці.")

    return save(fig, out_dir / "cr3bp.png")


# --- Нестійкість: виміряне зростання проти lambda^n ------------------------

def figure_stability(csv_dir, out_dir):
    table = load(csv_dir / "stability.csv")

    fig, ax = plt.subplots(figsize=(9, 5.8))

    for i, (index, orbit) in enumerate(groups(table, "orbit")):
        color = "C%d" % (i % 10)
        lam = orbit["lambda"][0]

        x, y = log_pairs(orbit["revolution"], orbit["separation"])
        ax.plot(x, y, "o-", color=color, markersize=4, linewidth=1.4,
                label="орбіта %s, λ = %.4g" % (label_of(index), lam))

        x, y = log_pairs(orbit["revolution"], orbit["envelope"])
        ax.plot(x, y, "--", color=color, linewidth=1.0, alpha=0.7)

    ax.set_yscale("log")
    ax.set_xlabel("обертів")
    ax.set_ylabel("відстань між траєкторією та зміщеною копією")
    ax.set_title("Зростання збурення 1e-12: виміряне (суцільна) "
                 "проти λⁿ (пунктир)")
    ax.grid(alpha=0.3, which="both")
    ax.legend(fontsize=8, loc="upper left")

    caption(fig,
            "Пунктир передбачає НАХИЛ, не величину: початкове зміщення лежить "
            "уздовж нестійкого напрямку лише частково, тож крива йде нижче, "
            "але паралельно — і саме паралельність каже, що зростання є "
            "власним значенням.\n"
            "Орбіти 0 і 1534 ростуть швидше за свої λ = 1.19 і λ = 1. Це не "
            "помилка інтегратора, а дефектна пара одиничних власних значень: "
            "жорданова клітина дає лінійний дрейф уздовж сімейства, і він "
            "надовго перекриває множник 1.19.")

    return save(fig, out_dir / "stability.png")


# --- Траєкторія в справжній ефемериді --------------------------------------

def figure_trajectory(csv_dir, out_dir):
    t = load(csv_dir / "halo_inertial.csv")
    n = len(t["x"])

    fig, axes = plt.subplots(2, 2, figsize=(12, 10.5))
    fig.suptitle("Halo-орбіта каталогу 1151, перенесена в поле десяти тіл: "
                 "%d діб" % round(t["days"][-1]))

    # Інерціальний баріцентричний вигляд. Тут не видно нічого, крім того, що
    # апарат летить разом із Землею навколо Сонця, — власне орбіта на цьому
    # масштабі тонша за лінію. Панель лишається саме тому: вона задає
    # масштаб, на тлі якого наступні три щось означають.
    ax = axes[0][0]
    ax.plot([v / GIGAMETRE for v in t["earth_x"]],
            [v / GIGAMETRE for v in t["earth_y"]],
            linewidth=2.5, color="0.7", label="Земля")
    ax.plot([v / GIGAMETRE for v in t["x"]],
            [v / GIGAMETRE for v in t["y"]],
            linewidth=0.9, color="C0", label="апарат")
    ax.set_xlabel("x, 10⁹ м")
    ax.set_ylabel("y, 10⁹ м")
    ax.set_title("інерціальна баріцентрична система")
    ax.set_aspect("equal", adjustable="datalim")
    ax.grid(alpha=0.3)
    ax.legend(fontsize=8)

    # Те саме з відкинутим рухом Землі. Аж тепер видно, що апарат ходить
    # за Місяцем і зовні від нього.
    ax = axes[0][1]
    for label, kx, ky, width, color in (
            ("Місяць", "moon_x", "moon_y", 1.6, "0.5"),
            ("апарат", "x", "y", 0.9, "C0")):
        ax.plot([(t[kx][i] - t["earth_x"][i]) / MEGAMETRE for i in range(n)],
                [(t[ky][i] - t["earth_y"][i]) / MEGAMETRE for i in range(n)],
                linewidth=width, color=color, label=label)
    ax.plot(0.0, 0.0, "o", color="C2", markersize=6)
    ax.annotate("Земля", (0.0, 0.0), textcoords="offset points",
                xytext=(6, 5), fontsize=8)
    ax.set_xlabel("x, 10⁶ м")
    ax.set_ylabel("y, 10⁶ м")
    ax.set_title("геоцентрична система")
    ax.set_aspect("equal", adjustable="datalim")
    ax.grid(alpha=0.3)
    ax.legend(fontsize=8)

    # І миттєва синодична система: ось де крива замикається в ту саму halo,
    # з якої її взяли. Це головне, що показує сторінка, — обидва твердження
    # істинні водночас.
    for ax, a, b, title in ((axes[1][0], "sx", "sy", "згори (x–y)"),
                            (axes[1][1], "sx", "sz", "збоку (x–z)")):
        ax.plot(t[a], t[b], linewidth=0.8, color="C0")
        ax.plot(t[a][0], t[b][0], "o", color="C3", markersize=5,
                label="старт")
        ax.set_xlabel(a[1:])
        ax.set_ylabel(b[1:])
        ax.set_title("миттєва синодична система, %s" % title)
        ax.set_aspect("equal", adjustable="datalim")
        ax.grid(alpha=0.3)
        ax.legend(fontsize=8)

    caption(fig,
            "Синодична система будується заново на кожну мить із фактичних "
            "положень Землі й Місяця, тому сім витків не збігаються точно: "
            "справжня відстань Земля–Місяць гуляє на десяту частину за "
            "місяць, і halo-орбіти CR3BP в реальній системі просто немає.\n"
            "Те, що криві лежать вузьким жмутом, а не розходяться, — це "
            "робота multiple shooting: збурення тут множиться на 594 за "
            "оберт, і жодне інтегрування з одного кінця такого не втримало б.")

    return save(fig, out_dir / "trajectory.png")


# --- Утримання на орбіті ---------------------------------------------------

def figure_station(csv_dir, out_dir):
    s = load(csv_dir / "station.csv")

    done = [i for i, c in enumerate(s["completed"]) if c > 0.5]
    partial = [i for i, c in enumerate(s["completed"])
               if c <= 0.5 and s["days"][i] > 0.0]
    never = [s["horizon"][i] for i, c in enumerate(s["completed"])
             if c <= 0.5 and s["days"][i] <= 0.0]

    fig, axes = plt.subplots(1, 2, figsize=(12, 5))
    fig.suptitle("Утримання на halo-орбіті після похибки виведення 1 км: "
                 "ціна проти горизонту прицілювання")

    for ax, key, title, unit in (
            (axes[0], "dv_per_year", "витрата палива", "м/с на рік"),
            (axes[1], "worst_offset_m",
             "найбільший відхід від опорної траєкторії", "м")):
        for rows, style, label in (
                (done, dict(marker="o", linestyle="-", color="C0",
                            markersize=5), "долетів до кінця"),
                (partial, dict(marker="x", linestyle="none", color="C3",
                               markersize=9), "зупинився на півдорозі")):
            x, y = log_pairs([s["horizon"][i] for i in rows],
                             [s[key][i] for i in rows])
            if x:
                ax.plot(x, y, label=label, **style)

        ax.set_yscale("log")
        ax.set_xlabel("горизонт, точок стикування вперед")
        ax.set_ylabel(unit)
        ax.set_title(title)
        ax.grid(alpha=0.3, which="both")
        ax.legend(fontsize=8)

    text = ("Дев'ять порядків за одне ціле число. Прицілювання на наступну "
            "точку стикування змушує апарат прийти туди з будь-якою "
            "швидкістю, якої це вимагає, — а біля L2 вона величезна. Далі "
            "ціна падає й виходить на полицю.\n"
            "Занадто далекий горизонт ламається інакше й не поступово: за "
            "два оберти матриця переходу орбіти з власним значенням 594 "
            "надто чутлива, щоб її обернути, і націлювання розбігається.")
    if never:
        listed = ", ".join(label_of(h) for h in never)
        template = ("\nГоризонти %s не пролетіли жодної ділянки, тож ціни "
                    "там немає й на графіку вони відсутні."
                    if len(never) > 1 else
                    "\nГоризонт %s не пролетів жодної ділянки, тож ціни там "
                    "немає й на графіку він відсутній.")
        text += template % listed
    caption(fig, text)

    return save(fig, out_dir / "station.png")


# --- Механіка невизначеності: зростання й проходи трекінгу -----------------

def figure_uncertainty(csv_dir, out_dir):
    u = load(csv_dir / "uncertainty.csv")

    passed = [i for i, p in enumerate(u["just_had_pass"]) if p > 0.5]

    fig, axes = plt.subplots(1, 2, figsize=(12, 5))
    fig.suptitle("Зростання невизначеності на halo-орбіті 1151 (λ=594/оберт) "
                 "між проходами трекінгу")

    for ax, key, title, unit in (
            (axes[0], "pos_sigma_m", "позиція, 1σ", "м"),
            (axes[1], "vel_sigma_mps", "швидкість, 1σ", "м/с")):
        x, y = log_pairs(u["days"], u[key])
        ax.plot(x, y, color="C0", linewidth=1.2, label="між проходами")

        px = [u["days"][i] for i in passed]
        py = [u[key][i] for i in passed]
        ax.scatter(px, py, color="C3", marker="v", zorder=3, s=30,
                  label="одразу після проходу")

        ax.set_yscale("log")
        ax.set_xlabel("доба місії")
        ax.set_ylabel(unit)
        ax.set_title(title)
        ax.grid(alpha=0.3, which="both")
        ax.legend(fontsize=8)

    text = ("Кожен прохід ділить дисперсію на 10 (σ на √10 ≈ 3.16) — "
            "ілюстративне число, не результат розрахунку каналу зв'язку "
            "(core/uncertainty.h). Зростання між проходами — не ілюстрація: "
            "та сама STM, що вже звірена з нелінійною динамікою в "
            "core/test/test_uncertainty.c, пронесена крізь реальне "
            "10-тільне поле.\n"
            "На перших проходах масштаб — кілометри, той самий порядок, що "
            "приклад PROJECT.md §8 (±40 км). Далі крива обганяє власну вісь: "
            "594-кратне зростання за оберт з часом переважає 3.16-кратне "
            "стиснення проходу, і до кінця місії σ сягає астрономічних "
            "чисел, з якими вже нема сенсу порівнювати. Це не помилка "
            "розрахунку, а межа ілюстративної моделі проходу: постійний "
            "коефіцієнт стиснення не встигає за нестійкістю цієї конкретної "
            "орбіти, і саме тому реальні місії біля L2 стежать за апаратом "
            "частіше, що ближче до критичних подій.")
    caption(fig, text)

    return save(fig, out_dir / "uncertainty.png")


# --- Що втрачає інтегратор -------------------------------------------------

def figure_accuracy(csv_dir, out_dir):
    jacobi = load(csv_dir / "jacobi.csv")
    growth = load(csv_dir / "jacobi_growth.csv")
    rev = load(csv_dir / "reversibility.csv")

    fig, axes = plt.subplots(2, 2, figsize=(12, 9.5))
    fig.suptitle("Критерії приймання 1 і 2: збереження константи Якобі "
                 "та оборотність")

    # Головне тут — не мала величина дрейфу, а те, що він іде за допуском
    # майже один в один. Дрейф, який перестав меншати від затягування
    # допуску, означав би структурну помилку — а жодне окреме вимірювання
    # цього не розрізняє.
    ax = axes[0][0]
    for name, case in groups(jacobi, "orbit"):
        x, y = log_pairs(case["tolerance"], case["drift"])
        ax.plot(x, y, "o-", markersize=5, label=label_of(name))

    ax.plot([1e-6, 1e-14], [1e-7, 1e-15], "--", color="0.6", linewidth=1.0,
            label="нахил 1:1")
    ax.set_xscale("log")
    ax.set_yscale("log")
    ax.invert_xaxis()
    ax.set_xlabel("допуск інтегратора, м")
    ax.set_ylabel("|ΔC / C| за 100 обертів")
    ax.set_title("дрейф іде за допуском")
    ax.grid(alpha=0.3, which="both")
    ax.legend(fontsize=8)

    ax = axes[0][1]
    for name, case in groups(jacobi, "orbit"):
        x, y = log_pairs(case["tolerance"], case["steps"])
        ax.plot(x, y, "o-", markersize=5, label=label_of(name))
    ax.set_xscale("log")
    ax.set_yscale("log")
    ax.invert_xaxis()
    ax.set_xlabel("допуск інтегратора, м")
    ax.set_ylabel("прийнятих кроків")
    ax.set_title("і чого він коштує")
    ax.grid(alpha=0.3, which="both")
    ax.legend(fontsize=8)

    ax = axes[1][0]
    for name, case in groups(growth, "orbit"):
        x, y = log_pairs(case["revolution"], case["drift"])
        ax.plot(x, y, linewidth=1.3, label=label_of(name))
    ax.set_yscale("log")
    ax.set_xlabel("обертів")
    ax.set_ylabel("|ΔC / C|")
    ax.set_title("накопичення в часі, допуск 1e-12")
    ax.grid(alpha=0.3, which="both")
    ax.legend(fontsize=8)

    # Оборотність: похибка, що росте лінійно з прольотом, — це округлення.
    # Похибка, що росте швидше за проліт, — це метод, який губить траєкторію.
    ax = axes[1][1]
    x, y = log_pairs(rev["years"], rev["error_m"])
    ax.plot(x, y, "o-", markersize=5, color="C0",
            label="похибка повернення, м")
    ax.set_xlabel("років вперед, і стільки ж назад")
    ax.set_ylabel("|r(0) після повернення − r(0)|, м")
    ax.set_yscale("log")
    ax.set_title("оборотність, місячна орбіта, допуск 1e-6 м")
    ax.grid(alpha=0.3, which="both")

    twin = ax.twinx()
    x, y = log_pairs(rev["years"], rev["energy_drift"])
    twin.plot(x, y, "s--", markersize=4, color="C1", label="дрейф енергії")
    twin.set_yscale("log")
    twin.set_ylabel("|ΔE / E|", color="C1")
    twin.tick_params(axis="y", labelcolor="C1")

    lines = ax.get_lines() + twin.get_lines()
    ax.legend(lines, [line.get_label() for line in lines], fontsize=8,
              loc="lower right")

    caption(fig,
            "Дві початкові умови навмисно: околиця L4 — спокійна орбіта й "
            "найкраща поведінка інтегратора, близький проліт дрейфує на три "
            "порядки більше за того самого допуску. Ця різниця — властивість "
            "орбіти, а не методу, і без другої кривої першу прочитали б як "
            "властивість методу.")

    return save(fig, out_dir / "accuracy.png")


# --- Регресія на JPL Horizons ----------------------------------------------

def figure_horizons(csv_dir, out_dir):
    h = load(csv_dir / "horizons.csv")

    fig, axes = plt.subplots(2, 2, figsize=(12, 9.5))
    fig.suptitle("Критерій приймання 4: власне інтегрування проти "
                 "опублікованої ефемериди JPL, 10 років")

    panels = (
        (axes[0][0], "moon_geo_m",
         "Місяць відносно Землі — геометрія, на якій стоїть гра", "м"),
        (axes[0][1], "earth_m", "Земля в баріцентричній системі", "м"),
        (axes[1][0], "earth_rel_m",
         "те саме, з відкинутим зсувом баріцентру моделі", "м"),
        (axes[1][1], "energy_drift", "дрейф повної енергії системи",
         "|ΔE / E|"),
    )

    for ax, key, title, unit in panels:
        for name, case in groups(h, "system"):
            x, y = log_pairs([d / YEAR_DAYS for d in case["days"]], case[key])
            ax.plot(x, y, linewidth=1.4, label=label_of(name))
        ax.set_yscale("log")
        ax.set_xlabel("років від J2000")
        ax.set_ylabel(unit)
        ax.set_title(title)
        ax.grid(alpha=0.3, which="both")
        ax.legend(fontsize=8)

    caption(fig,
            "Три тіла — не гірша версія десяти, а контроль: його похибка і є "
            "розміром фізики, якої бракує. Він помиляється на мільйони "
            "кілометрів у баріцентричній системі — і майже не помиляється в "
            "геометрії Земля–Місяць, бо втрачений внесок планет зміщує всю "
            "підсистему цілком.\n"
            "Обидві криві на баріцентричній панелі тепер КОЛИВАЮТЬСЯ, а не "
            "ростуть: залишковий імпульс прибирає nbody_anchor_barycentre, "
            "тож лінійного дрейфу немає, і лишається періодичне. У трьох тіл "
            "період видно — близько 12 років, це Юпітер: у моделі без нього "
            "Сонце не хитається навколо SSB, і вся похибка Землі — це саме "
            "те нехитання.\n"
            "Порівняння будувалося ловити помилки, а не досягати точності "
            "JPL: хибна система відліку, центр чи одиниця дали б сотні тисяч "
            "кілометрів із першої ж вибірки, а не повільне зростання.")

    return save(fig, out_dir / "horizons.png")


# --- Довжина спану ефемериди -----------------------------------------------

def figure_ephspan(csv_dir, out_dir):
    e = load(csv_dir / "ephspan.csv")
    cases = dict(groups(e, "source"))

    # Кінці коротших спанів — з самих даних, а не окремим списком: вони й так
    # там є, останнім днем кожної групи asset_*.
    ends = sorted(max(case["days"]) for name, case in cases.items()
                  if name.startswith("asset_"))

    # Малюються лише повний ассет і два контролі. Коротші спани не пропущені —
    # ex_ephspan довів, що вони бітово збігаються з повним, тож окремими
    # кривими вони лягли б рівно на неї. Замість шести однакових ліній —
    # шість точок на одній.
    #
    # Ассет малюється ОСТАННІМ і товщою пунктирною лінією. У першій версії він
    # ішов першим суцільним, і контроль лягав рівно поверх нього — тобто
    # найголовніший результат цієї сторінки («ассет збігається з контролем»)
    # виглядав як «кривої ассета немає».
    shown = ("raw_tol_1m", "raw_fixture_tol", "asset_3630d")
    styles = {"asset_3630d": {"linewidth": 2.6, "linestyle": "--",
                              "zorder": 4}}

    # Обидві осі логарифмічні, і це не косметика: похибка тут росте лінійно
    # з часом, а лінійний ріст на log-log — пряма з нахилом 1. Поруч
    # намальована саме така пряма, тож твердження «росте лінійно» читається
    # з картинки, а не приймається на віру з тексту.
    fig, axes = plt.subplots(1, 2, figsize=(12, 5))
    fig.suptitle("Чи тримає ассет ефемериди довший спан: 120 діб → 10 років")

    panels = (
        (axes[0], "moon_geo_m", "Місяць відносно Землі"),
        (axes[1], "earth_rel_m", "Земля, зсув баріцентру моделі відкинуто"),
    )

    for ax, key, title in panels:
        for name in shown:
            case = cases.get(name)
            if case is None:
                continue
            # Нуль і від'ємних тут немає, але перша епоха — сама початкова
            # умова, тобто x = 0, а її на логарифмічній осі не показати.
            pairs = [(d / YEAR_DAYS, v)
                     for d, v in zip(case["days"], case[key])
                     if d > 0.0 and v > 0.0]
            ax.plot([x for x, _ in pairs], [y for _, y in pairs],
                    label=label_of(name), **styles.get(name, {"linewidth": 1.4}))

        asset = cases.get("asset_3630d")
        if asset is not None:
            marks = [(d / YEAR_DAYS, v)
                     for d, v in zip(asset["days"], asset[key])
                     if d in ends and d > 0.0 and v > 0.0]
            ax.plot([d for d, _ in marks], [v for _, v in marks],
                    "o", markersize=6, color="0.15", zorder=5,
                    label="кінці спанів")

            # Пряма нахилу 1 через останню точку ассета — суто лінійка для
            # ока: крутіше за неї чи полого. Сам нахил тут НЕ підганяється,
            # його рахує ex_ephspan і друкує числом (k у error ~ t^k); цей
            # скрипт нічого не рахує, і заводити тут МНК заради зручності
            # означало б почати.
            if marks:
                x_end, y_end = marks[-1]
                xs = [x for x, _ in marks]
                guide_x = [min(xs), x_end]
                ax.plot(guide_x, [y_end * x / x_end for x in guide_x],
                        linestyle=":", color="0.55", linewidth=1.2,
                        zorder=1, label="нахил 1:1, для ока")

        ax.set_xscale("log")
        ax.set_yscale("log")
        ax.set_xlabel("років від J2000")
        ax.set_ylabel("м")
        ax.set_title(title)
        ax.grid(alpha=0.3, which="both")
        ax.legend(fontsize=8, loc="lower right")

    caption(fig,
            "Точки — де зупинявся б ассет завдовжки 120, 240, 480, 960, 1920 "
            "діб. Вони лежать на кривій повного спану, бо лежать на ній "
            "бітово: довший ассет не переписує коротший, а продовжує його. "
            "Похибка підгонки при цьому не росте зі спаном узагалі — вона "
            "властивість інтервалу (8 діб, ступінь 14), а не довжини.\n"
            "Крива ассета йде поверх контролю з допуском фікстури й збігається "
            "з ним — отже зростання тут не похибка представлення й не дрейф "
            "кукера, а розбіжність самої моделі з JPL: бракує J2 Землі й "
            "Місяця (PROJECT.md §4). Ex_ephspan міряє нахил числом: k ≈ 1.0 "
            "для Землі й ≈0.9 для Місяця в error ~ t^k. Стала відсутня сила "
            "дає рівно k = 1; заокруглення несимплектичного інтегратора дало "
            "б k = 2 — саме таке міряє оборотність в accuracy.png.\n"
            "Третя крива — старий допуск 1 м, який фікстура використовувала "
            "до цього виміру. Він не просто грубіший: на панелі Місяця він "
            "коливається в шість разів і йде НИЖЧЕ за правильну відповідь — "
            "числова похибка частково гасила відсутню фізику, і модель "
            "виглядала точнішою, ніж є.")

    return save(fig, out_dir / "ephspan.png")


def save(fig, path):
    fig.tight_layout()
    fig.savefig(path, dpi=140, bbox_inches="tight")
    plt.close(fig)
    print("  %s" % path)
    return path


FIGURES = (
    ("cr3bp", figure_cr3bp),
    ("stability", figure_stability),
    ("trajectory", figure_trajectory),
    ("station", figure_station),
    ("uncertainty", figure_uncertainty),
    ("accuracy", figure_accuracy),
    ("horizons", figure_horizons),
    ("ephspan", figure_ephspan),
)


def main():
    names = ", ".join(name for name, _ in FIGURES)

    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--csv", type=Path, default=Path("build/csv"),
                        help="каталог із CSV від `make csv`")
    parser.add_argument("--out", type=Path, default=Path("build/plots"),
                        help="куди складати PNG")
    parser.add_argument("--only", action="append", metavar="NAME",
                        help="побудувати лише названі: %s" % names)
    args = parser.parse_args()

    if not args.csv.is_dir():
        sys.exit("немає каталогу %s — спершу make csv" % args.csv)

    wanted = [(name, build) for name, build in FIGURES
              if args.only is None or name in args.only]
    if not wanted:
        sys.exit("нічого будувати: --only %s, а є лише %s"
                 % (", ".join(args.only), names))

    args.out.mkdir(parents=True, exist_ok=True)

    print("plot.py: %s -> %s" % (args.csv, args.out))
    for _, build in wanted:
        build(args.csv, args.out)


if __name__ == "__main__":
    main()
