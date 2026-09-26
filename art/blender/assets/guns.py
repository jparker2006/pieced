"""Guns: the rifle and the pump, first-person viewmodels (targets T02, T04; also T01, T03, T05, R4).

Chunky, toy-like Fortnite proportions in brass, dark wood and dark iron, each
powered by a glowing crystal:

* `rifle`: a SCAR-style assault rifle. Brass collars and bands, a dark-wood
  stock, forend and grip, a thick iron barrel with a brass muzzle, a glass
  chamber on top holding a faceted blue rune crystal, glowing rune windows on
  both sides of the forend, an energy-cell magazine with glowing slots, and
  chunky iron sights (a U-notch rear and a ring front sight).
* `pump`: stubby, with a flared brass bell muzzle, a ribbed wooden pump grip
  that slides, and a violet crystal floating in an open cradle, wrapped in three
  tilted gold rings (they spin on the rack).

Named parts (Rust finds them by name and animates them):

    rifle: Body, Stock, Chamber, Crystal, Mag, Muzzle
    pump:  Body, Stock, Crystal, Rings, PumpGrip, Shard, Muzzle
           (Shard: a small violet crystal shard, the pump's "shell"; it rests
           inside the crystal and is hidden in game until a reload pushes it in)

Attach points (empties; -Y forward, +Z up in Blender, so -Z forward in Bevy):

    MuzzleTip      the barrel's exit, facing along the barrel
    CrystalSocket  the crystal's centre (the Crystal part's pivot)
    GripR          the pistol grip, at the right glove's centre. Its +Z runs up
                   the raked grip axis. Both guns share the grip, so the right
                   glove fits both.
    GripL          where the left glove holds: the rifle's forend, the pump's
                   PumpGrip (parented to it, so it slides with the rack). +Z up.
    Sight          the rear sight notch, on the sight line (aim-down-sights eye line)
    SightFront     the front sight (ring centre / bead) on the same line

Model space: the gun faces Blender -Y (the muzzle end), +Z up, +X is the gun's
LEFT (the side the player sees). Parts are designed around the receiver (the
trigger at the origin); the finished gun is then lifted so its lowest point sits
on z = 0, the pipeline's "pivot on the ground" (the preview's ground disc stays
below it). Use the attach points, not the origin, to place the gun.

Every part is a closed mesh; small parts are joined into their part, coloured by
palette name (`art/palette.json`), with soft bevels and smooth shading.
"""

import math

import bmesh
from mathutils import Euler, Matrix, Vector

from lib import palette, scene, shapes
from lib.registry import Asset

# ---------------------------------------------------------------------------
# Geometry kit (local to the gun and glove families)
# ---------------------------------------------------------------------------


def _signed_pow(c, e):
    return math.copysign(abs(c) ** e, c)


def axis_frame(axis):
    """Two unit vectors perpendicular to `axis` (u, v), with u x v along axis."""
    a = Vector(axis).normalized()
    ref = Vector((0.0, 0.0, 1.0)) if abs(a.z) < 0.9 else Vector((1.0, 0.0, 0.0))
    u = ref.cross(a).normalized()
    v = a.cross(u).normalized()
    return u, v


def finish(bm, color=None, bevel=0.0, segments=2, angle_deg=35.0, matrix=None):
    """Turns a bmesh into a scratch object: bevel, transform, colour.

    With `color` None the bmesh must already carry palette tags (see `lathe`).
    """
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    obj = shapes.temp_object("Shape", shapes.mesh_from_bmesh(bm, "Shape"))
    if bevel > 0.0:
        shapes.bevel(obj, bevel, segments, angle_deg)
    if matrix is not None:
        obj.data.transform(matrix)
    if color is not None:
        palette.paint_object(obj, color)
    return obj


def place(location=(0.0, 0.0, 0.0), rotation=(0.0, 0.0, 0.0)):
    return Matrix.Translation(Vector(location)) @ Euler(rotation).to_matrix().to_4x4()


def rbox(center, size, color, bevel=0.0, segments=1, rotation=(0.0, 0.0, 0.0)):
    """A box (size = full extents), softly bevelled, rotated then moved to `center`."""
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    for v in bm.verts:
        v.co = Vector((v.co.x * size[0], v.co.y * size[1], v.co.z * size[2]))
    return finish(bm, color, bevel, segments, 30.0, place(center, rotation))


def ring_verts(bm, center, u, v, hu, hv, n, seg, a0=0.0):
    """A closed superellipse ring: n = 2 is an ellipse, higher n a rounded rectangle."""
    e = 2.0 / n
    out = []
    for k in range(seg):
        a = a0 + 2.0 * math.pi * k / seg
        out.append(bm.verts.new(center + u * (hu * _signed_pow(math.cos(a), e))
                                + v * (hv * _signed_pow(math.sin(a), e))))
    return out


