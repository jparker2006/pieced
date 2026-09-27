"""Guns: the rifle and the pump, first-person viewmodels (targets T02, T04; also T01, T03, T05, R4).

Toy-like Fortnite proportions, ornate and round as the targets paint them
(S1 round 3: more sides, near-elliptical collars and stocks, smooth-shaded
with the ink kept): polished gold brass with bands, rivets, rounded collars and
scrollwork ridges, warm red-brown wood with painted grain, dark iron, and a
glass chamber holding a big glowing faceted crystal (the game adds the
crackling energy inside, `fx::chamber`).

* `rifle` (T01-T03): a SCAR-style assault rifle. Rounded brass collars at both
  ends of a blue glass chamber with a deep blue interior, holding a faceted
  crystal (deep blue facets, bright cyan edges); a round, grained red-brown
  stock and forend; brass side plates with scrollwork; glowing rune windows on
  both sides of the forend; an energy-cell magazine with glowing slots; a round
  brass barrel ending in a dark iron muzzle cap; a ring front sight and a
  compact U-notch ear on the rear collar (the rear sight).
* `pump` (T04): a stubby shotgun. A long, smooth brass trumpet bell (a horn
  from the side, not a disc), a short iron barrel cradled by a chunky ribbed
  wooden pump grip that slides, and a violet crystal in a violet glass chamber
  between rounded brass collars, wrapped by two gem-studded gold rings (they
  spin on the rack).

Named parts (Rust finds them by name and animates them):

    rifle: Body, Stock, Chamber, ChamberBack, Crystal, Mag, Muzzle, Runes
    pump:  Body, Stock, Chamber, ChamberBack, Crystal, Rings, PumpGrip, Shard,
           Muzzle, Runes
           (Chamber: the glass tube, see-through in game. ChamberBack: an
           opaque tube just inside it whose faces look inward, so only its far
           wall draws: the deep glowing interior behind the crystal. Runes:
           every inlay that glows with the crystal (rune windows, magazine
           slots, rune rings, collar gems). Shard: a small violet crystal
           shard, the pump's "shell"; it rests inside the crystal and is hidden
           in game until a reload pushes it in)

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

Colour. Every part is a closed mesh coloured by palette name
(`art/palette.json`). Two passes paint on top of the base colours: brass faces
turned up to the sky take the polished highlight (`gun_brass_light`), and wood
takes streaks of lighter and darker grain along its length (`grain`). Both are
plain per-face palette colours, so they cost no triangles or textures.
"""

import math

import bmesh
from mathutils import Euler, Matrix, Vector

from lib import palette, scene, shapes
from lib.registry import Asset

# Palette names used here.
BRASS = "gun_brass"
BRASS_LIGHT = "gun_brass_light"
BRASS_DARK = "gun_brass_dark"
IRON = "gun_iron"
WOOD = "stock"
WOOD_LIGHT = "gun_wood_light"
WOOD_DARK = "gun_wood_dark"

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
Y = (0.0, 1.0, 0.0)
Z = (0.0, 0.0, 1.0)


def loft_y(stations, color, seg=24, bevel=0.0, bevel_segments=2, x=0.0):
    """A loft along the gun (Y) of rounded rectangles [(y, zc, half_w, half_h, n)]."""
    return loft([((x, y, zc), X, Z, hw, hh, n) for y, zc, hw, hh, n in stations],
                color, seg, bevel, bevel_segments)


def loft_tagged(stations, colors, seg=16, cap=True):
    """`loft_y` whose span i (station i to i + 1) takes palette colour
    colors[i] (the caps take the first and last): beaded bands in one mesh."""
    bm = palette.new_bmesh()
    rings = [ring_verts(bm, Vector((0.0, y, zc)), Vector(X), Vector(Z), hw, hh, n, seg)
             for y, zc, hw, hh, n in stations]
    for i, (a, b) in enumerate(zip(rings, rings[1:])):
        palette.tag(bm, shapes.bridge(bm, a, b), colors[i])
    if cap:
        palette.tag(bm, [bm.faces.new(list(reversed(rings[0])))], colors[0])
        palette.tag(bm, [bm.faces.new(rings[-1])], colors[-1])
    return finish(bm, None)


def band_y(y0, y1, zc, hw, hh, color, n=3.4, seg=20, bevel=0.0, bevel_segments=1):
    """A short rounded-rectangle band around the gun (collars, straps)."""
    return loft_y([(y0, zc, hw, hh, n), (y1, zc, hw, hh, n)], color, seg, bevel,
                  bevel_segments)


def cyl(p0, p1, r, color, seg=20, bevel=0.0, bevel_segments=2, cap=True):
    """A cylinder from p0 to p1 (open-ended with cap=False)."""
    p0, p1 = Vector(p0), Vector(p1)
    u, v = axis_frame(p1 - p0)
    return loft([(p0, u, v, r, r, 2.0), (p1, u, v, r, r, 2.0)], color, seg, bevel,
                bevel_segments, cap)


def lathe(origin, axis, profile, colors, seg=24, flutes=0, flute_depth=0.0, flute_from=None):
    """A surface of revolution: `profile` is a closed polygon [(t, r)] (t along
    `axis` from `origin`, r > 0 the radius); edge i (point i to i+1) takes
    colors[i] (a palette name, or one name for all).

    With `flutes` > 0, profile points at t >= `flute_from` are scalloped into
    that many rounded flutes `flute_depth` deep (a fluted, flared muzzle)."""
    origin, a = Vector(origin), Vector(axis).normalized()
    u, v = axis_frame(a)
    bm = palette.new_bmesh()
    rings = []
    for t, r in profile:
        ring = []
        for k in range(seg):
            ang = k * 2 * math.pi / seg
            rr = r
            if flutes and flute_from is not None and t >= flute_from:
                rr = r * (1.0 - flute_depth * (0.5 - 0.5 * math.cos(flutes * ang)))
            ring.append(bm.verts.new(origin + a * t + (u * math.cos(ang) + v * math.sin(ang)) * rr))
        rings.append(ring)
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


