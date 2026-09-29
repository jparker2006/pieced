"""The knight: the goofy armoured knight-wizard enemy (targets T03-T08, R4).

A small steel body in a flowing purple robe and cape, a big bucket helmet with
big round cartoon eyes peering through the visor slit over a clean grille, a
tall pointed floppy wizard hat with a gold star and a bent tip, and oversized
open gauntlets and boots.

**Hitbox fit (docs/M2-SPEC.md, the knight and Amendment B; gate S5).** The
model is made to fit the gameplay hitboxes of src/player.rs, never the reverse:
body capsule r 0.33 m from 0.05 to 1.45 m, head sphere r 0.20 m centred at
1.62 m (feet at the origin). Body parts stay inside the capsule and the helmet
and eyes inside the head sphere, each within 5 cm (`check_fit` enforces it at
build time, `tests/knight.rs` in the game); the parts also fill the hitboxes.
The hat (`Hat` and its floppy `HatTip`) is **cosmetic** (Amendment B, D49): like
a Fortnite cosmetic it takes no hits, so it may rise well above the head sphere
and is left out of the fit. The "big head" look comes from the helmet filling
the whole head sphere over a small torso, not from a bigger hitbox. Below
`FOOT_BAND` the capsule's rounded end cannot hold two boots standing on the
ground, so there the capsule counts as a cylinder (as it did for Milestone 1's
figure).

**Hierarchy** (Blender names = glTF node names = Bevy `Name`s). Pivots are
empties at the joints; procedural animation rotates them:

    knight
      PivotLegL, PivotLegR        hips          -> BootL, BootR (leg, greave, boot)
      Torso                       origin at the hips: bob or lean it to move the upper
                                  body (hips, belt, breastplate, mantle, pauldrons)
        PivotArmL, PivotArmR      shoulders     -> GauntletL, GauntletR (sleeve, arm, hand)
        PivotCape                 nape          -> Cape
        PivotRobe                 waist         -> Robe (the skirt, open at the front)
        PivotHead                 neck          -> Helmet -> EyeL, EyeR and the eye variants
          PivotHat                hat base      -> Hat -> PivotHatTip (the bend) -> HatTip

L and R are the knight's own left and right: L is Blender +X (Bevy model -X).
Each eye has four states, as separate parts in the same place: `Eye*` (open),
`EyeWide*`, `EyeBlink*` (closed lines) and `EyeX*`. Only the open eyes render in
the previews (the rest have `hide_render`); the game shows one state at a time by
toggling visibility. Each eye part's origin is the eye's centre.

Run this file directly for review renders and fit numbers (never opens a window):

    blender -b --factory-startup --python-exit-code 1 -P art/blender/assets/knight.py -- [OUT_DIR]

which writes knight_views.png (front, side, back, 3/4 back), knight_hitbox.png
(orthographic front and side with the hitboxes, +5 cm and -10 cm lines drawn on)
and knight_eyes.png (the four eye states) to OUT_DIR (default art/previews).
"""

import math
import os
import sys

if __name__ == "__main__":  # run directly: make `lib` importable as build.py does
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

import bmesh
import bpy
from mathutils import Matrix, Vector

from lib import palette, scene, shapes
from lib.registry import Asset

# ---------------------------------------------------------------------------
# Hitboxes (src/player.rs), Blender space: z up, feet at z = 0
# ---------------------------------------------------------------------------

BODY_RADIUS = 0.33
BODY_BOTTOM = 0.05
BODY_TOP = 1.45
HEAD_CENTER = 1.62
HEAD_RADIUS = 0.20
TOLERANCE = 0.05  # a part may poke out of its hitbox by at most this much
FOOT_BAND = 0.12  # below this height the capsule counts as a cylinder (boots on the ground)
HEAD_PARTS = ("Helmet",)  # plus every Eye* part
COSMETIC_PARTS = ("Hat", "HatTip")  # take no hits: left out of the fit (Amendment B)

# ---------------------------------------------------------------------------
# Palette names (art/palette.json)
# ---------------------------------------------------------------------------

STEEL = "knight_steel"
PURPLE = "knight_purple"
GOLD = "star_gold"
TRIM = "gold_rings"
DARK = "gun_iron"  # undersuit, sleeves, soles
BLACK = "pupil_black"  # pupils and the visor's dark inside
WHITE = "eye_white"
LEATHER = "stock"
GEM = "crystal_violet"

# ---------------------------------------------------------------------------
# Joints (world, Blender space). x is mirrored per side: s = +1 left, -1 right.
# ---------------------------------------------------------------------------

HIP = Vector((0.11, 0.0, 0.62))
SHOULDER = Vector((0.20, 0.0, 1.20))
NECK = Vector((0.0, 0.0, 1.42))
NAPE = Vector((0.0, 0.14, 1.28))
WAIST = Vector((0.0, 0.0, 0.745))
HAT_BASE = Vector((0.0, 0.0, 1.705))  # under the brim, so the dropped hat rests low on it
TORSO_ORIGIN = Vector((0.0, 0.0, 0.62))

# Helmet: rings are shrunk to stay this far inside the head sphere's centre.
HELMET_FIT = HEAD_RADIUS + 0.046
SLIT = (1.572, 1.706)  # visor slit, bottom and top: tall, for the big eyes
SLIT_DEPTH = 0.035
SLOTS = (1.49, 1.55)  # the grille's breathing slots, bottom and top
SLOT_DEPTH = 0.02
PLATE = (0.225, 0.19, 0.11)  # the face plate's ring: half width, half depth, corner radius
EYE_X = 0.058
EYE_Z = 1.639
EYE_R = (0.052, 0.061)  # the open eye's half width and half height (a tall cartoon oval)


def mirror(v, s):
    return Vector((v.x * s, v.y, v.z))


# ---------------------------------------------------------------------------
# Geometry helpers (local to the knight; everything is tagged with palette names)
# ---------------------------------------------------------------------------

def faces_of(verts):
    return list(dict.fromkeys(f for v in verts for f in v.link_faces))


def blob(bm, center, half, exponent=2.0, segments=12, rings=8, floor=None, rot=None,
         shape=None):
    """A superellipsoid (exponent 2 = ellipsoid, higher = rounded box).

    `shape(p)` may reshape the unit-sized point first; `rot` is a 3x3 matrix
    applied before translation; `floor` flattens everything below that height.
    """
    verts = bmesh.ops.create_uvsphere(bm, u_segments=segments, v_segments=rings,
                                      radius=1.0)["verts"]
    c = Vector(center)
    for v in verts:
        d = v.co.normalized()
        p = shapes.superellipsoid_point(d, exponent) if exponent != 2.0 else d.copy()
        if shape is not None:
            p = shape(p)
        p = Vector((p.x * half[0], p.y * half[1], p.z * half[2]))
        if rot is not None:
            p = rot @ p
        p += c
        if floor is not None and p.z < floor:
            p.z = floor
        v.co = p
    return faces_of(verts)


def rrect(a, b, c, corner=4, front=(), side=1, back=1):
    """A rounded rectangle's outline, counter-clockwise seen from above.

    Half width `a` (x), half depth `b` (y), corner radius `c`; starts at the front
    centre (0, -b) heading +x. `front` lists fractions (0..1) of the straight front
    half-edge at which extra points sit (mirrored), so rings made with the same
    arguments share their topology and can be bridged.
    """
    c = min(c, a - 1e-4, b - 1e-4)
    ax, by = a - c, b - c

    def arc(cx, cy, a0):
        return [(cx + c * math.cos(a0 + math.pi / 2 * k / corner),
                 cy + c * math.sin(a0 + math.pi / 2 * k / corner)) for k in range(corner + 1)]

    pts = [(0.0, -b)]
    pts += [(f * ax, -b) for f in front]
    pts += arc(ax, -by, -math.pi / 2)
    pts += [(a, -by + 2 * by * (k + 1) / (side + 1)) for k in range(side)]
    pts += arc(ax, by, 0.0)
    pts += [(ax - 2 * ax * (k + 1) / (back + 1), b) for k in range(back)]
    pts += arc(-ax, by, math.pi / 2)
    pts += [(-a, by - 2 * by * (k + 1) / (side + 1)) for k in range(side)]
    pts += arc(-ax, -by, math.pi)
    pts += [(-f * ax, -b) for f in reversed(front)]
    return pts


