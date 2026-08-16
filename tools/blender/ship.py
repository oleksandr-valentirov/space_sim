# Модель корабля й експорт для кукера (ROADMAP, T5d1; форма й фарба — T9).
#
# Запуск — тільки headless і тільки цим скриптом:
#
#   BLENDER=~/snap/steam/common/.steam/steam/steamapps/common/Blender/blender
#   "$BLENDER" -b --factory-startup -noaudio -P tools/blender/ship.py -- assets-src
#
# `--factory-startup` обов'язковий: без нього на вихід впливають налаштування
# користувача й увімкнені аддони, тобто ассет перестає бути функцією входу.
# Той самий клас, що `-ffast-math`.
#
# Що робить скрипт: будує корпус, чотири стабілізатори, ілюмінатор і антену,
# фарбує їх **вершинним кольором**, зберігає `.blend`, експортує glTF і кладе
# поруч **оракул** — числа, які порахував Blender. Ручна робота мишею тут не
# крок: вона не відтворюється й не переглядається в рев'ю.
#
# ## Осі
#
# Ніс уздовж −Y, верх уздовж +Z — тоді експорт дефолтами дає glTF з носом по
# +Z, тобто вже в конвенції `Scene::Ship`, і кукер не перетворює нічого
# (виміряно, скіл `blender-assets`).
#
# ## Габарит моделі — рівно `LENGTH_M`
#
# Уся геометрія лежить у `along` від 0 (п'яти стабілізаторів) до 1 (вістря
# конуса), тож висота моделі дорівнює `LENGTH_M` точно, а не «десь близько».
# Це не косметика: рушій нормалізує меш **його ж габаритом** (`Model::
# from_metres`), і якби стабілізатори вилазили за одиницю, число в оракулі
# перестало б збігатися з тим, що поділив кукер.
#
# ## Чому форма несиметрична
#
# Поворот гладкої кулі показати не можна взагалі: силует переходить сам у
# себе. Ніс відрізняє напрямок осі, стабілізатори — площину, а ілюмінатор і
# антена ламають симетрію 90°, яку самі стабілізатори лишають. Без цього
# оракул орієнтації перевіряв би нічого — рівно як фікстура над центром грані
# куба, на якій D13 і D14 прожили невидимими.
#
# ⚠ Одна антена лишається в моделі **навмисно**, хоч на референсі її немає:
# без неї корабель має дзеркало `z → −z` (корпус — тіло обертання, чотири
# стабілізатори переходять самі в себе, ілюмінатор стоїть на осі дзеркала), а
# дзеркало ховає помилку ліворукості осей повністю.

import json
import math
import os
import sys

import bmesh
import bpy

# ⚠ Steam оновлює Blender сам, а версія друкується в `.gltf` (`generator`).
# Тихе оновлення дало б діф у закоміченому ассеті без жодної зміни моделі —
# краще зупинка, ніж мовчазна зміна геометрії.
REQUIRED_VERSION = (5, 2)

# Габарит корабля в метрах — уздовж осі носа, від п'ят до вістря. Рушій тримає
# меш **одиничної висоти** й масштабує його `height_m` (V2), тож для гри це
# довідка; але моделювати треба в метрах, інакше числа в `.blend` нічого не
# означають.
LENGTH_M = 6.0

# Найбільший радіус корпусу — частка габариту. 0.113 зміряно з референсу:
# 87 пікселів півширини на 767 пікселів висоти.
RADIUS = 0.113

# Скільки граней у кола корпусу. 32 — той самий поділ, що в заглушки V1:
# силует уже гладкий, а вершин лишається кілька сотень.
SEGMENTS = 32

# Профіль корпусу від хвоста до носа: (частка габариту, частка найбільшого
# радіуса). Хвіст починається не з нуля — під ним ще п'яти стабілізаторів.
#
# Форма з референсу: сопло вузьким конусом, майже циліндричне денце корпусу,
# оживало, що плавно тоншає догори, і **короткий** носовий конус — не третина
# корабля, як здається на око, а восьма частина. Уступ під корпусом (два
# кільця на однаковому `along`) — це і є та кільцева площина, яку на референсі
# видно тінню між білим корпусом і сірим соплом.
PROFILE = [
    (0.013, 0.000),
    (0.013, 0.430),
    (0.039, 0.437),
    (0.117, 0.632),
    (0.117, 0.989),
    (0.241, 1.000),
    (0.372, 0.989),
    (0.428, 0.969),
    (0.434, 0.967),
    (0.502, 0.943),
    (0.632, 0.851),
    (0.763, 0.690),
    (0.841, 0.517),
    (0.874, 0.460),
    (1.000, 0.000),
]