def bead_ring(center, normal, radius, thickness, color, seg=24):
    """A rounded bead (a torus with a diamond section): the beading on collars."""
    t = thickness / 2.0
    profile = [(-t, radius), (0.0, radius + t * 0.9), (t, radius), (0.0, radius - t * 0.6)]
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


def dome(center, normal, radius, color=IRON, squash=0.55, seg=8):
    """A rivet head: a low dome on a surface, facing `normal` (a pole over one
    ring: 14 triangles, smooth-shaded into a dome at rivet size).

    Built vertex by vertex (not with holes_fill, whose cap starts on a varying
    vertex and so triangulates differently from run to run)."""
    bm = bmesh.new()
    pole = bm.verts.new((0.0, 0.0, radius * squash))
    ring = [bm.verts.new((radius * math.cos(2 * math.pi * k / seg),
                          radius * math.sin(2 * math.pi * k / seg), 0.0)) for k in range(seg)]
    for k in range(seg):
        bm.faces.new((pole, ring[k], ring[(k + 1) % seg]))
    bm.faces.new(list(reversed(ring)))
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


def gem(radius, stretch, color, edge, rotation=(0.0, 0.0, 0.0), inset=0.18):
    """A faceted gem with bright facet edges (T02's and T04's crystals): an
    icosahedron stretched along Y, each face inset so a rim of `edge` colour
    outlines it round a `color` centre."""
    bm = palette.new_bmesh()
    bmesh.ops.create_icosphere(bm, subdivisions=0, radius=radius)
    for v in bm.verts:
        v.co = Vector((v.co.x, v.co.y * stretch, v.co.z))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    faces = list(bm.faces)
    palette.tag(bm, faces, edge)
    res = bmesh.ops.inset_individual(bm, faces=faces, thickness=radius * inset, depth=0.0,
                                     use_even_offset=True)
    palette.tag(bm, faces, color)  # the inner faces keep the original faces
    del res
    return finish(bm, None, matrix=place((0, 0, 0), rotation))


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


def stroke(center, p, q, width, depth, color):
    """A thin raised bar from 2D point p to q (in the plane perpendicular to X),
    centred at depth `center.x`: glyph strokes and scrollwork."""
    (py, pz), (qy, qz) = p, q
    length = math.hypot(qy - py, qz - pz)
    ang = math.atan2(qz - pz, qy - py)
    mid = Vector((center[0], center[1] + (py + qy) / 2, center[2] + (pz + qz) / 2))
    return rbox(mid, (depth, length + width, width), color, rotation=(ang, 0.0, 0.0))


def ribbon_x(points, x, width, height, color):
    """A raised strip on a surface facing +-X (sign of `height`): the centre line
    `points` [(y, z)] swept with `width`, standing `height` proud of x. A cheap
    prism per segment pair (one closed tube), for scrollwork ridges."""
    bm = bmesh.new()
    pts = [Vector(p) for p in points]
    rings = []
    for i, p in enumerate(pts):
        a = pts[max(i - 1, 0)]
        b = pts[min(i + 1, len(pts) - 1)]
        t = (b - a).normalized()
        n = Vector((-t.y, t.x)) * (width / 2.0)
        rings.append([bm.verts.new((x, p.x - n.x, p.y - n.y)),
                      bm.verts.new((x, p.x + n.x, p.y + n.y)),
                      bm.verts.new((x + height, p.x + n.x * 0.55, p.y + n.y * 0.55)),
                      bm.verts.new((x + height, p.x - n.x * 0.55, p.y - n.y * 0.55))])
    for r0, r1 in zip(rings, rings[1:]):
        shapes.bridge(bm, r0, r1)
    bm.faces.new(list(reversed(rings[0])))
    bm.faces.new(rings[-1])
    return finish(bm, color)


def scroll(center, x, size, height, color, turns=1.25, steps=10, mirror=False, a0=0.0):
    """A spiral scroll ridge (filigree) on a side plate facing +-X: from the
    outside in, `size` across, standing `height` proud of `x`."""
    cy, cz = center
    pts = []
    for k in range(steps + 1):
        t = k / steps
        ang = a0 + turns * 2.0 * math.pi * t
        r = size * 0.5 * (1.0 - 0.72 * t)
        y = r * math.cos(ang)
        pts.append((cy + (-y if mirror else y), cz + r * math.sin(ang)))
    return ribbon_x(pts, x, size * 0.12, height, color)


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


# ---------------------------------------------------------------------------
# Paint passes: polished brass and wood grain
# ---------------------------------------------------------------------------

# A brass face turned up this far toward the sky takes the polished highlight.
POLISH_UP = 0.62


def polish(obj, up=Z, threshold=POLISH_UP):
    """Brass faces facing `up` take the polished highlight, as the targets paint
    a light band along the top of every collar and band."""
    pal = palette.palette()
    brass, light = pal.idx(BRASS) + 1, pal.idx(BRASS_LIGHT) + 1
    up = Vector(up).normalized()
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bm.normal_update()
    layer = palette.face_layer(bm)
    for f in bm.faces:
        if f[layer] == brass and f.normal.dot(up) > threshold:
            f[layer] = light
    bm.to_mesh(obj.data)
    bm.free()
    return obj


