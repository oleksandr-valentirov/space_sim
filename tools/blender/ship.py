# Модель корабля й експорт для кукера (ROADMAP, T5d1).
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
# зберігає `.blend`, експортує glTF і кладе поруч **оракул** — числа, які
# порахував Blender. Ручна робота мишею тут не крок: вона не відтворюється й
# не переглядається в рев'ю.
#
# ## Осі
#
# Ніс уздовж −Y, верх уздовж +Z — тоді експорт дефолтами дає glTF з носом по
# +Z, тобто вже в конвенції `Scene::Ship`, і кукер не перетворює нічого
# (виміряно, скіл `blender-assets`).
#
# ## Чому форма несиметрична
#
# Поворот гладкої кулі показати не можна взагалі: силует переходить сам у
# себе. Ніс відрізняє напрямок осі, стабілізатори — площину, а ілюмінатор і
# антена ламають симетрію 90°, яку самі стабілізатори лишають. Без цього
# оракул орієнтації перевіряв би нічого — рівно як фікстура над центром грані
# куба, на якій D13 і D14 прожили невидимими.

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

# Довжина корабля в метрах — уздовж осі носа. Рушій тримає меш **одиничної
# висоти** й масштабує його `height_m` (V2), тож для гри це довідка; але
# моделювати треба в метрах, інакше числа в `.blend` нічого не означають.
LENGTH_M = 6.0

# Найбільший радіус корпусу — частка довжини.
RADIUS = 0.20

# Скільки граней у кола корпусу. 32 — той самий поділ, що в заглушки V1:
# силует уже гладкий, а вершин лишається кілька сотень.
SEGMENTS = 32

# Профіль корпусу від хвоста до носа: (частка довжини, частка найбільшого
# радіуса). Вертикальні ділянки (однакове `t`) — кільця: сопловий комір і
# уступ під носовим конусом.
PROFILE = [
    (0.000, 0.000),
    (0.000, 0.62),
    (0.045, 0.62),
    (0.070, 0.78),
    (0.180, 0.94),
    (0.420, 1.00),
    (0.660, 1.00),
    (0.780, 0.92),
    (0.820, 0.72),
    (1.000, 0.00),
]

FINS = 4
# Стабілізатор у площині (частка довжини вздовж осі, частка радіуса впоперек).
FIN_PROFILE = [(0.02, 0.55), (0.30, 0.55), (0.00, 2.05)]
FIN_THICKNESS = 0.09

# Ілюмінатор — циліндр упоперек корпусу; антена — тонкий брусок над ним.
PORTHOLE_AT = 0.62
PORTHOLE_RADIUS = 0.26
PORTHOLE_REACH = (0.86, 1.16)
ANTENNA_AT = (0.30, 0.52)
ANTENNA_SIZE = (0.04, 0.04)
ANTENNA_REACH = (0.95, 2.30)


def require_blender():
    got = bpy.app.version[:2]
    if got != REQUIRED_VERSION:
        raise SystemExit(
            f"Blender {got[0]}.{got[1]}, а ассет кукався на "
            f"{REQUIRED_VERSION[0]}.{REQUIRED_VERSION[1]}: числа могли поїхати. "
            "Онови REQUIRED_VERSION свідомо, разом з перекуканим ассетом."
        )


def axis(along, out, angle):
    """Точка корпусу: `along` — частка довжини від хвоста, `out` — радіус."""
    # Ніс уздовж −Y, тобто хвіст у +Y: вздовж осі йдемо від +Y до −Y.
    y = (0.5 - along) * LENGTH_M
    return (
        out * math.cos(angle) * RADIUS * LENGTH_M,
        y,
        out * math.sin(angle) * RADIUS * LENGTH_M,
    )


