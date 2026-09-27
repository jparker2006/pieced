"""HUD icons (targets T01-T11): the hotbar's six slot icons, rendered from the
real models, and the small crystal and heart icons of the bars, the ammo
readout and the menu buttons.

Each icon is flat and bright: palette colours in two toon bands (crystals
glow), a three-quarter view, a thick ink outline on a transparent background,
drawn by `lib/raster.py` so the committed PNGs are byte-identical on every
build. Output (under `assets/ui/`):

    icons/rifle.png  icons/pump.png          the guns, muzzle up and to the right
    icons/wall_brick.png  icons/ramp_plank.png  icons/floor_plank.png
    icons/cone_plank.png
    icons/crystal_blue.png  icons/crystal_violet.png   ammo, shield, buttons
    icons/heart.png                                    the health bar
"""

import math

import bmesh
from mathutils import Matrix, Vector

from assets import guns, pieces
from lib import export, palette, raster, scene, shapes
from lib.registry import UiImage

SLOT = 128   # hotbar icons, px square
SMALL = 64   # crystal and heart, px square

# The guns read darker than the targets' hotbar at icon size: lift the wood and
# iron a step and make the brass gold (only in the icons).
BRIGHT_GUN = {"stock": "trunk", "gun_wood_light": "trunk", "gun_wood_dark": "stock",
              "gun_iron": "station_stone", "brass": "star_gold"}


def _model(assets, name):
    """Builds a registered model (with its palette colours baked) in the current scene."""
    asset = next(a for a in assets if a.name == name)
    root = scene.make_root(asset.name)
    asset.build(root)
    export.finish_meshes(root)
    return root


def _slot_icon(assets, name, azimuth, elevation, roll=0.0, mirror=False, glass=(), glow=(),
               skip=(), recolor=None, outline=4.0, line=1.2, depth_break=0.02):
    def render(path):
        root = _model(assets, name)
        tris = raster.gather(root, glass=glass, glow=glow, skip=skip, recolor=recolor)
        cam = raster.view(azimuth, elevation, roll, mirror)
        image = raster.render(tris, cam, (SLOT, SLOT), outline_px=outline, line_px=line,
                              depth_break=depth_break)
        raster.write_png(image, path)
    return render


# ---------------------------------------------------------------------------
# Crystal and heart
# ---------------------------------------------------------------------------

def _crystal(colors):
    """A tall faceted gem (a six-sided bipyramid with a short waist), front on."""
    bm = palette.new_bmesh()
    sides = 6
    waist = []
    for z, r in ((0.06, 0.34), (-0.06, 0.34)):
        waist.append([bm.verts.new((r * math.cos(math.pi / 2 + 2 * math.pi * k / sides),
                                    r * math.sin(math.pi / 2 + 2 * math.pi * k / sides) * 0.55,
                                    z)) for k in range(sides)])
    top = bm.verts.new((0.0, 0.0, 0.62))
    bottom = bm.verts.new((0.0, 0.0, -0.78))
    faces = []
    band = shapes.bridge(bm, waist[1], waist[0])
    for k in range(sides):
        j = (k + 1) % sides
        faces.append((bm.faces.new((waist[0][k], waist[0][j], top)), "top", k))
        faces.append((bm.faces.new((waist[1][j], waist[1][k], bottom)), "bottom", k))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    dark, mid, light = colors
    palette.tag(bm, band, mid)
    for f, where, k in faces:
        # Facets on the light's side (upper left) are bright, the far side dark.
        lit_side = f.calc_center_median().x < 0.0
        if where == "top":
            palette.tag(bm, [f], light if lit_side else mid)
        else:
            palette.tag(bm, [f], mid if lit_side else dark)
    return bm


def _crystal_icon(colors):
    def render(path):
        root = scene.make_root("crystal")
        scene.make_part("Crystal", shapes.mesh_from_bmesh(_crystal(colors)), root)
        export.finish_meshes(root)
        tris = raster.gather(root, glow={"Crystal"})
        cam = raster.view(18.0, 8.0)
        image = raster.render(tris, cam, (SMALL, SMALL), ss=6, outline_px=3.0, line_px=0.9,
                              depth_break=0.5)
        raster.write_png(image, path)
    return render