def rrect_reach(a, b, c):
    """The farthest distance of a rounded rectangle's outline from its centre."""
    c = min(c, a, b)
    return math.hypot(a - c, b - c) + c


def fit_in_sphere(z, a, b, c, centre_z, radius):
    """Shrinks a rounded-rectangle ring at height z to fit inside a sphere."""
    dz = z - centre_z
    limit = math.sqrt(max(radius * radius - dz * dz, 1e-6))
    k = min(1.0, limit / rrect_reach(a, b, c))
    return a * k, b * k, c * k


def _plate_front(z):
    """The face plate's front (y) at a height inside the visor slit: the slit's
    bottom and top rings are shrunk into the head sphere like every helmet ring,
    and the plate runs straight between them."""
    lo, hi = (fit_in_sphere(h, *PLATE, HEAD_CENTER, HELMET_FIT)[1] for h in SLIT)
    t = (z - SLIT[0]) / (SLIT[1] - SLIT[0])
    return -(lo + (hi - lo) * t)


# The slit's back wall (y) at the eyes' height, a hair behind the recess.
EYE_FLOOR = _plate_front(EYE_Z) + SLIT_DEPTH + 0.002


def ring(bm, pts, z, dx=0.0, dy=0.0, wave=None, lift=None):
    """Vertices for a 2D outline at height z. `wave(angle)` scales the radius,
    `lift(x, y)` raises individual vertices."""
    out = []
    for x, y in pts:
        k = wave(math.atan2(y, x)) if wave is not None else 1.0
        h = lift(x, y) if lift is not None else 0.0
        out.append(bm.verts.new((x * k + dx, y * k + dy, z + h)))
    return out


def loft(bm, rings):
    """Quads between consecutive rings (bottom to top): one face list per band."""
    return [shapes.bridge(bm, lo, hi) for lo, hi in zip(rings, rings[1:])]


def cap(bm, verts, up):
    return bm.faces.new(verts if up else list(reversed(verts)))


def tube(bm, points, segments=10, caps=True):
    """A round tube through [(point, radius)]."""
    path = [(Vector(p), (lambda r: lambda a: r)(r)) for p, r in points]
    _, sides, end_caps = shapes.loft(bm, path, segments, cap_start=caps, cap_end=caps)
    return sides + end_caps


def torus(bm, center, major, minor, n_major=16, n_minor=6, rot=None):
    """A ring in the xz plane (its hole looks along y), optionally rotated."""
    c = Vector(center)
    rings = []
    for i in range(n_major):
        a = 2 * math.pi * i / n_major
        d = Vector((math.cos(a), 0.0, math.sin(a)))
        r = []
        for j in range(n_minor):
            b = 2 * math.pi * j / n_minor
            p = d * (major + minor * math.cos(b)) + Vector((0.0, minor * math.sin(b), 0.0))
            r.append(bm.verts.new(c + (rot @ p if rot is not None else p)))
        rings.append(r)
    faces = []
    for i in range(n_major):
        lo, hi = rings[i], rings[(i + 1) % n_major]
        for j in range(n_minor):
            k = (j + 1) % n_minor
            faces.append(bm.faces.new((lo[j], lo[k], hi[k], hi[j])))
    return faces


def star_prism(bm, center, outer, inner, depth, normal, up):
    """A five-pointed star plate facing `normal`, one point towards `up`."""
    n = Vector(normal).normalized()
    u = (Vector(up) - n * Vector(up).dot(n)).normalized()
    r = n.cross(u)
    c = Vector(center)
    front, back = [], []
    for k in range(10):
        a = math.pi / 2 + math.pi * k / 5
        rad = outer if k % 2 == 0 else inner
        p = c + (r * math.cos(a) + u * math.sin(a)) * rad
        front.append(bm.verts.new(p + n * depth / 2))
        back.append(bm.verts.new(p - n * depth / 2))
    cf = bm.verts.new(c + n * depth / 2)
    cb = bm.verts.new(c - n * depth / 2)
    faces = []
    for k in range(10):
        j = (k + 1) % 10
        faces.append(bm.faces.new((cf, front[j], front[k])))
        faces.append(bm.faces.new((cb, back[k], back[j])))
        faces.append(bm.faces.new((front[k], front[j], back[j], back[k])))
    return faces


def recess(bm, faces, depth):
    """Pushes a region of front-facing faces into the surface by `depth` (+y)."""
    ret = bmesh.ops.extrude_face_region(bm, geom=faces)
    verts = [e for e in ret["geom"] if isinstance(e, bmesh.types.BMVert)]
    bmesh.ops.delete(bm, geom=[f for f in faces if f.is_valid], context="FACES")
    bmesh.ops.translate(bm, vec=Vector((0.0, depth, 0.0)), verts=verts)
    return faces_of(verts)


def surface_hit(bm, origin, direction):
    """Where a ray first hits the bmesh: (location, normal)."""
    from mathutils.bvhtree import BVHTree

    bm.normal_update()
    hit = BVHTree.FromBMesh(bm).ray_cast(Vector(origin), Vector(direction).normalized())
    if hit[0] is None:
        raise ValueError(f"ray from {origin} missed")
    return hit[0], hit[1]


def canonical(bm):
    """A copy of `bm` as triangles in a fixed order; frees `bm`.

    Some bmesh operators visit faces in hash-set order, so face order (and the
    exporter's triangulation of n-gons) can change from run to run while the
    vertices don't. `build-art.sh --check` needs byte-identical output, so every
    part is triangulated with fixed rules, each triangle starts at its lowest
    vertex index (winding kept), and the triangles are sorted.
    """
    bmesh.ops.triangulate(bm, faces=bm.faces[:], quad_method="FIXED", ngon_method="EAR_CLIP")
    bm.verts.index_update()
    layer = palette.face_layer(bm)
    tris = []
    for f in bm.faces:
        idx = [v.index for v in f.verts]
        k = idx.index(min(idx))
        tris.append((tuple(idx[k:] + idx[:k]), f[layer]))
    tris.sort()
    out = palette.new_bmesh()
    out_layer = palette.face_layer(out)
    verts = [out.verts.new(v.co) for v in bm.verts]
    for idx, tag in tris:
        out.faces.new([verts[i] for i in idx])[out_layer] = tag
    bm.free()
    return out


class Builder:
    """One part's palette-tagged bmesh, built in world (Blender) space."""

    def __init__(self):
        self.bm = palette.new_bmesh()

    def tag(self, faces, color):
        palette.tag(self.bm, faces, color)
        return faces

    def finish(self, name, parent, origin, parent_origin, sharp_deg=40.0):
        """Makes the part: vertices relative to `origin`, placed under `parent`.

        Every shell a part is made of is closed, so normals are recomputed to
        point outwards. Then the part is made canonical (see `canonical`).
        """
        bmesh.ops.recalc_face_normals(self.bm, faces=self.bm.faces[:])
        self.bm = canonical(self.bm)
        bmesh.ops.translate(self.bm, vec=-Vector(origin), verts=self.bm.verts)
        obj = scene.make_part(name, shapes.mesh_from_bmesh(self.bm), parent,
                              Vector(origin) - Vector(parent_origin))
        shapes.smooth_shading(obj, sharp_angle_deg=sharp_deg)
        return obj


# ---------------------------------------------------------------------------
# Head: helmet, eyes, hat
# ---------------------------------------------------------------------------

# x of the grille's slot edges on the face plate: four even slots, centred.
_SLOT_EDGES = (0.012, 0.036, 0.06, 0.084)