def hull(bm):
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

    faces = []
    for lower, upper in zip(rings, rings[1:]):
        if len(lower) == 1:
            faces += [
                bm.faces.new((lower[0], upper[k], upper[(k + 1) % SEGMENTS]))
                for k in range(SEGMENTS)
            ]
        elif len(upper) == 1:
            faces += [
                bm.faces.new((lower[k], upper[0], lower[(k + 1) % SEGMENTS]))
                for k in range(SEGMENTS)
            ]
        else:
            faces += [
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
    for face in faces:
        face.smooth = True


def prism(bm, points, thickness, angle):
    """Призма: багатокутник у площині (вздовж осі, назовні), товщина впоперек.

    Використовується стабілізаторами. Компоненти навмисно **не зшиваються**
    з корпусом: кожен лишається замкненою оболонкою, і знаковий об'єм цілого
    дорівнює сумі об'ємів навіть там, де вони перетинаються.
    """
    half = 0.5 * thickness * RADIUS * LENGTH_M
    side = (-math.sin(angle) * half, 0.0, math.cos(angle) * half)
    layers = []
    for sign in (-1.0, 1.0):
        layer = []
        for along, out in points:
            x, y, z = axis(along, out, angle)
            layer.append(bm.verts.new((x + sign * side[0], y, z + sign * side[2])))
        layers.append(layer)

    n = len(points)
    bm.faces.new(layers[0])
    bm.faces.new(list(reversed(layers[1])))
    for k in range(n):
        bm.faces.new(
            (
                layers[0][k],
                layers[0][(k + 1) % n],
                layers[1][(k + 1) % n],
                layers[1][k],
            )
        )


def tube(bm, at, radius, reach, segments=12):
    """Циліндр упоперек корпусу — ілюмінатор."""
    caps = []
    for out in reach:
        ring = []
        for k in range(segments):
            angle = 2.0 * math.pi * k / segments
            # Коло в площині (вздовж осі, вгору), винесене назовні по +X.
            along = at + radius * math.cos(angle) * RADIUS
            z = radius * math.sin(angle) * RADIUS * LENGTH_M
            x, y, _ = axis(along, out, 0.0)
            ring.append(bm.verts.new((x, y, z)))
        caps.append(ring)
    bm.faces.new(caps[0])
    bm.faces.new(list(reversed(caps[1])))
    for k in range(segments):
        bm.faces.new(
            (
                caps[0][k],
                caps[0][(k + 1) % segments],
                caps[1][(k + 1) % segments],
                caps[1][k],
            )
        )


def box(bm, span, size, reach):
    """Брусок над корпусом — антена. Ламає симетрію 90°, як і ілюмінатор."""
    corners = []
    for out in reach:
        for along in span:
            for side in (-1.0, 1.0):
                x, y, z = axis(along, out, 0.5 * math.pi)
                corners.append(
                    bm.verts.new((x + side * size[0] * RADIUS * LENGTH_M, y, z))
                )
    #  Порядок вершин: [out][along][side] — грані виписані явно.
    def at(o, a, s):
        return corners[o * 4 + a * 2 + s]

    quads = [
        (at(0, 0, 0), at(0, 0, 1), at(0, 1, 1), at(0, 1, 0)),
        (at(1, 0, 0), at(1, 1, 0), at(1, 1, 1), at(1, 0, 1)),
        (at(0, 0, 0), at(0, 1, 0), at(1, 1, 0), at(1, 0, 0)),
        (at(0, 0, 1), at(1, 0, 1), at(1, 1, 1), at(0, 1, 1)),
        (at(0, 0, 0), at(1, 0, 0), at(1, 0, 1), at(0, 0, 1)),
        (at(0, 1, 0), at(0, 1, 1), at(1, 1, 1), at(1, 1, 0)),
    ]
    for quad in quads:
        bm.faces.new(quad)


def build():
    bm = bmesh.new()
    hull(bm)
    for k in range(FINS):
        prism(bm, FIN_PROFILE, FIN_THICKNESS, 2.0 * math.pi * k / FINS)
    tube(bm, PORTHOLE_AT, PORTHOLE_RADIUS, PORTHOLE_REACH)
    box(bm, ANTENNA_AT, ANTENNA_SIZE, ANTENNA_REACH)

    bm.normal_update()
    # Нормалі назовні: у замкненої оболонки знаковий об'єм додатний, і це
    # єдине, що тут означає «назовні». Перевіряється, а не припускається.
    if bm.calc_volume(signed=True) < 0.0:
        for face in bm.faces:
            face.normal_flip()

    mesh = bpy.data.meshes.new("ship")
    bm.to_mesh(mesh)
    volume = bm.calc_volume(signed=True)
    bm.free()

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
    bpy.ops.export_scene.gltf(
        filepath=os.path.abspath(gltf),
        export_format="GLTF_SEPARATE",
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