def loft(stations, color, seg=24, bevel=0.0, bevel_segments=2, cap=True):
    """A closed tube through superellipse stations [(centre, u, v, hu, hv, n)]."""
    bm = bmesh.new()
    rings = [ring_verts(bm, Vector(c), Vector(u), Vector(v), hu, hv, n, seg)
             for c, u, v, hu, hv, n in stations]
    for a, b in zip(rings, rings[1:]):
        shapes.bridge(bm, a, b)
    if cap:
        bm.faces.new(list(reversed(rings[0])))
        bm.faces.new(rings[-1])
    return finish(bm, color, bevel, bevel_segments, 40.0)


X = (1.0, 0.0, 0.0)
Z = (0.0, 0.0, 1.0)


def loft_y(stations, color, seg=24, bevel=0.0, bevel_segments=2, x=0.0):
    """A loft along the gun (Y) of rounded rectangles [(y, zc, half_w, half_h, n)]."""
    return loft([((x, y, zc), X, Z, hw, hh, n) for y, zc, hw, hh, n in stations],
                color, seg, bevel, bevel_segments)


def band_y(y0, y1, zc, hw, hh, color, n=3.4, seg=20, bevel=0.0, bevel_segments=2):
    """A short rounded-rectangle band around the gun (collars, straps)."""
    return loft_y([(y0, zc, hw, hh, n), (y1, zc, hw, hh, n)], color, seg, bevel,
                  bevel_segments)


def cyl(p0, p1, r, color, seg=20, bevel=0.0, bevel_segments=2, cap=True):
    """A cylinder from p0 to p1 (open-ended with cap=False)."""
    p0, p1 = Vector(p0), Vector(p1)
    u, v = axis_frame(p1 - p0)
    return loft([(p0, u, v, r, r, 2.0), (p1, u, v, r, r, 2.0)], color, seg, bevel,
                bevel_segments, cap)


def lathe(origin, axis, profile, colors, seg=24):
    """A surface of revolution: `profile` is a closed polygon [(t, r)] (t along
    `axis` from `origin`, r > 0 the radius); edge i (point i to i+1) takes
    colors[i] (a palette name, or one name for all)."""
    origin, a = Vector(origin), Vector(axis).normalized()
    u, v = axis_frame(a)
    bm = palette.new_bmesh()
    rings = []
    for t, r in profile:
        rings.append([bm.verts.new(origin + a * t + (u * math.cos(k * 2 * math.pi / seg)
                                                     + v * math.sin(k * 2 * math.pi / seg)) * r)
                      for k in range(seg)])
    n = len(rings)
    for i in range(n):
        faces = shapes.bridge(bm, rings[i], rings[(i + 1) % n])
        palette.tag(bm, faces, colors if isinstance(colors, str) else colors[i])
    return finish(bm, None)


def hoop(center, normal, radius, width, thickness, color, seg=32):
    """A flat ring (rectangular section) around `normal`: bands, hoops."""
    w, t = width / 2.0, thickness / 2.0
    profile = [(-w, radius - t), (w, radius - t), (w, radius + t), (-w, radius + t)]
    return lathe(center, normal, profile, color, seg)


def arc_band(center, axis, r_in, r_out, depth, a0, a1, color, seg=16):
    """Part of a ring (rectangular section) from angle a0 to a1 (radians, from
    the frame's u towards v), capped at both ends: the C-shaped ring sight."""
    center, a = Vector(center), Vector(axis).normalized()
    u, v = axis_frame(a)
    bm = bmesh.new()
    loops = []
    for k in range(seg + 1):
        ang = a0 + (a1 - a0) * k / seg
        d = u * math.cos(ang) + v * math.sin(ang)
        loops.append([bm.verts.new(center + d * r + a * s)
                      for r, s in ((r_in, -depth / 2), (r_out, -depth / 2),
                                   (r_out, depth / 2), (r_in, depth / 2))])
    for l0, l1 in zip(loops, loops[1:]):
        shapes.bridge(bm, l0, l1)
    bm.faces.new(list(reversed(loops[0])))
    bm.faces.new(loops[-1])
    return finish(bm, color, 0.0)