def grain(obj, axis=Y, seed=1, across=55.0, along=3.5, dark=0.26, light=0.3):
    """Wood grain: every wood face takes a streak colour from noise stretched
    along `axis` (the grain), so long faces make long light and dark streaks."""
    pal = palette.palette()
    wood = pal.idx(WOOD) + 1
    tags = (pal.idx(WOOD_DARK) + 1, pal.idx(WOOD_LIGHT) + 1)
    a = Vector(axis).normalized()
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    layer = palette.face_layer(bm)
    for f in bm.faces:
        if f[layer] != wood:
            continue
        c = f.calc_center_median()
        s = c.dot(a)
        off = c - a * s
        v = shapes.perlin(off * across + a * (s * along), seed)
        if v > dark:
            f[layer] = tags[0]
        elif v < -light:
            f[layer] = tags[1]
    bm.to_mesh(obj.data)
    bm.free()
    return obj


def part(name, objects, parent, location=(0.0, 0.0, 0.0), rotation=(0.0, 0.0, 0.0),
         sharp_deg=40.0, flat=False, shine=True):
    """Joins scratch objects (built in model space) into a named part whose pivot
    is `location` (and whose axes are turned by `rotation`). Brass that faces the
    sky is polished (`shine`)."""
    obj = shapes.join(objects, name + "Tmp")
    if shine:
        polish(obj)
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

# Collars, bands and stocks are rounded rectangles this close to an ellipse
# (superellipse exponent: 2 is an ellipse), with this many sides, so they read
# round and smooth-shaded as the targets paint them, not boxy.
COLLAR_N = 2.5
COLLAR_SEG = 24
ROUND_SEG = 24


def se_side(hw, hh, n, f):
    """The point on the +x side of a superellipse (half extents hw, hh, exponent
    n) at height fraction f (-1..1), and its outward normal: (x, z, (nx, nz))."""
    z = f * hh
    x = hw * max(0.0, 1.0 - abs(f) ** n) ** (1.0 / n)
    gx = (x / hw) ** (n - 1) / hw
    gz = math.copysign((abs(z) / hh) ** (n - 1) / hh, z) if z else 0.0
    length = math.hypot(gx, gz)
    return x, z, (gx / length, gz / length)


def pistol_grip(objs, seed):
    """Grained wooden raked pistol grip with a brass cap, iron trigger guard,
    brass trigger."""
    def st(s, hu, hv, n=2.6):
        return (GRIP_TOP + GRIP_DOWN * s, X, GRIP_BACK, hu, hv, n)

    objs.append(grain(loft([st(-0.014, 0.019, 0.028), st(0.0, 0.019, 0.028),
                            st(0.05, 0.0197, 0.0287), st(0.10, 0.021, 0.0305),
                            st(0.140, 0.022, 0.032)],
                           WOOD, seg=16), axis=GRIP_DOWN, seed=seed, across=70.0, along=6.0))
    objs.append(loft([st(0.134, 0.0235, 0.0335), st(0.150, 0.0245, 0.0345),
                      st(0.158, 0.021, 0.030)], BRASS, seg=16))
    # Trigger guard: a bar under the trigger and a post up to the receiver.
    objs.append(rbox((0.0, -0.006, -0.072), (0.014, 0.076, 0.010), IRON, bevel=0.003))
    objs.append(rbox((0.0, -0.040, -0.058), (0.013, 0.012, 0.030), IRON, bevel=0.003))
    # Trigger.
    objs.append(rbox((0.0, -0.008, -0.056), (0.008, 0.010, 0.022), BRASS, bevel=0.003,
                     rotation=(math.radians(-14.0), 0.0, 0.0)))


def beaded_collar(objs, y0, y1, zc, hw, hh, rivet_r=0.0062, rivet_rows=(0.5, -0.5),
                  n=COLLAR_N, seg=COLLAR_SEG):
    """A chunky, rounded brass collar in one loft: dark chamfered edges, a raised
    ridge round its middle, and a column of iron rivets seated on the ridge on
    each side."""
    m = (y0 + y1) / 2
    k = min(1.0, (y1 - y0) / 0.044)   # narrow bands get narrow details
    profile = [(y0, -0.003), (y0 + 0.004 * k, 0.0), (m - 0.009 * k, 0.0),
               (m - 0.0065 * k, 0.005), (m + 0.0065 * k, 0.005), (m + 0.009 * k, 0.0),
               (y1 - 0.004 * k, 0.0), (y1, -0.003)]
    colors = [BRASS_DARK] + [BRASS] * 5 + [BRASS_DARK]
    objs.append(loft_tagged([(y, zc, hw + d, hh + d, n) for y, d in profile], colors, seg))
    points = []
    for s in (1.0, -1.0):
        for f in rivet_rows:
            x, z, (nx, nz) = se_side(hw + 0.005, hh + 0.005, n, f)
            points.append(((s * (x - 0.0012), m, zc + z), (s * nx, 0.0, nz)))
    rivets(objs, points, rivet_r)


def collar_gems(objs, ys, zc, hw, hh, color, f=0.0, n=COLLAR_N):
    """Small glowing gems set in each collar's sides, between the rivets."""
    x, z, (nx, nz) = se_side(hw + 0.005, hh + 0.005, n, f)
    for y in ys:
        for s in (1.0, -1.0):
            objs.append(dome((s * (x - 0.001), y, zc + z), (s * nx, 0.0, nz), 0.0058, color,
                             squash=0.8, seg=6))


