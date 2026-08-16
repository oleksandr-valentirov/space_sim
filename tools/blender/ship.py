# Ship model and export for the cooker (ROADMAP, T5d1; shape and paint, T9).
#
# Run headless only, and only through this script:
#
#   BLENDER=~/snap/steam/common/.steam/steam/steamapps/common/Blender/blender
#   "$BLENDER" -b --factory-startup -noaudio -P tools/blender/ship.py -- assets-src
#
# `--factory-startup` is mandatory: without it user preferences and enabled
# add-ons affect the output, i.e. the asset stops being a function of its
# inputs. Same class as `-ffast-math`.
#
# What the script does: builds the hull, four fins, a porthole and an antenna,
# paints them with **vertex colour**, saves the `.blend`, exports glTF and puts
# an **oracle** beside it -- the numbers Blender computed. Mouse work is not a
# step here: it does not reproduce and does not get reviewed.
#
# ## Axes
#
# Nose along -Y, up along +Z -- then a default export gives glTF with the nose
# at +Z, already the `Scene::Ship` convention, and the cooker transforms
# nothing (measured, skill `blender-assets`).
#
# ## The model's extent is exactly `LENGTH_M`
#
# All geometry lies in `along` from 0 (the fins' heels) to 1 (the cone's tip),
# so the model height equals `LENGTH_M` exactly, not "about that". Not
# cosmetic: the engine normalises the mesh by **its own extent**
# (`Model::from_metres`), and if the fins stuck out past one, the oracle number
# would stop matching what the cooker divided by.
#
# ## Why the shape is asymmetric
#
# The rotation of a smooth ball cannot be shown at all: the silhouette maps
# onto itself. The nose distinguishes the axis direction, the fins the plane,
# and the porthole and antenna break the 90-degree symmetry the fins leave.
# Without this an orientation oracle would check nothing -- exactly like the
# fixture above a cube face centre, where D13 and D14 lived invisibly.
#
# WARNING: one antenna stays in the model **deliberately**, though the
# reference has none: without it the ship has a `z -> -z` mirror (the hull is a
# surface of revolution, the four fins map onto themselves, the porthole sits
# on the mirror axis), and a mirror hides an axis-handedness error
# completely.

import json
import math
import os
import sys

import bmesh
import bpy

# WARNING: Steam updates Blender by itself, and the version is printed into
# the `.gltf` (`generator`). A silent update would produce a diff in the
# committed asset with no model change at all -- better a halt than a silent
# geometry change.
REQUIRED_VERSION = (5, 2)

# Ship extent in metres, along the nose axis from heels to tip. The engine
# keeps a **unit-height** mesh and scales it by `height_m` (V2), so for the
# game this is reference only; but modelling must happen in metres, or the
# numbers in the `.blend` mean nothing.
LENGTH_M = 6.0

# Largest hull radius, as a fraction of the extent. 0.113 measured off the
# reference: 87 pixels of half-width against 767 pixels of height.
RADIUS = 0.113

# How many faces in the hull's circle. 32 is the same subdivision as the V1
# placeholder: the silhouette is already smooth and the vertices stay in the
# hundreds.
SEGMENTS = 32

# Hull profile from tail to nose: (fraction of extent, fraction of the largest
# radius). The tail does not start at zero -- the fins' heels are below it.
#
# Shape from the reference: the nozzle a narrow cone, a nearly cylindrical hull
# base, an ogive thinning smoothly upwards, and a **short** nose cone -- an
# eighth of the ship rather than the third it looks like. The step under the
# hull (two rings at the same `along`) is the annular plane the reference shows
# as a shadow between the white hull and the grey nozzle.
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

# Paint boundaries as fractions of the extent. They sit **between** profile
# rings rather than on one: otherwise a band would depend on which way a
# comparison rounded.
#
# The panel seam is a 4 cm band between two adjacent rings. It has no geometry
# at all (the slope break there is under 0.2 degrees), deliberately: on the
# reference it is a painted line, not a step. There are no rivets -- those are
# a texture there, and no texture crosses the boundary yet (skill
# `blender-assets`).
NOZZLE_UNTIL = 0.117
SEAM = (0.428, 0.434)
CONE_FROM = 0.874

FINS = 4

# A fin is an outline in the plane (along the axis, outwards) plus a thickness
# fraction at each point: (fraction of extent, fraction of hull radius,
# fraction of thickness). The flat swept plate from the reference: a straight
# leading edge running from the hull down and out to a sharp heel, a short edge
# at the bottom, and a straight trailing edge back to the base. Thickness is
# uniform -- this is a plate, not a cast leg.
FIN_OUTLINE = [
    (0.360, 0.80, 1.0),
    (0.010, 2.25, 1.0),
    (0.000, 2.05, 1.0),
    (0.100, 0.80, 1.0),
]
FIN_THICKNESS = 0.08

