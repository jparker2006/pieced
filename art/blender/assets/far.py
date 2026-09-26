"""The far view: the stained-glass cathedral station, starships, floating
islands and the ringed planet (targets T01, T10, T11; D1 is the station's
original reference).

Far models are drawn by the game's unlit far material (vertex colours plus
distance haze, never outlined), so their light is baked in here: every face
takes its palette colour's lit or `_shadow` variant by its normal against one
art key light from the viewer's front-right, above (`KEY`). Models face Blender
-Y (the arena), +Z up, with their lowest point on z = 0 (they float; the game
places them by their attach points). "Left" and "right" in part names are as
seen from the arena.

Named parts the game re-dresses (src/far):
- `Glass*`: stained glass. It pulses (brightness and emissive) and glows through
  a halo at the matching `Glow*` attach point.
- `Waterfall*`: scrolling waterfall strips. Each is a *unit* strip (local x
  -0.5..0.5 across, z from 0 at the lip down to -1) scaled by its node to its
  real size, so the waterfall shader reads strip coordinates straight from the
  vertex position. The `Mist*` attach point with the same suffix marks the
  bottom of the fall (a mist halo).
- `Trail`: a ship's engine streak (drawn additively); `Engine` marks the glow.
"""

import math

import bmesh
import bpy
from mathutils import Matrix, Vector

from lib import palette, scene, shapes
from lib.registry import BUDGETS, Asset

# The spec's budget table has no planet; a ringed sphere needs about 1.5k.
BUDGETS.setdefault("planet", 2000)

# Toward the art key light: the viewer's front-right (+X, -Y), above.
KEY = Vector((0.55, -0.5, 0.67)).normalized()

# Lit colour -> the colour its faces take when turned away from KEY.
SHADOW_OF = {
    "station_stone": "station_stone_shadow",
    "cliff_dirt": "cliff_dirt_shadow",
    "grass": "grass_shadow",
    "foliage": "foliage_shadow",
    "trunk": "trunk_shadow",
    "rock": "rock_shadow",
    "knight_steel": "knight_steel_shadow",
    "brass": "brass_shadow",
    "brick": "brick_shadow",
}

GLASS_COLOURS = ["glass_cyan", "glass_pink", "glass_violet", "glass_yellow", "glass_cyan",
                 "crystal_blue", "glass_pink", "glass_violet"]
LEAD = "gun_iron"
WINDOW = "glass_yellow"
ROOF = "gun_iron"


# ---------------------------------------------------------------------------
# Small geometry helpers (local to this family)
# ---------------------------------------------------------------------------

def faces_of(verts):
    """Faces touching `verts`, each once, in a stable order."""
    seen, out = set(), []
    for v in verts:
        for f in v.link_faces:
            if f not in seen:
                seen.add(f)
                out.append(f)
    return out


def unique_verts(faces):
    """The vertices of `faces`, each once, in a stable order."""
    seen, out = set(), []
    for f in faces:
        for v in f.verts:
            if v not in seen:
                seen.add(v)
                out.append(v)
    return out


def shade(bm, faces, lit, shadow=None, threshold=0.0):
    """Tags faces with `lit`, or its shadow variant where they face away from KEY."""
    dark_name = shadow or SHADOW_OF.get(lit, lit)
    lit_faces, dark = [], []
    for f in faces:
        f.normal_update()
        (lit_faces if f.normal.dot(KEY) > threshold else dark).append(f)
    palette.tag(bm, lit_faces, lit)
    palette.tag(bm, dark, dark_name)


def ring_verts(bm, centre, radii, z, sides, angle0=0.0):
    """A CCW ring (seen from above). `radii` is a number or a list per vertex."""
    out = []
    for i in range(sides):
        a = angle0 + 2.0 * math.pi * i / sides
        r = radii[i] if isinstance(radii, (list, tuple)) else radii
        out.append(bm.verts.new((centre[0] + math.cos(a) * r, centre[1] + math.sin(a) * r, z)))
    return out


def prism(bm, centre, r0, r1, z0, z1, sides=8, angle0=0.0, top=True, bottom=False):
    """A (tapered) n-gon column. Returns its faces (outward normals)."""
    lower = ring_verts(bm, centre, r0, z0, sides, angle0)
    upper = ring_verts(bm, centre, r1, z1, sides, angle0)
    faces = shapes.bridge(bm, lower, upper)
    if top:
        faces.append(bm.faces.new(upper))
    if bottom:
        faces.append(bm.faces.new(list(reversed(lower))))
    return faces


def pyramid(bm, centre, r, z0, height, sides=8, angle0=0.0):
    """A pointed roof/spire on an n-gon base (no base face)."""
    base = ring_verts(bm, centre, r, z0, sides, angle0)
    apex = bm.verts.new((centre[0], centre[1], z0 + height))
    return [bm.faces.new((base[i], base[(i + 1) % sides], apex)) for i in range(sides)]


def box(bm, lo, hi, bottom=False):
    """An axis-aligned box. Returns its faces (outward normals)."""
    (x0, y0, z0), (x1, y1, z1) = lo, hi
    return extrude(bm, [(x0, z0), (x1, z0), (x1, z1), (x0, z1)], "y", y0, y1, bottom=bottom)


def oriented_box(bm, centre, size, yaw, bottom=True):
    """A box of `size` (x, y, z) centred on `centre`, turned `yaw` radians about Z."""
    faces = box(bm, [-s / 2 for s in size], [s / 2 for s in size], bottom=bottom)
    verts = unique_verts(faces)
    m = Matrix.Translation(centre) @ Matrix.Rotation(yaw, 4, "Z")
    bmesh.ops.transform(bm, matrix=m, verts=verts)
    return faces