def rear_sight(objs, y, base_z, sight_z):
    """A compact, rounded brass ear on the rear collar, split into two lobes: the
    rear sight's U-notch. The notch bottom sits just under the sight line."""
    h = sight_z - 0.006 - base_z
    objs.append(rbox((0.0, y, base_z + h / 2), (0.040, 0.026, h), BRASS, bevel=0.008,
                     segments=2))
    for s in (1.0, -1.0):
        objs.append(rbox((s * 0.0125, y, sight_z + 0.001), (0.012, 0.022, 0.012), BRASS,
                         bevel=0.004, segments=2))


def rivets(objs, points, radius=0.0055):
    """Iron rivet heads at (point, normal) pairs."""
    for p, n in points:
        objs.append(dome(p, n, radius))


def side_plates(objs, yc, zc, length, height, x, rivet_y):
    """Brass side plates on the receiver, each framed by a dark bead line with a
    pair of scrollwork ridges and a rivet at each end (the filigree)."""
    for s in (1.0, -1.0):
        objs.append(rbox((s * x, yc, zc), (0.006, length, height), BRASS, bevel=0.0018))
        xs = s * (x + 0.003)
        h, l = height / 2 - 0.004, length / 2 - 0.006
        for p, q in (((-l, h), (l, h)), ((-l, -h), (l, -h))):
            objs.append(stroke((xs, yc, zc), p, q, 0.0022, 0.0022, BRASS_DARK))
        for sign in (1.0, -1.0):
            objs.append(scroll((yc + sign * length * 0.2, zc), xs, height * 0.62,
                               s * 0.0018, BRASS_LIGHT, turns=1.1, steps=7,
                               mirror=sign < 0, a0=math.pi * 0.5))
        rivets(objs, [((s * (x + 0.003), y, zc), (s, 0.0, 0.0)) for y in rivet_y], 0.0052)


def round_stock(butt, ax, stations, seed):
    """A rounded, grained wooden stock (T01-T04): a smooth, near-elliptical loft
    of [(y, drop, half_w, half_h)] swelling toward the butt, and a brass butt
    plate with a dark rim."""
    objs = [grain(loft_y([(y, ax - d, hw, hh, 2.35) for y, d, hw, hh in stations], WOOD,
                         seg=ROUND_SEG), seed=seed, across=40.0, along=3.0, dark=0.3,
                  light=0.34)]
    y, d, hw, hh = stations[-1]
    c = ax - d
    objs.append(loft_tagged([(butt - 0.020, c, hw + 0.0006, hh + 0.0006, 2.35),
                             (butt - 0.017, c, hw + 0.0032, hh + 0.0032, 2.35),
                             (butt - 0.014, c, hw + 0.0022, hh + 0.0022, 2.35),
                             (butt, c, hw + 0.0022, hh + 0.0022, 2.35),
                             (butt + 0.004, c, hw - 0.0005, hh - 0.0005, 2.35)],
                            [BRASS_DARK, BRASS_DARK, BRASS, BRASS], seg=ROUND_SEG))
    return objs


# ---------------------------------------------------------------------------
# Rifle
# ---------------------------------------------------------------------------

R_AXIS = 0.062           # chamber / barrel axis height
R_SIGHT = 0.160          # sight line height (just over the collars: a compact ear)
R_CHAMBER_Y = -0.005     # chamber and crystal centre along the gun
R_CHAMBER_R = 0.050      # glass radius (the chamber arcs hug 0.046-0.048)
R_CHAMBER_HALF = 0.109   # half the glass length (viewmodel::CHAMBER_HALF_LENGTH)
R_COLLAR = (0.053, 0.064)   # chamber collars' half width and height
R_REAR_SIGHT_Y = 0.120
R_FRONT_SIGHT_Y = -0.392
R_BARREL_R = 0.029
R_MUZZLE_Y = -0.556
R_MUZZLE_LEN = 0.058
R_MAG_SEAT = Vector((0.0, -0.092, -0.046))
R_MAG_TILT = math.radians(-8.0)   # the magazine's foot leans forward
# The left glove on the forend, in front of the magazine, behind the rune window.
R_GRIP_L = Vector((0.0, -0.200, 0.031))
R_WINDOW = (-0.306, R_AXIS + 0.006)   # rune window centre (y, z) on each side