def build_helmet():
    """A big round bucket helmet filling the head sphere (and its tolerance): a flat
    face plate with a tall visor slit for the eyes over a clean four-slot grille,
    well-rounded corners, a bulging belly and a domed top under the hat."""
    b = Builder()
    bm = b.bm
    ax = PLATE[0] - PLATE[2]
    # (z, a, b, c) as drawn; each ring is then shrunk into the head sphere, which
    # tapers the bottom and domes the top.
    profile = [
        (1.446, 0.15, 0.13, 0.09),
        (1.461, 0.18, 0.158, 0.1),
        (1.475, 0.204, 0.176, 0.106),
        (SLOTS[0], *PLATE),
        (SLOTS[1], *PLATE),
        (SLIT[0], *PLATE),
        (SLIT[1], *PLATE),
        (1.713, 0.212, 0.18, 0.11),
        (1.721, 0.194, 0.166, 0.104),
        (1.73, 0.162, 0.14, 0.094),
        (1.737, 0.118, 0.103, 0.074),
        (1.742, 0.07, 0.062, 0.05),
    ]
    rings = []
    for z, a, d, c in profile:
        a, d, c = fit_in_sphere(z, a, d, c, HEAD_CENTER, HELMET_FIT)
        # The face plate's columns stand at the grille's slot edges on every
        # ring wide enough, so the slots come out upright and even.
        flat = a - min(c, a - 1e-4, d - 1e-4)
        front = tuple(x / (flat if flat > 0.09 else ax) for x in _SLOT_EDGES)
        rings.append(ring(bm, rrect(a, d, c, corner=4, front=front, side=1, back=2), z))
    faces = [f for band in loft(bm, rings) for f in band]
    faces.append(cap(bm, rings[0], up=False))
    faces.append(cap(bm, rings[-1], up=True))
    b.tag(faces, STEEL)

    def front_faces(z0, z1, xs, facing):
        bm.normal_update()
        return [f for f in bm.faces
                if z0 < f.calc_center_median().z < z1 and f.normal.y < facing
                and any(x0 < f.calc_center_median().x < x1 for x0, x1 in xs)]

    lim = ax * 0.99
    b.tag(recess(bm, front_faces(SLIT[0], SLIT[1], [(-lim, lim)], -0.97), SLIT_DEPTH), BLACK)
    e = _SLOT_EDGES
    slot_xs = [(e[0], e[1]), (e[2], e[3]), (-e[1], -e[0]), (-e[3], -e[2])]
    # The grille sits where the helmet already tapers, so its face tilts down a little.
    slots = front_faces(SLOTS[0], SLOTS[1], slot_xs, -0.9)
    if len(slots) != 4:
        raise ValueError(f"the grille found {len(slots)} slot faces, not 4")
    b.tag(recess(bm, slots, SLOT_DEPTH), BLACK)
    # Round "ear" bolts.
    for s in (1, -1):
        b.tag(blob(bm, (s * (PLATE[0] - 0.006), 0.02, 1.62), (0.018, 0.036, 0.036),
                   segments=10, rings=5), STEEL)
    return b


# The headshot dent (M4, D105): where it is pressed in (a direction from the
# head's centre: his right brow, the corner of the face plate beside the
# visor, in view from the front), its radius and depth (m).
DENT_DIR = Vector((-0.62, -0.72, 0.3)).normalized()
DENT_RADIUS = 0.095
DENT_DEPTH = 0.045
STEEL_SHADOW = "knight_steel_shadow"
STEEL_SHINE = "nail_highlight"


def build_dented_helmet():
    """The helmet after a headshot: a round dent knocked into his right brow
    beside the visor, its hollow in shadowed steel, so the cartoon ding reads
    for the rest of his life. Pressed inward only, so it stays inside the head
    sphere. The mesh is refined round the dent first so it presses in smooth."""
    b = build_helmet()
    bm = b.bm
    head = Vector((0.0, 0.0, HEAD_CENTER))
    centre, _ = surface_hit(bm, head + DENT_DIR * 0.5, -DENT_DIR)
    bm.verts.index_update()
    bm.edges.index_update()
    near = [e for e in bm.edges
            if min((v.co - centre).length for v in e.verts) < DENT_RADIUS * 1.2]
    near.sort(key=lambda e: sorted(_vkey(v) for v in e.verts))
    bmesh.ops.subdivide_edges(bm, edges=near, cuts=2, use_grid_fill=True)
    for v in bm.verts:
        d = (v.co - centre).length
        if d < DENT_RADIUS:
            k = (1.0 - (d / DENT_RADIUS) ** 2) ** 2
            v.co -= (v.co - head).normalized() * DENT_DEPTH * k
    bm.normal_update()
    # The hollow's steel in shadow, a bright crescent on its lower lip where
    # the light catches the bent metal (only steel: the visor stays black).
    layer = palette.face_layer(bm)
    steel = palette.palette().idx(STEEL) + 1
    hollow, lip = [], []
    for f in bm.faces:
        if f[layer] != steel:
            continue
        off = f.calc_center_median() - centre
        reach = max((v.co - centre).length for v in f.verts)
        if reach < DENT_RADIUS * 0.8:
            hollow.append(f)
        elif reach < DENT_RADIUS * 1.1 and off.z < -0.3 * off.length:
            lip.append(f)
    b.tag(hollow, STEEL_SHADOW)
    b.tag(lip, STEEL_SHINE)
    # Element order out of the bmesh operators above can vary run to run, and
    # `canonical` keeps vertex order: sort the vertices by position so
    # `build-art.sh --check` stays byte-identical.
    bm.verts.index_update()
    rank = [0] * len(bm.verts)
    for r, v in enumerate(sorted(bm.verts, key=_vkey)):
        rank[v.index] = r
    bm.verts.sort(key=lambda v: rank[v.index])
    bm.verts.index_update()
    return b


def _vkey(v):
    """A vertex's position, rounded: a run-independent sort key."""
    return (round(v.co.x, 6), round(v.co.y, 6), round(v.co.z, 6))


def eye_center(s):
    return Vector((s * EYE_X, EYE_FLOOR, EYE_Z))


def build_eye(s, state):
    """One eye in one state, inside the visor slit (origin = the eye's centre):
    a big cream oval with a big black pupil peeking toward the nose, as in the
    targets; wide is bigger with a pinprick pupil."""
    b = Builder()
    bm = b.bm
    c = eye_center(s)
    inward = -s  # towards the nose
    rx, rz = EYE_R
    if state == "open":
        b.tag(blob(bm, c, (rx, 0.03, rz), segments=14, rings=5), WHITE)
        p = c + Vector((inward * 0.012, -0.026, -0.008))
        b.tag(blob(bm, p, (0.026, 0.009, 0.03), segments=10, rings=4), BLACK)
    elif state == "wide":
        b.tag(blob(bm, c, (rx * 1.1, 0.031, rz * 1.08), segments=14, rings=5), WHITE)
        p = c + Vector((0.0, -0.031, 0.004))
        b.tag(blob(bm, p, (0.012, 0.007, 0.013), segments=8, rings=4), BLACK)
    elif state == "blink":
        # A closed eye: a light, gently sagging lid line against the dark slit.
        y = c.y - 0.022
        pts = [(Vector((c.x + dx, y, c.z + 0.002 - 0.014 * (1 - (dx / 0.046) ** 2))), 0.0085)
               for dx in (-0.046, -0.023, 0.0, 0.023, 0.046)]
        b.tag(tube(bm, pts, segments=6), WHITE)
    elif state == "x":
        y = c.y - 0.022
        for sx in (1, -1):
            pts = [(Vector((c.x - sx * 0.036, y, c.z + 0.04)), 0.0095),
                   (Vector((c.x + sx * 0.036, y, c.z - 0.04)), 0.0095)]
            b.tag(tube(bm, pts, segments=6), WHITE)
    else:
        raise ValueError(state)
    return b


# The hat's brim cross-section, (radius, height), inside to outside along the top
# and back underneath: thick and floppy, drooping at its edge.
_BRIM = [(0.14, 1.741), (0.2, 1.736), (0.248, 1.725), (0.28, 1.711), (0.293, 1.7),
         (0.281, 1.694), (0.226, 1.706), (0.14, 1.722)]

