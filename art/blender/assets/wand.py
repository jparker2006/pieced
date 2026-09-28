"""The grunt's crystal wand (docs/M3-SPEC.md → The grunt, item 10).

A short, chunky cartoon wand for the knight's right gauntlet: a brass pommel
knob, a dark-wood shaft that tapers toward the tip, a brass ferrule cup and a
big faceted orange-red crystal. Cartoon-thick so it reads at 15 m, and hot
orange-red so it can never be mistaken for the player's blue spells.

Parts and attach points (Blender axes; the wand points along -Y, the model's
front, so in Bevy it points along -Z):

    Wand     the pommel, shaft and ferrule in one mesh
    Crystal  the crystal (the game swaps in a glowing material that ramps up
             during the 0.4 s wind-up, `arena::visuals::wand`)
    Tip      an empty at the crystal's point, where the orb leaves

The model's origin is the grip: where the gauntlet holds it (not the ground,
like the gloves' grip frames). ≤ 400 triangles.
"""

from mathutils import Matrix, Vector

from assets import guns
from lib import scene
from lib.registry import Asset

BRASS = guns.BRASS
BRASS_DARK = guns.BRASS_DARK
WOOD = guns.WOOD_DARK
CRYSTAL = "wand_crystal"
CRYSTAL_LIGHT = "wand_crystal_light"

# Along the wand (-Y), metres from the grip: the pommel's end (behind it), the
# crystal's centre and the tip.
POMMEL_END = 0.12
CRYSTAL_CENTRE = 0.315
CRYSTAL_LENGTH = 0.18
TIP = 0.42
SEGMENTS = 8


def wand_body():
    """Pommel knob, tapering shaft and ferrule cup as one surface of revolution.

    The profile runs from the pommel's end along the outside to the cup's rim,
    then back along a thin core (hidden inside) to close the lathe."""
    start = POMMEL_END
    # (distance along -Y from the pommel's end, radius), with each edge's colour.
    profile = [
        (0.0, 0.006),     # pommel end
        (0.008, 0.02),    # pommel knob
        (0.024, 0.029),
        (0.044, 0.025),
        (0.058, 0.016),   # neck
        (0.07, 0.021),    # collar
        (0.084, 0.019),
        (0.09, 0.02),     # shaft
        (0.33, 0.0155),
        (0.335, 0.02),    # ferrule
        (0.35, 0.024),
        (0.365, 0.031),   # cup rim
        (0.372, 0.027),
        (0.372, 0.004),   # core, back to the pommel
    ]
    colors = [BRASS, BRASS, BRASS, BRASS_DARK, BRASS, BRASS, BRASS_DARK, WOOD,
              BRASS_DARK, BRASS, BRASS, BRASS, BRASS_DARK, BRASS_DARK]
    return guns.lathe((0.0, start, 0.0), (0.0, -1.0, 0.0), profile, colors, seg=SEGMENTS)


def wand_crystal():
    """A double-terminated hexagonal crystal, two-tone facets, seated in the cup."""
    obj = guns.shard(CRYSTAL_LENGTH, 0.04, 0.055, CRYSTAL, CRYSTAL_LIGHT, sides=6,
                     twist=0.26)
    obj.data.transform(Matrix.Translation(Vector((0.0, -CRYSTAL_CENTRE, 0.0))))
    return obj


def build_wand(root):
    guns.part("Wand", [wand_body()], root, sharp_deg=50.0)
    centre = (0.0, -CRYSTAL_CENTRE, 0.0)
    guns.part("Crystal", [wand_crystal()], root, location=centre, flat=True, shine=False)
    scene.make_attach("Tip", root, (0.0, -TIP, 0.0))


ASSETS = [
    Asset("wand", "wand", build_wand, "the grunt's crystal wand, origin at the grip"),
]