# Межі фарби в частках габариту. Стоять **між** кільцями профілю, а не на
# кільці: інакше смуга залежала б від того, куди округлиться порівняння.
#
# Шов панелі — смуга завширшки 4 см між двома сусідніми кільцями. Геометрії
# він не має взагалі (злам нахилу там менший за 0.2°), і це навмисно: на
# референсі це лінія фарби, а не уступ. Заклепок немає — вони там текстура, а
# текстур через межу ще не їде жодна (скіл `blender-assets`).
NOZZLE_UNTIL = 0.117
SEAM = (0.428, 0.434)
CONE_FROM = 0.874

FINS = 4

# Стабілізатор — контур у площині (вздовж осі, назовні) плюс частка товщини в
# кожній точці: (частка габариту, частка радіуса корпусу, частка товщини).
# Плоска стрілоподібна пластина з референсу: пряма передня кромка від корпусу
# вниз-назовні до гострої п'яти, коротка кромка внизу й пряма задня кромка
# назад до денця. Товщина однакова скрізь — це пластина, а не лита нога.
FIN_OUTLINE = [
    (0.360, 0.80, 1.0),
    (0.010, 2.25, 1.0),
    (0.000, 2.05, 1.0),
    (0.100, 0.80, 1.0),
]
FIN_THICKNESS = 0.08

# Антена — маленьке спинне перо тим самим кодом, що стабілізатор.
ANTENNA_OUTLINE = [
    (0.700, 0.70, 1.0),
    (0.755, 0.70, 1.0),
    (0.740, 1.22, 0.6),
    (0.712, 1.22, 0.6),
]
ANTENNA_THICKNESS = 0.07

# Ілюмінатор — кільця в частках радіуса корпусу разом з висотою над **самою
# поверхнею** корпусу (теж у частках радіуса). Не над площиною: пластина
# завширшки 0.65 радіуса на опуклому корпусі відстає від нього по краях на
# третину радіуса, і плоский обідок висів би в повітрі.
PORTHOLE_AT = 0.636
PORTHOLE_RIM = 0.655
PORTHOLE_GLASS = 0.506
PORTHOLE_SEGMENTS = 24
# (радіус, висота над поверхнею) від дна всередині корпусу до краю скла.
PORTHOLE_RINGS = [
    (PORTHOLE_RIM, -0.120),
    (PORTHOLE_RIM, 0.030),
    (PORTHOLE_GLASS, 0.030),
    (PORTHOLE_GLASS, 0.012),
]
# Скло — купол, а не диск: на референсі воно опукле, і саме опуклість дає йому
# власний відблиск замість плаского плями.
PORTHOLE_DOME = [
    (0.78 * PORTHOLE_GLASS, 0.048),
    (0.42 * PORTHOLE_GLASS, 0.070),
]
PORTHOLE_APEX = 0.078

# Фарба — **лінійне світло**, як усе в кадрі (T6), і без запеченого освітлення:
# у грі одне джерело й нуль ambient, тож підмальована тінь у базовому кольорі
# виглядала б брудом (скіл `blender-assets`).
ENAMEL = (0.82, 0.82, 0.82)
RED = (0.75, 0.050, 0.020)
YELLOW = (0.90, 0.60, 0.020)
STEEL = (0.30, 0.30, 0.31)
SEAM_PAINT = (0.10, 0.10, 0.10)
GLASS = (0.45, 0.56, 0.66)


def require_blender():
    got = bpy.app.version[:2]
    if got != REQUIRED_VERSION:
        raise SystemExit(
            f"Blender {got[0]}.{got[1]}, а ассет кукався на "
            f"{REQUIRED_VERSION[0]}.{REQUIRED_VERSION[1]}: числа могли поїхати. "
            "Онови REQUIRED_VERSION свідомо, разом з перекуканим ассетом."
        )


def axis(along, out, angle):
    """Точка корпусу: `along` — частка габариту від п'ят, `out` — радіус."""
    # Ніс уздовж −Y, тобто хвіст у +Y: вздовж осі йдемо від +Y до −Y.
    y = (0.5 - along) * LENGTH_M
    return (
        out * math.cos(angle) * RADIUS * LENGTH_M,
        y,
        out * math.sin(angle) * RADIUS * LENGTH_M,
    )