# The hat's crown: a tall soft cone, leaning back, that bends over to the
# knight's right (-x) and droops (targets T03-T07). The ring at `TIP_AT` is
# where the floppy tip turns (PivotHatTip): `Hat` is the crown up to it, `HatTip`
# the rest.
_CROWN = [
    ((0.0, 0.0, 1.712), 0.18),
    ((0.0, 0.006, 1.78), 0.152),
    ((-0.002, 0.016, 1.85), 0.124),
    ((-0.006, 0.03, 1.92), 0.097),
    ((-0.014, 0.046, 1.985), 0.074),
    ((-0.028, 0.06, 2.04), 0.057),
    ((-0.052, 0.072, 2.078), 0.045),
    ((-0.088, 0.082, 2.098), 0.035),
    ((-0.13, 0.09, 2.096), 0.027),
    ((-0.166, 0.096, 2.075), 0.019),
    ((-0.19, 0.1, 2.045), 0.013),
    ((-0.2, 0.1, 2.018), 0.007),
    ((-0.203, 0.1, 2.0), 0.002),
]
TIP_AT = 5


def hat_tip_pivot():
    return Vector(_CROWN[TIP_AT][0])


def build_wizard_hat():
    """The tall pointed wizard hat up to its bend: a wide, wavy, floppy brim
    tilted back, the crown and a big gold star. Cosmetic: it takes no hits."""
    b = Builder()
    bm = b.bm
    n = 24
    rings = []
    for r, z in _BRIM:
        pts = []
        flop = (r / 0.293) ** 2  # the outer brim moves most
        for k in range(n):
            a = 2 * math.pi * k / n
            rr = r * (1.0 + 0.03 * math.sin(3 * a + 0.4))
            # The front (-y) turned up off the eyes, the back down, and a wave.
            zz = z + flop * (-0.011 * math.sin(a) - 0.008 * math.sin(2 * a + 1.0))
            pts.append(bm.verts.new((rr * math.cos(a), rr * math.sin(a), zz)))
        rings.append(pts)
    brim = []
    for lo, hi in zip(rings, rings[1:] + rings[:1]):
        brim += shapes.bridge(bm, lo, hi)
    b.tag(brim, PURPLE)
    b.tag(tube(bm, [(Vector(p), r) for p, r in _CROWN[:TIP_AT + 1]], segments=12), PURPLE)
    # The gold star sits on the crown's front, facing out along the surface.
    at, normal = surface_hit(bm, (0.0, -0.5, 1.832), (0.0, 1.0, 0.0))
    b.tag(star_prism(bm, at + normal * 0.005, outer=0.052, inner=0.023, depth=0.012,
                     normal=normal, up=(0.0, 0.0, 1.0)), GOLD)
    return b


def build_wizard_hat_tip():
    """The hat's bent, drooping tip. Its base tucks just inside the crown's top,
    so it can flop about PivotHatTip without opening a gap."""
    b = Builder()
    bm = b.bm
    p, r = Vector(_CROWN[TIP_AT][0]), _CROWN[TIP_AT][1]
    down = (Vector(_CROWN[TIP_AT - 1][0]) - p).normalized()
    path = [(p + down * 0.03, r * 0.99), (p, r * 0.965)]
    path += [(Vector(q), rq) for q, rq in _CROWN[TIP_AT + 1:]]
    b.tag(tube(bm, path, segments=12), PURPLE)
    return b


# The logo's little hat (art/blender/assets/logo.py puts it on the I of PIECED):
# Milestone 2's first, short floppy knight hat, kept as it was so the logo
# stays as Jake scored it. The knight himself wears `build_wizard_hat`.
_LOGO_HAT_FIT = (HEAD_RADIUS + 0.012, HEAD_RADIUS + 0.043)  # soft-clamp start and limit
_LOGO_CROWN = [
    ((0.0, 0.0, 1.718), 0.15),
    ((-0.004, 0.005, 1.76), 0.122),
    ((-0.013, 0.014, 1.798), 0.094),
    ((-0.032, 0.028, 1.826), 0.071),
    ((-0.064, 0.045, 1.838), 0.054),
    ((-0.101, 0.062, 1.83), 0.041),
    ((-0.132, 0.077, 1.81), 0.03),
    ((-0.152, 0.088, 1.784), 0.021),
    ((-0.163, 0.094, 1.76), 0.013),
    ((-0.174, 0.093, 1.745), 0.008),
    ((-0.188, 0.086, 1.747), 0.003),
]


def soft_clamp(bm, center, r0, r1):
    """Squashes vertices farther than r0 from `center` smoothly into radius r1."""
    c = Vector(center)
    for v in bm.verts:
        d = (v.co - c).length
        if d > r0:
            k = r0 + (r1 - r0) * math.tanh((d - r0) / (r1 - r0))
            v.co = c + (v.co - c) * (k / d)


def build_hat():
    """The logo's short floppy wizard hat with a gold star (see above); the
    knight's own hat is `build_wizard_hat`."""
    b = Builder()
    bm = b.bm
    n = 28
    # The brim: a thick, wavy ring; its cross-section runs inside to outside
    # along the top and back underneath.
    section = [(0.12, 1.742), (0.18, 1.736), (0.212, 1.729), (0.229, 1.717),
               (0.221, 1.705), (0.19, 1.708), (0.12, 1.72)]
    rings = []
    for r, z in section:
        pts = []
        for k in range(n):
            a = 2 * math.pi * k / n
            rr = r * (1.0 + 0.025 * math.sin(3 * a + 0.4))
            zz = z - 0.008 * (r / 0.226) * math.sin(2 * a + 1.0)
            pts.append(bm.verts.new((rr * math.cos(a), rr * math.sin(a), zz)))
        rings.append(pts)
    brim = []
    for lo, hi in zip(rings, rings[1:] + rings[:1]):
        brim += shapes.bridge(bm, lo, hi)
    b.tag(brim, PURPLE)
    b.tag(tube(bm, [(Vector(p), r) for p, r in _LOGO_CROWN], segments=16), PURPLE)
    soft_clamp(bm, (0.0, 0.0, HEAD_CENTER), *_LOGO_HAT_FIT)
    at, normal = surface_hit(bm, (0.0, -0.5, 1.772), (0.0, 1.0, 0.0))
    b.tag(star_prism(bm, at + normal * 0.004, outer=0.036, inner=0.016, depth=0.01,
                     normal=normal, up=(0.0, 0.0, 1.0)), GOLD)
    return b


# ---------------------------------------------------------------------------
# Body: torso, robe, cape, arms, legs
# ---------------------------------------------------------------------------

# The coat's bodice, collar to belt: (z, half width, half depth, the front
# opening's half angle in degrees) of a rounded-square cross-section, a little
# outside the breastplate and mail everywhere.
_COAT = [(1.442, 0.11, 0.1, 12.0), (1.39, 0.144, 0.126, 13.0), (1.3, 0.182, 0.154, 14.0),
         (1.2, 0.204, 0.17, 14.0), (1.08, 0.194, 0.168, 14.0), (0.96, 0.186, 0.164, 14.0),
         (0.86, 0.18, 0.158, 15.0), (0.758, 0.172, 0.148, 17.0)]
# Columns round the coat from the opening's left edge: a thin gold trim at each edge.
COAT_US = [0.0] + [0.016 + 0.968 * k / 11 for k in range(12)] + [1.0]


def coat_point(row, u, inset):
    """A point on the coat: `row` indexes `_COAT`, u runs from the front
    opening's left edge round the back to its right edge."""
    z, a, d, gap = _COAT[row]
    g = math.radians(gap)
    th = -math.pi / 2 + g + u * (2 * math.pi - 2 * g)
    c, s = math.cos(th), math.sin(th)
    # A superellipse (exponent 3): squarer than an ellipse, like the breastplate.
    x = math.copysign(abs(c) ** (2 / 3), c)
    y = math.copysign(abs(s) ** (2 / 3), s)
    return Vector(((a - inset) * x, (d - inset) * y, z))