def dome(center, normal, radius, color="gun_iron", squash=0.55, seg=8):
    """A rivet head: a squashed half sphere on a surface, facing `normal`.

    Built vertex by vertex (not with holes_fill, whose cap starts on a varying
    vertex and so triangulates differently from run to run)."""
    bm = bmesh.new()
    pole = bm.verts.new((0.0, 0.0, radius * squash))
    rings = []
    for polar in (40.0, 90.0):
        p = math.radians(polar)
        rings.append([bm.verts.new((radius * math.sin(p) * math.cos(2 * math.pi * k / seg),
                                    radius * math.sin(p) * math.sin(2 * math.pi * k / seg),
                                    radius * math.cos(p) * squash)) for k in range(seg)])
    for k in range(seg):
        bm.faces.new((pole, rings[0][k], rings[0][(k + 1) % seg]))
    shapes.bridge(bm, rings[1], rings[0])
    bm.faces.new(list(reversed(rings[1])))
    rot = Vector((0.0, 0.0, 1.0)).rotation_difference(Vector(normal)).to_matrix().to_4x4()
    return finish(bm, color, matrix=Matrix.Translation(Vector(center)) @ rot)


def ball(center, radius, color, seg=10, rings=6):
    bm = bmesh.new()
    bmesh.ops.create_uvsphere(bm, u_segments=seg, v_segments=rings, radius=radius)
    return finish(bm, color, matrix=Matrix.Translation(Vector(center)))


def two_tone(obj, color, light, every=3):
    """Paints a crystal: most facets `color`, every `every`-th facet `light`, so the
    facets read even when the crystal glows flat."""
    palette.paint_object(obj, color)
    k = [0]

    def pick(_f):
        k[0] += 1
        return k[0] % every == 0

    palette.paint_object(obj, light, pick)
    return obj


def gem_icosa(radius, stretch, color, light, rotation=(0.0, 0.0, 0.0)):
    """A faceted gem: an icosahedron stretched along Y (the chamber axis)."""
    bm = bmesh.new()
    bmesh.ops.create_icosphere(bm, subdivisions=1, radius=radius)
    for v in bm.verts:
        v.co = Vector((v.co.x, v.co.y * stretch, v.co.z))
    return two_tone(finish(bm, None, matrix=place((0, 0, 0), rotation)), color, light)


def shard(length, radius, tip, color, light=None, sides=6, rotation=(0.0, 0.0, 0.0),
          twist=0.0):
    """A double-terminated crystal along Y: a prism with pointed ends.

    `length` is tip to tip, `tip` the length of each pyramid end."""
    bm = bmesh.new()
    half = length / 2.0
    y_ring = half - tip
    rings = []
    for y in (-y_ring, y_ring):
        rings.append([bm.verts.new((radius * math.cos(twist + 2 * math.pi * k / sides), y,
                                    radius * math.sin(twist + 2 * math.pi * k / sides)))
                      for k in range(sides)])
    t0 = bm.verts.new((0.0, -half, 0.0))
    t1 = bm.verts.new((0.0, half, 0.0))
    shapes.bridge(bm, rings[0], rings[1])
    for k in range(sides):
        j = (k + 1) % sides
        bm.faces.new((rings[0][j], rings[0][k], t0))
        bm.faces.new((rings[1][k], rings[1][j], t1))
    obj = finish(bm, None, matrix=place((0, 0, 0), rotation))
    return two_tone(obj, color, light or color, every=2)


def stroke(center, p, q, width, depth, color, normal_axis="X"):
    """A thin raised bar from 2D point p to q (in the plane perpendicular to X),
    centred at depth `center.x`: the glyph strokes of the rune windows."""
    (py, pz), (qy, qz) = p, q
    length = math.hypot(qy - py, qz - pz)
    ang = math.atan2(qz - pz, qy - py)
    mid = Vector((center[0], center[1] + (py + qy) / 2, center[2] + (pz + qz) / 2))
    return rbox(mid, (depth, length + width, width), color, rotation=(ang, 0.0, 0.0))


def triangulate(mesh):
    """Splits every face into triangles and sorts them into a fixed order.

    Blender's n-gon triangulation (the glTF exporter's, and bmesh's) makes the
    same triangles every run but not always in the same order, which breaks
    `build-art.sh --check`. Triangulating here with fixed rules, then sorting the
    triangles by their vertex indices, keeps the export byte-identical."""
    bm = bmesh.new()
    bm.from_mesh(mesh)
    bmesh.ops.triangulate(bm, faces=bm.faces, quad_method="FIXED", ngon_method="EAR_CLIP")

    n = len(bm.verts)

    def key(face):  # the triangle's vertex indices, lowest first, as one number
        idx = [v.index for v in face.verts]
        k = idx.index(min(idx))
        a, b, c = idx[k:] + idx[:k]
        return (a * n + b) * n + c

    bm.verts.index_update()
    bm.faces.sort(key=key)
    bm.faces.index_update()
    bm.to_mesh(mesh)
    bm.free()
    return mesh