# The antenna is a small dorsal blade, built by the same code as a fin.
ANTENNA_OUTLINE = [
    (0.700, 0.70, 1.0),
    (0.755, 0.70, 1.0),
    (0.740, 1.22, 0.6),
    (0.712, 1.22, 0.6),
]
ANTENNA_THICKNESS = 0.07

# The porthole is rings in fractions of the hull radius, together with a
# height above the hull's **actual surface** (also in fractions of the radius).
# Not above a plane: a plate 0.65 radii wide on a convex hull stands a third of
# a radius off it at the edges, and a flat rim would hang in the air.
PORTHOLE_AT = 0.636
PORTHOLE_RIM = 0.655
PORTHOLE_GLASS = 0.506
PORTHOLE_SEGMENTS = 24
# (radius, height above the surface) from the floor inside the hull to the
# glass rim.
PORTHOLE_RINGS = [
    (PORTHOLE_RIM, -0.120),
    (PORTHOLE_RIM, 0.030),
    (PORTHOLE_GLASS, 0.030),
    (PORTHOLE_GLASS, 0.012),
]
# The glass is a dome, not a disc: on the reference it is convex, and it is
# the convexity that gives it its own highlight instead of a flat patch.
PORTHOLE_DOME = [
    (0.78 * PORTHOLE_GLASS, 0.048),
    (0.42 * PORTHOLE_GLASS, 0.070),
]
PORTHOLE_APEX = 0.078

# Paint is **linear light** like everything in the frame (T6), with no baked
# lighting: the game has one light source and zero ambient, so a shadow painted
# into the base colour would read as dirt (skill `blender-assets`).
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
            f"Blender {got[0]}.{got[1]}, but the asset was cooked on "
            f"{REQUIRED_VERSION[0]}.{REQUIRED_VERSION[1]}: the numbers may have "
            "moved. Update REQUIRED_VERSION deliberately, together with a "
            "recooked asset."
        )


def axis(along, out, angle):
    """A hull point: `along` is the fraction of extent from the heels, `out`
    the radius."""
    # Nose along -Y, so the tail is at +Y: along the axis we go from +Y to -Y.
    y = (0.5 - along) * LENGTH_M
    return (
        out * math.cos(angle) * RADIUS * LENGTH_M,
        y,
        out * math.sin(angle) * RADIUS * LENGTH_M,
    )