def hull(bm, paint):
    """Корпус обертанням профілю. Гладке затінення — вдвічі менше вершин."""
    rings = []
    for along, out in PROFILE:
        if out == 0.0:
            rings.append([bm.verts.new(axis(along, 0.0, 0.0))])
            continue
        ring = []
        for k in range(SEGMENTS):
            angle = 2.0 * math.pi * k / SEGMENTS
            ring.append(bm.verts.new(axis(along, out, angle)))
        rings.append(ring)

    made = []
    for index, (lower, upper) in enumerate(zip(rings, rings[1:])):
        if len(lower) == 1:
            band = [
                bm.faces.new((lower[0], upper[k], upper[(k + 1) % SEGMENTS]))
                for k in range(SEGMENTS)
            ]
        elif len(upper) == 1:
            band = [
                bm.faces.new((lower[k], upper[0], lower[(k + 1) % SEGMENTS]))
                for k in range(SEGMENTS)
            ]
        else:
            band = [
                bm.faces.new(
                    (
                        lower[k],
                        upper[k],
                        upper[(k + 1) % SEGMENTS],
                        lower[(k + 1) % SEGMENTS],
                    )
                )
                for k in range(SEGMENTS)
            ]

        low, high = PROFILE[index], PROFILE[index + 1]
        # Кільце — це смуга, у якої зміна радіуса більша за підйом уздовж осі:
        # денце й уступ під корпусом. Такі смуги плоскі, і саме їхні краї
        # дають зламу де бути. Решта гладка.
        rise = abs(high[0] - low[0]) * LENGTH_M
        flare = abs(high[1] - low[1]) * RADIUS * LENGTH_M
        smooth = rise > flare
        middle = 0.5 * (low[0] + high[0])
        if middle < NOZZLE_UNTIL:
            colour = STEEL
        elif middle > CONE_FROM:
            colour = RED
        elif SEAM[0] < middle < SEAM[1]:
            colour = SEAM_PAINT
        else:
            colour = ENAMEL

        for face in band:
            face.smooth = smooth
        made += [(face, colour) for face in band]

    paint += made
    return [face for face, _ in made]


def blade(bm, paint, outline, thickness, angle, colour):
    """Перо: контур у площині (вздовж осі, назовні), товщина впоперек.

    Ним зроблені і стабілізатори, і антена. Компоненти навмисно **не
    зшиваються** з корпусом: кожен лишається замкненою оболонкою, і знаковий
    об'єм цілого дорівнює сумі об'ємів навіть там, де вони перетинаються.
    """
    side = (-math.sin(angle), 0.0, math.cos(angle))
    layers = []
    for sign in (-1.0, 1.0):
        layer = []
        for along, out, weight in outline:
            x, y, z = axis(along, out, angle)
            half = 0.5 * thickness * weight * RADIUS * LENGTH_M
            layer.append(
                bm.verts.new((x + sign * side[0] * half, y, z + sign * side[2] * half))
            )
        layers.append(layer)

    n = len(outline)
    made = [bm.faces.new(list(reversed(layers[0]))), bm.faces.new(layers[1])]
    for k in range(n):
        made.append(
            bm.faces.new(
                (
                    layers[0][k],
                    layers[0][(k + 1) % n],
                    layers[1][(k + 1) % n],
                    layers[1][k],
                )
            )
        )
    # Пластина плоска цілком: гладке затінення на кромці завтовшки 5 см
    # округлило б те, що на референсі гостре, і з'їло б саме ту лінію, за
    # якою стабілізатор видно з торця.
    for face in made:
        face.smooth = False

    paint += [(face, colour) for face in made]
    return made


def hull_radius_m(along):
    """Радіус корпусу на цій частці габариту, метри — лінійно між кільцями.

    Потрібен ілюмінаторові: він сідає **на поверхню**, а не на площину, і
    висоту над нею треба відкладати від правильного числа.
    """
    for (low, low_r), (high, high_r) in zip(PROFILE, PROFILE[1:]):
        if low <= along <= high and high > low:
            k = (along - low) / (high - low)
            return (low_r + k * (high_r - low_r)) * RADIUS * LENGTH_M
    raise SystemExit(f"{along} поза профілем корпусу")