def part(name, objects, parent, location=(0.0, 0.0, 0.0), rotation=(0.0, 0.0, 0.0),
         sharp_deg=40.0, flat=False):
    """Joins scratch objects (built in model space) into a named part whose pivot
    is `location` (and whose axes are turned by `rotation`)."""
    obj = shapes.join(objects, name + "Tmp")
    inv = place(location, rotation).inverted()
    obj.data.transform(inv)
    mesh = triangulate(shapes.detach_mesh(obj))
    p = scene.make_part(name, mesh, parent, location)
    p.rotation_euler = Euler(rotation)
    if flat:
        shapes.flat_shading(p)
    else:
        shapes.smooth_shading(p, sharp_angle_deg=sharp_deg)
    return p


def settle_on_ground(root):
    """Lifts every child of root so the model's lowest vertex sits on z = 0."""
    scene.update()
    low = min((o.matrix_world @ v.co).z for o in scene.descendants(root)
              if o.type == "MESH" for v in o.data.vertices)
    for child in root.children:
        child.location.z -= low
    scene.update()
    return -low


# ---------------------------------------------------------------------------
# Shared gun furniture
# ---------------------------------------------------------------------------

# The pistol grip (both guns): top of the grip axis, rake from vertical.
GRIP_TOP = Vector((0.0, 0.062, -0.040))
GRIP_RAKE = math.radians(18.0)
GRIP_DOWN = Vector((0.0, math.sin(GRIP_RAKE), -math.cos(GRIP_RAKE)))
GRIP_BACK = Vector((0.0, math.cos(GRIP_RAKE), math.sin(GRIP_RAKE)))
# Where the right glove's centre sits along the grip axis (from the top).
GRIP_R_DEPTH = 0.085
GRIP_R_ROTATION = (GRIP_RAKE, 0.0, 0.0)


def grip_r_location():
    return GRIP_TOP + GRIP_DOWN * GRIP_R_DEPTH


# The left glove wraps a rounded box this big (half extents x, z) around GripL:
# the rifle's forend and the pump's PumpGrip are shaped to match.
GRIP_L_HALF = (0.040, 0.036)


def pistol_grip(objs):
    """Wooden raked pistol grip with a brass cap, iron trigger guard, brass trigger."""
    def st(s, hu, hv, n=2.8):
        return (GRIP_TOP + GRIP_DOWN * s, X, GRIP_BACK, hu, hv, n)

    objs.append(loft([st(-0.014, 0.019, 0.028), st(0.0, 0.019, 0.028),
                      st(0.07, 0.020, 0.029), st(0.140, 0.022, 0.032)],
                     "stock", seg=20))
    objs.append(loft([st(0.134, 0.0235, 0.0335), st(0.156, 0.0235, 0.0335)],
                     "brass", seg=20, bevel=0.005, bevel_segments=1))
    # Trigger guard: a bar under the trigger and a post up to the receiver.
    objs.append(rbox((0.0, -0.006, -0.072), (0.014, 0.076, 0.010), "gun_iron", bevel=0.003))
    objs.append(rbox((0.0, -0.040, -0.058), (0.013, 0.012, 0.030), "gun_iron", bevel=0.003))
    # Trigger.
    objs.append(rbox((0.0, -0.008, -0.056), (0.008, 0.010, 0.022), "brass", bevel=0.003,
                     rotation=(math.radians(-14.0), 0.0, 0.0)))


def stepped_collar(objs, y0, y1, zc, hw, hh):
    """A chunky brass collar: a band with a raised ridge round its middle."""
    objs.append(band_y(y0, y1, zc, hw, hh, "brass", bevel=0.006))
    mid, half = (y0 + y1) / 2, (y1 - y0) * 0.2
    objs.append(band_y(mid - half, mid + half, zc, hw + 0.004, hh + 0.004, "brass",
                       bevel=0.003, bevel_segments=1))


def rear_sight(objs, y, base_z, sight_z):
    """A brass ear on a collar, split into two lobes: the rear sight's U-notch.
    The notch bottom sits just under the sight line."""
    objs.append(rbox((0.0, y, (base_z + sight_z - 0.009) / 2), (0.040, 0.026,
                     sight_z - 0.009 - base_z), "brass", bevel=0.004))
    for s in (1.0, -1.0):
        objs.append(rbox((s * 0.0145, y, sight_z - 0.002), (0.011, 0.024, 0.022), "brass",
                         bevel=0.004, segments=2))