def build_torso():
    """Dark hips, belt and buckle, small steel breastplate with a medallion,
    the purple coat's bodice over it, and the big steel pauldrons (on the torso,
    so the arms can fling up from under them)."""
    b = Builder()
    bm = b.bm

    def outline(a, d, c):
        return rrect(a, d, c, corner=3, side=2, back=3)

    # Dark hips and tights, showing through the robe's open front.
    hips = [(0.545, 0.118, 0.098, 0.07), (0.58, 0.152, 0.121, 0.085),
            (0.742, 0.15, 0.12, 0.075)]
    rings = [ring(bm, outline(a, d, c), z) for z, a, d, c in hips]
    faces = [f for band in loft(bm, rings) for f in band]
    faces += [cap(bm, rings[0], up=False), cap(bm, rings[-1], up=True)]
    b.tag(faces, DARK)

    # Belt with a gold buckle; the robe's top edge tucks under it.
    belt = [ring(bm, outline(a, d, c), z) for z, a, d, c in
            [(0.7, 0.14, 0.11, 0.07), (0.703, 0.164, 0.134, 0.08),
             (0.768, 0.164, 0.134, 0.08), (0.771, 0.14, 0.11, 0.07)]]
    faces = [f for band in loft(bm, belt) for f in band]
    faces += [cap(bm, belt[0], up=False), cap(bm, belt[-1], up=True)]
    b.tag(faces, LEATHER)
    buckle_c = Vector((0.0, -0.138, 0.736))
    outer_pts = rrect(0.045, 0.038, 0.012, corner=3)
    inner_pts = rrect(0.023, 0.016, 0.006, corner=3)
    rot = Matrix.Rotation(math.pi / 2, 3, "X")  # (x, y, z) -> (x, -z, y): local +z faces -y

    def plate_ring(pts, depth):
        return [bm.verts.new(buckle_c + rot @ Vector((x, y, depth))) for x, y in pts]

    fr_o, fr_i = plate_ring(outer_pts, 0.01), plate_ring(inner_pts, 0.01)
    bk_o, bk_i = plate_ring(outer_pts, -0.006), plate_ring(inner_pts, -0.003)
    b.tag(shapes.bridge(bm, fr_o, fr_i) + shapes.bridge(bm, bk_o, fr_o)
          + shapes.bridge(bm, fr_i, bk_i) + shapes.bridge(bm, bk_i, bk_o), GOLD)
    b.tag([cap(bm, bk_i, up=True)], LEATHER)

    # Dark mail at the waist under a small, keeled steel breastplate that runs up
    # under the mantle.
    def keel(amp):
        return lambda a: 1.0 + amp * max(0.0, math.cos(a + math.pi / 2)) ** 40

    chest = [(0.765, 0.138, 0.108, 0.07, 0.0, 0.0), (0.85, 0.142, 0.113, 0.072, -0.002, 0.0),
             (0.852, 0.156, 0.127, 0.08, -0.006, 0.05), (0.878, 0.159, 0.131, 0.08, -0.008, 0.07),
             (0.955, 0.16, 0.134, 0.082, -0.012, 0.08),
             # a lame's edge across the belly: a small outward step
             (0.957, 0.153, 0.127, 0.08, -0.012, 0.08),
             (1.07, 0.157, 0.134, 0.082, -0.014, 0.08),
             (1.2, 0.146, 0.122, 0.078, -0.008, 0.05), (1.33, 0.11, 0.095, 0.06, 0.0, 0.0)]
    rings = [ring(bm, rrect(a, d, c, corner=3, front=(0.45,), side=2, back=2), z, dy=dy,
                  wave=keel(k)) for z, a, d, c, dy, k in chest]
    bands = loft(bm, rings)
    b.tag(bands[0] + [cap(bm, rings[0], up=False)], DARK)
    b.tag([f for band in bands[1:] for f in band] + [cap(bm, rings[-1], up=True)], STEEL)

    # The coat's bodice: purple over the shoulders, round the back and down the
    # sides to the belt, open down the front on the breastplate, the opening's
    # edges trimmed in gold; the helmet sits on its collar.
    outer, lining, sides, top, bottom = sheet(bm, coat_point, range(len(_COAT)), COAT_US,
                                              0.012)
    gold, purple = [], []
    for row in outer:
        for j, f in enumerate(row):
            (gold if j in (0, len(row) - 1) else purple).append(f)
    b.tag(purple + lining + top + bottom, PURPLE)
    b.tag(gold + sides, GOLD)

    # Medallion: a gold ring with a violet gem on a short gold chain.
    med = Vector((0.0, -0.146, 1.175))
    tilt = Matrix.Rotation(math.radians(-10.0), 3, "X")
    b.tag(torus(bm, med, 0.034, 0.009, n_major=12, n_minor=5, rot=tilt), GOLD)
    b.tag(blob(bm, med + Vector((0.0, -0.002, 0.0)), (0.025, 0.011, 0.025), segments=8,
               rings=5, rot=tilt), GEM)
    for s in (1, -1):
        b.tag(tube(bm, [(Vector((s * 0.1, -0.118, 1.262)), 0.006),
                        (Vector((s * 0.05, -0.142, 1.222)), 0.006),
                        (Vector((s * 0.012, -0.15, 1.205)), 0.006)], segments=5), GOLD)

    # Big rounded pauldrons over the shoulder joints.
    for s in (1, -1):
        b.tag(blob(bm, (s * 0.215, 0.0, 1.228), (0.114, 0.124, 0.09), exponent=2.2,
                   segments=12, rings=6, floor=1.163), STEEL)
    return b


def sheet(bm, point, ts, us, thick):
    """A cloth panel with thickness, closed all round (so it outlines cleanly).

    `point(t, u, inset)` places the rows `ts` (top to bottom) and columns `us`;
    the lining is `point(t, u, thick)`. Returns (outer[t][u] faces of the outside,
    lining, side edges, top edge, bottom edge).
    """
    go = [[bm.verts.new(point(t, u, 0.0)) for u in us] for t in ts]
    gi = [[bm.verts.new(point(t, u, thick)) for u in us] for t in ts]
    rows, cols = len(ts), len(us)
    outer = [[None] * (cols - 1) for _ in range(rows - 1)]
    lining, sides, top, bottom = [], [], [], []
    for i in range(rows - 1):
        for j in range(cols - 1):
            outer[i][j] = bm.faces.new((go[i][j], go[i + 1][j], go[i + 1][j + 1], go[i][j + 1]))
            lining.append(bm.faces.new((gi[i][j], gi[i][j + 1], gi[i + 1][j + 1],
                                        gi[i + 1][j])))
        sides.append(bm.faces.new((go[i][0], gi[i][0], gi[i + 1][0], go[i + 1][0])))
        sides.append(bm.faces.new((go[i + 1][-1], gi[i + 1][-1], gi[i][-1], go[i][-1])))
    for j in range(cols - 1):
        top.append(bm.faces.new((go[0][j], go[0][j + 1], gi[0][j + 1], gi[0][j])))
        bottom.append(bm.faces.new((go[-1][j], gi[-1][j], gi[-1][j + 1], go[-1][j + 1])))
    return outer, lining, sides, top, bottom


ROBE_TOP = 0.752  # tucked under the belt
ROBE_HEM = 0.345  # at the back; the front edges ride higher


def robe_point(t, u, inset):
    """The robe: a flared skirt from the belt to below the knees, open at the
    front (the opening widens toward the hem), with soft folds that grow toward
    the hem. u runs from the opening's left edge round the back to its right."""
    gap = math.radians(15.0 + 27.0 * t)  # the opening's half angle
    th = -math.pi / 2 + gap + u * (2 * math.pi - 2 * gap)
    e = t ** 0.85
    a = 0.157 + (0.302 - 0.157) * e
    d = 0.13 + (0.282 - 0.13) * e
    k = 1.0 + 0.05 * t ** 1.2 * math.sin(9.0 * th + 0.3)
    hem = ROBE_HEM + 0.07 * ((1.0 - math.sin(th)) / 2.0) ** 2
    z = ROBE_TOP - t * (ROBE_TOP - hem)
    return Vector(((a * k - inset) * math.cos(th), (d * k - inset) * math.sin(th), z))