def rifle_body(objs):
    ax = R_AXIS
    hw, hh = R_COLLAR
    # Rounded, ornate brass collars at both ends of the glass chamber.
    for y0, y1 in ((0.098, 0.142), (-0.150, -0.106)):
        beaded_collar(objs, y0, y1, ax, hw, hh, 0.0064)
    for y in (-0.105, 0.097):
        objs.append(cyl((0.0, y - 0.004, ax), (0.0, y + 0.004, ax), R_CHAMBER_R + 0.002, IRON,
                        seg=ROUND_SEG))
    # Brass rails along the lower sides of the chamber, collar to collar.
    for s in (1.0, -1.0):
        objs.append(rbox((s * 0.037, R_CHAMBER_Y, ax - 0.041), (0.010, 0.206, 0.012), BRASS,
                         bevel=0.003))
    # A rounded ear on the front collar, and the rear collar's ear split into a
    # U-notch: the rear sight.
    objs.append(rbox((0.0, -0.128, ax + 0.070), (0.030, 0.026, 0.022), BRASS, bevel=0.008,
                     segments=2))
    rear_sight(objs, R_REAR_SIGHT_Y, ax + hh - 0.002, R_SIGHT)

    # Lower receiver (iron, well rounded) with ornate brass side plates.
    objs.append(rbox((0.0, -0.030, -0.018), (0.064, 0.270, 0.058), IRON, bevel=0.012,
                     segments=3))
    side_plates(objs, -0.035, -0.018, 0.190, 0.034, 0.0325, (-0.118, 0.048))
    pistol_grip(objs, seed=11)

    # Grained wooden forend in front of the chamber, rounded.
    objs.append(grain(loft_y([(-0.150, ax - 0.006, 0.041, 0.063, 2.8),
                              (-0.215, ax - 0.0055, 0.0406, 0.0624, 2.8),
                              (-0.290, ax - 0.004, 0.040, 0.061, 2.8),
                              (-0.345, ax - 0.003, 0.0395, 0.0595, 2.8),
                              (-0.395, ax - 0.002, 0.039, 0.058, 2.8)],
                             WOOD, seg=ROUND_SEG), seed=12, across=40.0, along=3.0,
                      dark=0.3, light=0.34))
    objs.append(rbox((0.0, -0.262, ax + 0.056), (0.036, 0.220, 0.010), BRASS, bevel=0.004))
    # Front band (brass, rounded) with rivets, carrying the front sight.
    beaded_collar(objs, -0.415, -0.370, ax - 0.002, 0.046, 0.064, 0.0058,
                  rivet_rows=(0.0,), n=2.6)
    fy = R_FRONT_SIGHT_Y
    post_top = R_SIGHT - 0.020
    objs.append(rbox((0.0, fy, (0.118 + post_top) / 2), (0.013, 0.016, post_top - 0.118),
                     IRON, bevel=0.003))
    objs.append(arc_band((0.0, fy, R_SIGHT), (0.0, -1.0, 0.0), 0.0135, 0.0235, 0.015,
                         math.radians(125.0), math.radians(415.0), IRON, seg=12))

    # Rune window frames on both sides of the forend (the glowing panels and
    # glyphs are the Runes part).
    wy, wz = R_WINDOW
    for s in (1.0, -1.0):
        objs.append(rbox((s * 0.040, wy, wz), (0.010, 0.094, 0.082), IRON, bevel=0.005,
                         segments=2))
        rivets(objs, [((s * 0.0455, wy + dy, wz + dz), (s, 0.0, 0.0))
                      for dy in (-0.040, 0.040) for dz in (-0.035, 0.035)], 0.0036)

    # A round brass barrel with a dark iron band, a brass bead, and a brass bead
    # seating the iron muzzle cap.
    objs.append(cyl((0.0, -0.395, ax), (0.0, R_MUZZLE_Y + 0.002, ax), R_BARREL_R, BRASS,
                    seg=ROUND_SEG))
    r = R_BARREL_R
    band = [(0.000, r), (0.000, r + 0.004), (0.003, r + 0.0075), (0.006, r + 0.0055),
            (0.028, r + 0.0055), (0.031, r + 0.0075), (0.034, r + 0.004), (0.034, r)]
    objs.append(lathe((0.0, -0.466, ax), (0.0, -1.0, 0.0), band,
                      [IRON, BRASS_DARK, BRASS, IRON, BRASS, BRASS_DARK, IRON, IRON],
                      seg=ROUND_SEG))
    objs.append(bead_ring((0.0, -0.432, ax), Y, r, 0.007, BRASS, seg=ROUND_SEG))
    objs.append(bead_ring((0.0, R_MUZZLE_Y + 0.004, ax), Y, r + 0.001, 0.008, BRASS,
                          seg=ROUND_SEG))


def rifle_stock():
    """The rounded, warm red-brown wooden stock (T01-T03)."""
    return round_stock(0.471, R_AXIS,
                       [(0.118, 0.002, 0.044, 0.058),
                        (0.160, 0.007, 0.046, 0.064),
                        (0.215, 0.016, 0.048, 0.071),
                        (0.280, 0.025, 0.049, 0.078),
                        (0.345, 0.034, 0.050, 0.084),
                        (0.400, 0.040, 0.050, 0.088),
                        (0.451, 0.044, 0.050, 0.090)], seed=13)


def rifle_mag():
    """The energy cell, built at its seat and leaning forward (its glowing slots
    are the Runes part)."""
    objs = [rbox((0.0, 0.0, -0.078), (0.050, 0.068, 0.148), IRON, bevel=0.010, segments=2),
            rbox((0.0, 0.0, -0.010), (0.056, 0.074, 0.018), BRASS, bevel=0.005),
            rbox((0.0, 0.0, -0.154), (0.058, 0.078, 0.020), BRASS, bevel=0.007),
            rbox((0.0, 0.0, -0.140), (0.057, 0.076, 0.004), BRASS_DARK)]
    for s in (1.0, -1.0):
        for y in (0.026, -0.026):
            objs.append(dome((s * 0.029, y, -0.010), (s, 0.0, 0.0), 0.0042))
    return _at_mag(objs)


def _at_mag(objs):
    m = Matrix.Translation(R_MAG_SEAT) @ Matrix.Rotation(R_MAG_TILT, 4, "X")
    for o in objs:
        o.data.transform(m)
    return objs


def rifle_muzzle():
    """A dark iron muzzle cap (T01-T03): a short round can with a rolled lip,
    a brass back rim and a dark bore."""
    base = Vector((0.0, R_MUZZLE_Y, R_AXIS))
    L = R_MUZZLE_LEN
    profile = [(0.0, 0.022), (0.0, 0.031), (0.004, 0.0355), (0.010, 0.037),
               (L - 0.010, 0.037), (L - 0.004, 0.0385), (L, 0.035), (L, 0.025),
               (L - 0.012, 0.0195)]
    colors = [BRASS_DARK, BRASS_DARK, IRON, IRON, IRON, IRON, IRON, IRON, IRON]
    return [lathe(base, (0.0, -1.0, 0.0), profile, colors, seg=28)]