def _heart_outline(n=56):
    pts = []
    for k in range(n):
        t = 2 * math.pi * k / n
        x = 16 * math.sin(t) ** 3
        y = 13 * math.cos(t) - 5 * math.cos(2 * t) - 2 * math.cos(3 * t) - math.cos(4 * t)
        pts.append((x / 17.0, y / 17.0))
    return pts


def _heart():
    """A puffy heart: a thick outline with a rounded front, plus a shine blob."""
    bm = palette.new_bmesh()
    outline = _heart_outline()
    rings = []
    # Rings from the back rim to the front dome: (scale, depth).
    for s, y in ((1.0, 0.16), (1.0, -0.02), (0.93, -0.12), (0.78, -0.19), (0.55, -0.23)):
        rings.append([bm.verts.new((x * s, y, z * s + 0.03 * (1 - s))) for x, z in outline])
    faces = []
    for lo, hi in zip(rings, rings[1:]):
        faces += shapes.bridge(bm, lo, hi)
    faces.append(bm.faces.new(list(reversed(rings[-1]))))
    faces.append(bm.faces.new(rings[0]))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    palette.tag(bm, bm.faces, "hud_health")
    # The cartoon shine: a small white oval on the upper left of the dome.
    shine = []
    for k in range(12):
        a = 2 * math.pi * k / 12
        shine.append(bm.verts.new((-0.42 + 0.13 * math.cos(a), -0.245,
                                   0.33 + 0.075 * math.sin(a))))
    f = bm.faces.new(shine)
    palette.tag(bm, [f], "grid_line")
    return bm


def _heart_icon(path):
    root = scene.make_root("heart")
    scene.make_part("Heart", shapes.mesh_from_bmesh(_heart()), root)
    export.finish_meshes(root)
    tris = raster.gather(root)
    cam = raster.view(0.0, 0.0)
    image = raster.render(tris, cam, (SMALL, SMALL), ss=6, outline_px=3.0, line_px=0.0)
    raster.write_png(image, path)


IMAGES = [
    UiImage("icon_rifle", "icons/rifle.png",
            _slot_icon(guns.ASSETS, "rifle", 72.0, 16.0, roll=-34.0, mirror=True,
                       glass={"Chamber"}, glow={"Crystal", "Runes"}, recolor=BRIGHT_GUN),
            "hotbar: the rifle, muzzle up-right"),
    UiImage("icon_pump", "icons/pump.png",
            _slot_icon(guns.ASSETS, "pump", 72.0, 16.0, roll=-34.0, mirror=True,
                       glass={"Chamber"}, glow={"Crystal", "Runes"}, skip={"Shard"},
                       recolor=BRIGHT_GUN),
            "hotbar: the pump, muzzle up-right"),
    UiImage("icon_wall", "icons/wall_brick.png",
            _slot_icon(pieces.ASSETS, "wall_brick", 28.0, 14.0, outline=4.5),
            "hotbar: the brick wall"),
    UiImage("icon_ramp", "icons/ramp_plank.png",
            _slot_icon(pieces.ASSETS, "ramp_plank", -118.0, 24.0, outline=4.5),
            "hotbar: the plank ramp"),
    UiImage("icon_floor", "icons/floor_plank.png",
            _slot_icon(pieces.ASSETS, "floor_plank", 35.0, 38.0, outline=4.5),
            "hotbar: the plank floor"),
    UiImage("icon_cone", "icons/cone_plank.png",
            _slot_icon(pieces.ASSETS, "cone_plank", 32.0, 13.0, outline=4.5),
            "hotbar: the plank cone"),
    UiImage("icon_crystal_blue", "icons/crystal_blue.png",
            _crystal_icon(("spell_blue", "crystal_blue", "barrier_cyan")),
            "the blue crystal: shield bar, rifle ammo, menu buttons"),
    UiImage("icon_crystal_violet", "icons/crystal_violet.png",
            _crystal_icon(("knight_purple", "crystal_violet", "glass_violet")),
            "the violet crystal: pump ammo"),
    UiImage("icon_heart", "icons/heart.png", _heart_icon, "the health bar's heart"),
]