def rivets(objs, points, radius=0.0055):
    """Iron rivet heads at (point, normal) pairs."""
    for p, n in points:
        objs.append(dome(p, n, radius))




# ---------------------------------------------------------------------------
# Rifle
# ---------------------------------------------------------------------------

R_AXIS = 0.062           # chamber / barrel axis height
R_SIGHT = 0.185          # sight line height (high, so ADS sees over the gun)
R_CHAMBER_Y = -0.005     # chamber and crystal centre along the gun
R_REAR_SIGHT_Y = 0.120
R_FRONT_SIGHT_Y = -0.392
R_MUZZLE_Y = -0.548
R_MUZZLE_LEN = 0.070
R_MAG_SEAT = Vector((0.0, -0.092, -0.046))
R_MAG_TILT = math.radians(-8.0)   # the magazine's foot leans forward
# The left glove on the forend, in front of the magazine, behind the rune window.
R_GRIP_L = Vector((0.0, -0.200, 0.031))


def rifle_body(objs):
    ax = R_AXIS
    # Brass collars at both ends of the glass chamber, each with a top ear.
    for y0, y1 in ((0.098, 0.142), (-0.150, -0.106)):
        stepped_collar(objs, y0, y1, ax, 0.053, 0.071)
        rivets(objs, [((s * 0.0575, (y0 + y1) / 2, ax), (s, 0.0, 0.0)) for s in (1.0, -1.0)],
               0.0068)
    for y in (-0.105, 0.097):
        objs.append(cyl((0.0, y - 0.004, ax), (0.0, y + 0.004, ax), 0.052, "gun_iron", seg=20))
    # Front ear, and the rear ear split into a chunky U-notch: the rear sight.
    objs.append(rbox((0.0, -0.128, ax + 0.076), (0.034, 0.030, 0.026), "brass", bevel=0.008,
                     segments=2))
    rear_sight(objs, R_REAR_SIGHT_Y, ax + 0.062, R_SIGHT)

    # Lower receiver (iron) with brass side plates and rivets.
    objs.append(rbox((0.0, -0.030, -0.018), (0.068, 0.270, 0.060), "gun_iron", bevel=0.009,
                     segments=2))
    for s in (1.0, -1.0):
        objs.append(rbox((s * 0.0345, -0.035, -0.018), (0.006, 0.190, 0.034), "brass",
                         bevel=0.002))
        rivets(objs, [((s * 0.0376, y, -0.018), (s, 0.0, 0.0)) for y in (-0.115, 0.045)])
    pistol_grip(objs)

    # Wooden forend in front of the chamber.
    objs.append(loft_y([(-0.150, ax - 0.006, 0.041, 0.063, 3.2),
                        (-0.290, ax - 0.004, 0.040, 0.061, 3.2),
                        (-0.395, ax - 0.002, 0.039, 0.058, 3.2)],
                       "stock", seg=24))
    objs.append(rbox((0.0, -0.262, ax + 0.060), (0.050, 0.220, 0.012), "brass", bevel=0.004,
                     segments=2))
    # Front band (brass) with rivets, carrying the front sight.
    objs.append(band_y(-0.415, -0.370, ax - 0.002, 0.046, 0.066, "brass", bevel=0.006))
    rivets(objs, [((s * 0.0465, -0.392, ax + dz), (s, 0.0, 0.0))
                  for s in (1.0, -1.0) for dz in (0.026, -0.030)])
    fy = R_FRONT_SIGHT_Y
    post_top = R_SIGHT - 0.020
    objs.append(rbox((0.0, fy, (0.124 + post_top) / 2), (0.013, 0.016, post_top - 0.124),
                     "gun_iron", bevel=0.003))
    objs.append(arc_band((0.0, fy, R_SIGHT), (0.0, -1.0, 0.0), 0.0135, 0.0235, 0.015,
                         math.radians(125.0), math.radians(415.0), "gun_iron", seg=14))

    # Rune windows on both sides of the forend: iron frame, glowing panel, glyph.
    wy, wz = -0.318, ax + 0.008
    for s in (1.0, -1.0):
        objs.append(rbox((s * 0.041, wy, wz), (0.010, 0.090, 0.080), "gun_iron", bevel=0.004))
        objs.append(rbox((s * 0.0448, wy, wz), (0.004, 0.072, 0.062), "crystal_blue",
                         bevel=0.0015))
        gx = s * 0.0472
        dy, dz = 0.024, 0.022
        for p, q in (((-dy, 0.0), (0.0, dz)), ((0.0, dz), (dy, 0.0)),
                     ((dy, 0.0), (0.0, -dz)), ((0.0, -dz), (-dy, 0.0)),
                     ((0.0, -0.027), (0.0, 0.027))):
            objs.append(stroke((gx, wy, wz), p, q, 0.0045, 0.003, "barrier_cyan"))

    # Thick iron barrel with a brass band.
    objs.append(cyl((0.0, -0.395, ax), (0.0, R_MUZZLE_Y - 0.004, ax), 0.031, "gun_iron",
                    seg=20))
    objs.append(cyl((0.0, -0.470, ax), (0.0, -0.498, ax), 0.037, "brass", seg=20,
                    bevel=0.005))


