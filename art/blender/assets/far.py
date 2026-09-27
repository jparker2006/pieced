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

# Three-tone ramps (lit, mid, shadow) for the far stone and rock.
STONE = ("far_stone_light", "far_stone", "far_stone_shadow")
SPIRE = ("far_stone", "far_stone_shadow", "far_stone_dark")
ROCK = ("far_rock", "far_rock_shadow", "far_rock_dark")

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


def cone_depth(f):
    """Depth fraction of the underside's cone at radius fraction `f` from the axis."""
    pts = [(1.0, 0.1)] + [(r, d) for d, r, _ in CONE] + [(0.0, 1.0)]
    for (r0, d0), (r1, d1) in zip(pts, pts[1:]):
        if r1 <= f <= r0:
            t = (f - r1) / max(r0 - r1, 1e-6)
            return d1 + (d0 - d1) * t
    return 0.1


def floating_rock(bm, centre, radius, depth, seed, sides=14, cols=None, top="grass",
                  top_z=0.0, plaza=0.0, squash=1.0, shadow=0.0):
    """A floating mass as the targets paint it (T01, T10, T11): a flat grass top
    at `top_z` whose lip drapes over the rim in drips, a band of rock faces and
    a jagged inverted cone of rock, with rock columns jutting below it as
    stalactites, so the underside reads as columnar and spiky.

    `cols` lists the stalactite rings as (radius fraction, count, column radius
    fraction, extra length as a fraction of `depth`). With `plaza` > 0 the
    top's middle (that radius) is stone, for a building. `shadow` (0..0.5)
    turns more of the rock to its shadow tones (the station's rock is
    backlit by the galaxy in T01 and T10). Returns the rim as
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
    rings = [lip] + [ring(r, -d * depth, jag, drop=0.04) for d, r, jag in CONE]
    apex = bm.verts.new((cx + rng.uniform(-0.08, 0.08) * radius,
                         cy + rng.uniform(-0.08, 0.08) * radius, top_z - depth * 0.97))
    rock_faces = []
    for upper, lower in zip(rings, rings[1:]):
        rock_faces += shapes.bridge(bm, lower, upper)
    rock_faces += [bm.faces.new((rings[-1][i], apex, rings[-1][(i + 1) % sides]))
                   for i in range(sides)]
    shade3(bm, rock_faces, ROCK, hi=0.18 + shadow, lo=-0.28 + shadow, jitter=jitter)
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
            surface = cone_depth(fj) * depth
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
    shade3(bm, col_faces, ROCK, hi=0.2 + shadow, lo=-0.25 + shadow, jitter=jitter)
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


def glass_colour(scheme, u, v, width, height, k, j, rows, cols, rng, memo):
    """The palette colour of a glass cell centred at (u, v)."""
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
                memo[key] = JEWELS[(ring * 3 + sector * 2 + (sector + ring) % 2) % len(JEWELS)]
            return memo[key]
        key = ("l", k // 2, round(m))
        if key not in memo:
            memo[key] = JEWELS[rng.randrange(len(JEWELS))]
        return memo[key]
    # Tall lancets: symmetric jewel blocks with a gold-and-white medallion.
    centre_row = rows * 0.62
    if abs(j - mid) + abs(k - centre_row) * 0.6 < 1.3:
        return "far_glass_gold" if (k + j) % 2 else "far_glass_white"
    key = ("b", k // 2, round(m))
    if key not in memo:
        memo[key] = JEWELS[rng.randrange(len(JEWELS))]
    return memo[key]


def stained_glass(bm, width, height, rows, cols, seed, matrix, scheme="lancet", lead=0.14):
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
                                          rng, memo))
    leads = []
    for f, _, _ in cells:
        size = min(e.calc_length() for e in f.edges)
        res = bmesh.ops.inset_individual(bm, faces=[f], thickness=size * lead, depth=0.0,
                                         use_even_offset=True)
        leads += res["faces"]
    palette.tag(bm, leads, LEAD)
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
                 scheme="lancet", yaw=0.0, frame=None):
    """A stained-glass window set flush into a wall facing -Y at y = `y_face`
    (after turning by `yaw` about the window's centre line): the glass a hair
    proud of the wall, a raised stone frame round it."""
    fw = frame if frame is not None else max(1.2, width * 0.09)
    m = placed(x, y_face, sill, yaw)
    stained_glass(glass, width, height, rows, cols, seed, m @ Matrix.Translation((0, -0.3, 0)),
                  scheme)
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
# Station: a massive dark gothic cathedral on a floating rock (T01, T02, T10)
# ---------------------------------------------------------------------------

PLAT_R = 150.0          # the platform's grass rim
ROCK_DEPTH = 118.0      # platform top to the cone's apex
ARCADE_R = 126.0        # the lower arcade wall round the cathedral
ARCADE_H = 30.0
RING_Z = 40.0           # the ring walkway, just above the arcade
RING_R = (193.0, 203.0)
NAVE_HW = 30.0          # the nave: half width, front and back, wall top, roof peak
NAVE_Y = (-66.0, 64.0)
NAVE_TOP = 128.0
NAVE_PEAK = 172.0
# The great west window on the nave front: width, height, sill.
GREAT_WINDOW = (44.0, 118.0, 36.0)
# The crossing flèche: its base on the roof and its tip, the station's top.
FLECHE = (146.0, 267.0)

# Towers: (x, y, base z, shaft top, half width, spire height, shape, window)
# with window = None or (group, width, height, sill, rows, cols). Mirrored for
# every x > 0; "sq" towers carry four corner pinnacles. The whole cathedral is
# broader than it is tall, as the targets paint it from the arena: about 300 m
# across its towers and 267 m to the flèche's tip.
TOWERS = [
    # The facade's flanking towers.
    (40.0, -64.0, ARCADE_H, 150.0, 8.5, 64.0, "sq", ("side", 7.0, 64.0, 66.0, 10, 2)),
    # The wing towers with the great tall windows (T10's far left and right).
    (98.0, -40.0, ARCADE_H, 146.0, 16.0, 52.0, "sq", ("towers", 22.0, 104.0, 38.0, 16, 5)),
    # Between the facade and the wings.
    (68.0, -54.0, ARCADE_H, 118.0, 7.0, 56.0, "oct", ("side", 4.4, 40.0, 62.0, 7, 2)),
    # Outer towers on the platform's edge.
    (136.0, -14.0, 0.0, 104.0, 8.0, 50.0, "oct", ("side", 4.6, 36.0, 50.0, 6, 2)),
    (124.0, 40.0, 0.0, 86.0, 7.0, 44.0, "oct", None),
    # Behind: the skyline's depth, at several heights.
    (58.0, 26.0, ARCADE_H, 138.0, 8.0, 62.0, "oct", None),
    (36.0, 80.0, ARCADE_H, 116.0, 7.0, 52.0, "oct", None),
    (90.0, 70.0, ARCADE_H, 100.0, 7.5, 46.0, "sq", None),
    (18.0, 112.0, 0.0, 90.0, 6.0, 42.0, "oct", None),
    (112.0, -74.0, 0.0, 64.0, 5.5, 34.0, "oct", None),
]
# Thin pinnacles on the arcade's top, every few metres round it.
ARCADE_SEGMENTS = 40
# Flying buttresses: y positions along the nave, from the aisles to the nave wall.
BUTTRESS_YS = (-36.0, -8.0, 20.0, 48.0)
# Satellite rocks under the ring: (angle deg, radius, top z, rock radius, depth, seed).
SATELLITES = [(205.0, 212.0, -16.0, 28.0, 56.0, 131), (236.0, 206.0, -26.0, 22.0, 46.0, 133),
              (304.0, 206.0, -22.0, 24.0, 50.0, 137), (336.0, 212.0, -12.0, 30.0, 58.0, 139),
              (160.0, 208.0, -28.0, 20.0, 40.0, 141), (20.0, 208.0, -24.0, 20.0, 42.0, 143)]
# Waterfalls off the platform rim: (angle deg, width, length).
STATION_FALLS = [(238.0, 15.0, 185.0), (257.0, 12.0, 170.0), (283.0, 17.0, 195.0),
                 (302.0, 12.0, 160.0)]
# Waterfalls off satellite rocks: (satellite index, width, length).
SATELLITE_FALLS = [(0, 9.0, 130.0), (3, 10.0, 140.0), (1, 7.0, 110.0)]


def tower(stone, glass, x, y, z0, z1, hw, spire, shape, window, seed):
    """A gothic tower: shaft, cornice, a tall spire and (square towers) four
    corner pinnacles; a stained-glass window on its front."""
    if shape == "sq":
        faces = prism(stone, (x, y), hw * 1.414, hw * 1.414, z0, z1, 4, math.pi / 4, top=False)
        for k in (0.34, 0.68):
            zc = z0 + (z1 - z0) * k
            faces += prism(stone, (x, y), hw * 1.52, hw * 1.52, zc, zc + 1.8, 4, math.pi / 4)
        faces += prism(stone, (x, y), hw * 1.56, hw * 1.56, z1, z1 + hw * 0.35, 4, math.pi / 4)
        shade3(stone, faces, STONE)
        # An octagonal belfry stage, then the spire.
        bel = prism(stone, (x, y), hw * 0.95, hw * 0.9, z1 + hw * 0.35, z1 + hw * 2.0, 8,
                    math.pi / 8, top=False)
        shade3(stone, bel, STONE)
        shade3(stone, pyramid(stone, (x, y), hw * 1.0, z1 + hw * 2.0, spire, 8, math.pi / 8),
               SPIRE)
        for k in range(4):
            a = math.pi / 4 + k * math.pi / 2
            px, py = x + math.cos(a) * hw * 1.3, y + math.sin(a) * hw * 1.3
            pinnacle(stone, px, py, z1 + hw * 0.35, hw * 2.6, hw * 0.2)
        face_y = y - hw
    else:
        faces = prism(stone, (x, y), hw, hw, z0, z1, 8, math.pi / 8, top=False)
        zc = z0 + (z1 - z0) * 0.6
        faces += prism(stone, (x, y), hw * 1.08, hw * 1.08, zc, zc + 1.6, 8, math.pi / 8)
        faces += prism(stone, (x, y), hw * 1.14, hw * 1.14, z1, z1 + hw * 0.3, 8, math.pi / 8)
        shade3(stone, faces, STONE)
        shade3(stone, pyramid(stone, (x, y), hw * 1.02, z1 + hw * 0.3, spire, 8, math.pi / 8),
               SPIRE)
        for k in range(4):
            a = k * math.pi / 2
            px, py = x + math.cos(a) * hw * 1.05, y + math.sin(a) * hw * 1.05
            pinnacle(stone, px, py, z1 + hw * 0.3, hw * 1.9, hw * 0.16)
        face_y = y - hw * math.cos(math.pi / 8)
    if window is not None:
        group, w, h, sill, rows, cols = window
        glass_window(stone, glass[group], x, face_y, sill, w, h, rows, cols, seed)


def nave(stone, glass):
    """The nave: a tall block with a steep roof, the great window on its front,
    aisles, a transept, the apse and the crossing flèche (the tallest point)."""
    y0, y1 = NAVE_Y
    hw = NAVE_HW
    top, peak = NAVE_TOP, NAVE_PEAK
    body = extrude(stone, [(-hw, ARCADE_H), (hw, ARCADE_H), (hw, top), (0, peak), (-hw, top)],
                   "y", y0, y1)
    shade3(stone, body, STONE)
    # A raised gable frame round the front's peak and a finial.
    gable = extrude(stone, [(-hw - 3, top - 5), (-hw, top - 5), (0, peak - 3), (hw, top - 5),
                            (hw + 3, top - 5), (0, peak + 5)], "y", y0 - 2.0, y0 + 2.0)
    shade3(stone, gable, STONE)
    pinnacle(stone, 0.0, y0, peak + 3, 26.0, 2.0)
    w, h, sill = GREAT_WINDOW
    glass_window(stone, glass["nave"], 0.0, y0, sill, w, h, 24, 10, 1, scheme="rose", frame=3.0)
    # Aisles with lean-to roofs.
    for side in (-1, 1):
        prof = ([(hw, ARCADE_H), (hw + 26, ARCADE_H), (hw + 26, 82), (hw, 96)] if side > 0 else
                [(-hw - 26, ARCADE_H), (-hw, ARCADE_H), (-hw, 96), (-hw - 26, 82)])
        shade3(stone, extrude(stone, prof, "y", y0 + 16, y1 - 10), STONE)
    # Transept.
    tr = extrude(stone, [(-8, ARCADE_H), (30, ARCADE_H), (30, 116), (11, 140), (-8, 116)],
                 "x", -84, 84)
    shade3(stone, tr, STONE)
    # Apse: a polygonal end with a conical roof.
    apse = prism(stone, (0, y1), 26, 26, ARCADE_H, 112, 10, math.pi / 10, top=False)
    shade3(stone, apse, STONE)
    shade3(stone, pyramid(stone, (0, y1), 28, 112, 40, 10, math.pi / 10), SPIRE)
    # The crossing flèche.
    base, tip = FLECHE
    fl = prism(stone, (0, 11), 9.5, 9, base, base + 22, 8, math.pi / 8, top=False)
    fl += prism(stone, (0, 11), 11, 11, base + 22, base + 25, 8, math.pi / 8)
    shade3(stone, fl, STONE)
    shade3(stone, pyramid(stone, (0, 11), 10, base + 25, tip - base - 25, 8, math.pi / 8), SPIRE)
    for k in range(8):
        a = math.pi / 8 + k * math.pi / 4
        pinnacle(stone, math.cos(a) * 12, 11 + math.sin(a) * 12, base + 22, 20, 1.1)
    # Flying buttresses: piers on the aisles' outer walls with pinnacles, and
    # slanted arms up to the nave wall.
    for side in (-1, 1):
        for y in BUTTRESS_YS:
            px = side * (hw + 29)
            pier = oriented_box(stone, (px, y, (ARCADE_H + 96) / 2), (5.0, 4.0, 96 - ARCADE_H),
                                0.0, bottom=False)
            shade3(stone, pier, STONE)
            pinnacle(stone, px, y, 96, 24, 2.2)
            arm = (extrude(stone, [(hw + 26, 84), (hw + 29, 90), (hw, 122), (hw, 115)], "y",
                           y - 1.4, y + 1.4) if side > 0 else
                   extrude(stone, [(-hw - 29, 90), (-hw - 26, 84), (-hw, 115), (-hw, 122)], "y",
                           y - 1.4, y + 1.4))
            shade3(stone, arm, STONE)
    # Pinnacles along the nave's eaves.
    for side in (-1, 1):
        for k in range(6):
            pinnacle(stone, side * (hw + 1.5), y0 + 14 + k * 20, top, 18, 1.3)


def arcade(stone, warm):
    """The lower arcade: a curtain wall round the cathedral with two rows of
    warm lit arched windows on the side facing the arena, a balcony on top and
    a row of thin pinnacles."""
    n = ARCADE_SEGMENTS
    r0, r1 = ARCADE_R, ARCADE_R + 4.0
    a0 = math.pi / n
    base_o = ring_verts(stone, (0, 0), r1, 0.0, n, a0)
    top_o = ring_verts(stone, (0, 0), r1, ARCADE_H, n, a0)
    lip_o = ring_verts(stone, (0, 0), r1 + 3.0, ARCADE_H, n, a0)
    lip_t = ring_verts(stone, (0, 0), r1 + 3.0, ARCADE_H + 3.0, n, a0)
    in_t = ring_verts(stone, (0, 0), r0, ARCADE_H + 3.0, n, a0)
    faces = shapes.bridge(stone, base_o, top_o)
    faces += [stone.faces.new((top_o[i], top_o[(i + 1) % n], lip_o[(i + 1) % n], lip_o[i]))
              for i in range(n)]
    faces += shapes.bridge(stone, lip_o, lip_t)
    faces += [stone.faces.new((lip_t[i], lip_t[(i + 1) % n], in_t[(i + 1) % n], in_t[i]))
              for i in range(n)]
    shade3(stone, faces, STONE)
    face_r = r1 * math.cos(math.pi / n)
    for i in range(n):
        a = a0 + 2 * math.pi * (i + 0.5) / n
        if math.sin(a) > 0.35:
            continue  # the back: no windows
        x, y = math.cos(a) * face_r, math.sin(a) * face_r
        yaw = a + math.pi / 2
        warm_window(warm, x, y, 4.0, 4.6, 14.0, yaw)
        warm_window(warm, x, y, 21.0, 3.4, 6.5, yaw)
    for i in range(n):
        a = a0 + 2 * math.pi * i / n
        pier = oriented_box(stone, (math.cos(a) * (r1 + 0.9), math.sin(a) * (r1 + 0.9),
                                    ARCADE_H / 2), (2.6, 2.6, ARCADE_H), a, bottom=False)
        shade3(stone, pier, STONE)
        if i % 2 == 0:
            pinnacle(stone, math.cos(a) * (r1 + 1.5), math.sin(a) * (r1 + 1.5), ARCADE_H + 3.0,
                     13.0 + 6.0 * ((i // 2) % 3 == 0), 1.0)


def ring_walkway(bm, lamps):
    """The floating ring walkway round the cathedral: a deck with a parapet,
    carried on six bridges from the arcade, lit by warm lamps along its rim."""
    sides = 72
    r0, r1 = RING_R
    z0, z1 = RING_Z - 2.2, RING_Z + 2.2
    inner_lo = ring_verts(bm, (0, 0), r0, z0, sides)
    inner_hi = ring_verts(bm, (0, 0), r0, z1, sides)
    outer_lo = ring_verts(bm, (0, 0), r1, z0, sides)
    outer_hi = ring_verts(bm, (0, 0), r1, z1, sides)
    rail = ring_verts(bm, (0, 0), r1, z1 + 2.4, sides)
    rail_in = ring_verts(bm, (0, 0), r1 - 1.0, z1 + 2.4, sides)
    rail_foot = ring_verts(bm, (0, 0), r1 - 1.0, z1, sides)
    faces = shapes.bridge(bm, outer_lo, outer_hi)
    faces += shapes.bridge(bm, inner_hi, inner_lo)
    faces += shapes.bridge(bm, outer_hi, rail)
    faces += shapes.bridge(bm, rail, rail_in)
    faces += shapes.bridge(bm, rail_in, rail_foot)
    for i in range(sides):
        j = (i + 1) % sides
        faces.append(bm.faces.new((inner_hi[i], inner_hi[j], rail_foot[j], rail_foot[i])))
        faces.append(bm.faces.new((outer_lo[i], outer_lo[j], inner_lo[j], inner_lo[i])))
    shade3(bm, faces, STONE)
    for i in range(0, sides, 2):
        a = 2 * math.pi * (i + 0.5) / sides
        m = (Matrix.Translation((math.cos(a) * (r1 + 0.1), math.sin(a) * (r1 + 0.1), z0 + 0.8))
             @ Matrix.Rotation(a + math.pi / 2, 4, "Z"))
        quad = [lamps.verts.new(m @ Vector(p))
                for p in ((-1.6, 0, 0), (1.6, 0, 0), (1.6, 0, 2.2), (-1.6, 0, 2.2))]
        palette.tag(lamps, [lamps.faces.new(quad)], WARM[1])
    for deg in (0.0, 60.0, 120.0, 180.0, 215.0, 325.0):
        a = math.radians(deg)
        mid = (ARCADE_R + r0) / 2.0
        length = r0 - ARCADE_R + 2.0
        spoke = oriented_box(bm, (math.cos(a) * mid, math.sin(a) * mid, RING_Z - 1.0),
                             (length, 6.0, 3.0), a)
        shade3(bm, spoke, STONE)
        # A pinnacle where the bridge meets the ring.
        pinnacle(bm, math.cos(a) * r1, math.sin(a) * r1, z1, 20.0, 1.4)


def build_station(root):
    """The cathedral station (T10, T01, T02): a massive dark gothic cathedral of
    slate-blue stone bristling with spires and pinnacles at every height, with
    buttresses and tall pointed stained-glass windows set flush in its walls
    (a huge jewel-toned great window in the middle), a lower arcade of warm
    windows, a ring walkway round it, and a big jagged rock underside with
    satellite crags, pouring waterfalls from its rim."""
    # Base: the floating rock with a grass rim round a stone plaza, and the
    # satellite crags under the ring.
    bm = palette.new_bmesh()
    cols = [(0.22, 5, 0.09, 0.2), (0.45, 9, 0.08, 0.18), (0.66, 13, 0.07, 0.15),
            (0.86, 17, 0.06, 0.12)]
    rim = floating_rock(bm, (0.0, 0.0), PLAT_R, ROCK_DEPTH, seed=101, sides=40, cols=cols,
                        plaza=PLAT_R - 12.0, shadow=0.3)
    sat_rims = []
    for deg, dist, z, r, d, s in SATELLITES:
        a = math.radians(deg)
        c = (math.cos(a) * dist, math.sin(a) * dist)
        sat_rims.append((c, z, floating_rock(bm, c, r, d, seed=s, sides=12, top_z=z, shadow=0.3)))
    made_part("Base", bm, root)

    stone = palette.new_bmesh()
    glass = {g: palette.new_bmesh() for g in ("nave", "towers", "side", "windows")}
    nave(stone, glass)
    for k, (x, y, z0, z1, hw, spire, shape, window) in enumerate(TOWERS):
        for side in (-1, 1):
            tower(stone, glass, side * x, y, z0, z1, hw, spire, shape, window, 11 + 2 * k + side)
    arcade(stone, glass["windows"])
    ring_walkway(stone, glass["windows"])
    # The crystal over the door (T10): a tall blue-white diamond with its lamp.
    w, h, sill = GREAT_WINDOW
    cy = -(ARCADE_R + 16.0)
    res = bmesh.ops.create_cone(glass["nave"], cap_ends=False, segments=4, radius1=0.0,
                                radius2=5.5, depth=28.0,
                                matrix=Matrix.Translation((0.0, cy, 17.0)))
    low = faces_of(res["verts"])
    res = bmesh.ops.create_cone(glass["nave"], cap_ends=False, segments=4, radius1=5.5,
                                radius2=0.0, depth=20.0,
                                matrix=Matrix.Translation((0.0, cy, 41.0)))
    high = faces_of(res["verts"])
    shade(glass["nave"], low + high, "far_glass_white", shadow="crystal_blue", threshold=-0.1)
    made_part("Cathedral", stone, root)
    names = {"nave": "GlassNave", "towers": "GlassTowers", "side": "GlassSide",
             "windows": "GlassWindows"}
    for g, name in names.items():
        made_part(name, glass[g], root)

    # Glow points: the great window, the crystal and each wing tower's window.
    y0 = NAVE_Y[0]
    scene.make_attach("GlowNave", root, (0.0, y0 - 6.0, sill + h * 0.55))
    scene.make_attach("GlowCrystal", root, (0.0, cy - 3.0, 31.0))
    wing = TOWERS[1]
    for side, name in ((-1, "Left"), (1, "Right")):
        _, ww, wh, wsill, _, _ = wing[7]
        scene.make_attach(f"GlowTower{name}", root,
                          (side * wing[0], wing[1] - wing[4] - 6.0, wsill + wh * 0.5))

    # Waterfalls pouring off the rim and the satellite crags.
    k = 1
    for deg, width, length in STATION_FALLS:
        waterfall_from_rim(root, k, rim, deg, width, length, inset=3.0)
        k += 1
    for i, width, length in SATELLITE_FALLS:
        c, z, srim = sat_rims[i]
        a = math.degrees(math.atan2(c[1], c[0]))
        waterfall_from_rim(root, k, srim, a, width, length, centre=c, z=z, inset=1.5)
        k += 1
    scene.make_attach("Platform", root, (0.0, 0.0, 0.0))
    lowest = max([ROCK_DEPTH] + [l + 1.0 for _, _, l in STATION_FALLS]
                 + [-SATELLITES[i][2] + l + 1.0 for i, _, l in SATELLITE_FALLS])
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
BANDS = [(-1.01, "far_planet_shadow"), (-0.5, "far_planet_band"), (-0.3, "far_planet"),
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
    Asset("station", "station", build_station, "gothic cathedral station on a floating rock"),
]