def porthole(bm, paint):
    """Ілюмінатор: обідок на корпусі й купол скла в ньому."""

    def ring(radius, height):
        made = []
        for k in range(PORTHOLE_SEGMENTS):
            angle = 2.0 * math.pi * k / PORTHOLE_SEGMENTS
            # Коло в площині (вздовж осі, вгору), винесене назовні по +X.
            along = PORTHOLE_AT + radius * math.cos(angle) * RADIUS
            z = radius * math.sin(angle) * RADIUS * LENGTH_M
            surface = hull_radius_m(along)
            # Корпус — тіло обертання, тож на цій `along` його поверхня в
            # площині `z` стоїть на `sqrt(r² − z²)`. Під самим краєм великого
            # ілюмінатора це помітно менше за `r`, і саме тому обідок треба
            # класти сюди, а не на дотичну площину.
            base = math.sqrt(max(surface * surface - z * z, 0.0))
            made.append(bm.verts.new((base + height * RADIUS * LENGTH_M, axis(along, 0.0, 0.0)[1], z)))
        return made

    rings = [ring(radius, height) for radius, height in PORTHOLE_RINGS]
    dome = [ring(radius, height) for radius, height in PORTHOLE_DOME]
    apex = bm.verts.new(
        (
            hull_radius_m(PORTHOLE_AT) + PORTHOLE_APEX * RADIUS * LENGTH_M,
            axis(PORTHOLE_AT, 0.0, 0.0)[1],
            0.0,
        )
    )

    def band(lower, upper):
        made = []
        for k in range(PORTHOLE_SEGMENTS):
            j = (k + 1) % PORTHOLE_SEGMENTS
            made.append(bm.faces.new((lower[k], lower[j], upper[j], upper[k])))
        return made

    # Обхід один на всі смуги: обідок усередину — це та сама смуга, а не
    # перевернута. Що вона дивиться всередину, каже геометрія (радіус меншає),
    # а не другий порядок вершин.
    rim = [bm.faces.new(list(reversed(rings[0])))]
    for lower, upper in zip(rings, rings[1:]):
        rim += band(lower, upper)

    layers = [rings[-1]] + dome
    glass = []
    for lower, upper in zip(layers, layers[1:]):
        glass += band(lower, upper)
    glass += [
        bm.faces.new((layers[-1][k], layers[-1][(k + 1) % PORTHOLE_SEGMENTS], apex))
        for k in range(PORTHOLE_SEGMENTS)
    ]

    for face in rim:
        face.smooth = False
    for face in glass:
        face.smooth = True
    paint += [(face, YELLOW) for face in rim]
    paint += [(face, GLASS) for face in glass]
    return rim + glass


def shell_volume(faces):
    """Знаковий об'єм саме цих граней — по компоненту, а не по всій моделі."""
    total = 0.0
    for face in faces:
        points = [v.co for v in face.verts]
        for k in range(1, len(points) - 1):
            total += points[0].dot(points[k].cross(points[k + 1]))
    return total / 6.0


def close_shell(name, faces):
    """Перевірити оболонку й повернути її назовні, якщо вона вивернута.

    Дві перевірки, і друга важливіша за першу:

    1. **Кожне ребро пройдене двічі й у різні боки.** Це і замкненість, і
       узгодженість обходу разом. `bmesh.calc_volume(signed=True)` не ловить
       ні того, ні іншого: він рахує **суму** по всій моделі, тож і окрема
       вивернута оболонка, і окрема неузгоджена грань у ній ховаються за
       рештою. Написана вона тут не про запас — на ній одразу впали кришки
       призми, які від T5d стояли поверненими всередину, і побачити це в
       кадрі було ніяк: усередині корпусу.
    2. **Об'єм додатний.** Обхід, узгоджений з собою, все ще може дивитися
       всередину цілком; тоді оболонка перевертається одним рухом.
    """
    seen = set()
    for face in faces:
        points = list(face.verts)
        for k, a in enumerate(points):
            edge = (a, points[(k + 1) % len(points)])
            if edge in seen:
                raise SystemExit(f"{name}: ребро {edge} пройдене двічі в один бік")
            seen.add(edge)
    for a, b in seen:
        if (b, a) not in seen:
            raise SystemExit(f"{name}: ребро {(a, b)} без пари — оболонка не замкнена")

    if shell_volume(faces) < 0.0:
        for face in faces:
            face.normal_flip()
    if shell_volume(faces) <= 0.0:
        raise SystemExit(f"{name}: нульовий об'єм")