def rifle_stock():
    ax = R_AXIS
    objs = [loft_y([(0.118, ax - 0.002, 0.041, 0.056, 3.4),
                    (0.200, ax - 0.014, 0.041, 0.066, 3.4),
                    (0.330, ax - 0.032, 0.040, 0.082, 3.4),
                    (0.448, ax - 0.044, 0.041, 0.090, 3.4)],
                   "stock", seg=22, bevel=0.012)]
    objs.append(band_y(0.446, 0.468, ax - 0.045, 0.042, 0.091, "gun_iron", bevel=0.006))
    return objs


def rifle_mag():
    """The energy cell, built at its seat and leaning forward."""
    objs = [rbox((0.0, 0.0, -0.078), (0.050, 0.068, 0.148), "gun_iron", bevel=0.008, segments=2),
            rbox((0.0, 0.0, -0.010), (0.056, 0.074, 0.018), "brass", bevel=0.004),
            rbox((0.0, 0.0, -0.154), (0.058, 0.078, 0.020), "brass", bevel=0.006, segments=2)]
    for s in (1.0, -1.0):
        for y in (-0.019, 0.0, 0.019):
            objs.append(rbox((s * 0.0255, y, -0.080), (0.004, 0.009, 0.098), "crystal_blue"))
    m = Matrix.Translation(R_MAG_SEAT) @ Matrix.Rotation(R_MAG_TILT, 4, "X")
    for o in objs:
        o.data.transform(m)
    return objs


def rifle_muzzle():
    """A chunky brass muzzle with a dark bore and a glowing rune ring."""
    base = Vector((0.0, R_MUZZLE_Y, R_AXIS))
    fwd = (0.0, -1.0, 0.0)
    L = R_MUZZLE_LEN
    profile = [(0.0, 0.020), (0.0, 0.035), (0.008, 0.043), (L - 0.013, 0.043),
               (L - 0.005, 0.040), (L, 0.031), (L, 0.020)]
    colors = ["brass"] * 6 + ["gun_iron"]
    return [lathe(base, fwd, profile, colors, seg=24),
            hoop(base + Vector((0.0, -0.026, 0.0)), fwd, 0.0445, 0.009, 0.004,
                 "crystal_blue", seg=20)]


def build_rifle(root):
    body = []
    rifle_body(body)
    part("Body", body, root)
    part("Stock", rifle_stock(), root, location=(0.0, 0.118, R_AXIS))
    chamber = cyl((0.0, R_CHAMBER_Y - 0.110, R_AXIS), (0.0, R_CHAMBER_Y + 0.108, R_AXIS),
                  0.055, "glass_cyan", seg=24, cap=False)
    socket = (0.0, R_CHAMBER_Y, R_AXIS)
    part("Chamber", [chamber], root, location=socket)
    crystal = gem_icosa(0.036, 1.22, "crystal_blue", "barrier_cyan",
                        rotation=(0.35, 0.25, 0.55))
    crystal.data.transform(Matrix.Translation(Vector(socket)))
    part("Crystal", [crystal], root, location=socket, flat=True)
    part("Mag", rifle_mag(), root, location=R_MAG_SEAT)
    part("Muzzle", rifle_muzzle(), root, location=(0.0, R_MUZZLE_Y, R_AXIS))

    scene.make_attach("MuzzleTip", root, (0.0, R_MUZZLE_Y - R_MUZZLE_LEN, R_AXIS))
    scene.make_attach("CrystalSocket", root, socket)
    scene.make_attach("GripR", root, grip_r_location(), GRIP_R_ROTATION)
    scene.make_attach("GripL", root, R_GRIP_L)
    scene.make_attach("Sight", root, (0.0, R_REAR_SIGHT_Y, R_SIGHT))
    scene.make_attach("SightFront", root, (0.0, R_FRONT_SIGHT_Y, R_SIGHT))
    settle_on_ground(root)


# ---------------------------------------------------------------------------
# Pump
# ---------------------------------------------------------------------------