def hull(bm, paint):
    """Hull by revolving the profile. Smooth shading halves the vertices."""
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
        # A ring is a band whose radius change exceeds its rise along the
        # axis: the base and the step under the hull. Such bands are flat, and
        # it is their edges that give the crease somewhere to live. The rest is
        # smooth.
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
    """A blade: an outline in the plane (along the axis, outwards) with
    thickness across it.

    Both the fins and the antenna are made with it. The components are
    deliberately **not stitched** to the hull: each stays a closed shell, and
    the signed volume of the whole equals the sum of the volumes even where
    they intersect.
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
    # The plate is flat throughout: smooth shading on a 5 cm edge would round
    # what the reference makes sharp and would eat the very line by which a fin
    # is seen end-on.
    for face in made:
        face.smooth = False

    paint += [(face, colour) for face in made]
    return made


def hull_radius_m(along):
    """Hull radius at this fraction of the extent, in metres -- linear between
    rings.

    The porthole needs it: it sits **on the surface** rather than on a plane,
    and the height above it must be measured from the right number.
    """
    for (low, low_r), (high, high_r) in zip(PROFILE, PROFILE[1:]):
        if low <= along <= high and high > low:
            k = (along - low) / (high - low)
            return (low_r + k * (high_r - low_r)) * RADIUS * LENGTH_M
    raise SystemExit(f"{along} is outside the hull profile")


def porthole(bm, paint):
    """Porthole: a rim on the hull and a glass dome inside it."""

    def ring(radius, height):
        made = []
        for k in range(PORTHOLE_SEGMENTS):
            angle = 2.0 * math.pi * k / PORTHOLE_SEGMENTS
            # A circle in the plane (along the axis, up), carried outwards
            # along +X.
            along = PORTHOLE_AT + radius * math.cos(angle) * RADIUS
            z = radius * math.sin(angle) * RADIUS * LENGTH_M
            surface = hull_radius_m(along)
            # The hull is a surface of revolution, so at this `along` its
            # surface in the `z` plane sits at `sqrt(r^2 - z^2)`. Right at the
            # edge of a large porthole that is noticeably less than `r`, which
            # is exactly why the rim must be laid here rather than on a tangent
            # plane.
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

    # One winding for all bands: the rim turning inwards is the same band, not
    # a reversed one. That it faces inwards is stated by the geometry (the
    # radius decreases), not by a second vertex order.
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
    """Signed volume of exactly these faces -- per component, not per model."""
    total = 0.0
    for face in faces:
        points = [v.co for v in face.verts]
        for k in range(1, len(points) - 1):
            total += points[0].dot(points[k].cross(points[k + 1]))
    return total / 6.0


def close_shell(name, faces):
    """Check a shell and turn it outwards if it is inside out.

    Two checks, and the second matters more than the first:

    1. **Every edge is traversed twice, in opposite directions.** That is
       closedness and winding consistency at once.
       `bmesh.calc_volume(signed=True)` catches neither: it computes a **sum**
       over the whole model, so both an individually inverted shell and an
       individually inconsistent face inside it hide behind the rest. This was
       not written as insurance -- it immediately failed on the prism caps,
       which had faced inwards since T5d, and there was no way to see that in
       frame: they are inside the hull.
    2. **The volume is positive.** A winding consistent with itself can still
       face inwards entirely; then the shell is flipped in one move.
    """
    seen = set()
    for face in faces:
        points = list(face.verts)
        for k, a in enumerate(points):
            edge = (a, points[(k + 1) % len(points)])
            if edge in seen:
                raise SystemExit(f"{name}: edge {edge} traversed twice in the same direction")
            seen.add(edge)
    for a, b in seen:
        if (b, a) not in seen:
            raise SystemExit(f"{name}: edge {(a, b)} unpaired -- the shell is not closed")

    if shell_volume(faces) < 0.0:
        for face in faces:
            face.normal_flip()
    if shell_volume(faces) <= 0.0:
        raise SystemExit(f"{name}: zero volume")


def build():
    bm = bmesh.new()
    paint = []
    shells = {"hull": hull(bm, paint)}
    for k in range(FINS):
        angle = 2.0 * math.pi * k / FINS
        shells[f"fin {k}"] = blade(
            bm, paint, FIN_OUTLINE, FIN_THICKNESS, angle, RED
        )
    shells["antenna"] = blade(
        bm, paint, ANTENNA_OUTLINE, ANTENNA_THICKNESS, 0.5 * math.pi, STEEL
    )
    shells["porthole"] = porthole(bm, paint)

    for name, faces in shells.items():
        close_shell(name, faces)

    bm.normal_update()
    volume = bm.calc_volume(signed=True)
    colours = [colour for _, colour in paint]
    if len(colours) != len(bm.faces):
        raise SystemExit(f"{len(colours)} colours for {len(bm.faces)} faces")

    mesh = bpy.data.meshes.new("ship")
    bm.to_mesh(mesh)
    bm.free()

    # Paint per **corner**, not per vertex: at the seam between hull and cone
    # the colour must jump, while the vertex there is shared. bmesh preserves
    # face order, so `paint` runs alongside `mesh.polygons` index for index.
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
    # Radius of the bounding sphere about the origin: `near` and the
    # third-person camera (V2) rest on it, and it does not follow from the
    # bounds -- the fins already stick out past the hull.
    extent = max(
        math.dist((0.0, 0.0, 0.0), tuple(v.co)) for v in obj.data.vertices
    )

    blend = os.path.join(out, "ship.blend")
    gltf = os.path.join(out, "ship.gltf")
    bpy.ops.wm.save_as_mainfile(filepath=os.path.abspath(blend))
    # `export_vertex_color="ACTIVE"` explicitly, not by default: the default
    # `MATERIAL` emits colour only when a material reads it, and the model has
    # no materials at all (`COLOR_0` carries the paint).
    bpy.ops.export_scene.gltf(
        filepath=os.path.abspath(gltf),
        export_format="GLTF_SEPARATE",
        export_vertex_color="ACTIVE",
    )

    # The oracle ships with the asset and is computed by **another tool**: our
    # cooker must reproduce these numbers from its own `.bin` reader rather
    # than from its own recomputation of the model.
    #
    # WARNING: the bounds here are in **Blender axes**, not glTF, and
    # deliberately so: to compare them the reader must apply the axis
    # permutation (nose -Y -> +Z), so the oracle asks about that too. The same
    # numbers in glTF axes already sit in the `POSITION` accessor -- a second,
    # independent oracle over the same `.bin`.
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

    print("asset: " + gltf)
    for key, value in sorted(oracle.items()):
        print(f"  {key}: {value}")


main()
