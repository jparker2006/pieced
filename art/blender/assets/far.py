"""The far view: the gothic cathedral station, starships, floating islands and
the ringed planet (targets T01, T02, T10, T11; D1 is the station's original
reference).

Far models are drawn by the game's unlit far material (vertex colours plus
distance haze, never outlined), so their light is baked in here: every face
takes a palette colour's lit, mid or shadow tone by its normal against one art
key light from the viewer's front-right, above (`KEY`). Models face Blender -Y
(the arena), +Z up, with their lowest point on z = 0 (they float; the game
places them by their attach points). "Left" and "right" in part names are as
seen from the arena.

Named parts the game re-dresses (src/far):
- `Glass*`: stained glass in four pulse groups, each its own material:
  `GlassNave` (the great west window and the crystal over the door),
  `GlassTowers` (the wing towers' tall windows), `GlassSide` (the other tower
  windows) and `GlassWindows` (the warm arcade windows). Each glows through
  halos at `Glow*` attach points named after its group (`GlowNave`,
  `GlowCrystal`, `GlowTowerLeft`, ...).
- `Waterfall*`: scrolling waterfall strips. Each is a *unit* strip (local x
  -0.5..0.5 across, z from 0 at the lip down to -1) scaled by its node to its
  real size, so the waterfall shader reads strip coordinates straight from the
  vertex position. Every strip is the same unit mesh, so the game draws them all
  from one. The `Mist*` attach point with the same suffix marks the bottom of
  the fall (a mist halo).
- `Trail`: a ship's engine streak (drawn additively); `Engine` marks the glow.
"""

import math

import bmesh
import bpy
from mathutils import Matrix, Vector

from lib import palette, scene, shapes
from lib.registry import BUDGETS, Asset

# The spec's budget table has no planet; a banded sphere and its thin ring need
# about 1.7k.
BUDGETS.setdefault("planet", 2000)

# Toward the art key light: the viewer's front-right (+X, -Y), above.
KEY = Vector((0.55, -0.5, 0.67)).normalized()

# Lit colour -> the colour its faces take when turned away from KEY.
SHADOW_OF = {
    "cliff_dirt": "cliff_dirt_shadow",
    "grass": "grass_shadow",
    "foliage": "foliage_shadow",
    "trunk": "trunk_shadow",
    "rock": "rock_shadow",
    "knight_steel": "knight_steel_shadow",
    "brass": "brass_shadow",
    "brick": "brick_shadow",
}

# Three-tone ramps (lit, mid, shadow) for the far stone and rock. The far
# shadow tones are named `_shade`, not `_shadow`: a `_shadow` pair would shift
# the palette's shared shadow tint that the UI renders use (lib/raster.py).
STONE = ("far_stone_light", "far_stone", "far_stone_shade")
SPIRE = ("far_stone", "far_stone_shade", "far_stone_dark")
ROCK = ("far_rock", "far_rock_shade", "far_rock_dark")

# Jewel glass (T10): the great window's rings and the towers' lancets.
JEWELS = ["far_glass_blue", "far_glass_teal", "far_glass_gold", "far_glass_magenta",
          "glass_violet", "far_glass_blue", "glass_cyan", "far_glass_gold", "far_glass_teal",
          "far_glass_magenta"]
LEAD = "gun_iron"
WARM = ("star_gold", "glass_yellow")


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


# Where each ramp turns lit / mid / shadow (normal . KEY). Stone faces looking
# at the arena (0.5) stay mid-tone, so the station's front reads dark against
# the galaxy with only its right-hand flanks and tops catching the key light.
TONES = {STONE: (0.52, 0.0), SPIRE: (0.52, 0.05)}


def shade3(bm, faces, ramp, hi=None, lo=None, jitter=None):
    """Tags faces lit / mid / shadow from a three-colour ramp by their normal
    against KEY. `jitter(face)` (about -0.3..0.3) shifts single faces between
    tones, for painted variety."""
    d_hi, d_lo = TONES.get(ramp, (0.3, -0.2))
    hi = d_hi if hi is None else hi
    lo = d_lo if lo is None else lo
    groups = ([], [], [])
    for f in faces:
        f.normal_update()
        d = f.normal.dot(KEY) + (jitter(f) if jitter else 0.0)
        groups[0 if d > hi else 1 if d > lo else 2].append(f)
    for g, name in zip(groups, ramp):
        palette.tag(bm, g, name)


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
    profile's lowest edge is dropped (it would face down, unseen).
    """
    def point(p, a):
        return (p[0], a, p[1]) if axis == "y" else (a, p[0], p[1])

    near = [bm.verts.new(point(p, a0)) for p in profile]
    far = [bm.verts.new(point(p, a1)) for p in profile]
    n = len(profile)
    faces = []
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


def lift_children(root, dz):
    """Raises every part and attach point by dz (so the lowest point sits on z = 0)."""
    for child in root.children:
        child.location.z += dz


def made_part(name, bm, root):
    obj = scene.make_part(name, shapes.mesh_from_bmesh(bm), root)
    shapes.flat_shading(obj)
    return obj


def placed(x, y, z, yaw=0.0):
    """Translation, then a turn about Z (a panel in the x-z plane facing -Y)."""
    return Matrix.Translation((x, y, z)) @ Matrix.Rotation(yaw, 4, "Z")


def nearest_rim(rim, deg):
    """The rim point whose outward angle is closest to `deg`."""
    a = math.radians(deg)
    return min(rim, key=lambda p: abs(math.atan2(math.sin(p[1] - a), math.cos(p[1] - a))))


# ---------------------------------------------------------------------------
# Floating rock: a grass top, a draping lip and a columnar underside
# ---------------------------------------------------------------------------

# The underside's cone as (depth fraction, radius fraction, jag) per ring, from
# the rock band under the lip down to the last ring above the apex.
CONE = [(0.18, 0.96, 0.03), (0.36, 0.84, 0.1), (0.54, 0.64, 0.13), (0.72, 0.42, 0.16),
        (0.87, 0.2, 0.18)]


def cone_depth(f, cone=None):
    """Depth fraction of the underside's cone at radius fraction `f` from the axis."""
    pts = [(1.0, 0.1)] + [(r, d) for d, r, _ in cone or CONE] + [(0.0, 1.0)]
    for (r0, d0), (r1, d1) in zip(pts, pts[1:]):
        if r1 <= f <= r0:
            t = (f - r1) / max(r0 - r1, 1e-6)
            return d1 + (d0 - d1) * t
    return 0.1


