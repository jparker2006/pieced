"""The PIECED logo (target T12): chunky brass letters with blue crystal inlays and
the knight's tiny floppy purple wizard hat, gold star and all, on the I.

The letters are the HUD font's (`assets/fonts/`), extruded and bevelled, set
with a jaunty bounce: gold faces over brass sides, rivets, crystal filling the
counters of the P and the D, a crystal shard set into the I, a gem on the first
E and a crystal ball held in the C. Drawn by `lib/raster.py` (flat toon bands,
thick ink outline, transparent background) into `assets/ui/logo.png`.
"""

import math
import os

import bmesh
import bpy
from mathutils import Matrix, Vector

from assets import knight
from lib import export, palette, raster, scene, shapes
from lib.registry import UiImage

_REPO = os.path.normpath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
FONT = os.path.join(_REPO, "assets", "fonts", "LilitaOne-Regular.ttf")

WORD = "PIECED"
EXTRUDE = 0.11     # half the letters' depth
BEVEL = 0.035
GAP = 0.045        # between letters
INSET = 0.034      # the brass border round a crystal inlay
SIZE = (1200, 520)  # px

# Per letter: (tilt degrees, counter-clockwise; lift; scale).
BOUNCE = [(4.0, 0.03, 1.0), (-3.0, 0.07, 1.08), (3.5, -0.02, 1.0),
          (-4.0, 0.03, 1.0), (3.0, -0.01, 1.0), (-4.5, 0.02, 1.0)]

CRYSTAL = ("spell_blue", "crystal_blue", "barrier_cyan", "waterfall_blue")
# Text-space (x right, y up, z towards the viewer) -> Blender (model faces -Y).
STAND_UP = Matrix.Rotation(math.radians(90.0), 4, "X")


def _font():
    return bpy.data.fonts.load(FONT, check_existing=True)


def _text_mesh(ch, extrude, bevel, offset=0.0):
    cu = bpy.data.curves.new("Glyph", "FONT")
    cu.body = ch
    cu.font = _font()
    cu.size = 1.0
    cu.resolution_u = 8
    cu.offset = offset
    cu.extrude = extrude
    cu.bevel_depth = bevel
    cu.bevel_resolution = 2
    obj = scene.link(bpy.data.objects.new("GlyphTmp", cu))
    scene.update()
    depsgraph = bpy.context.evaluated_depsgraph_get()
    mesh = bpy.data.meshes.new_from_object(obj.evaluated_get(depsgraph))
    bpy.data.objects.remove(obj)
    bpy.data.curves.remove(cu)
    return mesh


def _loops(ch):
    """The glyph's outline loops (2D, text space), from its flat filled mesh."""
    mesh = _text_mesh(ch, 0.0, 0.0)
    bm = bmesh.new()
    bm.from_mesh(mesh)
    bpy.data.meshes.remove(mesh)
    boundary = [e for e in bm.edges if e.is_boundary]
    nxt = {}
    for e in boundary:
        a, b = e.verts
        nxt.setdefault(a.index, []).append(b.index)
        nxt.setdefault(b.index, []).append(a.index)
    co = {v.index: (v.co.x, v.co.y) for v in bm.verts}
    seen, loops = set(), []
    for start in sorted(nxt):
        if start in seen:
            continue
        loop, prev, cur = [], None, start
        while cur not in seen:
            seen.add(cur)
            loop.append(co[cur])
            options = [n for n in nxt[cur] if n != prev and n not in seen]
            if not options:
                break
            prev, cur = cur, options[0]
        if len(loop) >= 3:
            loops.append(loop)
    bm.free()
    return loops


def _inside(pt, loop):
    x, y = pt
    hit = False
    for (x0, y0), (x1, y1) in zip(loop, loop[1:] + loop[:1]):
        if (y0 > y) != (y1 > y) and x < x0 + (y - y0) * (x1 - x0) / (y1 - y0):
            hit = not hit
    return hit


def _in_glyph(pt, loops):
    return sum(_inside(pt, loop) for loop in loops) % 2 == 1


def _holes(loops):
    """Loops inside an odd number of other loops: the counters (of P, D...)."""
    return [loop for loop in loops
            if sum(_inside(loop[0], other) for other in loops if other is not loop) % 2 == 1]


