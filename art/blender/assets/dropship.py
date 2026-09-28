"""The knights' drop ship (docs/M3-SPEC.md → Ship arrivals, D82).

A bigger, chunkier cousin of the far view's starship (`far.py`'s `ship`), built
for close range and the toon look: a fat steel hull with a knight-purple belly
and dorsal stripe, swept purple wings with brass leading edges and red tips, a
purple tail fin, a blue crystal canopy, warm porthole windows, twin iron
engines with violet exhausts, and underneath, in a brass socket, the big violet
hull crystal the beam pours from. About 16 m long and 15 m across the wings: it
hovers 12–16 m over the island and has to read as the knights' ride.

Parts and attach points (Blender axes: the ship's nose points along -Y, its
front; +Z up; the origin is the middle of the hull):

    Hull     hull, canopy, wings, fin, engines and trim in one mesh
    Crystal  the hull crystal, pointing down (the game swaps in a glowing
             violet material, `waves::ships_visuals`)
    Beam     an empty at the crystal's lowest point, where the beam starts:
             `waves::ships::CRYSTAL_DROP` below the origin (a test keeps the
             two in step)

≤ 2,000 triangles.
"""

import math

import bmesh
from mathutils import Matrix, Vector

from assets import guns
from lib import palette, scene, shapes
from lib.ao import AoSettings
from lib.registry import BUDGETS, Asset

BUDGETS.setdefault("dropship", 2000)

STEEL = "knight_steel"
PURPLE = "knight_purple"
BRASS = guns.BRASS
IRON = "gun_iron"
RED = "ghost_red"
CANOPY = "crystal_blue"
WINDOW = "glass_yellow"
EXHAUST = "crystal_violet"
CRYSTAL = "crystal_violet"
CRYSTAL_LIGHT = "glass_violet"

# The hull's cross-section is squashed to this height fraction of its width.
FLAT = 0.62
# The crystal: its length (tip to tip), radius, and its lowest point below the
# origin (keep equal to `waves::ships::CRYSTAL_DROP`).
CRYSTAL_LENGTH = 2.6
CRYSTAL_RADIUS = 0.75
CRYSTAL_DROP = 2.9
SEG = 12


def hull():
    """The fuselage: a loft along -Y, squashed flat, steel above and below (the
    belly is what the island sees), a purple band round its flanks and a
    purple stripe along the spine."""
    bm = palette.new_bmesh()
    path = [((0.0, -8.2, 0.05), 0.15), ((0.0, -7.2, 0.1), 1.0), ((0.0, -5.2, 0.15), 1.9),
            ((0.0, -2.5, 0.1), 2.5), ((0.0, 1.0, 0.0), 2.6), ((0.0, 4.5, 0.05), 2.1),
            ((0.0, 7.0, 0.2), 1.3), ((0.0, 7.8, 0.25), 0.6)]
    _, sides, caps = shapes.loft(bm, [(c, (lambda r: lambda a: r)(r)) for c, r in path], SEG,
                                 angle0=math.pi / SEG)
    bmesh.ops.scale(bm, vec=(1.0, 1.0, FLAT), verts=list({v for f in sides + caps for v in f.verts}))
    for f in sides + caps:
        f.normal_update()
        c = f.calc_center_median()
        n = f.normal
        if abs(n.z) < 0.4:
            name = PURPLE
        elif n.z > 0.93 and abs(c.x) < 0.7:
            name = PURPLE
        else:
            name = STEEL
        palette.tag(bm, [f], name)
    return guns.finish(bm)


def canopy():
    """A low crystal dome over the nose."""
    profile = [(0.0, 0.0), (0.0, 1.05), (0.35, 0.95), (0.6, 0.62), (0.72, 0.0)]
    obj = guns.lathe((0.0, 0.0, 0.0), (0.0, 0.0, 1.0), profile, CANOPY, seg=10)
    obj.data.transform(Matrix.Translation(Vector((0.0, -4.6, 0.85)))
                       @ Matrix.Diagonal(Vector((1.0, 1.9, 1.0, 1.0))))
    return obj


def slab(points, thickness, top, edge, lead=None):
    """A flat plate through the outline `points` (x, y, z; counter-clockwise
    seen from above), `thickness` thick: `top` above and below, `edge` round
    its rim, and `lead` on the edge from points[0] to points[1] if given."""
    bm = palette.new_bmesh()
    up = [bm.verts.new((x, y, z + thickness / 2)) for x, y, z in points]
    down = [bm.verts.new((x, y, z - thickness / 2)) for x, y, z in points]
    faces = [bm.faces.new(up), bm.faces.new(list(reversed(down)))]
    palette.tag(bm, faces, top)
    rim = shapes.bridge(bm, down, up)
    palette.tag(bm, rim, edge)
    if lead is not None:
        palette.tag(bm, rim[:1], lead)
    return guns.finish(bm)


