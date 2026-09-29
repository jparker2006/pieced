"""The knights' drop ship (docs/M3-SPEC.md → Ship arrivals, D82; M4 art:
targets M4-V1, V7).

A gilded flying saucer: a broad brass disc with a bright rolled rim, a ring
of glowing blue portholes round it, a blue crystal canopy on top and a
knight-purple underside (what the island sees), and underneath, in a brass
socket, the big violet hull crystal the beam pours from. About 14.6 m
across: it hovers 12–16 m over the island and has to read as the knights'
ride from far away (round, gold, blue lights), not as a jet.

Parts and attach points (Blender axes: the ship's front is -Y; +Z up; the
origin is the middle of the disc):

    Hull     disc, rim, canopy, portholes and socket in one mesh
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

# M4 art (targets M4-V1, V7): a gilded airship. Brass hull, a dark bronze
# belly, knight-purple flanks and wings, glowing blue windows.
STEEL = "gun_brass"
BELLY = "gun_brass_dark"
PURPLE = "knight_purple"
BRASS = guns.BRASS
IRON = "gun_iron"
RED = "gun_brass_light"
CANOPY = "crystal_blue"
WINDOW = "crystal_blue"
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


RIM = 7.3  # the disc's radius at its rim (m)
PORTHOLES = 12


def disc():
    """The saucer: a purple underside dish, a bright brass rim band, a brass
    top sloping up to the canopy's collar. A surface of revolution about Z
    (its inner, hidden faces close it)."""
    # The underside (what the island sees, M4-V1): bronze panel rings and a
    # glowing blue emitter ring round the crystal's socket, on purple.
    profile = [(-1.25, 1.45), (-1.2, 2.2), (-1.12, 2.55), (-0.95, 4.0), (-0.85, 4.45),
               (-0.45, 6.4), (-0.1, 7.1), (0.25, RIM),
               (0.55, 7.26), (0.75, 6.7), (1.3, 5.2), (1.4, 4.9), (1.55, 4.8),
               (2.0, 3.2), (2.15, 2.8), (2.2, 0.25), (-1.25, 0.25)]
    colors = [BELLY, WINDOW, PURPLE, BELLY, PURPLE, BELLY, RED, RED, BELLY, STEEL, BELLY,
              STEEL, STEEL, BELLY, STEEL, PURPLE, PURPLE]
    return guns.lathe((0.0, 0.0, 0.0), (0.0, 0.0, 1.0), profile, colors, seg=24)


def canopy():
    """A blue crystal dome on top."""
    profile = [(0.0, 0.01), (0.0, 2.95), (0.75, 2.6), (1.25, 1.75), (1.5, 0.01)]
    return guns.lathe((0.0, 0.0, 2.08), (0.0, 0.0, 1.0), profile, CANOPY, seg=24)


def portholes():
    """A ring of glowing blue portholes round the rim band."""
    out = []
    for k in range(PORTHOLES):
        a = 2 * math.pi * (k + 0.5) / PORTHOLES
        d = Vector((math.cos(a), math.sin(a), 0.0))
        c = d * (RIM - 0.03) + Vector((0.0, 0.0, 0.22))
        out.append(guns.lathe(c, d, [(0.0, 0.0), (0.0, 0.2), (0.07, 0.17), (0.09, 0.0)],
                              WINDOW, seg=8))
    return out


def belly():
    """The brass socket the crystal hangs from, and a keel strake."""
    socket_z = -1.35
    profile = [(0.0, 1.3), (0.22, 1.42), (0.42, 1.05), (0.5, 0.3), (0.0, 0.3)]
    socket = guns.lathe((0.0, 0.0, socket_z + 0.2), (0.0, 0.0, -1.0), profile, BRASS, seg=SEG)
    return [socket]


def build_dropship(root):
    parts = [disc(), canopy()] + portholes() + belly()
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
          "the knights' drop ship: a gilded saucer, ~14.6 m, violet hull crystal underneath",
          ao=AoSettings(radius=1.0, rays=32, open_parts=("Crystal",))),
]