def build_robe():
    """The flowing purple robe with a gold hem and gold-trimmed front edges."""
    b = Builder()
    ts = (0.0, 0.2, 0.4, 0.6, 0.8, 0.94, 1.0)  # the last row is the gold hem
    trim = 0.018
    us = [0.0] + [trim + (1.0 - 2 * trim) * k / 14 for k in range(15)] + [1.0]
    outer, lining, sides, top, bottom = sheet(b.bm, robe_point, ts, us, 0.012)
    gold, purple = [], []
    for i, row in enumerate(outer):
        for j, f in enumerate(row):
            (gold if i == len(outer) - 1 or j in (0, len(row) - 1) else purple).append(f)
    b.tag(purple + lining + top, PURPLE)
    b.tag(gold + sides + bottom, GOLD)
    return b


def cape_point(t, u, inset):
    """The cape: from the shoulders, round the back, flaring wide behind the robe."""
    z = 1.29 - t * (1.29 - 0.33)
    a = 0.158 + 0.172 * t ** 1.3
    d = 0.16 + 0.148 * t + 0.016 * math.sin(math.pi * t)  # clears the robe
    a0 = math.radians(24.0 - 14.0 * t)
    th = a0 + (math.pi - 2 * a0) * u
    k = 1.0 + 0.055 * t * (0.5 + 0.5 * math.sin(7 * math.pi * u))  # folds bulge outwards
    return Vector(((a * k - inset) * math.cos(th), (d * k - inset) * math.sin(th), z))


def build_cape():
    """A big purple cape hanging from the nape over the robe, with a gold hem."""
    b = Builder()
    ts = (0.0, 0.14, 0.28, 0.42, 0.56, 0.7, 0.84, 0.955, 1.0)  # the last row is the gold hem
    us = [k / 10 for k in range(11)]
    outer, lining, sides, top, bottom = sheet(b.bm, cape_point, ts, us, 0.013)
    b.tag([f for row in outer[:-1] for f in row] + lining + sides + top, PURPLE)
    b.tag(outer[-1] + bottom, GOLD)
    return b


def build_arm(s):
    """A purple robe sleeve belled at the elbow with a gold trim, a steel
    vambrace, a flared gauntlet cuff and a big open cartoon glove: a chunky palm,
    four sausage fingers and a thumb, its back to the front. The hand reaches as
    far out as the capsule's 5 cm tolerance allows."""
    b = Builder()
    bm = b.bm

    def m(x, y, z):
        return Vector((s * x, y, z))

    b.tag(tube(bm, [(m(0.212, 0.0, 1.2), 0.066), (m(0.238, 0.006, 1.08), 0.07),
                    (m(0.25, 0.004, 1.004), 0.084)], segments=10), PURPLE)
    b.tag(tube(bm, [(m(0.251, 0.004, 1.008), 0.086), (m(0.253, 0.0, 0.984), 0.088)],
               segments=10), GOLD)
    b.tag(tube(bm, [(m(0.254, -0.004, 0.992), 0.057), (m(0.262, -0.02, 0.87), 0.063)],
               segments=8), STEEL)
    b.tag(tube(bm, [(m(0.264, -0.024, 0.878), 0.066), (m(0.266, -0.03, 0.848), 0.087),
                    (m(0.266, -0.034, 0.826), 0.093)], segments=12), STEEL)
    hand = (0.266, -0.034, 0.752)
    b.tag(blob(bm, m(*hand), (0.076, 0.046, 0.07), exponent=2.4, segments=12, rings=7),
          STEEL)
    hx, hy = hand[0], hand[1]
    for k, dx in enumerate((-0.051, -0.017, 0.017, 0.051)):
        length = 0.088 if k in (1, 2) else 0.075
        base = m(hx + dx, hy, 0.712)
        mid = m(hx + dx * 1.1, hy + 0.004, 0.712 - length * 0.55)
        tip = m(hx + dx * 1.22, hy + 0.016, 0.712 - length)
        end = tip + (tip - mid).normalized() * 0.013
        b.tag(tube(bm, [(base, 0.022), (mid, 0.022), (tip, 0.019), (end, 0.008)],
                   segments=6), STEEL)
    thumb = [(m(hx - 0.056, hy - 0.018, 0.768), 0.023), (m(hx - 0.076, hy - 0.046, 0.742), 0.022),
             (m(hx - 0.083, hy - 0.068, 0.717), 0.02)]
    end = thumb[-1][0] + (thumb[-1][0] - thumb[-2][0]).normalized() * 0.013
    b.tag(tube(bm, thumb + [(end, 0.009)], segments=6), STEEL)
    return b


def build_leg(s):
    """Dark thigh, winged knee cop, steel greave and a big round-toed boot."""
    b = Builder()
    bm = b.bm

    def m(x, y, z):
        return Vector((s * x, y, z))

    b.tag(tube(bm, [(m(0.108, 0.0, 0.66), 0.058), (m(0.128, 0.0, 0.5), 0.058),
                    (m(0.144, 0.0, 0.385), 0.055)], segments=8), DARK)
    b.tag(blob(bm, m(0.147, -0.034, 0.372), (0.058, 0.036, 0.052), segments=10, rings=5),
          STEEL)
    b.tag(blob(bm, m(0.196, -0.008, 0.372), (0.016, 0.05, 0.058), segments=8, rings=4),
          STEEL)
    b.tag(tube(bm, [(m(0.147, 0.0, 0.372), 0.052), (m(0.156, 0.0, 0.29), 0.058),
                    (m(0.162, 0.0, 0.215), 0.07)], segments=10), STEEL)
    b.tag(tube(bm, [(m(0.162, 0.0, 0.235), 0.07), (m(0.164, 0.0, 0.208), 0.09),
                    (m(0.165, -0.004, 0.168), 0.095)], segments=12), STEEL)

    def toe_bulge(p):
        # Wider and taller towards the toe (front, -y), flatter at the heel.
        f = max(0.0, -p.y)
        return Vector((p.x * (1.0 + 0.1 * f), p.y, p.z * (1.0 + 0.12 * f)))

    boot = blob(bm, m(0.167, -0.064, 0.08), (0.09, 0.171, 0.088), exponent=2.4,
                segments=16, rings=10, floor=0.0, shape=toe_bulge)
    b.tag(boot, STEEL)
    b.tag([f for f in boot if f.calc_center_median().z < 0.012], DARK)  # the sole
    return b


# ---------------------------------------------------------------------------
# Assembly
# ---------------------------------------------------------------------------

EYE_STATES = (("Eye", "open"), ("EyeWide", "wide"), ("EyeBlink", "blink"), ("EyeX", "x"))


def pivot(name, parent, world, parent_world):
    return scene.make_attach(name, parent, Vector(world) - Vector(parent_world))


def build_hat_parts(parent, parent_origin):
    """`Hat` (origin at HAT_BASE, the brim's base) under `parent`, with the floppy
    `HatTip` under `PivotHatTip` at the crown's bend."""
    hat = build_wizard_hat().finish("Hat", parent, HAT_BASE, parent_origin)
    bend = hat_tip_pivot()
    tip = pivot("PivotHatTip", hat, bend, HAT_BASE)
    build_wizard_hat_tip().finish("HatTip", tip, bend, bend)
    return hat


def build_knight(root, check=True):
    origin = Vector((0.0, 0.0, 0.0))
    for side, s in (("L", 1), ("R", -1)):
        hip = mirror(HIP, s)
        leg = pivot(f"PivotLeg{side}", root, hip, origin)
        build_leg(s).finish(f"Boot{side}", leg, hip, hip)
    torso = build_torso().finish("Torso", root, TORSO_ORIGIN, origin)
    for side, s in (("L", 1), ("R", -1)):
        sh = mirror(SHOULDER, s)
        arm = pivot(f"PivotArm{side}", torso, sh, TORSO_ORIGIN)
        build_arm(s).finish(f"Gauntlet{side}", arm, sh, sh)
    cape = pivot("PivotCape", torso, NAPE, TORSO_ORIGIN)
    build_cape().finish("Cape", cape, NAPE, NAPE)
    robe = pivot("PivotRobe", torso, WAIST, TORSO_ORIGIN)
    build_robe().finish("Robe", robe, WAIST, WAIST)
    head = pivot("PivotHead", torso, NECK, TORSO_ORIGIN)
    helmet = build_helmet().finish("Helmet", head, NECK, NECK, sharp_deg=50.0)
    for prefix, state in EYE_STATES:
        for side, s in (("L", 1), ("R", -1)):
            eye = build_eye(s, state).finish(f"{prefix}{side}", helmet, eye_center(s), NECK,
                                             sharp_deg=89.0)
            eye.hide_render = state != "open"
    hat = pivot("PivotHat", head, HAT_BASE, NECK)
    build_hat_parts(hat, HAT_BASE)
    if check:
        check_fit(root)