def extrude(bm, profile, axis, a0, a1, bottom=False):
    """Extrudes a 2D profile along `axis` ("x" or "y") from a0 to a1.

    For axis "y" the profile is (x, z), counter-clockwise seen from -Y (the
    front); for axis "x" it is (y, z), counter-clockwise seen from +X. The end
    caps and sides face outward. With `bottom=False` a face lying on the
    profile's lowest edge is still made (it is part of the sides); only the caps
    are always made.
    """
    def point(p, a):
        return (p[0], a, p[1]) if axis == "y" else (a, p[0], p[1])

    near = [bm.verts.new(point(p, a0)) for p in profile]
    far = [bm.verts.new(point(p, a1)) for p in profile]
    n = len(profile)
    faces = []
    # Near cap faces -a (for "y": -Y, the profile's CCW side; for "x": the far side).
    if axis == "y":
        faces.append(bm.faces.new(near))
        faces.append(bm.faces.new(list(reversed(far))))
    else:
        faces.append(bm.faces.new(list(reversed(near))))
        faces.append(bm.faces.new(far))
    for i in range(n):
        j = (i + 1) % n
        quad = (near[i], far[i], far[j], near[j]) if axis == "y" else (near[j], far[j], far[i], near[i])
        faces.append(bm.faces.new(quad))
    if not bottom:
        low = min(p[1] for p in profile)
        keep = []
        for f in faces:
            f.normal_update()
            centre = f.calc_center_median()
            if f.normal.z < -0.99 and abs(centre.z - low) < 1e-6:
                bm.faces.remove(f)
            else:
                keep.append(f)
        faces = keep
    return faces


def add_mesh_faces(bm, other):
    """Moves every face of another bmesh (with its palette tags) into bm."""
    mesh = shapes.mesh_from_bmesh(other)
    bm.from_mesh(mesh)
    bpy.data.meshes.remove(mesh)


def lift_children(root, dz):
    """Raises every part and attach point by dz (so the lowest point sits on z = 0)."""
    for child in root.children:
        child.location.z += dz


def made_part(name, bm, root, smooth=False):
    obj = scene.make_part(name, shapes.mesh_from_bmesh(bm), root)
    if smooth:
        shapes.smooth_shading(obj, sharp_angle_deg=40.0)
    else:
        shapes.flat_shading(obj)
    return obj


# ---------------------------------------------------------------------------
# Floating rock, grass tops, trees, towers
# ---------------------------------------------------------------------------

def floating_rock(bm, centre, radius, depth, seed, sides=14, squash=1.0, spikes=4,
                  top="grass", rim_z=0.0):
    """A floating island mass: a grass (or stone) top at z = rim_z, a grass lip, a
    dirt band and a jagged rocky cone hanging to rim_z - depth.

    Returns (top_faces, rim_points): the rim as (x, y) world points with their
    outward angle, for placing waterfalls.
    """
    rng = shapes.rng(seed)
    cx, cy = centre
    angles = [2 * math.pi * (i + rng.uniform(-0.25, 0.25)) / sides for i in range(sides)]
    rad = [radius * (1.0 + rng.uniform(-0.1, 0.1) + 0.12 * shapes.perlin(
        (math.cos(a) * 1.3, math.sin(a) * 1.3, 0.0), seed)) for a in angles]

    def ring(scale, z, jag=0.0, twist=0.0):
        out = []
        for i, a in enumerate(angles):
            k = scale * (1.0 + (jag if i % 2 else -jag) + rng.uniform(-0.5, 0.5) * jag)
            aa = a + twist
            out.append(bm.verts.new((cx + math.cos(aa) * rad[i] * k,
                                     cy + math.sin(aa) * rad[i] * k * squash,
                                     rim_z + z + rng.uniform(-0.03, 0.03) * depth * (1 if z else 0))))
        return out

    rim = ring(1.0, 0.0)
    for v in rim:
        v.co.z = rim_z + rng.uniform(-0.01, 0.015) * radius
    centre_v = bm.verts.new((cx, cy, rim_z + radius * 0.07))
    top_faces = [bm.faces.new((rim[i], rim[(i + 1) % sides], centre_v)) for i in range(sides)]
    shade(bm, top_faces, top)
    lip = ring(0.99, -0.05 * depth)
    lip_faces = shapes.bridge(bm, lip, rim)
    shade(bm, lip_faces, "grass" if top == "grass" else top)
    rings = [lip]
    levels = [(-0.15, 0.94, 0.0), (-0.34, 0.8, 0.13), (-0.56, 0.58, 0.17), (-0.78, 0.32, 0.2)]
    for z, s, jag in levels:
        rings.append(ring(s, z * depth, jag, twist=rng.uniform(-0.12, 0.12)))
    apex = bm.verts.new((cx + rng.uniform(-0.1, 0.1) * radius, cy + rng.uniform(-0.1, 0.1) * radius,
                         rim_z - depth))
    bands = [shapes.bridge(bm, lower, upper) for upper, lower in zip(rings, rings[1:])]
    tip = [bm.faces.new((rings[-1][i], apex, rings[-1][(i + 1) % sides])) for i in range(sides)]
    # A band of dirt under the lip, craggy rock below.
    shade(bm, bands[0], "cliff_dirt")
    rock_faces = [f for band in bands[1:] for f in band] + tip
    for f in rock_faces:
        c = f.calc_center_median()
        n = shapes.perlin((c.x / radius * 1.7, c.y / radius * 1.7, c.z / radius * 1.7), seed + 3)
        shade(bm, [f], "rock" if n > 0.18 else "cliff_dirt")
    # Hanging spikes.
    for k in range(spikes):
        ring_i = 1 + (k % 3)
        i = (k * 7 + seed) % sides
        v = rings[ring_i][i].co
        inward = Vector((cx - v.x, cy - v.y, 0.0)) * 0.25
        length = depth * rng.uniform(0.35, 0.7)
        r = radius * rng.uniform(0.1, 0.18)
        res = bmesh.ops.create_cone(
            bm, cap_ends=True, cap_tris=False, segments=5, radius1=0.0, radius2=r, depth=length,
            matrix=Matrix.Translation(v + inward - Vector((0, 0, length * 0.42))))
        shade(bm, faces_of(res["verts"]), "cliff_dirt")
    rim_points = [((cx + math.cos(a) * rad[i], cy + math.sin(a) * rad[i] * squash), a)
                  for i, a in enumerate(angles)]
    return top_faces, rim_points