def rifle_runes():
    """Everything on the rifle that glows with its crystal: the rune windows'
    panels and glyphs, the magazine's slots, the muzzle cap's rune ring and the
    gems in the chamber collars."""
    objs = []
    wy, wz = R_WINDOW
    for s in (1.0, -1.0):
        objs.append(rbox((s * 0.0448, wy, wz), (0.004, 0.076, 0.066), "crystal_blue",
                         bevel=0.0015))
        gx = s * 0.0472
        dy, dz = 0.025, 0.023
        # The target's glyph: two triangles meeting point to point, and a bar.
        for p, q in (((-dy, dz), (dy, -dz)), ((-dy, -dz), (dy, dz)),
                     ((-dy, dz), (-dy, -dz)), ((dy, dz), (dy, -dz))):
            objs.append(stroke((gx, wy, wz), p, q, 0.0045, 0.003, "barrier_cyan"))
    slots = []
    for s in (1.0, -1.0):
        for y in (-0.019, 0.0, 0.019):
            slots.append(rbox((s * 0.0255, y, -0.080), (0.004, 0.009, 0.098), "crystal_blue"))
    objs += _at_mag(slots)
    # A thin rune ring round the barrel just behind the muzzle cap.
    objs.append(hoop(Vector((0.0, R_MUZZLE_Y + 0.020, R_AXIS)), (0.0, -1.0, 0.0),
                     R_BARREL_R + 0.0012, 0.006, 0.004, "crystal_blue", seg=ROUND_SEG))
    hw, hh = R_COLLAR
    collar_gems(objs, (0.120, -0.128), R_AXIS, hw, hh, "crystal_blue")
    return objs


def chamber_glass(y0, y1, zc, r, body, light, seg=28):
    """The glass chamber: an open tube, `body` tinted, with a long `light`
    highlight streak along its upper side facing the player (the targets'
    glass shine)."""
    bm = palette.new_bmesh()
    u, v = axis_frame((0.0, 1.0, 0.0))
    rings = []
    for y in (y0, y0 + (y1 - y0) * 0.2, y0 + (y1 - y0) * 0.8, y1):
        c = Vector((0.0, y, zc))
        rings.append([bm.verts.new(c + (u * math.cos(2 * math.pi * k / seg)
                                        + v * math.sin(2 * math.pi * k / seg)) * r)
                      for k in range(seg)])
    faces = []
    for a, b in zip(rings, rings[1:]):
        faces += shapes.bridge(bm, a, b)
    palette.tag(bm, faces, body)
    bm.normal_update()
    # One streak along the upper side facing the player (+X), in the middle.
    shine = [f for f in faces if 0.5 < f.normal.z < 0.9 and f.normal.x > 0.0]
    mid = [f for f in shine if abs(f.calc_center_median().y - (y0 + y1) / 2) < (y1 - y0) * 0.31]
    palette.tag(bm, mid, light)
    return finish(bm, None)


def chamber_back(y0, y1, zc, r, color, seg=24):
    """The chamber's lit interior: an opaque tube just inside the glass whose
    faces look inward, so only its far wall draws (back faces are culled):
    the deep, glowing blue (or violet) behind the crystal that the targets
    paint, while the glass in front stays clear enough to show the crystal."""
    bm = palette.new_bmesh()
    u, v = axis_frame((0.0, 1.0, 0.0))
    rings = [[bm.verts.new(Vector((0.0, y, zc)) + (u * math.cos(2 * math.pi * k / seg)
                                                   + v * math.sin(2 * math.pi * k / seg)) * r)
              for k in range(seg)] for y in (y0, y1)]
    palette.tag(bm, shapes.bridge(bm, rings[0], rings[1]), color)
    obj = finish(bm, None)
    flipped = bmesh.new()
    flipped.from_mesh(obj.data)
    bmesh.ops.reverse_faces(flipped, faces=flipped.faces)
    flipped.to_mesh(obj.data)
    flipped.free()
    return obj