def floating_rock(bm, centre, radius, depth, seed, sides=14, cols=None, top="grass",
                  top_z=0.0, plaza=0.0, squash=1.0, shadow=0.0, crag=False, cone=None):
    """A floating mass as the targets paint it (T01, T10, T11): a flat grass top
    at `top_z` whose lip drapes over the rim in drips, a band of rock faces and
    a jagged inverted cone of rock, with rock columns jutting below it as
    stalactites, so the underside reads as columnar and spiky.

    `cols` lists the stalactite rings as (radius fraction, count, column radius
    fraction, extra length as a fraction of `depth`). With `plaza` > 0 the
    top's middle (that radius) is stone, for a building. `shadow` (0..0.5)
    turns more of the rock to its shadow tones (the station's rock is
    backlit by the galaxy in T01 and T10); `crag` paints it in the castle
    rock's cool blue-grey (M3-C4) instead of brown. Returns the rim as
    [((x, y), outward angle)] for placing waterfalls.
    """
    rng = shapes.rng(seed)
    cx, cy = centre
    angles = [2 * math.pi * (i + rng.uniform(-0.22, 0.22)) / sides for i in range(sides)]
    rad = [radius * (1.0 + rng.uniform(-0.07, 0.07) + 0.09 * shapes.perlin(
        (math.cos(a) * 1.3, math.sin(a) * 1.3, 0.0), seed)) for a in angles]

    def ring(scale, z, jag=0.0, drop=0.0):
        out = []
        for i, a in enumerate(angles):
            k = scale * (1.0 + (jag if i % 2 else -jag) * rng.uniform(0.5, 1.3))
            dz = -drop * depth * rng.uniform(0.0, 1.0) * (i % 2)
            out.append(bm.verts.new((cx + math.cos(a) * rad[i] * k,
                                     cy + math.sin(a) * rad[i] * k * squash, top_z + z + dz)))
        return out

    rim = ring(1.0, 0.0)
    # The top: flat grass, or a grass band round a stone plaza.
    if plaza > 0.0:
        bands, middle = shapes.cap_rings(bm, rim, [radius - plaza], z=top_z)
        shade(bm, bands[0], top)
        shade3(bm, [middle], STONE)
    else:
        mid = bm.verts.new((cx, cy, top_z + radius * 0.03))
        shade(bm, [bm.faces.new((rim[i], rim[(i + 1) % sides], mid)) for i in range(sides)], top)
    # The grass lip, draping in drips: every other vertex hangs lower.
    lip_h = max(0.9, depth * 0.05)
    lip = ring(1.012, -lip_h)
    for i, v in enumerate(lip):
        v.co.z -= lip_h * (1.1 if i % 2 else 0.0) * rng.uniform(0.6, 1.3)
    shade(bm, shapes.bridge(bm, lip, rim), top)

    def jitter(f):
        c = f.calc_center_median()
        return 0.35 * shapes.perlin((c.x / radius * 2.1, c.y / radius * 2.1, c.z / radius * 1.3),
                                    seed + 5)

    # The jagged cone: rings stepping in and down, every other vertex dropped
    # and pushed out, so its faces read as columns.
    rings = [lip] + [ring(r, -d * depth, jag, drop=0.04) for d, r, jag in cone or CONE]
    apex = bm.verts.new((cx + rng.uniform(-0.08, 0.08) * radius,
                         cy + rng.uniform(-0.08, 0.08) * radius, top_z - depth * 0.97))
    rock_faces = []
    for upper, lower in zip(rings, rings[1:]):
        rock_faces += shapes.bridge(bm, lower, upper)
    rock_faces += [bm.faces.new((rings[-1][i], apex, rings[-1][(i + 1) % sides]))
                   for i in range(sides)]
    ramp = CRAG if crag else ROCK
    shade3(bm, rock_faces, ramp, hi=0.18 + shadow, lo=-0.28 + shadow, jitter=jitter)
    # Stalactites jutting below the cone.
    cols = cols or [(0.35, 4, 0.2, 0.22), (0.68, 7, 0.14, 0.16)]
    col_faces = []
    for f, count, cr, extra in cols:
        a0 = rng.uniform(0, 2 * math.pi)
        for j in range(count):
            a = a0 + 2 * math.pi * (j + rng.uniform(-0.25, 0.25)) / count
            fj = f * rng.uniform(0.9, 1.1)
            r = radius * fj
            x, y = cx + math.cos(a) * r, cy + math.sin(a) * r * squash
            surface = cone_depth(fj, cone) * depth
            length = extra * depth * rng.uniform(0.6, 1.3)
            cr_j = radius * cr * rng.uniform(0.8, 1.2)
            z0 = top_z - surface + 0.12 * depth
            z_mid = top_z - surface - length * 0.3
            upper = ring_verts(bm, (x, y), cr_j, z0, 6, rng.uniform(0, 1))
            lower = ring_verts(bm, (x, y), cr_j * 0.85, z_mid, 6, rng.uniform(0, 1))
            tip = bm.verts.new((x + rng.uniform(-0.1, 0.1) * cr_j, y + rng.uniform(-0.1, 0.1) * cr_j,
                                top_z - surface - length))
            col_faces += shapes.bridge(bm, lower, upper)
            col_faces += [bm.faces.new((lower[i], tip, lower[(i + 1) % 6])) for i in range(6)]
    shade3(bm, col_faces, ramp, hi=0.2 + shadow, lo=-0.25 + shadow, jitter=jitter)
    return [((cx + math.cos(a) * rad[i], cy + math.sin(a) * rad[i] * squash), a)
            for i, a in enumerate(angles)]


def far_tree(bm, base, scale=1.0, seed=0):
    """A puffy far tree: a six-sided trunk and a round crown of three puffs."""
    x, y, z = base
    rng = shapes.rng(seed)
    trunk = prism(bm, (x, y), 0.7 * scale, 0.5 * scale, z, z + 4.0 * scale, sides=5, top=False)
    shade(bm, trunk, "trunk")
    puffs = [((0.0, 0.0, 6.4), 3.2, 1), ((-2.0, 0.5, 5.2), 2.2, 0), ((1.9, -0.6, 5.4), 2.3, 0)]
    for (px, py, pz), r, sub in puffs:
        res = bmesh.ops.create_icosphere(
            bm, subdivisions=sub + 1, radius=r * scale * rng.uniform(0.92, 1.08),
            matrix=Matrix.Translation((x + px * scale, y + py * scale, z + pz * scale)))
        shade(bm, faces_of(res["verts"]), "foliage", threshold=-0.1)


def pinnacle(bm, x, y, z0, h, r, ramp=SPIRE):
    """A thin gothic pinnacle: a square shaft and a tall pyramid."""
    faces = prism(bm, (x, y), r, r * 0.9, z0, z0 + h * 0.4, 4, math.pi / 4, top=False)
    shade3(bm, faces, STONE)
    shade3(bm, pyramid(bm, (x, y), r * 1.15, z0 + h * 0.4, h * 0.6, 4, math.pi / 4), ramp)


def far_castle(bm, base, scale=1.0, seed=0):
    """A little gothic castle for an island: a keep, round towers with dark
    spires and warm windows (T01, T11)."""
    x, y, z = base
    rng = shapes.rng(seed)
    s = scale
    keep = box(bm, (x - 3.2 * s, y - 2.6 * s, z), (x + 3.2 * s, y + 2.6 * s, z + 7.5 * s))
    shade3(bm, keep, STONE)
    shade3(bm, pyramid(bm, (x, y), 4.2 * s, z + 7.5 * s, 5.0 * s, 4, math.pi / 4), SPIRE)
    towers = [(-4.2, -2.4, 13.0, 1.6), (4.4, -1.8, 10.0, 1.4), (1.2, 3.6, 16.0, 1.8)]
    for tx, ty, h, r in towers:
        h *= rng.uniform(0.9, 1.1)
        shaft = prism(bm, (x + tx * s, y + ty * s), r * s, r * 0.92 * s, z, z + h * s, 6,
                      top=False)
        shade3(bm, shaft, STONE)
        spire = pyramid(bm, (x + tx * s, y + ty * s), r * 1.25 * s, z + h * s, r * 4.2 * s, 6)
        shade3(bm, spire, SPIRE)
        win = box(bm, (x + (tx - 0.35) * s, y + (ty - r) * s - 0.15, z + (h - 4.0) * s),
                  (x + (tx + 0.35) * s, y + (ty - r) * s, z + (h - 2.2) * s))
        palette.tag(bm, win, WARM[0])


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


def waterfall_from_rim(root, suffix, rim, deg, width, length, centre=(0.0, 0.0), z=0.0,
                       inset=1.0):
    """A waterfall spilling off the rim point nearest `deg`, set `inset` in from it."""
    (px, py), a = nearest_rim(rim, deg)
    inward = Vector((centre[0] - px, centre[1] - py, 0.0)).normalized() * inset
    return add_waterfall(root, suffix, (px + inward.x, py + inward.y, z - 0.6), a, width, length)


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


def arch_outline(width, height, n_arc=6, offset=0.0):
    """The window outline as (u, v) points, counter-clockwise seen from the front,
    grown outward by `offset`."""
    spring = height - width * math.sqrt(3.0) / 2.0
    w = width + 2 * offset
    r = width + offset
    end = math.acos((width / 2.0) / r)
    right = [(w / 2.0, -offset), (w / 2.0, spring)]
    for k in range(1, n_arc):
        ang = end * k / n_arc
        right.append((-width / 2.0 + math.cos(ang) * r, spring + math.sin(ang) * r))
    top = (0.0, spring + math.sin(end) * r)
    left = [(-u, v) for (u, v) in reversed(right)]
    return [(-w / 2.0, -offset)] + right + [top] + left[:-1]