P_AXIS = 0.066           # crystal and barrel axis height
P_SIGHT = 0.178          # sight line height
P_CRYSTAL_Y = -0.012     # crystal centre (between the collars)
P_REAR_COLLAR = (0.068, 0.110)
P_FRONT_COLLAR = (-0.132, -0.092)
P_REAR_SIGHT_Y = 0.089
P_FRONT_SIGHT_Y = -0.470
P_MUZZLE_Y = -0.500
P_BELL_LEN = 0.120
P_PUMP = Vector((0.0, -0.330, 0.035))   # the pump grip's rest centre
P_PUMP_HALF = (0.040, 0.052)
P_PUMP_TRAVEL = 0.09                      # how far the rack pulls the grip back
P_TUBE_Z = 0.010                          # magazine tube under the barrel
# Rings: three hoops round the crystal, spaced along it, each tipped from the gun
# axis toward a different side (so their spin reads), band width and thickness.
P_RING_RADIUS = 0.054
P_RING_SPACING = 0.050   # rings sit along the crystal at -1, 0, +1 spacings
P_RING_WIDTH = 0.010
P_RING_THICK = 0.005
P_RING_TILT = math.radians(22.0)


def pump_body(objs):
    ax = P_AXIS
    # Brass collars framing the open crystal cradle.
    for y0, y1 in (P_REAR_COLLAR, P_FRONT_COLLAR):
        stepped_collar(objs, y0, y1, ax, 0.052, 0.070)
        rivets(objs, [((s * 0.0565, (y0 + y1) / 2, ax), (s, 0.0, 0.0)) for s in (1.0, -1.0)],
               0.0065)
    # Rear ear with the iron U-notch sight.
    rear_sight(objs, P_REAR_SIGHT_Y, ax + 0.060, P_SIGHT)

    # The spine under the cradle (iron) with brass plates, a brass saddle and rivets.
    objs.append(rbox((0.0, -0.012, -0.027), (0.066, 0.270, 0.046), "gun_iron", bevel=0.009,
                     segments=2))
    objs.append(rbox((0.0, P_CRYSTAL_Y, -0.004), (0.046, 0.200, 0.010), "brass", bevel=0.003))
    for s in (1.0, -1.0):
        objs.append(rbox((s * 0.0335, -0.020, -0.026), (0.006, 0.200, 0.028), "brass",
                         bevel=0.002))
        rivets(objs, [((s * 0.0366, y, -0.026), (s, 0.0, 0.0)) for y in (-0.105, 0.065)])
    pistol_grip(objs)

    # Barrel (iron) and the magazine tube under it.
    objs.append(cyl((0.0, -0.120, ax), (0.0, P_MUZZLE_Y - 0.004, ax), 0.033, "gun_iron",
                    seg=20))
    objs.append(cyl((0.0, -0.120, P_TUBE_Z), (0.0, -0.462, P_TUBE_Z), 0.022, "gun_iron",
                    seg=16))
    # Front band holding barrel and tube, with rivets; the front sight on top.
    fy = P_FRONT_SIGHT_Y
    objs.append(loft_y([(fy - 0.017, 0.041, 0.041, 0.062, 3.2),
                        (fy + 0.017, 0.041, 0.041, 0.062, 3.2)], "brass", seg=20, bevel=0.006))
    rivets(objs, [((s * 0.0415, fy, 0.040), (s, 0.0, 0.0)) for s in (1.0, -1.0)])
    objs.append(rbox((0.0, fy, (0.100 + P_SIGHT) / 2), (0.012, 0.016, P_SIGHT - 0.100),
                     "gun_iron", bevel=0.003))
    objs.append(ball((0.0, fy, P_SIGHT), 0.0095, "brass", seg=10, rings=6))


def pump_stock():
    ax = P_AXIS
    objs = [loft_y([(0.100, ax - 0.002, 0.042, 0.060, 3.4),
                    (0.170, ax - 0.014, 0.042, 0.070, 3.4),
                    (0.280, ax - 0.030, 0.042, 0.082, 3.4),
                    (0.335, ax - 0.036, 0.043, 0.086, 3.4)],
                   "stock", seg=22, bevel=0.012)]
    objs.append(band_y(0.333, 0.355, ax - 0.037, 0.044, 0.087, "gun_iron", bevel=0.006))
    return objs