def build_rifle(root):
    body = []
    rifle_body(body)
    part("Body", body, root)
    part("Stock", rifle_stock(), root, location=(0.0, 0.118, R_AXIS))
    socket = (0.0, R_CHAMBER_Y, R_AXIS)
    glass = chamber_glass(R_CHAMBER_Y - R_CHAMBER_HALF, R_CHAMBER_Y + R_CHAMBER_HALF, R_AXIS,
                          R_CHAMBER_R, "gun_glass_blue", "barrier_cyan")
    part("Chamber", [glass], root, location=socket, shine=False)
    back = chamber_back(R_CHAMBER_Y - R_CHAMBER_HALF, R_CHAMBER_Y + R_CHAMBER_HALF, R_AXIS,
                        R_CHAMBER_R - 0.0025, "gun_glass_blue")
    part("ChamberBack", [back], root, location=socket, shine=False)
    # A deep blue body with bright cyan facet edges, so the facets read through
    # the glow (T01-T03): the edges take the crystal highlight.
    crystal = gem(0.036, 1.25, "gun_glass_blue", "crystal_blue", rotation=(0.35, 0.25, 0.55),
                  inset=0.22)
    crystal.data.transform(Matrix.Translation(Vector(socket)))
    part("Crystal", [crystal], root, location=socket, flat=True, shine=False)
    part("Mag", rifle_mag(), root, location=R_MAG_SEAT)
    part("Muzzle", rifle_muzzle(), root, location=(0.0, R_MUZZLE_Y, R_AXIS), sharp_deg=60.0)
    part("Runes", rifle_runes(), root, flat=True, shine=False)

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
P_SIGHT = 0.156          # sight line height (just over the collars)
P_CRYSTAL_Y = -0.012     # crystal centre (between the collars)
P_REAR_COLLAR = (0.068, 0.110)
P_FRONT_COLLAR = (-0.140, -0.100)
P_COLLAR = (0.054, 0.064)
P_CHAMBER_R = 0.050      # glass radius
P_BARREL_R = 0.030
P_REAR_SIGHT_Y = 0.089
P_FRONT_SIGHT_Y = -0.412
P_MUZZLE_Y = -0.430
P_BELL_LEN = 0.128
P_PUMP = Vector((0.0, -0.310, 0.040))   # the pump grip's rest centre
P_PUMP_HALF = (0.047, 0.052)
P_PUMP_LEN = 0.150
P_PUMP_TRAVEL = 0.09                      # how far the rack pulls the grip back
# Rings: two gem-studded gold rings hugging the glass inboard of the collars,
# `P_RING_SPACING` either side of the crystal, each tipped a little off the
# gun's axis (so their spin reads as a wobble too).
P_RING_RADIUS = 0.0565
P_RING_SPACING = 0.056
P_RING_WIDTH = 0.010
P_RING_THICK = 0.006
P_RING_TILT = math.radians(7.0)
P_RING_GEMS = 4


def pump_body(objs):
    ax = P_AXIS
    hw, hh = P_COLLAR
    # Rounded, ornate brass collars framing the glass chamber.
    for y0, y1 in (P_REAR_COLLAR, P_FRONT_COLLAR):
        beaded_collar(objs, y0, y1, ax, hw, hh, 0.0064)
    for y in (P_FRONT_COLLAR[1] + 0.001, P_REAR_COLLAR[0] - 0.001):
        objs.append(cyl((0.0, y - 0.004, ax), (0.0, y + 0.004, ax), P_CHAMBER_R + 0.002, IRON,
                        seg=ROUND_SEG))
    # Rear ear with the U-notch sight.
    rear_sight(objs, P_REAR_SIGHT_Y, ax + hh - 0.004, P_SIGHT)
    # Brass rails along the lower sides of the chamber (T04's frame).
    y0, y1 = P_FRONT_COLLAR[1], P_REAR_COLLAR[0]
    for s in (1.0, -1.0):
        objs.append(rbox((s * 0.037, (y0 + y1) / 2, ax - 0.041), (0.010, y1 - y0 + 0.004, 0.012),
                         BRASS, bevel=0.003))

    # The receiver under the chamber (iron, rounded) with ornate brass side plates.
    objs.append(rbox((0.0, -0.016, -0.027), (0.064, 0.262, 0.046), IRON, bevel=0.012,
                     segments=3))
    side_plates(objs, -0.020, -0.026, 0.200, 0.026, 0.0325, (-0.110, 0.070))
    pistol_grip(objs, seed=21)

    # A round iron barrel, a brass bead ahead of the chamber (clear of the rack).
    objs.append(cyl((0.0, -0.120, ax), (0.0, P_MUZZLE_Y + 0.002, ax), P_BARREL_R, IRON,
                    seg=ROUND_SEG))
    objs.append(bead_ring((0.0, -0.150, ax), Y, P_BARREL_R, 0.007, BRASS, seg=ROUND_SEG))
    # Front band round the barrel (beaded), with the front sight's post and bead.
    fy = P_FRONT_SIGHT_Y
    beaded_collar(objs, fy - 0.010, fy + 0.010, ax, 0.037, 0.037, 0.0048,
                  rivet_rows=(0.0,), n=2.0)
    objs.append(rbox((0.0, fy, (ax + 0.030 + P_SIGHT) / 2), (0.011, 0.014, P_SIGHT - ax - 0.030),
                     IRON, bevel=0.003))
    objs.append(ball((0.0, fy, P_SIGHT), 0.0095, BRASS, seg=10, rings=6))


def pump_stock():
    """The rounded, warm red-brown wooden stock (T04)."""
    return round_stock(0.358, P_AXIS,
                       [(0.100, 0.002, 0.045, 0.060),
                        (0.135, 0.008, 0.047, 0.065),
                        (0.180, 0.015, 0.048, 0.071),
                        (0.235, 0.024, 0.049, 0.078),
                        (0.290, 0.031, 0.050, 0.083),
                        (0.338, 0.036, 0.050, 0.086)], seed=23)


def pump_grip():
    """The pump grip (T04): five chunky rounded wooden ribs cut apart by deep
    grooves, cradling the barrel from below, grained, between brass end rims.
    Centred on P_PUMP."""
    c = P_PUMP
    hw, hh = P_PUMP_HALF
    length, ribs, groove_w, depth = P_PUMP_LEN, 5, 0.008, 0.012
    n = 2.5
    rib = (length - (ribs - 1) * groove_w) / ribs
    y = c.y + length / 2
    stations = []
    for k in range(ribs):
        y_end = y - rib
        stations += [(y, c.z, hw - depth * 0.4, hh - depth * 0.4, n),
                     (y - rib * 0.3, c.z, hw, hh, n),
                     (y_end + rib * 0.3, c.z, hw, hh, n),
                     (y_end, c.z, hw - depth * 0.4, hh - depth * 0.4, n)]
        if k < ribs - 1:
            stations.append((y_end - groove_w * 0.5, c.z, hw - depth, hh - depth, n))
        y = y_end - groove_w
    objs = [grain(loft_y(stations, WOOD, seg=18), seed=24, across=60.0, along=5.0)]
    for dy in (-(length / 2 + 0.005), length / 2 + 0.005):
        objs.append(band_y(c.y + dy - 0.006, c.y + dy + 0.006, c.z, hw - 0.001, hh - 0.001,
                           BRASS, n=n, seg=18))
    return objs