def wing(side):
    """A swept wing on `side` (+1 right, -1 left), with a red tip block."""
    s = side
    pts = [(s * 2.2, -2.6, -0.35), (s * 7.6, 2.2, -0.75), (s * 7.6, 4.2, -0.75),
           (s * 2.2, 4.4, -0.35)]
    if s < 0:
        pts.reverse()
    w = slab(pts, 0.42, PURPLE, BRASS)
    tip = guns.rbox((s * 7.6, 3.2, -0.75), (0.7, 2.4, 0.8), RED, bevel=0.12)
    light = guns.shard(0.9, 0.2, 0.3, EXHAUST, CRYSTAL_LIGHT, sides=5,
                       rotation=(0.0, 0.0, 0.0))
    light.data.transform(Matrix.Translation(Vector((s * 7.6, 1.6, -0.75))))
    return [w, tip, light]


def fin():
    """The tail fin: a swept purple blade with a brass cap."""
    pts = [(0.0, 3.2, 1.2), (0.0, 7.4, 1.2), (0.0, 8.4, 3.9), (0.0, 6.6, 3.9)]
    bm = palette.new_bmesh()
    left = [bm.verts.new((-0.2, y, z)) for _, y, z in pts]
    right = [bm.verts.new((0.2, y, z)) for _, y, z in pts]
    faces = [bm.faces.new(list(reversed(left))), bm.faces.new(right)]
    palette.tag(bm, faces, PURPLE)
    rim = shapes.bridge(bm, left, right)
    palette.tag(bm, rim, PURPLE)
    palette.tag(bm, rim[2:3], BRASS)
    return [guns.finish(bm)]


def engine(side):
    """An iron engine pod along the hull's flank, a brass band, a violet exhaust."""
    x = side * 2.7
    profile = [(0.0, 0.2), (0.4, 0.85), (1.6, 1.0), (4.2, 0.95), (4.6, 0.8), (4.6, 0.55),
               (4.3, 0.55), (4.3, 0.1)]
    colors = [IRON, IRON, IRON, IRON, IRON, IRON, EXHAUST, EXHAUST]
    pod = guns.lathe((x, 2.9, -0.55), (0.0, 1.0, 0.0), profile, colors, seg=10)
    band = guns.hoop((x, 3.9, -0.55), (0.0, 1.0, 0.0), 1.02, 0.35, 0.12, BRASS, seg=10)
    return [pod, band]


def portholes():
    """Two warm round windows along each flank."""
    out = []
    for s in (-1, 1):
        for y in (-2.2, -0.2):
            win = guns.lathe((s * 2.3, y, 0.3), (s, 0.0, 0.0),
                             [(0.0, 0.0), (0.0, 0.34), (0.22, 0.3), (0.26, 0.0)], WINDOW, seg=8)
            out.append(win)
            out.append(guns.hoop((s * 2.32, y, 0.3), (s, 0.0, 0.0), 0.38, 0.1, 0.08, BRASS,
                                 seg=8))
    return out


def belly():
    """The brass socket the crystal hangs from, and a keel strake."""
    socket_z = -1.35
    profile = [(0.0, 1.3), (0.22, 1.42), (0.42, 1.05), (0.5, 0.3), (0.0, 0.3)]
    socket = guns.lathe((0.0, 0.0, socket_z + 0.2), (0.0, 0.0, -1.0), profile, BRASS, seg=SEG)
    keel = guns.rbox((0.0, 3.6, -1.35), (0.5, 4.0, 0.5), PURPLE, bevel=0.1)
    return [socket, keel]


def build_dropship(root):
    parts = [hull(), canopy()] + wing(1) + wing(-1) + fin() + engine(1) + engine(-1)
    parts += portholes() + belly()
    guns.part("Hull", parts, root, sharp_deg=40.0)
    centre = CRYSTAL_DROP - CRYSTAL_LENGTH / 2
    crystal = guns.shard(CRYSTAL_LENGTH, CRYSTAL_RADIUS, 0.62, CRYSTAL, CRYSTAL_LIGHT, sides=6,
                         twist=0.3)
    # The shard lies along Y; stand it up along Z.
    crystal.data.transform(Matrix.Translation(Vector((0.0, 0.0, -centre)))
                           @ Matrix.Rotation(math.pi / 2, 4, "X"))
    guns.part("Crystal", [crystal], root, location=(0.0, 0.0, -centre), flat=True, shine=False)
    scene.make_attach("Beam", root, (0.0, 0.0, -CRYSTAL_DROP))


ASSETS = [
    Asset("dropship", "dropship", build_dropship,
          "the knights' drop ship, ~16 m, violet hull crystal underneath",
          ao=AoSettings(radius=1.0, rays=32, open_parts=("Crystal",))),
]