def far_tree(bm, base, scale=1.0, seed=0):
    """A puffy low-poly tree: a six-sided trunk and three icosphere puffs."""
    x, y, z = base
    rng = shapes.rng(seed)
    trunk = prism(bm, (x, y), 0.7 * scale, 0.5 * scale, z, z + 4.2 * scale, sides=6, top=False)
    shade(bm, trunk, "trunk")
    puffs = [((0.0, 0.0, 6.2), 3.0, 2), ((-1.9, 0.4, 5.0), 2.1, 1), ((1.8, -0.5, 5.3), 2.2, 1),
             ((0.3, 1.2, 7.6), 1.9, 1)]
    for (px, py, pz), r, sub in puffs:
        res = bmesh.ops.create_icosphere(
            bm, subdivisions=sub, radius=r * scale * rng.uniform(0.92, 1.08),
            matrix=Matrix.Translation((x + px * scale, y + py * scale, z + pz * scale)))
        shade(bm, faces_of(res["verts"]), "foliage", threshold=-0.15)


def far_tower(bm, base, scale=1.0):
    """A little gothic chapel tower: an octagonal stone shaft, cornice, dark spire."""
    x, y, z = base
    shaft = prism(bm, (x, y), 2.4 * scale, 2.2 * scale, z, z + 13 * scale, sides=8, top=False)
    shade(bm, shaft, "station_stone")
    corn = prism(bm, (x, y), 2.8 * scale, 2.8 * scale, z + 13 * scale, z + 14 * scale, sides=8,
                 top=True)
    shade(bm, corn, "station_stone")
    roof = pyramid(bm, (x, y), 2.6 * scale, z + 14 * scale, 9 * scale, sides=8)
    palette.tag(bm, roof, ROOF)
    side = prism(bm, (x + 3.2 * scale, y + 0.5 * scale), 1.3 * scale, 1.2 * scale, z,
                 z + 8 * scale, sides=6, top=False)
    shade(bm, side, "station_stone")
    palette.tag(bm, pyramid(bm, (x + 3.2 * scale, y + 0.5 * scale), 1.5 * scale, z + 8 * scale,
                            5 * scale, sides=6), ROOF)
    win = box(bm, (x - 0.5 * scale, y - 2.45 * scale, z + 8 * scale),
              (x + 0.5 * scale, y - 2.3 * scale, z + 10.5 * scale))
    palette.tag(bm, win, WINDOW)


# ---------------------------------------------------------------------------
# Waterfalls
# ---------------------------------------------------------------------------

WATERFALL_XS = [-0.5, -1.0 / 6.0, 1.0 / 6.0, 0.5]
WATERFALL_ZS = [0.0, -0.06, -0.2, -0.42, -0.7, -1.0]


def waterfall_bulge(x, z):
    """How far the unit strip bows out toward -y (the front) at (x, z)."""
    return -0.09 * math.sqrt(max(-z, 0.0)) - 0.035 * (1.0 - (2.0 * x) ** 2)


def unit_waterfall_mesh(name):
    """The unit waterfall strip (see the module docs), facing -Y."""
    bm = palette.new_bmesh()
    grid = [[bm.verts.new((x, waterfall_bulge(x, z), z)) for x in WATERFALL_XS]
            for z in WATERFALL_ZS]
    faces = []
    for r in range(len(WATERFALL_ZS) - 1):
        for c in range(len(WATERFALL_XS) - 1):
            a, b = grid[r][c], grid[r][c + 1]
            d, e = grid[r + 1][c], grid[r + 1][c + 1]
            faces.append(bm.faces.new((a, d, e, b)))
    palette.tag(bm, faces, "waterfall_blue")
    return shapes.mesh_from_bmesh(bm, name)


def add_waterfall(root, suffix, lip, outward_angle, width, length):
    """A waterfall part spilling from `lip` (x, y, z) toward `outward_angle`, and its mist point."""
    name = f"Waterfall{suffix}"
    obj = scene.make_part(name, unit_waterfall_mesh(name), root, location=lip)
    yaw = outward_angle + math.pi / 2
    obj.rotation_euler = (0.0, 0.0, yaw)
    obj.scale = (width, width, length)
    shapes.flat_shading(obj)
    bottom = Matrix.Rotation(yaw, 3, "Z") @ Vector((0.0, waterfall_bulge(0.0, -1.0) * width, 0.0))
    scene.make_attach(f"Mist{suffix}", root,
                      (lip[0] + bottom.x, lip[1] + bottom.y, lip[2] - length))
    return obj


# ---------------------------------------------------------------------------
# Stained glass
# ---------------------------------------------------------------------------

def arch_half_width(v, width, height):
    """Half width of an equilateral pointed arch window at height v (0 = sill)."""
    spring = height - width * math.sqrt(3.0) / 2.0
    if v <= spring:
        return width / 2.0
    d = v - spring
    return max(0.0, math.sqrt(max(width * width - d * d, 0.0)) - width / 2.0)


def arch_outline(width, height, n_arc=10, offset=0.0):
    """The window outline as (u, v) points, counter-clockwise seen from the front,
    grown outward by `offset`."""
    spring = height - width * math.sqrt(3.0) / 2.0
    w = width + 2 * offset
    r = width + offset
    # The right-hand arc is centred on the left springing point, and meets its
    # mirror on the centre line.
    end = math.acos((width / 2.0) / r)
    right = [(w / 2.0, -offset), (w / 2.0, spring)]
    for k in range(1, n_arc):
        ang = end * k / n_arc
        right.append((-width / 2.0 + math.cos(ang) * r, spring + math.sin(ang) * r))
    top = (0.0, spring + math.sin(end) * r)
    left = [(-u, v) for (u, v) in reversed(right)]
    return [(-w / 2.0, -offset)] + right + [top] + left[:-1]