def pump_bell():
    """The flared brass bell (T04): a beaded throat, then a long, smooth trumpet
    flare out to a thick rolled lip, so from the side it reads as a horn; brass
    inside, and a dark bore."""
    base = Vector((0.0, P_MUZZLE_Y, P_AXIS))
    L = P_BELL_LEN
    profile = [(0.000, 0.022), (0.000, 0.035), (0.008, 0.040), (0.016, 0.036),
               (0.034, 0.035), (0.060, 0.039), (0.084, 0.048), (0.102, 0.059),
               (0.115, 0.071), (L - 0.004, 0.080), (L, 0.077), (L, 0.071),
               (L - 0.006, 0.066), (0.100, 0.049), (0.064, 0.030), (0.036, 0.023)]
    # The rolled lip is polished bright, so the bell's rim reads from behind.
    colors = ([BRASS, BRASS, BRASS_DARK, BRASS] + [BRASS] * 5
              + [BRASS_LIGHT, BRASS_LIGHT, BRASS_DARK, BRASS_DARK, BRASS_DARK, IRON, IRON])
    return [lathe(base, (0.0, -1.0, 0.0), profile, colors, seg=32)]


def pump_rings(center):
    """Two gold rings hugging the glass either side of the crystal, each set with
    small violet gems (so their spin reads) and tipped a little off the axis."""
    objs = []
    for k, side in enumerate((-1.0, 1.0)):
        az = math.radians(90.0 + 180.0 * k)
        n = Vector((math.sin(P_RING_TILT) * math.cos(az), math.cos(P_RING_TILT),
                    math.sin(P_RING_TILT) * math.sin(az)))
        c = Vector(center) + Vector((0.0, side * P_RING_SPACING, 0.0))
        objs.append(hoop(c, n, P_RING_RADIUS, P_RING_WIDTH, P_RING_THICK, "gold_rings", seg=28))
        u, v = axis_frame(n)
        for g in range(P_RING_GEMS):
            a = 2.0 * math.pi * (g + 0.5 * k) / P_RING_GEMS + math.pi / 4.0
            d = u * math.cos(a) + v * math.sin(a)
            objs.append(dome(c + d * (P_RING_RADIUS + P_RING_THICK * 0.5), d, 0.0052,
                             "crystal_violet", squash=0.9, seg=6))
    return objs


def pump_runes():
    """The pump's glowing inlays: a violet rune ring round the bell's neck and
    small violet gems set in each chamber collar's side."""
    objs = [hoop(Vector((0.0, P_MUZZLE_Y - 0.034, P_AXIS)), (0.0, -1.0, 0.0), 0.0355,
                 0.007, 0.004, "crystal_violet", seg=ROUND_SEG)]
    hw, hh = P_COLLAR
    collar_gems(objs, ((P_REAR_COLLAR[0] + P_REAR_COLLAR[1]) / 2,
                       (P_FRONT_COLLAR[0] + P_FRONT_COLLAR[1]) / 2), P_AXIS, hw, hh,
                "crystal_violet")
    return objs


def build_pump(root):
    body = []
    pump_body(body)
    part("Body", body, root)
    part("Stock", pump_stock(), root, location=(0.0, 0.100, P_AXIS))
    socket = Vector((0.0, P_CRYSTAL_Y, P_AXIS))
    y0, y1 = P_FRONT_COLLAR[1], P_REAR_COLLAR[0]
    glass = chamber_glass(y0, y1, P_AXIS, P_CHAMBER_R, "gun_glass_violet", "glass_violet")
    part("Chamber", [glass], root, location=socket, shine=False)
    back = chamber_back(y0, y1, P_AXIS, P_CHAMBER_R - 0.0025, "gun_glass_violet")
    part("ChamberBack", [back], root, location=socket, shine=False)
    crystal = gem(0.0365, 1.75, "crystal_violet", "glass_violet", rotation=(0.12, 0.2, 0.5),
                  inset=0.22)
    crystal.data.transform(Matrix.Translation(socket))
    part("Crystal", [crystal], root, location=socket, flat=True, shine=False)
    part("Rings", pump_rings(socket), root, location=socket, sharp_deg=50.0, shine=False)
    shell = shard(0.038, 0.0105, 0.012, "crystal_violet", "glass_violet", sides=6)
    shell.data.transform(Matrix.Translation(socket))
    part("Shard", [shell], root, location=socket, flat=True, shine=False)
    grip = part("PumpGrip", pump_grip(), root, location=P_PUMP)
    part("Muzzle", pump_bell(), root, location=(0.0, P_MUZZLE_Y, P_AXIS), sharp_deg=60.0)
    part("Runes", pump_runes(), root, flat=True, shine=False)

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
    Asset("rifle", "gun", build_rifle, "ornate brass-and-crystal assault rifle viewmodel"),
    Asset("pump", "gun", build_pump,
          "stubby bell-mouthed pump with a violet crystal in a ringed glass chamber"),
]