def build():
    bm = bmesh.new()
    paint = []
    shells = {"корпус": hull(bm, paint)}
    for k in range(FINS):
        angle = 2.0 * math.pi * k / FINS
        shells[f"стабілізатор {k}"] = blade(
            bm, paint, FIN_OUTLINE, FIN_THICKNESS, angle, RED
        )
    shells["антена"] = blade(
        bm, paint, ANTENNA_OUTLINE, ANTENNA_THICKNESS, 0.5 * math.pi, STEEL
    )
    shells["ілюмінатор"] = porthole(bm, paint)

    for name, faces in shells.items():
        close_shell(name, faces)

    bm.normal_update()
    volume = bm.calc_volume(signed=True)
    colours = [colour for _, colour in paint]
    if len(colours) != len(bm.faces):
        raise SystemExit(f"{len(colours)} кольорів на {len(bm.faces)} граней")

    mesh = bpy.data.meshes.new("ship")
    bm.to_mesh(mesh)
    bm.free()

    # Фарба по **кутках**, а не по вершинах: на шві корпусу з конусом колір
    # мусить стрибати, а вершина там спільна. Порядок граней bmesh зберігає,
    # тож `paint` іде поруч із `mesh.polygons` індекс в індекс.
    attribute = mesh.color_attributes.new(name="paint", type="FLOAT_COLOR", domain="CORNER")
    for polygon, colour in zip(mesh.polygons, colours):
        for loop in polygon.loop_indices:
            attribute.data[loop].color = (colour[0], colour[1], colour[2], 1.0)
    mesh.color_attributes.active_color_index = 0

    obj = bpy.data.objects.new("ship", mesh)
    bpy.context.collection.objects.link(obj)
    return obj, volume


def bounds(obj):
    low = [float("inf")] * 3
    high = [float("-inf")] * 3
    for vertex in obj.data.vertices:
        for k in range(3):
            low[k] = min(low[k], vertex.co[k])
            high[k] = max(high[k], vertex.co[k])
    return low, high


def main():
    require_blender()
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    out = argv[0] if argv else "assets-src"
    os.makedirs(out, exist_ok=True)

    bpy.ops.wm.read_factory_settings(use_empty=True)
    obj, volume = build()

    low, high = bounds(obj)
    # Радіус обмежувальної сфери навколо початку координат: на ньому стоять
    # `near` і камера третьої особи (V2), і з габаритів він не виводиться —
    # стабілізатори вже виступають за корпус.
    extent = max(
        math.dist((0.0, 0.0, 0.0), tuple(v.co)) for v in obj.data.vertices
    )

    blend = os.path.join(out, "ship.blend")
    gltf = os.path.join(out, "ship.gltf")
    bpy.ops.wm.save_as_mainfile(filepath=os.path.abspath(blend))
    # `export_vertex_color="ACTIVE"` — явно, а не дефолтом: дефолт `MATERIAL`
    # віддає колір лише тоді, коли його читає матеріал, а матеріалів у моделі
    # немає взагалі (фарбу везе `COLOR_0`).
    bpy.ops.export_scene.gltf(
        filepath=os.path.abspath(gltf),
        export_format="GLTF_SEPARATE",
        export_vertex_color="ACTIVE",
    )

    # Оракул їде разом з ассетом і рахується **іншим інструментом**: наш
    # кукер мусить відтворити ці числа зі свого читача `.bin`, а не з
    # власного перерахунку моделі.
    #
    # ⚠ Габарити тут у **осях Blender**, а не glTF, і навмисно: щоб звірити їх,
    # читач мусить застосувати перестановку осей (ніс −Y → +Z), тобто оракул
    # питає ще й про неї. Ті самі числа в осях glTF уже лежать в акесорі
    # `POSITION` — це другий, незалежний оракул на той самий `.bin`.
    oracle = {
        "blender": ".".join(str(v) for v in bpy.app.version),
        "length_m": LENGTH_M,
        "volume_m3": volume,
        "blender_min": low,
        "blender_max": high,
        "extent_m": extent,
        "vertices_in_blender": len(obj.data.vertices),
        "triangles": sum(len(p.vertices) - 2 for p in obj.data.polygons),
    }
    with open(os.path.join(out, "ship.oracle.json"), "w", encoding="utf-8") as f:
        json.dump(oracle, f, indent=2, sort_keys=True)
        f.write("\n")

    print("ассет: " + gltf)
    for key, value in sorted(oracle.items()):
        print(f"  {key}: {value}")


main()