def stained_glass(bm, width, height, rows, cols, seed, matrix):
    """A mosaic of leaded glass cells filling a pointed-arch window of the given
    size, in the u-v plane (u right, v up, facing -Y), then placed by `matrix`.
    Colours are symmetric about the centre line with a gold medallion."""
    rng = shapes.rng(seed)
    grid = []
    for k in range(rows):
        v = height * k / rows
        hw = arch_half_width(v, width, height)
        row = []
        for j in range(cols + 1):
            u = hw * (-1.0 + 2.0 * j / cols)
            if 0 < j < cols and 0 < k:
                u += rng.uniform(-0.28, 0.28) * width / cols
                v_j = v + rng.uniform(-0.28, 0.28) * height / rows
            else:
                v_j = v
            row.append(bm.verts.new((u, 0.0, v_j)))
        grid.append(row)
    apex = bm.verts.new((0.0, 0.0, height))
    cells = []
    for k in range(rows - 1):
        for j in range(cols):
            f = bm.faces.new((grid[k][j], grid[k][j + 1], grid[k + 1][j + 1], grid[k + 1][j]))
            cells.append((f, k, j))
    for j in range(cols):
        f = bm.faces.new((grid[rows - 1][j], grid[rows - 1][j + 1], apex))
        cells.append((f, rows - 1, j))
    verts = unique_verts([f for f, _, _ in cells])
    bmesh.ops.transform(bm, matrix=matrix, verts=verts)
    centre_row = rows * 0.62
    colours = {}
    for f, k, j in cells:
        m = min(j, cols - 1 - j)
        mid = (cols - 1) / 2.0
        medallion = abs(j - mid) + abs(k - centre_row) * 0.7 < 1.6
        if medallion:
            name = "star_gold" if (k + j) % 2 else "glass_yellow"
        else:
            key = (k // 2, m)
            if key not in colours:
                colours[key] = GLASS_COLOURS[(rng.randrange(len(GLASS_COLOURS)) + seed) %
                                             len(GLASS_COLOURS)]
            name = colours[key]
        palette.tag(bm, [f], name)
    leads = []
    for f, _, _ in cells:
        size = min(e.calc_length() for e in f.edges)
        res = bmesh.ops.inset_individual(bm, faces=[f], thickness=size * 0.16, depth=0.0,
                                         use_even_offset=True)
        leads += res["faces"]
    palette.tag(bm, leads, LEAD)


def window_frame(bm, width, height, fw, depth, matrix, mullion=True):
    """The stone frame around a pointed-arch window: a front band `fw` wide, the
    reveal back to the glass, outer sides and a flat back. Placed by `matrix`."""
    inner = arch_outline(width, height, offset=0.0)
    outer = arch_outline(width, height, offset=fw)
    n = len(inner)
    front = -depth * 0.45
    back = depth * 0.55
    fi = [bm.verts.new((u, front, v)) for u, v in inner]
    fo = [bm.verts.new((u, front, v)) for u, v in outer]
    gi = [bm.verts.new((u, 0.05, v)) for u, v in inner]
    bo = [bm.verts.new((u, back, v)) for u, v in outer]
    faces = []
    for i in range(n):
        j = (i + 1) % n
        faces.append(bm.faces.new((fo[i], fo[j], fi[j], fi[i])))   # front band
        faces.append(bm.faces.new((fi[i], fi[j], gi[j], gi[i])))   # reveal
        faces.append(bm.faces.new((bo[i], bo[j], fo[j], fo[i])))   # outer sides
    faces.append(bm.faces.new(list(reversed(bo))))                 # back
    if mullion:
        spring = height - width * math.sqrt(3.0) / 2.0
        faces += box(bm, (-fw * 0.3, front, 0.0), (fw * 0.3, 0.0, spring))
    verts = unique_verts(faces)
    bmesh.ops.transform(bm, matrix=matrix, verts=verts)
    shade(bm, faces, "station_stone")


def placed(x, y, z, yaw=0.0, lean=0.0):
    """Translation, then turn about Z (yaw), then lean about the panel's depth axis."""
    return (Matrix.Translation((x, y, z)) @ Matrix.Rotation(yaw, 4, "Z")
            @ Matrix.Rotation(lean, 4, "Y"))


# ---------------------------------------------------------------------------
# Station
# ---------------------------------------------------------------------------

BASE_DEPTH = 118.0  # platform top to the lowest rock spike
RING_Z = 50.0
RING_R = (113.0, 124.0)
# (name suffix, x, y, bottom z, width, height, yaw, lean, rows, cols, seed)
SAILS = [
    ("LeftInner", -62.0, 16.0, 6.0, 40.0, 220.0, -0.22, -0.18, 22, 7, 3),
    ("LeftOuter", -100.0, 28.0, 2.0, 34.0, 185.0, -0.44, -0.46, 20, 6, 5),
    ("RightInner", 62.0, 16.0, 6.0, 40.0, 220.0, 0.22, 0.18, 22, 7, 7),
    ("RightOuter", 100.0, 28.0, 2.0, 34.0, 185.0, 0.44, 0.46, 20, 6, 11),
]
# The great west window: (width, height, bottom z, y of the facade).
WEST_WINDOW = (32.0, 86.0, 34.0, -31.0)
# Spire towers: (x, y, base z, shaft height, radius, spire height, sides); a
# spire height of 0 marks the two square front towers.
SPIRES = [
    (-32.0, -32.0, 32.0, 100.0, 9.0, 0.0, 4),
    (32.0, -32.0, 32.0, 100.0, 9.0, 0.0, 4),
    (-50.0, 17.0, 32.0, 84.0, 5.5, 50.0, 8),     # transept ends
    (50.0, 17.0, 32.0, 84.0, 5.5, 50.0, 8),
    (-40.0, 42.0, 32.0, 74.0, 6.0, 52.0, 8),
    (40.0, 42.0, 32.0, 74.0, 6.0, 52.0, 8),
    (-18.0, 64.0, 32.0, 100.0, 6.5, 62.0, 8),
    (18.0, 64.0, 32.0, 100.0, 6.5, 62.0, 8),
    (0.0, 76.0, 32.0, 116.0, 7.5, 72.0, 8),
    (-28.0, -10.0, 70.0, 24.0, 3.2, 28.0, 6),    # pinnacle spires on the aisle roofs
    (28.0, -10.0, 70.0, 24.0, 3.2, 28.0, 6),
    (-28.0, 12.0, 72.0, 30.0, 3.4, 30.0, 6),
    (28.0, 12.0, 72.0, 30.0, 3.4, 30.0, 6),
    (-28.0, 34.0, 70.0, 24.0, 3.2, 28.0, 6),
    (28.0, 34.0, 70.0, 24.0, 3.2, 28.0, 6),
    (-52.0, -36.0, 32.0, 36.0, 3.5, 30.0, 6),    # podium corners
    (52.0, -36.0, 32.0, 36.0, 3.5, 30.0, 6),
    (-52.0, 44.0, 32.0, 46.0, 4.0, 34.0, 8),
    (52.0, 44.0, 32.0, 46.0, 4.0, 34.0, 8),
    (-74.0, -36.0, 0.0, 48.0, 4.5, 34.0, 8),     # on the platform
    (74.0, -36.0, 0.0, 48.0, 4.5, 34.0, 8),
    (-78.0, 52.0, 0.0, 72.0, 5.0, 42.0, 8),
    (78.0, 52.0, 0.0, 72.0, 5.0, 42.0, 8),
    (-58.0, -54.0, 0.0, 30.0, 3.5, 24.0, 6),
    (58.0, -54.0, 0.0, 30.0, 3.5, 24.0, 6),
    (-30.0, 86.0, 0.0, 60.0, 4.5, 36.0, 8),
    (30.0, 86.0, 0.0, 60.0, 4.5, 36.0, 8),
    (-62.0, 72.0, 0.0, 86.0, 5.0, 50.0, 8),
    (62.0, 72.0, 0.0, 86.0, 5.0, 50.0, 8),
]


def spire_tower(bm, x, y, z0, shaft, r, spire, sides):
    """A gothic tower: shaft, cornice, pointed spire and four corner pinnacles."""
    angle0 = math.pi / sides
    faces = prism(bm, (x, y), r, r * 0.93, z0, z0 + shaft, sides, angle0, top=False)
    top = z0 + shaft
    faces += prism(bm, (x, y), r * 1.12, r * 1.12, top, top + r * 0.35, sides, angle0)
    faces += pyramid(bm, (x, y), r * 0.95, top + r * 0.35, spire, sides, angle0)
    shade(bm, faces, "station_stone")
    for k in range(4):
        a = math.pi / 4 + k * math.pi / 2
        px, py = x + math.cos(a) * r * 1.05, y + math.sin(a) * r * 1.05
        pin = prism(bm, (px, py), r * 0.2, r * 0.2, top, top + r * 0.9, 4, math.pi / 4, top=False)
        pin += pyramid(bm, (px, py), r * 0.24, top + r * 0.9, r * 1.6, 4, math.pi / 4)
        shade(bm, pin, "station_stone")


def front_tower(bm, x, y, z0, shaft, r):
    """A square front tower with an octagonal belfry and the tallest spires."""
    faces = prism(bm, (x, y), r * 1.414, r * 1.35, z0, z0 + shaft, 4, math.pi / 4, top=True)
    top = z0 + shaft
    faces += prism(bm, (x, y), r * 0.95, r * 0.9, top, top + 24, 8, math.pi / 8, top=False)
    faces += prism(bm, (x, y), r * 1.05, r * 1.05, top + 24, top + 27, 8, math.pi / 8)
    faces += pyramid(bm, (x, y), r * 0.95, top + 27, 70.0, 8, math.pi / 8)
    shade(bm, faces, "station_stone")
    for k in range(4):
        a = math.pi / 4 + k * math.pi / 2
        px, py = x + math.cos(a) * r * 1.3, y + math.sin(a) * r * 1.3
        pin = prism(bm, (px, py), 1.4, 1.4, top, top + 12, 4, math.pi / 4, top=False)
        pin += pyramid(bm, (px, py), 1.7, top + 12, 16, 4, math.pi / 4)
        shade(bm, pin, "station_stone")


def cathedral(bm, windows):
    """The stone body: podium, nave with pitched roof, aisles, transept, apse,
    crossing flèche, front towers, spire towers and flying buttresses."""
    shade(bm, box(bm, (-56, -40, 0), (56, 48, 32)), "station_stone")          # podium
    # Nave with a steep roof (profile in x-z), extruded along y.
    nave = extrude(bm, [(-20, 32), (20, 32), (20, 118), (0, 152), (-20, 118)], "y", -30, 52)
    shade(bm, nave, "station_stone")
    for side in (-1, 1):
        aisle = extrude(bm, [(20, 32), (36, 32), (36, 70), (20, 82)] if side > 0
                        else [(-36, 32), (-20, 32), (-20, 82), (-36, 70)], "y", -20, 48)
        shade(bm, aisle, "station_stone")
    transept = extrude(bm, [(4, 32), (30, 32), (30, 96), (17, 120), (4, 96)], "x", -50, 50)
    shade(bm, transept, "station_stone")
    apse = prism(bm, (0, 52), 19, 19, 32, 104, 8, math.pi / 8)
    apse += pyramid(bm, (0, 52), 20, 104, 34, 8, math.pi / 8)
    shade(bm, apse, "station_stone")
    # Crossing lantern and flèche: the tallest point.
    fl = prism(bm, (0, 17), 9, 8.5, 116, 150, 8, math.pi / 8, top=False)
    fl += prism(bm, (0, 17), 10, 10, 150, 153, 8, math.pi / 8)
    fl += pyramid(bm, (0, 17), 9, 153, 104, 8, math.pi / 8)
    shade(bm, fl, "station_stone")
    for x, y, z0, shaft, r, spire, sides in SPIRES:
        if spire == 0.0:
            front_tower(bm, x, y, z0, shaft, r)
        else:
            spire_tower(bm, x, y, z0, shaft, r, spire, sides)
    # Flying buttresses: slanted piers from the aisles up to the nave wall.
    for side in (-1, 1):
        for y in (-8.0, 12.0, 32.0):
            pier = oriented_box(bm, (side * 40.0, y, 60.0), (4.0, 3.0, 56.0), 0.0)
            shade(bm, pier, "station_stone")
            arm = extrude(bm, ([(36, 80), (40, 84), (22, 108), (20, 104)] if side > 0 else
                               [(-40, 84), (-36, 80), (-20, 104), (-22, 108)]), "y", y - 1.2, y + 1.2)
            shade(bm, arm, "station_stone")
    # Rows of lit arched windows on the podium front and the towers.
    for i in range(9):
        x = -44 + i * 11.0
        if abs(x) < 8:
            continue
        windows.append(((x, -40.3, 8.0), 3.4, 12.0))
    for x in (-32.0, 32.0):
        for z in (50.0, 76.0, 102.0):
            windows.append(((x, -32.0 - 9.3, z), 3.0, 10.0))
    for x in (-28.0, 28.0):
        windows.append(((x, -20.3, 42.0), 3.0, 16.0))


def glass_windows(bm, windows):
    """Small glowing windows, slightly in front of their walls (facing -Y)."""
    faces = []
    for (x, y, z), w, h in windows:
        faces += box(bm, (x - w / 2, y - 0.3, z), (x + w / 2, y, z + h), bottom=True)
        faces += pyramid(bm, (x, y - 0.15), w * 0.7, z + h, w * 0.8, 4, math.pi / 4)
    palette.tag(bm, faces, WINDOW)


def ring_walkway(bm):
    """The floating ring walkway around the cathedral, with a low parapet and spokes."""
    sides = 64
    r0, r1 = RING_R
    cy = 6.0
    z0, z1 = RING_Z - 1.6, RING_Z + 1.6
    inner_lo = ring_verts(bm, (0, cy), r0, z0, sides)
    inner_hi = ring_verts(bm, (0, cy), r0, z1, sides)
    outer_lo = ring_verts(bm, (0, cy), r1, z0, sides)
    outer_hi = ring_verts(bm, (0, cy), r1, z1, sides)
    rail_in = ring_verts(bm, (0, cy), r1 - 1.2, z1 + 2.6, sides)
    rail_out = ring_verts(bm, (0, cy), r1, z1 + 2.6, sides)
    faces = shapes.bridge(bm, outer_lo, outer_hi)              # outer wall
    faces += shapes.bridge(bm, inner_hi, inner_lo)             # inner wall (faces in)
    faces += shapes.bridge(bm, outer_hi, rail_out)             # parapet outside
    rail_foot = ring_verts(bm, (0, cy), r1 - 1.2, z1, sides)
    faces += shapes.bridge(bm, rail_in, rail_foot)             # parapet inside
    faces += shapes.bridge(bm, rail_out, rail_in)              # parapet top
    for i in range(sides):
        j = (i + 1) % sides
        faces.append(bm.faces.new((inner_hi[i], inner_hi[j], rail_foot[j], rail_foot[i])))  # deck
        faces.append(bm.faces.new((outer_lo[i], outer_lo[j], inner_lo[j], inner_lo[i])))    # soffit
    shade(bm, faces, "station_stone")
    for k in range(4):
        a = math.pi / 4 + k * math.pi / 2
        mid = (r0 + 58.0) / 2.0
        length = r0 - 58.0 + 2.0
        spoke = oriented_box(bm, (math.cos(a) * mid, cy + math.sin(a) * mid, RING_Z),
                             (length, 5.0, 2.6), a)
        shade(bm, spoke, "station_stone")


def build_station(root):
    """The stained-glass cathedral station (T10, T01): a gothic cathedral with a
    forest of spires on a floating rock with waterfalls, a ring walkway, and four
    huge leaded-glass solar sails fanning out beside a great west window."""
    # Base: the floating rock with a stone plaza and a grass rim.
    bm = palette.new_bmesh()
    _, rim = floating_rock(bm, (0.0, 8.0), 104.0, BASE_DEPTH, seed=101, sides=22, squash=0.82,
                           spikes=9, top="grass")
    # Satellite rocks hanging off the sides (T10).
    for (x, y, z, r, d, s) in [(-128.0, -6.0, -34.0, 26.0, 50.0, 111),
                               (132.0, 14.0, -44.0, 22.0, 44.0, 113),
                               (-40.0, -104.0, -70.0, 16.0, 30.0, 117)]:
        floating_rock(bm, (x, y), r, d, seed=s, sides=10, spikes=2, rim_z=z)
    plaza = prism(bm, (0.0, 8.0), 78.0, 78.0, 0.0, 1.2, 20, top=True)
    shade(bm, plaza, "station_stone")
    made_part("Base", bm, root)

    windows = []
    bm = palette.new_bmesh()
    cathedral(bm, windows)
    # Stone frames for the great west window and the four sails.
    ww, wh, wz, wy = WEST_WINDOW
    window_frame(bm, ww, wh, 3.0, 3.0, placed(0.0, wy, wz), mullion=True)
    for _, x, y, z, w, h, yaw, lean, _, _, _ in SAILS:
        window_frame(bm, w, h, 3.2, 4.0, placed(x, y, z, yaw, lean), mullion=True)
    made_part("Cathedral", bm, root)

    bm = palette.new_bmesh()
    ring_walkway(bm)
    made_part("Ring", bm, root)

    bm = palette.new_bmesh()
    ww, wh, wz, wy = WEST_WINDOW
    stained_glass(bm, ww, wh, 18, 8, 1, placed(0.0, wy, wz))
    made_part("GlassNave", bm, root)
    scene.make_attach("GlowNave", root, (0.0, wy - 3.0, wz + wh * 0.55))
    for suffix, x, y, z, w, h, yaw, lean, rows, cols, seed in SAILS:
        bm = palette.new_bmesh()
        m = placed(x, y, z, yaw, lean)
        stained_glass(bm, w, h, rows, cols, seed, m)
        made_part(f"GlassSail{suffix}", bm, root)
        centre = m @ Vector((0.0, -4.0, h * 0.5))
        scene.make_attach(f"GlowSail{suffix}", root, tuple(centre))

    bm = palette.new_bmesh()
    glass_windows(bm, windows)
    made_part("GlassWindows", bm, root)

    # Waterfalls pouring off the front of the rock.
    falls = [(-124.0, 12.0, 150.0), (-98.0, 16.0, 175.0), (-70.0, 11.0, 160.0),
             (-44.0, 13.0, 140.0)]
    for k, (deg, width, length) in enumerate(falls, start=1):
        a = math.radians(deg)
        (px, py), _ = min(rim, key=lambda p: abs(math.atan2(math.sin(p[1] - a),
                                                            math.cos(p[1] - a))))
        inward = Vector((0.0 - px, 8.0 - py, 0.0)).normalized() * 2.5
        add_waterfall(root, k, (px + inward.x, py + inward.y, -1.5), a, width, length)
    scene.make_attach("Platform", root, (0.0, 8.0, 0.0))
    lift_children(root, max(BASE_DEPTH, max(f[2] for f in falls) + 1.5))


# ---------------------------------------------------------------------------
# Far islands
# ---------------------------------------------------------------------------

def island(root, radius, depth, seed, fall, extras):
    """A far floating island: grass top, craggy underside, `extras(bm)` on top,
    and one waterfall (lip angle in degrees, width, length) spilling off its front."""
    bm = palette.new_bmesh()
    _, rim = floating_rock(bm, (0.0, 0.0), radius, depth, seed, sides=16, spikes=7)
    extras(bm)
    made_part("Island", bm, root)
    deg, width, length = fall
    a = math.radians(deg)
    (px, py), _ = min(rim, key=lambda p: abs(math.atan2(math.sin(p[1] - a), math.cos(p[1] - a))))
    inward = Vector((-px, -py, 0.0)).normalized() * 0.8
    add_waterfall(root, "", (px + inward.x, py + inward.y, -0.6), a, width, length)
    scene.make_attach("Top", root, (0.0, 0.0, 0.0))
    lift_children(root, max(depth, length + 0.6))


def build_far_island_a(root):
    """A round grassy island with two puffy trees and a long waterfall (T01)."""
    def extras(bm):
        far_tree(bm, (-6.0, 3.0, 0.8), 2.2, seed=1)
        far_tree(bm, (8.0, 6.0, 0.6), 1.7, seed=2)
        far_tree(bm, (1.0, -8.0, 0.6), 1.2, seed=3)
    island(root, 20.0, 26.0, 201, (-95.0, 6.0, 46.0), extras)


def build_far_island_b(root):
    """An island with a little gothic chapel tower, a tree and a waterfall (T01, T11)."""
    def extras(bm):
        far_tower(bm, (1.0, 3.0, 0.6), 1.45)
        far_tree(bm, (-9.0, -1.0, 0.8), 1.8, seed=4)
    island(root, 18.0, 30.0, 211, (-70.0, 5.0, 52.0), extras)


def build_far_island_c(root):
    """A small craggy islet with one tree and a thin waterfall (T11)."""
    def extras(bm):
        far_tree(bm, (1.0, 2.0, 0.8), 1.6, seed=6)
    island(root, 11.0, 20.0, 223, (-110.0, 3.5, 32.0), extras)


# ---------------------------------------------------------------------------
# Ship
# ---------------------------------------------------------------------------

def build_ship(root):
    """A small starship (T01, T11): a pointed silver hull with swept wings, red
    fins, a crystal canopy, twin engines and a blue engine streak."""
    bm = palette.new_bmesh()
    # Hull: a six-sided loft from the nose (-Y) to the tail.
    path = [((0, -11.0, 0.0), 0.05), ((0, -8.0, 0.1), 0.9), ((0, -3.5, 0.2), 1.7),
            ((0, 2.0, 0.2), 2.0), ((0, 7.0, 0.1), 1.6)]
    _, sides, caps = shapes.loft(bm, [(c, (lambda r: lambda a: r)(r)) for c, r in path], 6,
                                 angle0=math.pi / 6)
    hull = sides + caps
    shade(bm, hull, "glove_white", shadow="knight_steel", threshold=-0.2)
    # Swept wings (flat wedges) and a tail fin.
    for s in (-1, 1):
        pts = [(s * 1.6, -2.0, -0.2), (s * 1.6, 5.5, -0.2), (s * 10.0, 8.5, -0.5),
               (s * 9.0, 5.0, -0.5)]
        top = [bm.verts.new((x, y, z + 0.45)) for x, y, z in pts]
        bot = [bm.verts.new((x, y, z)) for x, y, z in pts]
        if s > 0:  # keep both wings counter-clockwise seen from above
            top.reverse()
            bot.reverse()
        wf = [bm.faces.new(top), bm.faces.new(list(reversed(bot)))]
        wf += shapes.bridge(bm, bot, top)
        shade(bm, wf, "glove_white", shadow="knight_steel", threshold=-0.2)
        tip = oriented_box(bm, (s * 9.6, 7.0, -0.1), (0.8, 3.6, 2.4), 0.0)
        palette.tag(bm, tip, "ghost_red")
    fin = [(0.0, 1.5, 1.6), (0.0, 7.0, 1.6), (0.0, 8.6, 6.0), (0.0, 6.2, 6.2)]
    left = [bm.verts.new((-0.25, y, z)) for _, y, z in fin]
    right = [bm.verts.new((0.25, y, z)) for _, y, z in fin]
    ff = [bm.faces.new(list(reversed(left))), bm.faces.new(right)]
    ff += shapes.bridge(bm, left, right)
    palette.tag(bm, ff, "ghost_red")
    # Twin engines with glowing exhausts.
    for s in (-1, 1):
        path = [((s * 3.0, 1.0, -0.3), 0.9), ((s * 3.0, 4.0, -0.3), 1.15), ((s * 3.0, 9.0, -0.3), 1.0)]
        _, esides, ecaps = shapes.loft(bm, [(c, (lambda r: lambda a: r)(r)) for c, r in path], 6)
        shade(bm, esides + ecaps[:1], "gun_iron")
        palette.tag(bm, ecaps[1:], "crystal_blue")
    # Crystal canopy.
    can = prism(bm, (0.0, -5.0), 0.95, 0.4, 1.6, 2.6, 6, math.pi / 6, top=True)
    palette.tag(bm, can, "crystal_blue")
    made_part("Hull", bm, root)

    # Engine streak: crossed double-sided tapered planes behind each engine.
    bm = palette.new_bmesh()
    for s in (-1, 1):
        x0, y0, z0 = s * 3.0, 9.2, -0.3
        for plane in ("h", "v"):
            for flip in (False, True):
                def p(off, y):
                    return (x0 + off, y, z0) if plane == "h" else (x0, y, z0 + off)
                a, b = bm.verts.new(p(-0.9, y0)), bm.verts.new(p(0.9, y0))
                c, d = bm.verts.new(p(0.45, y0 + 14.0)), bm.verts.new(p(-0.45, y0 + 14.0))
                e = bm.verts.new(p(0.0, y0 + 36.0))
                quad = [a, b, c, d]
                tri = [d, c, e]
                if flip:
                    quad.reverse()
                    tri.reverse()
                palette.tag(bm, [bm.faces.new(quad)], "crystal_blue")
                palette.tag(bm, [bm.faces.new(tri)], "spell_blue")
    made_part("Trail", bm, root)
    scene.make_attach("Engine", root, (0.0, 9.6, -0.3))
    scene.make_attach("Center", root, (0.0, 0.0, 0.0))
    lift_children(root, 1.4)


# ---------------------------------------------------------------------------
# Planet
# ---------------------------------------------------------------------------

def uv_sphere(bm, radius, sides, stacks):
    """A UV sphere with a stable face order (bmesh's create_uvsphere orders its
    faces differently from run to run, which would break byte-identical builds)."""
    top = bm.verts.new((0.0, 0.0, radius))
    bottom = bm.verts.new((0.0, 0.0, -radius))
    rings = []
    for j in range(1, stacks):
        phi = math.pi * j / stacks
        rings.append(ring_verts(bm, (0.0, 0.0), radius * math.sin(phi), radius * math.cos(phi),
                                sides))
    faces = [bm.faces.new((rings[0][i], rings[0][(i + 1) % sides], top)) for i in range(sides)]
    for upper, lower in zip(rings, rings[1:]):
        faces += shapes.bridge(bm, lower, upper)
    faces += [bm.faces.new((rings[-1][i], bottom, rings[-1][(i + 1) % sides]))
              for i in range(sides)]
    return faces


PLANET_R = 90.0
PLANET_TILT = Matrix.Rotation(math.radians(-22.0), 4, "Y") @ Matrix.Rotation(math.radians(-13.0), 4, "X")
BANDS = [(-1.01, "planet_violet"), (-0.62, "glass_violet"), (-0.4, "planet_violet"),
         (-0.12, "galaxy_purple"), (0.1, "glass_pink"), (0.3, "planet_violet"),
         (0.58, "glass_violet"), (0.8, "planet_violet")]


def build_planet(root):
    """A ringed planet (T01, T11): lavender bands, a shadowed limb and a tilted ring."""
    bm = palette.new_bmesh()
    faces = uv_sphere(bm, PLANET_R, 28, 16)
    bmesh.ops.transform(bm, matrix=PLANET_TILT, verts=unique_verts(faces))
    for f in faces:
        f.normal_update()
    # Colour by latitude in the planet's own frame, shade in the tilted one.
    inv = PLANET_TILT.inverted()
    for f in faces:
        lat = (inv @ f.calc_center_median()).z / PLANET_R
        name = [n for lo, n in BANDS if lat >= lo][-1]
        shade(bm, [f], name, shadow="galaxy_deep", threshold=-0.3)
    made_part("Body", bm, root)

    bm = palette.new_bmesh()
    sides = 56
    bands = [(1.38, 1.52, "glass_violet"), (1.52, 1.76, "galaxy_purple"), (1.82, 2.02, "cloud")]
    ring_faces = []
    for r0, r1, name in bands:
        for up in (True, False):
            inner = ring_verts(bm, (0, 0), r0 * PLANET_R, 0.0, sides)
            outer = ring_verts(bm, (0, 0), r1 * PLANET_R, 0.0, sides)
            fs = []
            for i in range(sides):
                j = (i + 1) % sides
                quad = (inner[i], outer[i], outer[j], inner[j])
                fs.append(bm.faces.new(quad if up else tuple(reversed(quad))))
            ring_faces.append((fs, name, up))
    all_verts = unique_verts([f for fs, _, _ in ring_faces for f in fs])
    bmesh.ops.transform(bm, matrix=PLANET_TILT, verts=all_verts)
    # The tilted ring dips below the sphere; put the lowest point on the ground.
    lowest = max(PLANET_R, max(-v.co.z for v in all_verts))
    for fs, name, up in ring_faces:
        palette.tag(bm, fs, name if up else "galaxy_deep")
    made_part("Rings", bm, root)
    scene.make_attach("Center", root, (0.0, 0.0, 0.0))
    lift_children(root, lowest)


ASSETS = [
    Asset("far_island_a", "far_island", build_far_island_a, "grassy far island, two trees, waterfall"),
    Asset("far_island_b", "far_island", build_far_island_b, "far island with a chapel tower, waterfall"),
    Asset("far_island_c", "far_island", build_far_island_c, "small craggy islet with a tree, waterfall"),
    Asset("planet", "planet", build_planet, "ringed lavender planet, radius 90 m"),
    Asset("ship", "ship", build_ship, "small starship with an engine streak, about 20 m"),
    Asset("station", "station", build_station, "stained-glass cathedral station on a floating rock"),
]
