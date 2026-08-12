#!/usr/bin/env python3
"""Вивантаження опублікованих halo-орбіт з каталогу JPL (ROADMAP C2).

Навіщо: C2 розділений на дві частини навмисно. Спершу відтворити **чужу**
орбіту — це чистий тест інтегратора без власної машинерії пошуку, з високою
ймовірністю успіху й доброю діагностикою: якщо не замикається чужа орбіта,
проблема в C1, і шукати треба там. І лише потім шукати орбіту самому
диференціальною корекцією.

ВАЖЛИВО. У JPL власне значення mass_ratio (1.215058560962404e-02), і воно
відрізняється від порахованого з GM у data/horizons/gm.csv
(0.012150584269542) у восьмій цифрі. Опублікована початкова умова замкнеться
лише з тим самим mu — тому mu зберігається поруч із орбітами, а не береться
з іншого джерела.

    python3 scripts/fetch_halo_orbits.py
"""

import json
import sys
import urllib.request
from pathlib import Path

API = ("https://ssd-api.jpl.nasa.gov/periodic_orbits.api"
       "?sys=earth-moon&family=halo&libr=2&branch=S")

OUT_DIR = Path("data/jpl_halo")
ORBITS = OUT_DIR / "halo_l2_south.csv"
MU_FILE = OUT_DIR / "mu.txt"

# Індекси в каталозі, обрані за розкидом індексу стабільності: від майже
# нейтральної орбіти до такої, де збурення росте у 300 разів за оберт.
PICKS = [0, 383, 767, 1151, 1534]


def main():
    print("завантаження %s" % API)
    with urllib.request.urlopen(API, timeout=60) as response:
        payload = json.load(response)

    system = payload["system"]
    fields = payload["fields"]
    rows = payload["data"]

    expected = ["x", "y", "z", "vx", "vy", "vz", "jacobi", "period", "stability"]
    if fields != expected:
        sys.exit("формат API змінився: поля %s" % fields)

    OUT_DIR.mkdir(parents=True, exist_ok=True)

    MU_FILE.write_text(
        "# Відношення мас Земля-Місяць за каталогом JPL.\n"
        "# НЕ збігається з порахованим з data/horizons/gm.csv у 8-й цифрі;\n"
        "# опубліковані орбіти замикаються лише з цим значенням.\n"
        "%s\n" % system["mass_ratio"])

    lines = [
        "# Halo-орбіти навколо Earth-Moon L2, південна гілка.",
        "# Джерело: NASA/JPL Three-Body Periodic Orbits API, %s"
        % payload["signature"]["source"],
        "# Каталог містить %s орбіт; тут %d, обраних за розкидом стабільності."
        % (payload["count"], len(PICKS)),
        "# Одиниці безрозмірні. mu — у mu.txt поруч.",
        "# index,x,y,z,vx,vy,vz,jacobi,period,stability",
    ]

    for i in PICKS:
        if i >= len(rows):
            sys.exit("індекс %d поза каталогом (%d орбіт)" % (i, len(rows)))
        values = [str(v).strip() for v in rows[i]]
        lines.append("%d,%s" % (i, ",".join(values)))

    ORBITS.write_text("\n".join(lines) + "\n")

    print("записано %s (%d орбіт) і %s" % (ORBITS, len(PICKS), MU_FILE))
    print("  mu = %s" % system["mass_ratio"])
    for i in PICKS:
        r = rows[i]
        print("  індекс %-5d період %-22s стабільність %s" % (i, r[7], r[8]))


if __name__ == "__main__":
    main()