def _bounds(loops):
    xs = [p[0] for loop in loops for p in loop]
    ys = [p[1] for loop in loops for p in loop]
    return min(xs), min(ys), max(xs), max(ys)


def _solid_ok(pt, loops, r):
    """True when a disc of radius r round pt lies inside the glyph."""
    return _in_glyph(pt, loops) and all(
        _in_glyph((pt[0] + r * math.cos(a), pt[1] + r * math.sin(a)), loops)
        for a in (k * math.pi / 4 for k in range(8)))


# ---------------------------------------------------------------------------
# Pieces of a letter (built in text space, z towards the viewer)
# ---------------------------------------------------------------------------

def _paint_letter(mesh):
    bm = bmesh.new()
    bm.from_mesh(mesh)
    palette.face_layer(bm)
    bm.faces.ensure_lookup_table()
    for f in bm.faces:
        n = f.normal
        if n.z > 0.8:
            palette.tag(bm, [f], "spell_gold")
        elif n.z < -0.5:
            palette.tag(bm, [f], "brass_shadow")
        else:
            palette.tag(bm, [f], "gold_rings")
    bm.to_mesh(mesh)
    bm.free()


def _inlay(ch, z, centre):
    """A crystal pane in the letter's shape, inset by INSET: facets radiate from
    `centre` (text space), bright towards the upper left."""
    mesh = _text_mesh(ch, 0.0, 0.0, offset=-INSET)
    bm = bmesh.new()
    bm.from_mesh(mesh)
    bpy.data.meshes.remove(mesh)
    bmesh.ops.subdivide_edges(bm, edges=bm.edges[:], cuts=2, use_grid_fill=True)
    bmesh.ops.triangulate(bm, faces=bm.faces[:])
    palette.face_layer(bm)
    for v in bm.verts:
        v.co.z = z
    cx, cy = centre
    sectors = 7
    for f in bm.faces:
        f.normal_update()
        if f.normal.z < 0:
            f.normal_flip()
        c = f.calc_center_median()
        a = math.atan2(c.y - cy, c.x - cx)
        s = int(((a + math.pi) / (2 * math.pi)) * sectors + 0.5) % sectors
        palette.tag(bm, [f], CRYSTAL[(s * 3) % len(CRYSTAL)])
    return bm


def _facet_fill(bm, loop, z, colors):
    """A flat crystal pane filling `loop`: facets fanned from its centre."""
    cx = sum(p[0] for p in loop) / len(loop)
    cy = sum(p[1] for p in loop) / len(loop)
    centre = bm.verts.new((cx, cy, z))
    ring = [bm.verts.new((x, y, z)) for x, y in loop]
    n = len(ring)
    sectors = 5
    for k in range(n):
        j = (k + 1) % n
        f = bm.faces.new((centre, ring[k], ring[j]))
        f.normal_update()
        if f.normal.z < 0:
            f.normal_flip()
        a = math.atan2((loop[k][1] - cy), (loop[k][0] - cx))
        s = int(((a + math.pi) / (2 * math.pi)) * sectors) % sectors
        palette.tag(bm, [f], colors[s % len(colors)])


def _gem(bm, centre, rx, ry, depth, colors, sides=6):
    """A faceted gem lying on the letter face: a flattened bipyramid."""
    cx, cy, cz = centre
    ring = [bm.verts.new((cx + rx * math.cos(math.pi / 2 + 2 * math.pi * k / sides),
                          cy + ry * math.sin(math.pi / 2 + 2 * math.pi * k / sides), cz))
            for k in range(sides)]
    top = bm.verts.new((cx, cy, cz + depth))
    faces = []
    for k in range(sides):
        j = (k + 1) % sides
        f = bm.faces.new((ring[k], ring[j], top))
        f.normal_update()
        if f.normal.z < 0:
            f.normal_flip()
        faces.append(f)
        palette.tag(bm, [f], colors[k % len(colors)])
    return faces


def _rivet(bm, x, y, z, r):
    geom = bmesh.ops.create_uvsphere(bm, u_segments=10, v_segments=5, radius=r)
    verts = geom["verts"]
    for v in verts:
        v.co = Vector((v.co.x + x, v.co.y + y, z + max(v.co.z, -0.2 * r) * 0.55))
    faces = list({f for v in verts for f in v.link_faces})
    palette.tag(bm, faces, "nail_head")