def pump_grip():
    """The pump grip: chunky wood cut into five ribs by four grooves, brass end rims.
    Centred on P_PUMP."""
    c = P_PUMP
    hw, hh = P_PUMP_HALF
    length, grooves, groove_w, depth = 0.190, 4, 0.009, 0.0045
    rib = (length - grooves * groove_w) / (grooves + 1)
    y = c.y + length / 2
    stations = [(y, c.z, hw, hh, 3.0)]
    for _ in range(grooves):
        y -= rib
        stations.append((y, c.z, hw, hh, 3.0))
        for f in (0.3, 0.7):
            stations.append((y - groove_w * f, c.z, hw - depth, hh - depth, 3.0))
        y -= groove_w
        stations.append((y, c.z, hw, hh, 3.0))
    stations.append((c.y - length / 2, c.z, hw, hh, 3.0))
    objs = [loft_y(stations, "stock", seg=18)]  # the brass rims cover its ends
    for dy in (-0.091, 0.091):
        objs.append(band_y(c.y + dy - 0.006, c.y + dy + 0.006, c.z, hw + 0.003, hh + 0.003,
                           "brass", n=3.0, bevel=0.003, bevel_segments=1))
    return objs


def pump_bell():
    """The flared brass bell: outer flare, rolled lip, brass throat, dark bore, violet glow."""
    base = Vector((0.0, P_MUZZLE_Y, P_AXIS))
    fwd = (0.0, -1.0, 0.0)
    profile = [(0.000, 0.022), (0.000, 0.035), (0.030, 0.039), (0.060, 0.048),
               (0.086, 0.064), (0.104, 0.080), (0.114, 0.090), (0.120, 0.084),
               (0.114, 0.075), (0.090, 0.058), (0.060, 0.038), (0.034, 0.024)]
    colors = ["brass"] * 11 + ["gun_iron"]
    return [lathe(base, fwd, profile, colors, seg=26),
            hoop(base + Vector((0.0, -0.036, 0.0)), fwd, 0.0245, 0.006, 0.005,
                 "crystal_violet", seg=16)]


def pump_rings(center):
    """Three gold hoops round the crystal, spaced along it, each tipped from the gun
    axis toward a different side (120 degrees apart), so they wobble as they spin."""
    objs = []
    for k in range(3):
        az = math.radians(90.0 + 120.0 * k)
        n = Vector((math.sin(P_RING_TILT) * math.cos(az), math.cos(P_RING_TILT),
                    math.sin(P_RING_TILT) * math.sin(az)))
        c = Vector(center) + Vector((0.0, (k - 1) * P_RING_SPACING, 0.0))
        objs.append(hoop(c, n, P_RING_RADIUS, P_RING_WIDTH, P_RING_THICK, "gold_rings", seg=30))
    return objs


def build_pump(root):
    body = []
    pump_body(body)
    part("Body", body, root)
    part("Stock", pump_stock(), root, location=(0.0, 0.100, P_AXIS))
    socket = Vector((0.0, P_CRYSTAL_Y, P_AXIS))
    crystal = shard(0.150, 0.026, 0.032, "crystal_violet", "glass_violet", sides=6,
                    rotation=(0.0, math.radians(30.0), 0.0))
    crystal.data.transform(Matrix.Translation(socket))
    part("Crystal", [crystal], root, location=socket, flat=True)
    part("Rings", pump_rings(socket), root, location=socket, sharp_deg=50.0)
    shell = shard(0.038, 0.0105, 0.012, "crystal_violet", "glass_violet", sides=6)
    shell.data.transform(Matrix.Translation(socket))
    part("Shard", [shell], root, location=socket, flat=True)
    grip = part("PumpGrip", pump_grip(), root, location=P_PUMP)
    part("Muzzle", pump_bell(), root, location=(0.0, P_MUZZLE_Y, P_AXIS), sharp_deg=60.0)

    scene.make_attach("MuzzleTip", root, (0.0, P_MUZZLE_Y - P_BELL_LEN, P_AXIS))
    scene.make_attach("CrystalSocket", root, socket)
    scene.make_attach("GripR", root, grip_r_location(), GRIP_R_ROTATION)
    # GripL rides on the pump grip: its bottom plus the glove's half height.
    grip_l_local = (0.0, 0.0, -P_PUMP_HALF[1] + GRIP_L_HALF[1])
    scene.make_attach("GripL", grip, grip_l_local)
    scene.make_attach("Sight", root, (0.0, P_REAR_SIGHT_Y, P_SIGHT))
    scene.make_attach("SightFront", root, (0.0, P_FRONT_SIGHT_Y, P_SIGHT))
    settle_on_ground(root)


ASSETS = [
    Asset("rifle", "gun", build_rifle, "brass-and-crystal assault rifle viewmodel"),
    Asset("pump", "gun", build_pump, "stubby bell-mouthed pump with a ringed violet crystal"),
]