def glass_colour(scheme, u, v, width, height, k, j, rows, cols, rng, memo, jewels=None):
    """The palette colour of a glass cell centred at (u, v)."""
    jewels = jewels or JEWELS
    mid = (cols - 1) / 2.0
    m = abs(j - mid)
    if scheme == "rose":
        # A rose of jewel rings round a blue-white heart, lancets below.
        cu, cv = 0.0, height * 0.6
        du, dv = u / (width * 0.5), (v - cv) / (width * 0.5)
        d = math.hypot(du, dv)
        if d < 0.2:
            return "far_glass_white"
        if d < 1.05 and v > height * 0.3:
            ring = int((d - 0.2) / 0.22)
            sector = int(math.atan2(abs(du), dv) / (math.pi / 5))
            key = ("r", ring, sector)
            if key not in memo:
                memo[key] = jewels[(ring * 3 + sector * 2 + (sector + ring) % 2) % len(jewels)]
            return memo[key]
        key = ("l", k // 2, round(m))
        if key not in memo:
            memo[key] = jewels[rng.randrange(len(jewels))]
        return memo[key]
    # Tall lancets: symmetric jewel blocks with a gold-and-white medallion.
    centre_row = rows * 0.62
    if abs(j - mid) + abs(k - centre_row) * 0.6 < 1.3:
        return "far_glass_gold" if (k + j) % 2 else "far_glass_white"
    key = ("b", k // 2, round(m))
    if key not in memo:
        memo[key] = jewels[rng.randrange(len(jewels))]
    return memo[key]


def stained_glass(bm, width, height, rows, cols, seed, matrix, scheme="lancet", lead=0.14,
                  jewels=None, lead_colour=LEAD):
    """A mosaic of leaded glass cells filling a pointed-arch window of the given
    size, in the u-v plane (u right, v up, facing -Y), then placed by `matrix`.
    Colours are symmetric about the centre line."""
    rng = shapes.rng(seed)
    grid = []
    for k in range(rows):
        v = height * k / rows
        hw = arch_half_width(v, width, height)
        row = []
        for j in range(cols + 1):
            u = hw * (-1.0 + 2.0 * j / cols)
            v_j = v
            if 0 < j < cols and 0 < k:
                u += rng.uniform(-0.25, 0.25) * width / cols
                v_j = v + rng.uniform(-0.25, 0.25) * height / rows
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
    memo = {}
    for f, k, j in cells:
        c = f.calc_center_median()
        palette.tag(bm, [f], glass_colour(scheme, c.x, c.z, width, height, k, j, rows, cols,
                                          rng, memo, jewels))
    leads = []
    for f, _, _ in cells:
        size = min(e.calc_length() for e in f.edges)
        res = bmesh.ops.inset_individual(bm, faces=[f], thickness=size * lead, depth=0.0,
                                         use_even_offset=True)
        leads += res["faces"]
    palette.tag(bm, leads, lead_colour)
    bmesh.ops.transform(bm, matrix=matrix, verts=unique_verts([f for f, _, _ in cells] + leads))


def window_frame(bm, width, height, fw, matrix, proud=1.4):
    """The raised stone frame round a window set in a wall whose face is the
    local plane y = 0 (facing -Y): a front band `fw` wide standing `proud` m
    out, its reveal back to the glass and its outer side back to the wall."""
    inner = arch_outline(width, height, offset=0.0)
    outer = arch_outline(width, height, offset=fw)
    n = len(inner)
    fi = [bm.verts.new((u, -proud, v)) for u, v in inner]
    fo = [bm.verts.new((u, -proud, v)) for u, v in outer]
    gi = [bm.verts.new((u, 0.0, v)) for u, v in inner]
    wo = [bm.verts.new((u, 0.0, v)) for u, v in outer]
    faces = []
    for i in range(n):
        j = (i + 1) % n
        faces.append(bm.faces.new((fo[i], fo[j], fi[j], fi[i])))   # front band
        faces.append(bm.faces.new((fi[i], fi[j], gi[j], gi[i])))   # reveal
        faces.append(bm.faces.new((wo[i], wo[j], fo[j], fo[i])))   # outer side
    bmesh.ops.transform(bm, matrix=matrix, verts=unique_verts(faces))
    shade3(bm, faces, STONE)


def glass_window(stone, glass, x, y_face, sill, width, height, rows, cols, seed,
                 scheme="lancet", yaw=0.0, frame=None, jewels=None, lead_colour=LEAD):
    """A stained-glass window set flush into a wall facing -Y at y = `y_face`
    (after turning by `yaw` about the window's centre line): the glass a hair
    proud of the wall, a raised stone frame round it."""
    fw = frame if frame is not None else max(1.2, width * 0.09)
    m = placed(x, y_face, sill, yaw)
    stained_glass(glass, width, height, rows, cols, seed, m @ Matrix.Translation((0, -0.3, 0)),
                  scheme, jewels=jewels, lead_colour=lead_colour)
    window_frame(stone, width, height, fw, m)


def warm_window(bm, x, y, sill, w, h, yaw=0.0):
    """A small warm-lit arched window (the arcade's lamps) on a wall through
    (x, y) facing -Y turned by `yaw`, a hair proud of it."""
    m = placed(x, y, sill, yaw) @ Matrix.Translation((0.0, -0.3, 0.0))
    spring = h - w * 0.6
    pts = [(-w / 2, 0.0), (w / 2, 0.0), (w / 2, spring), (0.0, h), (-w / 2, spring)]
    f = bm.faces.new([bm.verts.new(m @ Vector((u, 0.0, v))) for u, v in pts])
    palette.tag(bm, [f], WARM[0])


# ---------------------------------------------------------------------------
# Station: the starlit citadel, a castle city on a floating rock (M3-C4)
# ---------------------------------------------------------------------------
#
# C4 (D94): C3's vast dark-indigo castle city on a jagged floating rock with
# waterfalls, and C2's taller centre stacked like a wedding cake of towers.
# From the plaza up: a town of little houses and turrets round the rim, the
# great walled terrace with its arcades and glowing gate, the great hall with
# the gold-and-teal rose window (the heart), a ring of cone-roofed towers round
# it, and the keep rising in three drums to the needle spire. Every spire ends
# in a gold star; hundreds of warm windows are one part (`GlassWindows`).
# Bridges reach out to a tower rock on each side, and two satellite rocks
# carry little castle clusters, all pouring waterfalls. The golden rune ring,
# the lanterns and the aura are the game's (src/far): the model only marks
# where the aura glows (`Aura*`).

# Roofs: deep violet-indigo cones; trims and finials: gold.
ROOF = ("gun_glass_violet", "knight_purple_shadow", "far_stone_dark")
GOLD = ("gun_brass_light", "gun_brass", "gun_brass_dark")
# The castle rock: cool blue-grey crags (C3, C4), not the islands' brown.
CRAG = ("far_rock_shade", "far_rock_dark", "far_stone_dark")
# The castle's walls: C4's dark indigo stone, so the windows, the rose, the
# ring and the lanterns carry the light (in greyscale the castle is a dark
# mass pricked with lights).
CSTONE = ("far_stone", "far_stone_shade", "far_stone_dark")
TONES[ROOF] = (0.42, -0.1)
TONES[GOLD] = (0.25, -0.35)
TONES[CRAG] = (0.3, -0.2)
TONES[CSTONE] = (0.45, -0.05)
# The flanking lancets: C4's mint-teal, cyan and white panes in gold tracery.
LANCET_JEWELS = ["far_glass_teal", "glass_cyan", "far_glass_blue", "far_glass_teal",
                 "far_glass_white", "far_glass_teal"]

PLAT_R = 170.0          # the plaza's rim
ROCK_DEPTH = 160.0      # plaza to the cone's apex
# The rock's underside: steep columnar cliffs under the rim, then the taper
# (see CONE).
CASTLE_CONE = [(0.12, 0.97, 0.05), (0.28, 0.9, 0.08), (0.46, 0.74, 0.12), (0.64, 0.52, 0.15),
               (0.83, 0.27, 0.18)]
TERRACE = (0.0, 4.0, 98.0, 34.0)    # the great terrace: centre x, y, radius, wall top
HALL_FRONT = -56.0      # the great hall's facade (y), on the terrace
HALL_BACK = 30.0
HALL_HW = 46.0
HALL_TOP = 136.0        # its wall top and gable peak
HALL_PEAK = 172.0
ROSE = (58.0, 112.0, 36.0)          # the rose window: width, height, sill
GATE = (15.0, 27.0)     # the glowing gate in the terrace's front: width, height
KEEP = (0.0, 38.0)      # the keep's centre (behind the hall)
# The keep's drums: (radius, bottom, top, sides, window rows).
KEEP_DRUMS = [(32.0, TERRACE[3], 188.0, 12, 6), (22.0, 188.0, 246.0, 10, 3),
              (13.5, 246.0, 276.0, 8, 2)]
KEEP_SPIRE = 58.0       # the keep's crowning roof: from its top drum to the tip
# Towers: (x, y, base z, top z, radius, roof height, sides, window rows).
# Mirrored for x > 0 (with small differences so the skyline isn't a mirror).
TOWERS = [
    # The facade's flanking towers stand apart (square, `FLANKS`).
    # The ring of towers round the hall, on the terrace.
    (78.0, -26.0, 34.0, 124.0, 8.0, 50.0, 8, 4),
    (88.0, 18.0, 34.0, 148.0, 9.0, 58.0, 8, 5),
    (68.0, 60.0, 34.0, 172.0, 8.5, 60.0, 8, 5),
    (36.0, 84.0, 34.0, 144.0, 7.5, 50.0, 8, 4),
    # The great side towers on the plaza, carrying the bridges.
    (136.0, 6.0, 0.0, 176.0, 11.0, 64.0, 8, 7),
    # Behind: the skyline's depth.
    (108.0, 82.0, 0.0, 122.0, 9.0, 46.0, 8, 4),
    (56.0, 118.0, 0.0, 108.0, 8.0, 42.0, 8, 3),
    (0.0, 124.0, 0.0, 150.0, 10.0, 50.0, 8, 4),
    # A middle ring on the plaza, filling the skyline between the terrace and
    # the side towers, and slender spires between the hall and the keep.
    (114.0, -54.0, 0.0, 104.0, 7.5, 44.0, 8, 4),
    (120.0, 46.0, 0.0, 136.0, 8.0, 50.0, 8, 4),
    (30.0, 36.0, 34.0, 206.0, 6.0, 46.0, 8, 5),
    (88.0, -70.0, 0.0, 70.0, 6.0, 30.0, 8, 3),
]
# The facade's flanking towers: (x, y, half width, shaft top, lancet window).
FLANKS = (56.0, HALL_FRONT, 10.5, 168.0, (12.0, 74.0, 54.0, 12, 3))
# The terrace's corner turrets: every other one of its 18 corners.
TERRACE_SIDES = 18
# Satellite rocks: (x, y, top z, radius, depth, seed, cluster).
SATELLITES = [
    (150.0, -216.0, -66.0, 48.0, 88.0, 131, "town"),     # C4's lower-right castle rock
    (-158.0, -196.0, -52.0, 34.0, 66.0, 133, "town"),    # its lower-left twin
    (-262.0, -24.0, -8.0, 30.0, 64.0, 137, "tower"),     # the left bridge's tower rock
    (258.0, 20.0, -2.0, 30.0, 66.0, 139, "tower"),       # the right bridge's
]
# Bridges from the great side towers to the tower rocks: (x0, x1, y, deck z).
BRIDGES = [(-138.0, -244.0, 4.0, 26.0), (138.0, 240.0, 12.0, 30.0)]
# Waterfalls off the plaza rim: (angle deg, width, length).
STATION_FALLS = [(238.0, 16.0, 236.0), (256.0, 12.0, 214.0), (284.0, 21.0, 250.0),
                 (303.0, 14.0, 204.0)]
# Waterfalls off the satellites: (satellite index, angle deg, width, length).
SATELLITE_FALLS = [(0, 262.0, 11.0, 150.0), (1, 280.0, 8.0, 120.0), (2, 250.0, 7.0, 96.0)]


def front_facing(a, lim=0.35):
    """Whether an outward angle `a` (radians) faces the arena's half (-Y)."""
    return math.sin(a) < lim


def cone_roof(bm, x, y, z0, r, h, sides=8, lean=(0.0, 0.0), angle0=0.0):
    """A cartoon witch-hat roof on a round shaft of radius `r` topped at `z0`:
    a drooping eave, a concave cone that bends toward `lean` (the tip's offset,
    metres) and a needle point. Returns its faces."""
    # (radius factor, height fraction); the lean grows toward the tip.
    profile = [(1.22, -0.03), (0.84, 0.13), (0.5, 0.36), (0.22, 0.66)]
    rings = []
    for k, (rf, hf) in enumerate(profile):
        t = max(hf, 0.0) ** 1.6
        rings.append(ring_verts(bm, (x + lean[0] * t, y + lean[1] * t), r * rf, z0 + h * hf,
                                sides, angle0))
    faces = []
    for lower, upper in zip(rings, rings[1:]):
        faces += shapes.bridge(bm, lower, upper)
    apex = bm.verts.new((x + lean[0], y + lean[1], z0 + h))
    top = rings[-1]
    faces += [bm.faces.new((top[i], top[(i + 1) % sides], apex)) for i in range(sides)]
    # The eave's underside, seen from below (T10).
    under = ring_verts(bm, (x, y), r * 0.98, z0 + h * 0.01, sides, angle0)
    faces += shapes.bridge(bm, rings[0], under)
    return faces


def star_tip(bm, x, y, z, size):
    """A little gold star on a spire's tip: two crossed four-point diamonds,
    seen from both sides."""
    faces = []
    for axis in ("x", "y"):
        pts = [(0.0, size), (size * 0.28, 0.0), (0.0, -size * 0.7), (-size * 0.28, 0.0)]
        if axis == "x":
            vs = [bm.verts.new((x + u, y, z + v)) for u, v in pts]
        else:
            vs = [bm.verts.new((x, y + u, z + v)) for u, v in pts]
        faces.append(bm.faces.new(vs))
        faces.append(bm.faces.new(list(reversed([bm.verts.new(v.co) for v in vs]))))
    palette.tag(bm, faces, "spell_gold")
    return faces


def window_row(win, x, y, r, sides, angle0, sill, w, h, per_face=1, lim=0.35, inset=1.0):
    """Warm arched windows round a prism (centre x, y, radius r) on the faces
    turned toward the arena: `per_face` side by side, sill at `sill`."""
    face_r = r * math.cos(math.pi / sides) * inset
    width = 2.0 * r * math.sin(math.pi / sides)
    n = 0
    for i in range(sides):
        a = angle0 + 2.0 * math.pi * (i + 0.5) / sides
        if not front_facing(a, lim):
            continue
        yaw = a + math.pi / 2.0
        for k in range(per_face):
            off = (k + 0.5) / per_face - 0.5
            dx, dy = math.cos(yaw) * off * width, math.sin(yaw) * off * width
            warm_window(win, x + math.cos(a) * face_r + dx, y + math.sin(a) * face_r + dy,
                        sill, w, h, yaw)
            n += 1
    return n


def turret(stone, win, x, y, z0, z1, r, roof_h, rng, sides=8, rows=2, star=True,
           win_size=(3.0, 5.2), lean_k=0.12, band=True, hang=False, bartizans=0):
    """A round castle tower: a shaft, a gold band under a corbelled top, a
    cone roof and a star. Warm windows in `rows` on its front. A `hang`ing
    turret clings to a wall on a corbelled cone instead of standing."""
    a0 = rng.uniform(0.0, 1.0)
    shaft = prism(stone, (x, y), r, r * 0.95, z0, z1, sides, a0, top=False)
    shade3(stone, shaft, CSTONE)
    if hang:
        base = ring_verts(stone, (x, y), r, z0, sides, a0)
        tip = stone.verts.new((x, y, z0 - r * 1.8))
        shade3(stone, [stone.faces.new((base[(i + 1) % sides], base[i], tip))
                       for i in range(sides)], CSTONE)
    if band:
        corbel = prism(stone, (x, y), r * 0.96, r * 1.1, z1 - r * 0.5, z1, sides, a0, top=False)
        shade3(stone, corbel, GOLD)
        r_top = r * 1.1
    else:
        r_top = r
    ang = rng.uniform(0.0, 2.0 * math.pi)
    lean = (math.cos(ang) * roof_h * lean_k * rng.uniform(0.3, 1.0),
            math.sin(ang) * roof_h * lean_k * rng.uniform(0.3, 1.0))
    shade3(stone, cone_roof(stone, x, y, z1, r_top, roof_h, sides, lean, a0), ROOF)
    if star:
        star_tip(win, x + lean[0], y + lean[1], z1 + roof_h + r * 0.25, max(2.2, r * 0.42))
    # Little hanging turrets clinging under the corbel, turned to the arena.
    for k in range(bartizans):
        a = -math.pi / 2 + (k - (bartizans - 1) / 2) * 1.5
        br = max(2.4, r * 0.34)
        bx, by = x + math.cos(a) * (r + br * 0.5), y + math.sin(a) * (r + br * 0.5)
        turret(stone, win, bx, by, z1 - r * 1.9, z1 - r * 0.3, br, br * 3.4, rng, sides=6,
               rows=1, win_size=(1.8, 3.0), hang=True, band=False)
    w, h = win_size
    span = (z1 - r * 0.6) - z0
    for k in range(rows):
        sill = z1 - r * 0.6 - (k + 1) * span / (rows + 0.6)
        if sill > z0 + 1.5:
            window_row(win, x, y, r * 0.975, sides, a0, sill, w, h)


def house(stone, win, x, y, a, rng, z=0.0):
    """A little town house at (x, y, z) with its front turned out along angle
    `a`: a stone box, a steep violet gable roof and one or two warm windows."""
    w, d = rng.uniform(9.0, 15.0), rng.uniform(8.0, 11.0)
    h = rng.uniform(8.0, 17.0)
    rh = rng.uniform(6.0, 10.0)
    yaw = a + math.pi / 2.0
    m = placed(x, y, z, yaw)
    body = box(stone, (-w / 2, -d / 2, 0.0), (w / 2, d / 2, h))
    bmesh.ops.transform(stone, matrix=m, verts=unique_verts(body))
    shade3(stone, body, CSTONE, jitter=lambda f: rng.uniform(-0.12, 0.12))
    # A gable end turned out to the rim.
    roof = extrude(stone, [(-w / 2 - 1.0, h - 0.6), (w / 2 + 1.0, h - 0.6), (0.0, h + rh)],
                   "y", -d / 2 - 1.2, d / 2 + 1.2)
    bmesh.ops.transform(stone, matrix=m, verts=unique_verts(roof))
    shade3(stone, roof, ROOF)
    n = 1 if w < 11.5 else 2
    for k in range(n):
        off = ((k + 0.5) / n - 0.5) * w * 0.8
        p = m @ Vector((off, -d / 2, 0.0))
        warm_window(win, p.x, p.y, z + h * 0.3, 2.6, 4.2, yaw)


def drum(stone, win, x, y, r, z0, z1, sides, rows, per_face, angle0=0.0, win_size=(3.4, 6.0),
         parapet=True):
    """A castle drum: a wide round (n-gon) wall stage with rows of warm windows
    on its front, a gold string course and a parapet. Returns its top radius."""
    faces = prism(stone, (x, y), r, r, z0, z1, sides, angle0, top=False)
    shade3(stone, faces, CSTONE)
    top_r = r
    if parapet:
        band = prism(stone, (x, y), r * 1.02, r * 1.07, z1 - 2.2, z1, sides, angle0, top=False)
        shade3(stone, band, GOLD)
        par = prism(stone, (x, y), r * 1.07, r * 1.07, z1, z1 + 3.2, sides, angle0, top=False)
        cap = prism(stone, (x, y), r * 1.07, r * 0.9, z1 + 3.2, z1 + 3.2, sides, angle0,
                    top=True)
        shade3(stone, par + cap, CSTONE)
        top_r = r * 0.9
    w, h = win_size
    for k in range(rows):
        sill = z0 + 4.0 + k * (z1 - z0 - 8.0) / rows
        window_row(win, x, y, r, sides, angle0, sill, w, h, per_face)
    return top_r


def panel(bm, pts, m, depth, colour):
    """A flat glass panel through the (u, v) outline `pts` (counter-clockwise
    seen from the front), `depth` metres toward the viewer, placed by `m`."""
    f = bm.faces.new([bm.verts.new(m @ Vector((u, -depth, v))) for u, v in pts])
    palette.tag(bm, [f], colour)
    return f


def rose_window(glass, stone, x, y_face, sill, width, height):
    """The great window (C4's heart): a pointed arch of gold tracery holding a
    rose of teal, white and gold petals round a glowing heart, over four
    teal lancets, in a raised stone frame. Drawn as panels proud of a gold
    backing plate, so the tracery is the plate showing between them."""
    m = placed(x, y_face, sill)
    panel(glass, arch_outline(width, height, n_arc=8), m, 0.3, "gun_brass")
    cv = height * 0.64
    big = width * 0.45
    # The rose: a heart, a ring of petals, an outer ring of cells.
    sectors = 12
    def ring_cell(r0, r1, a0, a1, colour, depth=0.7):
        pts = [(math.cos(a0) * r0, cv + math.sin(a0) * r0), (math.cos(a0) * r1, cv + math.sin(a0) * r1),
               (math.cos(a1) * r1, cv + math.sin(a1) * r1), (math.cos(a1) * r0, cv + math.sin(a1) * r0)]
        return panel(glass, petal_ccw(pts), m, depth, colour)
    heart = [(math.cos(2 * math.pi * k / sectors) * big * 0.2,
              cv + math.sin(2 * math.pi * k / sectors) * big * 0.2) for k in range(sectors)]
    panel(glass, heart, m, 0.8, "far_glass_white")
    gap = 0.09
    for k in range(sectors):
        a0 = 2 * math.pi * (k + gap) / sectors
        a1 = 2 * math.pi * (k + 1 - gap) / sectors
        am = (a0 + a1) / 2
        # A pointed petal.
        petal = [(math.cos(a0) * big * 0.27, cv + math.sin(a0) * big * 0.27),
                 (math.cos(am) * big * 0.66, cv + math.sin(am) * big * 0.66),
                 (math.cos(a1) * big * 0.27, cv + math.sin(a1) * big * 0.27)]
        panel(glass, petal_ccw(petal), m, 0.7,
              "far_glass_teal" if k % 2 else "far_glass_white")
        for half in (0, 1):
            b0 = 2 * math.pi * (k + half * 0.5 + gap * 0.5) / sectors
            b1 = 2 * math.pi * (k + half * 0.5 + 0.5 - gap * 0.5) / sectors
            ring_cell(big * 0.72, big * 0.97, b0, b1,
                      ["far_glass_gold", "far_glass_teal", "glass_cyan", "far_glass_teal"][(2 * k + half) % 4])
    # Four lancets under the rose.
    lw = width * 0.15
    for k in range(4):
        u = (k - 1.5) * width * 0.21
        out = arch_outline(lw, cv - big - height * 0.08, n_arc=3)
        panel(glass, [(u + a, height * 0.05 + b) for a, b in out], m, 0.7,
              "far_glass_teal" if k in (1, 2) else "glass_cyan")
    window_frame(stone, width, height, 3.2, m, proud=1.6)


def petal_ccw(pts):
    """`pts` reordered counter-clockwise (seen from the front, u right, v up)."""
    area = sum(a[0] * b[1] - b[0] * a[1] for a, b in zip(pts, pts[1:] + pts[:1]))
    return pts if area > 0 else list(reversed(pts))


def gate(stone, glass, win, x, y_face, w, h):
    """The terrace's great gate: a tall pointed arch glowing gold, a stone frame,
    and lamps lining the stair down to the rim."""
    m = placed(x, y_face, 0.0)
    outline = arch_outline(w, h, n_arc=5)
    f = glass.faces.new([glass.verts.new(m @ Vector((u, -0.4, v))) for u, v in outline])
    palette.tag(glass, [f], "far_glass_gold")
    inner = arch_outline(w * 0.55, h * 0.72, n_arc=4)
    f = glass.faces.new([glass.verts.new(m @ Vector((u, -0.6, v))) for u, v in inner])
    palette.tag(glass, [f], "far_glass_white")
    window_frame(stone, w, h, 2.4, m, proud=2.2)


def stair(stone, win, x, y0, y1, w):
    """The processional way from the gate to the rim: a pale road with a gold
    kerb and warm lamps along it."""
    road = [stone.verts.new(p) for p in ((x - w / 2, y1, 0.25), (x + w / 2, y1, 0.25),
                                          (x + w / 2, y0, 0.25), (x - w / 2, y0, 0.25))]
    palette.tag(stone, [stone.faces.new(road)], "far_stone_light")
    for side in (-1, 1):
        kerb = box(stone, (x + side * w / 2 - 0.8, y1, 0.0), (x + side * w / 2 + 0.8, y0, 1.6))
        shade3(stone, kerb, GOLD)
        n = 6
        for k in range(n):
            ly = y1 + (y0 - y1) * (k + 0.5) / n
            post = box(stone, (x + side * (w / 2 + 2.0) - 0.5, ly - 0.5, 0.0),
                       (x + side * (w / 2 + 2.0) + 0.5, ly + 0.5, 5.0))
            shade3(stone, post, CSTONE)
            lamp = box(win, (x + side * (w / 2 + 2.0) - 1.4, ly - 1.4, 5.0),
                       (x + side * (w / 2 + 2.0) + 1.4, ly + 1.4, 8.0))
            palette.tag(win, lamp, WARM[1])


def terrace(stone, glass, win, rng):
    """The great terrace, the cake's first layer: an 18-sided walled platform
    with two rows of arcade windows, a gold string course, a parapet, corner
    turrets and the gate in its front face."""
    cx, cy, r, top = TERRACE
    n = TERRACE_SIDES
    a0 = -math.pi / 2 - math.pi / n     # a face centred on the front
    drum(stone, win, cx, cy, r, 0.0, top, n, 2, 4, a0, win_size=(3.6, 7.5))
    for i in range(0, n, 2):
        a = a0 + 2 * math.pi * i / n
        tx, ty = cx + math.cos(a) * r * 1.02, cy + math.sin(a) * r * 1.02
        if not front_facing(a, 0.6):
            continue
        turret(stone, win, tx, ty, 0.0, top + 22.0 + rng.uniform(-3, 6), 6.5, 30.0, rng,
               sides=8, rows=2, win_size=(2.6, 4.6))
    face_y = cy - r * math.cos(math.pi / n)
    gate(stone, glass, win, cx, face_y - 0.1, *GATE)
    stair(stone, win, cx, face_y - 2.5, -PLAT_R + 6.0, 13.0)


def hall(stone, glass, rng):
    """The great hall on the terrace: a tall block with a steep violet roof and
    the rose window, the castle's glowing heart, on its gabled facade; the
    flanking square towers carry tall teal lancets."""
    z0 = TERRACE[3]
    hw, top, peak = HALL_HW, HALL_TOP, HALL_PEAK
    body = extrude(stone, [(-hw, z0), (hw, z0), (hw, top), (0.0, peak), (-hw, top)], "y",
                   HALL_FRONT, HALL_BACK)
    walls = [f for f in body if abs(f.normal.z) < 0.3]
    roofs = [f for f in body if f.normal.z >= 0.3]
    shade3(stone, walls, CSTONE)
    shade3(stone, roofs, ROOF)
    # A gold-edged gable frame proud of the facade, and its pinnacle.
    gable = extrude(stone, [(-hw - 3, top - 6), (-hw, top - 6), (0, peak - 4), (hw, top - 6),
                            (hw + 3, top - 6), (0, peak + 5)], "y", HALL_FRONT - 2.5,
                    HALL_FRONT + 1.0)
    shade3(stone, gable, GOLD)
    w, h, sill = ROSE
    rose_window(glass["nave"], stone, 0.0, HALL_FRONT, sill, w, h)
    # Warm windows up the facade beside the rose.
    for side in (-1, 1):
        for k in range(6):
            warm_window(glass["windows"], side * 39.5, HALL_FRONT, z0 + 10.0 + k * 15.5, 3.4, 7.0)
    # The facade's flanking towers with their lancets.
    fx, fy, fhw, ftop, lancet = FLANKS
    for side in (-1, 1):
        x = side * fx
        faces = prism(stone, (x, fy), fhw * 1.414, fhw * 1.414, z0, ftop, 4, math.pi / 4,
                      top=False)
        shade3(stone, faces, CSTONE)
        band = prism(stone, (x, fy), fhw * 1.5, fhw * 1.56, ftop - 3.0, ftop, 4, math.pi / 4)
        shade3(stone, band, GOLD)
        bel = prism(stone, (x, fy), fhw * 0.95, fhw * 0.9, ftop, ftop + 24.0, 8, math.pi / 8,
                    top=False)
        shade3(stone, bel, CSTONE)
        shade3(stone, cone_roof(stone, x, fy, ftop + 24.0, fhw * 0.98, 70.0, 8,
                                (side * 2.5, 1.5), math.pi / 8), ROOF)
        star_tip(glass["windows"], x + side * 2.5, fy + 1.5, ftop + 24.0 + 70.0 + 3.0, 4.5)
        window_row(glass["windows"], x, fy, fhw * 0.95, 8, math.pi / 8, ftop + 8.0, 3.0, 7.0)
        for k in range(4):
            a = math.pi / 4 + k * math.pi / 2
            pinnacle(stone, x + math.cos(a) * fhw * 1.3, fy + math.sin(a) * fhw * 1.3, ftop,
                     fhw * 2.4, fhw * 0.2)
        lw, lh, lsill, rows, cols = lancet
        glass_window(stone, glass["towers"], x, fy - fhw, lsill, lw, lh, rows, cols,
                     5 + side, frame=1.6, jewels=LANCET_JEWELS, lead_colour="gun_brass")
    # Pinnacles along the hall's eaves.
    for side in (-1, 1):
        for k in range(5):
            pinnacle(stone, side * (hw + 1.0), HALL_FRONT + 14 + k * 18, top, 16, 1.2)


def keep(stone, glass, win, rng):
    """The keep behind the hall: three drums stacked like a wedding cake, each
    ringed with turrets, crowned by the tallest cone and its needle."""
    x, y = KEEP
    for k, (r, z0, z1, sides, rows) in enumerate(KEEP_DRUMS):
        a0 = rng.uniform(0.0, 1.0)
        drum(stone, win, x, y, r, z0, z1, sides, rows, 2 if r > 20 else 1, a0,
             win_size=(3.2, 6.0) if k < 2 else (2.6, 5.0))
        # Turrets clinging to the drum's rim.
        turrets = [(200.0, 340.0, 55.0, 125.0), (215.0, 325.0, 90.0), (245.0, 295.0)][k]
        for deg in turrets:
            a = math.radians(deg)
            tr = [6.2, 4.6, 3.4][k]
            tx, ty = x + math.cos(a) * (r + tr * 0.4), y + math.sin(a) * (r + tr * 0.4)
            t_top = z1 + [26.0, 18.0, 12.0][k] + rng.uniform(-3.0, 4.0)
            turret(stone, win, tx, ty, z1 - [58.0, 36.0, 22.0][k], t_top, tr,
                   [34.0, 26.0, 18.0][k], rng, sides=8 if tr > 4 else 6, rows=2,
                   win_size=(2.4, 4.2), hang=True)
    r, _, z1, sides, _ = KEEP_DRUMS[-1]
    shade3(stone, cone_roof(stone, x, y, z1 + 3.2, r * 1.02, KEEP_SPIRE, 8, (1.5, 2.0)), ROOF)
    star_tip(win, x + 1.5, y + 2.0, z1 + 3.2 + KEEP_SPIRE + 4.0, 7.0)


def town(stone, win, rng):
    """The town round the rim: rows of little houses facing out, with turrets
    among them, thickest across the front and thinning behind."""
    placed_at = []

    def clear(x, y, r):
        if math.hypot(x - TERRACE[0], y - TERRACE[1]) < TERRACE[2] + r + 4.0:
            return False
        if abs(x) < 22.0 and y < 0.0:
            return False  # the processional way
        if abs(x) > 140.0 and any(abs(y - by) < r + 7.0 for _, _, by, _ in BRIDGES):
            return False  # under the bridges
        for tx, ty, *_ in TOWERS:
            for sx in (-1, 1):
                if math.hypot(x - sx * tx, y - ty) < 14.0 + r:
                    return False
        return all(math.hypot(x - px, y - py) > pr + r + 1.5 for px, py, pr in placed_at)

    rows = [(PLAT_R - 17.0, 40), (PLAT_R - 38.0, 30)]
    for ring_r, count in rows:
        for i in range(count):
            a = 2 * math.pi * (i + rng.uniform(-0.3, 0.3)) / count
            if math.sin(a) > 0.55 and rng.random() < 0.6:
                continue  # sparser behind
            rr = ring_r + rng.uniform(-4.0, 4.0)
            x, y = math.cos(a) * rr, math.sin(a) * rr
            if not clear(x, y, 8.0):
                continue
            placed_at.append((x, y, 8.0))
            if rng.random() < 0.24 and not (abs(x) < 50.0 and y < 0.0):
                h = rng.uniform(26.0, 52.0)
                turret(stone, win, x, y, 0.0, h, rng.uniform(4.0, 6.0), rng.uniform(16.0, 26.0),
                       rng, sides=6, rows=2, win_size=(2.6, 4.4))
            else:
                house(stone, win, x, y, a, rng)


def arch_bridge(stone, win, x0, x1, y, z, width, arches, depth):
    """An arcaded bridge along x from x0 to x1 at deck height z: a deck with a
    gold rail over a row of pointed arches."""
    lo, hi = min(x0, x1), max(x0, x1)
    span = (hi - lo) / arches
    prof = [(lo, z + 2.5), (lo, z - depth)]
    for k in range(arches):
        a, b = lo + k * span, lo + (k + 1) * span
        pier = span * 0.16
        prof += [(a + pier, z - depth), (a + pier, z - depth * 0.45),
                 ((a + b) / 2, z - 2.6), (b - pier, z - depth * 0.45), (b - pier, z - depth)]
    prof += [(hi, z - depth), (hi, z + 2.5)]
    faces = extrude(stone, [(p[0], p[1]) for p in prof], "y", y - width / 2, y + width / 2,
                    bottom=True)
    shade3(stone, faces, CSTONE)
    rail = box(stone, (lo, y - width / 2 - 0.4, z + 2.5), (hi, y + width / 2 + 0.4, z + 3.6))
    shade3(stone, rail, GOLD)
    for k in range(arches + 1):
        lx = lo + k * span
        lamp = box(win, (lx - 1.2, y - width / 2 - 1.4, z + 3.6), (lx + 1.2, y - width / 2 + 0.2,
                                                                   z + 6.4))
        palette.tag(win, lamp, WARM[1])


def satellite_cluster(stone, win, c, z, radius, kind, rng):
    """A little castle on a satellite rock: a tall tower and a few houses
    ("town"), or one bridge tower ("tower")."""
    cx, cy = c
    if kind == "tower":
        turret(stone, win, cx, cy, z, z + 96.0, 10.0, 44.0, rng, rows=5)
        for k in range(2):
            a = math.radians(250.0 + 50.0 * k)
            house(stone, win, cx + math.cos(a) * radius * 0.6, cy + math.sin(a) * radius * 0.6,
                  a, rng, z=z)
        return
    turret(stone, win, cx + radius * 0.1, cy + radius * 0.15, z, z + 70.0, 8.0, 36.0, rng, rows=4)
    for k, deg in enumerate((200.0, 330.0, 20.0)):
        a = math.radians(deg)
        turret(stone, win, cx + math.cos(a) * radius * 0.55, cy + math.sin(a) * radius * 0.55, z,
               z + rng.uniform(30.0, 46.0), 4.8, 20.0, rng, sides=6, rows=2, win_size=(2.4, 4.0))
    for k in range(6):
        a = math.radians(215.0 + k * 22.0)
        rr = radius * rng.uniform(0.55, 0.78)
        house(stone, win, cx + math.cos(a) * rr, cy + math.sin(a) * rr, a, rng, z=z)


def build_station(root):
    """The castle station (M3-C4): the starlit citadel on its floating rock."""
    rng = shapes.rng(2026)
    bm = palette.new_bmesh()
    cols = [(0.2, 6, 0.08, 0.22), (0.42, 10, 0.075, 0.2), (0.62, 14, 0.065, 0.17),
            (0.8, 18, 0.055, 0.13), (0.93, 22, 0.04, 0.08)]
    rim = floating_rock(bm, (0.0, 0.0), PLAT_R, ROCK_DEPTH, seed=101, sides=64, cols=cols,
                        plaza=PLAT_R - 10.0, shadow=0.25, crag=True, cone=CASTLE_CONE,
                        top="foliage_shadow")
    sat_rims = []
    for x, y, z, r, d, s, _ in SATELLITES:
        sat_rims.append(floating_rock(bm, (x, y), r, d, seed=s, sides=14, top_z=z, shadow=0.25,
                                      crag=True, top="foliage_shadow"))
    made_part("Base", bm, root)

    stone = palette.new_bmesh()
    glass = {g: palette.new_bmesh() for g in ("nave", "towers", "side", "windows")}
    win = glass["windows"]
    terrace(stone, glass["nave"], win, rng)
    hall(stone, glass, rng)
    keep(stone, glass, win, rng)
    for (x, y, z0, z1, r, roof_h, sides, rows) in TOWERS:
        for side in ((1,) if x == 0.0 else (-1, 1)):
            k = 0.94 if side < 0 else 1.0
            turret(stone, win, side * x, y + (3.0 if side < 0 else 0.0), z0, z0 + (z1 - z0) * k,
                   r, roof_h * (1.0 if side > 0 else 1.06), rng, sides, rows,
                   bartizans=2 if r >= 8.0 and y < 70.0 else 0)
    town(stone, win, rng)
    # Bridges from the great side towers out to the tower rocks.
    for (x0, x1, y, z) in BRIDGES:
        arch_bridge(stone, win, x0, x1, y, z, 9.0, 4, 34.0)
    for (x, y, z, r, d, s, kind) in SATELLITES:
        satellite_cluster(stone, win, (x, y), z, r, kind, rng)
    made_part("Castle", stone, root)
    names = {"nave": "GlassNave", "towers": "GlassTowers", "side": "GlassSide",
             "windows": "GlassWindows"}
    for g, name in names.items():
        if len(glass[g].faces):
            made_part(name, glass[g], root)
        else:
            glass[g].free()

    # Glow points: the rose window, the gate and the flanking lancets; the
    # golden aura round the hall and the keep's crown.
    w, h, sill = ROSE
    scene.make_attach("GlowNave", root, (0.0, HALL_FRONT - 8.0, sill + h * 0.55))
    face_y = TERRACE[1] - TERRACE[2] * math.cos(math.pi / TERRACE_SIDES)
    scene.make_attach("GlowDoor", root, (0.0, face_y - 6.0, GATE[1] * 0.5))
    fx, fy, fhw, _, (lw, lh, lsill, _, _) = FLANKS
    for side, name in ((-1, "Left"), (1, "Right")):
        scene.make_attach(f"GlowTower{name}", root, (side * fx, fy - fhw - 5.0, lsill + lh * 0.5))
    scene.make_attach("AuraHall", root, (0.0, -10.0, 120.0))
    scene.make_attach("AuraKeep", root, (KEEP[0], KEEP[1] + 10.0, 262.0))

    # Waterfalls off the rim and the satellites.
    k = 1
    for deg, width, length in STATION_FALLS:
        waterfall_from_rim(root, k, rim, deg, width, length, inset=3.0)
        k += 1
    for i, deg, width, length in SATELLITE_FALLS:
        x, y, z = SATELLITES[i][:3]
        waterfall_from_rim(root, k, sat_rims[i], deg, width, length, centre=(x, y), z=z,
                           inset=1.5)
        k += 1
    scene.make_attach("Platform", root, (0.0, 0.0, 0.0))
    lowest = max([ROCK_DEPTH] + [l + 1.0 for _, _, l in STATION_FALLS]
                 + [-SATELLITES[i][2] + l + 1.0 for i, _, _, l in SATELLITE_FALLS]
                 + [-z + d for _, _, z, _, d, _, _ in SATELLITES])
    lift_children(root, lowest)


# ---------------------------------------------------------------------------
# Far islands
# ---------------------------------------------------------------------------

def island(root, radius, depth, seed, falls, extras, cols=None, sides=14):
    """A far floating island: grass top, columnar underside, `extras(bm)` on
    top, and waterfalls [(lip angle deg, width, length)] spilling off its front
    (none for the tiny islets)."""
    bm = palette.new_bmesh()
    rim = floating_rock(bm, (0.0, 0.0), radius, depth, seed, sides=sides, cols=cols)
    extras(bm)
    made_part("Island", bm, root)
    lowest = depth
    for k, (deg, width, length) in enumerate(falls):
        suffix = "" if len(falls) == 1 else str(k + 1)
        waterfall_from_rim(root, suffix, rim, deg, width, length, inset=0.8)
        lowest = max(lowest, length + 0.6)
    scene.make_attach("Top", root, (0.0, 0.0, 0.0))
    lift_children(root, lowest)


def build_far_island_a(root):
    """A round grassy island with three puffy trees and a long waterfall (T01, T11)."""
    def extras(bm):
        far_tree(bm, (-7.0, 4.0, 0.6), 1.9, seed=1)
        far_tree(bm, (7.5, 6.0, 0.6), 1.5, seed=2)
        far_tree(bm, (1.0, -8.0, 0.6), 1.1, seed=3)
    island(root, 22.0, 30.0, 201, [(-95.0, 6.5, 70.0)], extras)


def build_far_island_b(root):
    """An island with a little gothic castle, a tree and a waterfall (T01, T11)."""
    def extras(bm):
        far_castle(bm, (2.0, 3.0, 0.4), 1.25, seed=4)
        far_tree(bm, (-10.0, -2.0, 0.6), 1.6, seed=5)
    island(root, 19.0, 28.0, 211, [(-70.0, 5.5, 64.0)], extras)


def build_far_island_c(root):
    """A small craggy island with one tree and a thin waterfall (T11)."""
    def extras(bm):
        far_tree(bm, (1.0, 2.0, 0.6), 1.3, seed=6)
    island(root, 11.0, 20.0, 223, [(-110.0, 3.4, 44.0)], extras,
           cols=[(0.5, 5, 0.2, 0.2)], sides=12)


def build_far_island_d(root):
    """A big island with a castle and a grove, pouring two waterfalls (T11's
    right-hand island)."""
    def extras(bm):
        far_castle(bm, (-4.0, 8.0, 0.4), 1.9, seed=7)
        for k, (x, y, s) in enumerate([(14.0, -6.0, 1.8), (19.0, 6.0, 1.5), (-18.0, -8.0, 1.6),
                                       (4.0, -16.0, 1.2)]):
            far_tree(bm, (x, y, 0.6), s, seed=10 + k)
    island(root, 32.0, 44.0, 231, [(-78.0, 8.0, 92.0), (-124.0, 5.0, 70.0)], extras,
           cols=[(0.3, 5, 0.16, 0.2), (0.6, 8, 0.12, 0.16), (0.85, 10, 0.09, 0.1)], sides=16)


def build_far_islet(root):
    """A tiny floating pebble with a tuft of grass (the targets' specks)."""
    island(root, 5.0, 8.0, 241, [], lambda bm: None, cols=[(0.45, 3, 0.26, 0.25)], sides=8)


# ---------------------------------------------------------------------------
# Ship
# ---------------------------------------------------------------------------

def build_ship(root):
    """A small starship (T01, T11): a pointed silver hull with swept wings, red
    fins, a crystal canopy, twin engines and a blue engine streak."""
    bm = palette.new_bmesh()
    path = [((0, -11.0, 0.0), 0.05), ((0, -8.0, 0.1), 0.9), ((0, -3.5, 0.2), 1.7),
            ((0, 2.0, 0.2), 2.0), ((0, 7.0, 0.1), 1.6)]
    _, sides, caps = shapes.loft(bm, [(c, (lambda r: lambda a: r)(r)) for c, r in path], 6,
                                 angle0=math.pi / 6)
    hull = sides + caps
    shade(bm, hull, "glove_white", shadow="knight_steel", threshold=-0.2)
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
    for s in (-1, 1):
        path = [((s * 3.0, 1.0, -0.3), 0.9), ((s * 3.0, 4.0, -0.3), 1.15), ((s * 3.0, 9.0, -0.3), 1.0)]
        _, esides, ecaps = shapes.loft(bm, [(c, (lambda r: lambda a: r)(r)) for c, r in path], 6)
        shade(bm, esides + ecaps[:1], "gun_iron")
        palette.tag(bm, ecaps[1:], "crystal_blue")
    can = prism(bm, (0.0, -5.0), 0.95, 0.4, 1.6, 2.6, 6, math.pi / 6, top=True)
    palette.tag(bm, can, "crystal_blue")
    made_part("Hull", bm, root)

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
PLANET_TILT = Matrix.Rotation(math.radians(-20.0), 4, "Y") @ Matrix.Rotation(math.radians(-12.0), 4, "X")
# Soft bands by latitude in the planet's frame, lit from above (its pole leans
# toward the key light), so the limb darkens smoothly downward with no stepped
# terminator: (lowest latitude, colour).
BANDS = [(-1.01, "far_planet_shade"), (-0.5, "far_planet_band"), (-0.3, "far_planet"),
         (-0.2, "far_planet_band"), (-0.04, "far_planet"), (0.3, "far_planet_light"),
         (0.4, "far_planet"), (0.52, "far_planet_light")]


def build_planet(root):
    """The ringed gas giant (T01, T03, T11): a soft lavender sphere with subtle
    bands, a shadowed limb away from the key light and one thin pale ring."""
    bm = palette.new_bmesh()
    faces = uv_sphere(bm, PLANET_R, 36, 18)
    bmesh.ops.transform(bm, matrix=PLANET_TILT, verts=unique_verts(faces))
    inv = PLANET_TILT.inverted()
    for f in faces:
        f.normal_update()
        lat = (inv @ f.calc_center_median()).z / PLANET_R
        palette.tag(bm, [f], [n for lo, n in BANDS if lat >= lo][-1])
    made_part("Body", bm, root)

    bm = palette.new_bmesh()
    sides = 56
    bands = [(1.42, 1.47), (1.52, 1.68)]
    ring_faces = []
    for r0, r1 in bands:
        for up in (True, False):
            inner = ring_verts(bm, (0, 0), r0 * PLANET_R, 0.0, sides)
            outer = ring_verts(bm, (0, 0), r1 * PLANET_R, 0.0, sides)
            fs = []
            for i in range(sides):
                j = (i + 1) % sides
                quad = (inner[i], outer[i], outer[j], inner[j])
                fs.append(bm.faces.new(quad if up else tuple(reversed(quad))))
            ring_faces.append((fs, up))
    all_verts = unique_verts([f for fs, _ in ring_faces for f in fs])
    bmesh.ops.transform(bm, matrix=PLANET_TILT, verts=all_verts)
    lowest = max(PLANET_R, max(-v.co.z for v in all_verts))
    for fs, up in ring_faces:
        palette.tag(bm, fs, "far_planet_ring" if up else "far_planet_band")
    made_part("Rings", bm, root)
    scene.make_attach("Center", root, (0.0, 0.0, 0.0))
    lift_children(root, lowest)


ASSETS = [
    Asset("far_island_a", "far_island", build_far_island_a, "grassy far island, three trees, waterfall"),
    Asset("far_island_b", "far_island", build_far_island_b, "far island with a little castle, waterfall"),
    Asset("far_island_c", "far_island", build_far_island_c, "small craggy island with a tree, waterfall"),
    Asset("far_island_d", "far_island", build_far_island_d, "big far island, castle and grove, two waterfalls"),
    Asset("far_islet", "far_island", build_far_islet, "tiny floating pebble with a grass top"),
    Asset("planet", "planet", build_planet, "soft lavender ringed gas giant, radius 90 m"),
    Asset("ship", "ship", build_ship, "small starship with an engine streak, about 20 m"),
    Asset("station", "station", build_station, "the starlit citadel: a castle city on a floating rock"),
]