def _letter(i, ch, x0):
    """Builds letter i at x offset x0; returns (objects, advance, top centre)."""
    loops = _loops(ch)
    lo_x, lo_y, hi_x, hi_y = _bounds(loops)
    w, h = hi_x - lo_x, hi_y - lo_y
    front = EXTRUDE + BEVEL
    tilt, lift, scale = BOUNCE[i]
    pivot = Vector(((lo_x + hi_x) / 2, (lo_y + hi_y) / 2, 0.0))
    xf = (Matrix.Translation(Vector((x0 - lo_x, lift, 0.0)) + pivot)
          @ Matrix.Rotation(math.radians(tilt), 4, "Z") @ Matrix.Scale(scale, 4)
          @ Matrix.Translation(-pivot))
    tag = f"{ch}{i}"
    objs = []

    body = _text_mesh(ch, EXTRUDE, BEVEL)
    _paint_letter(body)
    body.transform(STAND_UP @ xf)
    objs.append(scene.make_part(f"Letter{tag}", body, None))

    rivets = palette.new_bmesh()
    if ch in "PID":
        # A crystal inlay in a brass border.
        crystal = _inlay(ch, front + 0.003, (lo_x + w * 0.35, lo_y + h * 0.62))
    else:
        crystal = palette.new_bmesh()
    if ch == "C":
        # A crystal ball held in the C's mouth, behind its face.
        cx, cy = lo_x + w * 0.56, (lo_y + hi_y) / 2
        ring = [(cx + h * 0.23 * math.cos(2 * math.pi * k / 20),
                 cy + h * 0.23 * math.sin(2 * math.pi * k / 20)) for k in range(20)]
        _facet_fill(crystal, ring, EXTRUDE * 0.25, CRYSTAL)
    elif ch == "E" and i == 2:
        # A gem on the stem.
        _gem(crystal, (lo_x + w * 0.2, (lo_y + hi_y) / 2, front), w * 0.13, h * 0.2, 0.06,
             CRYSTAL)
    # Rivets near the corners, wherever the letter's face is wide enough there.
    r = 0.028
    for fx, fy in ((0.16, 0.86), (0.84, 0.86), (0.16, 0.14), (0.84, 0.14)):
        pt = (lo_x + w * fx, lo_y + h * fy)
        if ch in "PID":
            break  # crystal letters: no room on the thin brass border
        for _ in range(4):
            if _solid_ok(pt, loops, r * 1.6):
                _rivet(rivets, pt[0], pt[1], front - 0.004, r)
                break
            # Slide towards the letter's middle and try again.
            pt = (pt[0] + (0.5 - fx) * w * 0.12, pt[1] + (0.5 - fy) * h * 0.08)
    for name, bm in ((f"Crystal{tag}", crystal), (f"Rivets{tag}", rivets)):
        if len(bm.faces):
            bm.transform(STAND_UP @ xf)
            objs.append(scene.make_part(name, shapes.mesh_from_bmesh(bm), None))
        else:
            bm.free()
    top = STAND_UP @ xf @ Vector(((lo_x + hi_x) / 2, hi_y, 0.0))
    return objs, w * scale + GAP, top


def _hat(on_top):
    """The knight's floppy hat, shrunk to sit on the I, tipped to the right."""
    b = knight.build_hat()
    base = Vector((0.0, 0.0, 1.705))
    hat = b.finish("Hat", None, base, (0.0, 0.0, 0.0))
    hat.scale = (1.9, 1.9, 1.9)
    hat.rotation_euler = (0.0, math.radians(12.0), 0.0)
    hat.location = on_top + Vector((-0.01, 0.0, -0.035))
    return hat


def _render(path):
    root = scene.make_root("logo")
    x = 0.0
    tops = []
    for i, ch in enumerate(WORD):
        objs, advance, top = _letter(i, ch, x)
        for o in objs:
            o.parent = root
        tops.append(top)
        x += advance
    _hat(tops[1]).parent = root
    export.finish_meshes(root)
    glow = {o.name for o in scene.descendants(root) if o.name.startswith("Crystal")}
    tris = raster.gather(root, glow=glow)
    image = raster.render(tris, raster.view(12.0, -10.0), SIZE, ss=3, outline_px=7.0,
                          line_px=2.0, depth_break=0.012)
    raster.write_png(image, path)


IMAGES = [
    UiImage("logo", "logo.png", _render, "the PIECED logo for the pause menu and loading screen"),
]