# ---------------------------------------------------------------------------
# Hitbox fit
# ---------------------------------------------------------------------------

def is_head_part(name):
    return name in HEAD_PARTS or name.startswith("Eye")


def is_cosmetic(name):
    return name in COSMETIC_PARTS


def capsule_overshoot(p):
    """How far a point lies outside the body capsule (negative = inside)."""
    radial = math.hypot(p.x, p.y)
    if p.z < FOOT_BAND:  # boots on the ground: the capsule counts as a cylinder here
        return radial - BODY_RADIUS
    z = min(max(p.z, BODY_BOTTOM + BODY_RADIUS), BODY_TOP - BODY_RADIUS)
    return math.hypot(radial, p.z - z) - BODY_RADIUS


def sphere_overshoot(p):
    return (p - Vector((0.0, 0.0, HEAD_CENTER))).length - HEAD_RADIUS


def fit_report(root):
    """{part: worst overshoot (m)} for every non-cosmetic mesh part against its
    own hitbox."""
    scene.update()
    out = {}
    for obj in scene.descendants(root):
        if obj.type != "MESH" or is_cosmetic(obj.name):
            continue
        test = sphere_overshoot if is_head_part(obj.name) else capsule_overshoot
        mw = obj.matrix_world
        out[obj.name] = max(test(mw @ v.co) for v in obj.data.vertices)
    return out


def silhouette_gaps(root, cell=0.01):
    """How far each hitbox's silhouette sticks out past the model's (the fill rule).

    Rasterises the visible, non-cosmetic parts seen from the front (along y) and
    the side (along x) on a `cell` grid and returns {(view, hitbox): (gap m, (u, z))}:
    the worst distance from a point of the hitbox's silhouette to the model's.
    """
    import numpy as np

    scene.update()
    u0, v0 = -0.5, -0.05
    nu, nv = int(1.0 / cell), int(2.0 / cell)
    uc = u0 + (np.arange(nu) + 0.5) * cell
    vc = v0 + (np.arange(nv) + 0.5) * cell
    out = {}
    for view, axis in (("front", 0), ("side", 1)):
        mask = np.zeros((nv, nu), dtype=bool)
        for obj in scene.descendants(root):
            if obj.type != "MESH" or obj.hide_render or is_cosmetic(obj.name):
                continue
            mesh = obj.data
            mesh.calc_loop_triangles()
            mw = obj.matrix_world
            pts = np.array([((mw @ v.co)[axis], (mw @ v.co)[2]) for v in mesh.vertices])
            for tri in mesh.loop_triangles:
                a, b, c = pts[list(tri.vertices)]
                lo, hi = np.minimum(np.minimum(a, b), c), np.maximum(np.maximum(a, b), c)
                i0, i1 = max(0, int((lo[0] - u0) / cell)), min(nu - 1, int((hi[0] - u0) / cell))
                j0, j1 = max(0, int((lo[1] - v0) / cell)), min(nv - 1, int((hi[1] - v0) / cell))
                d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1])
                if abs(d) < 1e-12 or i1 < i0 or j1 < j0:
                    continue
                uu, vv = np.meshgrid(uc[i0:i1 + 1], vc[j0:j1 + 1])
                l1 = ((b[1] - c[1]) * (uu - c[0]) + (c[0] - b[0]) * (vv - c[1])) / d
                l2 = ((c[1] - a[1]) * (uu - c[0]) + (a[0] - c[0]) * (vv - c[1])) / d
                mask[j0:j1 + 1, i0:i1 + 1] |= (l1 >= 0) & (l2 >= 0) & (1 - l1 - l2 >= 0)
        jj, ii = np.nonzero(mask)
        model = np.stack([uc[ii], vc[jj]], axis=1)
        uu, vv = np.meshgrid(uc, vc)
        lo, hi = BODY_BOTTOM + BODY_RADIUS, BODY_TOP - BODY_RADIUS
        body = uu ** 2 + (vv - np.clip(vv, lo, hi)) ** 2 <= BODY_RADIUS ** 2
        head = uu ** 2 + (vv - HEAD_CENTER) ** 2 <= HEAD_RADIUS ** 2
        for name, hit in (("body", body), ("head", head)):
            pts = np.stack([uu[hit], vv[hit]], axis=1)
            best = np.empty(len(pts))
            for k in range(0, len(pts), 1024):
                chunk = pts[k:k + 1024]
                best[k:k + 1024] = np.sqrt(((chunk[:, None, :] - model[None, :, :]) ** 2)
                                           .sum(-1)).min(1)
            w = int(best.argmax())
            out[(view, name)] = (float(best[w]), tuple(round(float(x), 3) for x in pts[w]))
    return out


def check_fit(root):
    worst = fit_report(root)
    bad = {k: round(v, 4) for k, v in worst.items() if v > TOLERANCE}
    if bad:
        raise ValueError(f"knight parts poke out of their hitboxes by more than "
                         f"{TOLERANCE} m: {bad}")


def build_knight_hat(root):
    """The knight's hat on its own, for the prop that drops off his head when he
    is eliminated (src/fx/hat.rs, target T08). The same `Hat` and `HatTip`
    geometry as on the knight, with its pivot at the base of the brim
    (`PivotHat`), so the prop starts exactly where the hat sat."""
    build_hat_parts(root, HAT_BASE)


def build_knight_helmet_dent(root):
    """The dented helmet (M4, D105) on its own: `HelmetDent`, the same helmet
    with the headshot's dent, with its origin at the neck like `Helmet`, so
    the game shows it in the helmet's place (src/fx/armor.rs). Its ambient
    occlusion is baked against the rest of the knight (hat, torso, mantle),
    so it shades exactly like the helmet it swaps with; those occluders are
    dropped before export (`pieced_occluder`)."""
    build_knight(root, check=False)
    helmet = next(o for o in scene.descendants(root) if o.name == "Helmet")
    for obj in [helmet] + scene.descendants(helmet):
        bpy.data.objects.remove(obj, do_unlink=True)
    for obj in scene.descendants(root):
        obj["pieced_occluder"] = True
    origin = Vector((0.0, 0.0, 0.0))
    build_dented_helmet().finish(DENT_PART, root, NECK, origin, sharp_deg=50.0)


DENT_PART = "HelmetDent"

ASSETS = [
    Asset("knight", "knight", build_knight, "the goofy armoured knight-wizard enemy"),
    # Budgeted as part of the knight.
    Asset("knight_hat", "knight", build_knight_hat,
          "the knight's tall floppy hat, pivot at its base"),
    # Shown instead of the knight's helmet after a headshot (never both).
    Asset("knight_helmet_dent", "knight", build_knight_helmet_dent,
          "the knight's helmet dented by a headshot, origin at the neck"),
]


# ---------------------------------------------------------------------------
# Review renders (run this file directly; see the module docstring)
# ---------------------------------------------------------------------------

def _review(out_dir):
    import numpy as np

    from lib import bitmap, export
    from lib import preview as pv

    scene.reset()
    root = scene.make_root("knight")
    build_knight(root, check=False)
    export.finish_meshes(root)
    tris = sum(shapes.triangles(o) for o in scene.descendants(root) if o.type == "MESH")
    print(f"KNIGHT triangles {tris}")
    tri = {o.name: shapes.triangles(o) for o in scene.descendants(root) if o.type == "MESH"}
    for name, v in sorted(fit_report(root).items()):
        print(f"KNIGHT fit {name:10s} {'head' if is_head_part(name) else 'body'} "
              f"overshoot {v * 100:+.1f} cm, {tri[name]} tris")
    for name in COSMETIC_PARTS:
        obj = bpy.data.objects[name]
        top = max((obj.matrix_world @ v.co).z for v in obj.data.vertices)
        print(f"KNIGHT cosmetic {name:7s} (no fit) top at {top:.3f} m, {tri[name]} tris")

    for (view, name), (gap, where) in sorted(silhouette_gaps(root).items()):
        print(f"KNIGHT fill {view:5s} {name}: hitbox sticks out {gap * 100:.1f} cm at {where}")

    p = palette.palette()
    tint = pv._shadow_tint(p)
    sc = bpy.context.scene
    size = 640
    sc.render.engine = "BLENDER_EEVEE"
    sc.render.resolution_x = size
    sc.render.resolution_y = size
    sc.render.film_transparent = True
    sc.render.image_settings.file_format = "PNG"
    sc.render.image_settings.color_mode = "RGBA"
    sc.eevee.taa_render_samples = 16
    sc.view_settings.view_transform = "Standard"
    sc.view_settings.look = "None"
    world = bpy.data.worlds.new("ReviewWorld")
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs["Strength"].default_value = 0.0
    sc.world = world
    toon = pv._toon_material()
    ink = pv._ink_material(p)
    meshes = [o for o in scene.descendants(root) if o.type == "MESH"]
    for obj in meshes:
        pv._shadow_attribute(obj.data, p, tint)
        obj.data.materials.clear()
        obj.data.materials.append(toon)
        obj.data.materials.append(ink)
        mod = obj.modifiers.new("Outline", "SOLIDIFY")
        mod.thickness = 0.006
        mod.offset = 1.0
        mod.use_flip_normals = True
        mod.use_rim = False
        mod.material_offset = 1
    ground = pv._ground(p, 0.9, tint)
    pv._shadow_attribute(ground.data, p, tint)
    ground.data.materials.append(toon)
    sun = bpy.data.objects.new("Key", bpy.data.lights.new("Key", "SUN"))
    sun.data.energy = math.pi
    sun.rotation_euler = (-pv.KEY_DIR).to_track_quat("-Z", "Y").to_euler()
    sc.collection.objects.link(sun)
    cam = bpy.data.objects.new("Cam", bpy.data.cameras.new("Cam"))
    sc.collection.objects.link(cam)
    sc.camera = cam

    top = np.array(p.srgb["galaxy_deep"], dtype=np.float32)
    bottom = np.array(p.srgb["cloud"], dtype=np.float32) * 0.55 + top * 0.45
    ramp = np.linspace(0.0, 1.0, size, dtype=np.float32)[:, None, None]
    sky = np.broadcast_to(top[None, None, :] * (1 - ramp) + bottom[None, None, :] * ramp,
                          (size, size, 3))

    def shoot(az, el, target, ortho=None, dist=4.5, lens=50.0):
        a, e = math.radians(az), math.radians(el)
        cam.location = Vector(target) + dist * Vector((math.sin(a) * math.cos(e),
                                                       -math.cos(a) * math.cos(e), math.sin(e)))
        cam.rotation_euler = (Vector(target) - cam.location).to_track_quat("-Z", "Y").to_euler()
        cam.data.type = "ORTHO" if ortho else "PERSP"
        if ortho:
            cam.data.ortho_scale = ortho
        cam.data.lens = lens
        path = os.path.join(out_dir, "_knight_tmp.png")
        sc.render.filepath = path
        bpy.ops.render.render(write_still=True)
        img = bitmap.over(bitmap.load_png(path), sky)
        os.remove(path)
        return img

    os.makedirs(out_dir, exist_ok=True)
    mid = (0.0, 0.0, 0.94)
    tall = (0.0, 0.0, 1.06)  # frames the hat too
    views = [shoot(0, 4, tall, ortho=2.3), shoot(90, 4, tall, ortho=2.3),
             shoot(180, 8, tall, ortho=2.3), shoot(-140, 18, tall, dist=5.6)]
    bitmap.save_png(np.concatenate(views, axis=1), os.path.join(out_dir, "knight_views.png"))

    # A rough hit take (the game's pose comes from src/knight.rs; this only
    # checks how the parts read in it): arms flung up, a leg kicked, leaning back.
    pose = {"PivotArmL": (0.0, -2.35, 0.0), "PivotArmR": (0.25, 1.45, 0.0),
            "PivotLegL": (-0.7, -0.15, 0.0), "PivotLegR": (0.12, 0.0, 0.0),
            "Torso": (-0.28, 0.0, 0.05), "PivotHead": (-0.15, 0.0, 0.0),
            "PivotRobe": (-0.2, 0.0, 0.0), "PivotCape": (0.35, 0.0, 0.0),
            "PivotHatTip": (0.25, 0.2, 0.0)}
    for name, rot in pose.items():
        bpy.data.objects[name].rotation_euler = rot
    posed = [shoot(-25, 6, tall, dist=5.6), shoot(0, 4, tall, ortho=2.3)]
    for name in pose:
        bpy.data.objects[name].rotation_euler = (0.0, 0.0, 0.0)
    bitmap.save_png(np.concatenate(posed, axis=1), os.path.join(out_dir, "knight_hit.png"))

    # Hitbox overlay: orthographic front and side views with the hitbox outlines,
    # the +5 cm tolerance and the -10 cm fill line drawn on in image space.
    span = 2.05  # metres across the image
    centre = Vector(mid)

    def px(u, z):  # (horizontal metres, height) -> pixel (col, row)
        return (size / 2 + u / span * size, size / 2 - (z - centre.z) / span * size)

    def stadium(inflate):
        r = BODY_RADIUS + inflate
        lo, hi = BODY_BOTTOM + BODY_RADIUS, BODY_TOP - BODY_RADIUS
        pts = []
        for k in range(181):
            a = math.pi * k / 180
            pts.append((r * math.cos(a), hi + r * math.sin(a)))
        for k in range(181):
            a = math.pi + math.pi * k / 180
            pts.append((r * math.cos(a), lo + r * math.sin(a)))
        return pts + pts[:1]

    def circle(inflate):
        r = HEAD_RADIUS + inflate
        return [(r * math.cos(2 * math.pi * k / 360), HEAD_CENTER + r * math.sin(2 * math.pi * k / 360))
                for k in range(361)]

    def draw(img, pts, rgb, dash=0, t=1):
        for k, ((u0, z0), (u1, z1)) in enumerate(zip(pts, pts[1:])):
            if dash and (k // dash) % 2:
                continue
            (c0, r0), (c1, r1) = px(u0, z0), px(u1, z1)
            n = int(max(abs(c1 - c0), abs(r1 - r0))) + 1
            for i in range(n + 1):
                c = round(c0 + (c1 - c0) * i / n)
                r = round(r0 + (r1 - r0) * i / n)
                bitmap.fill(img, c - t + 1, r - t + 1, c + t, r + t, rgb)

    panels = []
    for az in (0, 90):
        img = shoot(az, 0, mid, ortho=span, dist=6.0)
        for shape in (stadium, circle):
            draw(img, shape(0.0), (1.0, 0.85, 0.1), t=2)
            draw(img, shape(TOLERANCE), (1.0, 0.3, 0.3), dash=6)
            draw(img, shape(-0.10), (0.3, 1.0, 1.0), dash=6)
        panels.append(img)
    bitmap.save_png(np.concatenate(panels, axis=1), os.path.join(out_dir, "knight_hitbox.png"))

    # The four eye states, close up.
    eyes = {o.name: o for o in meshes if o.name.startswith("Eye")}
    shots = []
    for prefix, _ in EYE_STATES:
        for name, o in eyes.items():
            o.hide_render = not (name[:-1] == prefix)
        shots.append(shoot(-18, 8, (0.0, 0.0, 1.66), ortho=0.62, dist=3.0))
    bitmap.save_png(np.concatenate(shots, axis=1), os.path.join(out_dir, "knight_eyes.png"))
    print(f"KNIGHT review renders -> {out_dir}")


if __name__ == "__main__":
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    repo = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", ".."))
    _review(os.path.abspath(argv[0]) if argv else os.path.join(repo, "art", "previews"))
