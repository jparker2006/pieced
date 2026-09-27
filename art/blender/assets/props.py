"""Props: rocks, stumps and trees (targets T01, T03, T05, T11).

Chunky, rounded, flat-colour cartoon shapes. Rocks and stumps are the solid arena
props of D29: rocks 1.0-1.4 m tall (crouch cover), stumps at most 0.7 m (jumpable).
Trees stand on the island margin. All sit on the ground at the origin and face
Blender -Y.

Painted detail (D48) is baked into the face colours, never into the shapes (the
solid props' colliders are fitted to their bounds): rocks mix lighter and darker
facets and grow moss on their crowns, stumps alternate bark shades and keep a
dark heart in their rings, and tree canopies catch sunlit highlights.
"""

import math

import bmesh
import bpy
from mathutils import Matrix, Vector

from lib import palette, scene, shapes
from lib.registry import Asset


# ---------------------------------------------------------------------------
# Rocks
# ---------------------------------------------------------------------------
#
# The painted rocks (T01, T03, T05, T11) are smooth, rounded grey boulders:
# a soft pebble silhouette with a few broad, gently rounded facets, a lighter
# top plane and darker flanks, no moss. So a rock is the convex hull of a lumpy
# superellipsoid (the same shape its collider is hulled from), with near-flat
# faces merged into broad planes and smooth normals across every edge gentler
# than a facet's corner, which rounds the shading without losing the planes.


def boulder(seed, size, points=110, exponent=2.3, lump=0.1, base=-0.45, lean=0.1,
            facet_deg=7.0):
    """A rounded cartoon boulder, fitted to `size` (x, y, z), standing on z = 0."""
    r = shapes.rng(seed)
    pts = []
    for d in shapes.fibonacci_sphere(points):
        p = shapes.superellipsoid_point(d, exponent)
        k = 1.0 + lump * shapes.perlin(d * 1.1, seed) + r.uniform(-0.5, 0.5) * lump * 0.25
        p = p * k
        p.z = max(p.z, base)  # flat underside, a little inside the widest point
        p.x += lean * max(0.0, p.z)  # lean the crown so it isn't symmetric
        pts.append(p)
    bm = shapes.hull_blob(pts)
    shapes.dissolve_flat(bm, facet_deg)
    bmesh.ops.triangulate(bm, faces=bm.faces[:], quad_method="BEAUTY", ngon_method="BEAUTY")
    shapes.fit_to_box(bm, size, ground=True)
    return bm


def rock_mesh(parts):
    """Joins boulders [(bmesh, offset)] into one rock mesh tagged `rock`."""
    out = bmesh.new()
    for bm, offset in parts:
        bm.transform(Matrix.Translation(offset))
        mesh = shapes.mesh_from_bmesh(bm)
        out.from_mesh(mesh)
        bpy.data.meshes.remove(mesh)
    palette.tag_all(out, "rock")
    return shapes.mesh_from_bmesh(out)


def paint_rock(mesh, seed):
    """Facet colours for a rock, painted in planes like the target rocks: a
    warm, light top (`rock`) on the faces turned up to the sky, one flank in the
    same light stone and the other in cool grey (`rock_dark`), so the broad
    facets read from any light; the toon bands shade each on top of that."""
    del seed
    bm = bmesh.new()
    bm.from_mesh(mesh)
    bm.normal_update()
    palette.tag_all(bm, "rock")
    palette.tag(bm, [f for f in bm.faces
                     if f.normal.z < 0.5 and (f.normal.x + 0.35 * f.normal.y > 0.1 or f.normal.z < -0.3)],
                "rock_dark")
    bm.to_mesh(mesh)
    bm.free()
    return mesh


def finish_rock(root, mesh):
    body = scene.make_part("Body", mesh, root)
    shapes.smooth_shading(body, sharp_angle_deg=24.0)
    return body


def build_rock_a(root):
    """A big, rounded leaning boulder, 1.25 m: the main crouch cover (T05, T03)."""
    main = boulder(seed=11, size=(1.9, 1.5, 1.25), points=120, exponent=2.4, lean=0.14)
    finish_rock(root, paint_rock(rock_mesh([(main, Vector((0, 0, 0)))]), 11))
    scene.make_attach("Top", root, (0.0, 0.0, 1.25))


def build_rock_b(root):
    """A rounder 1.05 m boulder with a small buddy rock at its front-right (T01)."""
    main = boulder(seed=23, size=(1.45, 1.3, 1.05), points=96, exponent=2.2, lean=-0.1)
    buddy = boulder(seed=29, size=(0.7, 0.62, 0.5), points=40, exponent=2.3, lean=0.05)
    finish_rock(root, paint_rock(rock_mesh([(main, Vector((0, 0, 0))),
                                            (buddy, Vector((0.72, -0.42, 0)))]), 23))
    scene.make_attach("Top", root, (0.0, 0.0, 1.05))


# ---------------------------------------------------------------------------
# Stumps
# ---------------------------------------------------------------------------

def build_stump_a(root):
    """A sawn-off stump, 0.56 m (T01, T03, T11): a round bark barrel of plates
    split by thin dark grooves, four roots flaring out into the grass, and a
    pale ring top inside a bark rim, with two dark growth rings."""
    bm = palette.new_bmesh()
    plates = 9
    step = 2 * math.pi / plates
    angles = []
    for k in range(plates):
        angles += [(k * step, "plate"), (k * step + step * 0.78, "groove")]
    roots = [0.5, 2.1, 3.6, 5.2]  # root directions (radians)

    def lobes(a):
        return sum(max(0.0, math.cos(a - c)) ** 8 for c in roots)

    height = 0.56
    rng = shapes.rng(5)
    tops = [height - 0.015 + rng.uniform(-0.015, 0.015) for _ in angles]
    levels = [
        (lambda i: 0.0, lambda a: 0.49 + 0.14 * lobes(a)),
        (lambda i: 0.13, lambda a: 0.465 + 0.035 * lobes(a)),
        (lambda i: tops[i], lambda a: 0.45),
    ]
    rings = []
    for z_fn, r_fn in levels:
        ring = []
        for i, (a, kind) in enumerate(angles):
            rr = r_fn(a) * (0.965 if kind == "groove" else 1.0)
            ring.append(bm.verts.new((math.cos(a) * rr, math.sin(a) * rr, z_fn(i))))
        rings.append(ring)
    for lower, upper in zip(rings, rings[1:]):
        faces = shapes.bridge(bm, lower, upper)
        for i, f in enumerate(faces):
            if angles[i][1] == "groove":
                color = "stump_bark_line"
            else:
                color = "trunk" if (i // 2) % 3 == 1 else "stump_bark"
            palette.tag(bm, [f], color)
    # The top: a bark rim, pale wood, a dark growth ring, pale wood, another
    # dark ring, and the pale heart.
    bands, centre = shapes.cap_rings(bm, rings[-1], [0.045, 0.1, 0.026, 0.09, 0.026], z=height)
    for k, band in enumerate(bands):
        palette.tag(bm, band, ["stump_bark", "stump_rings", "stump_ring_line", "stump_rings",
                               "stump_ring_line"][k])
    palette.tag(bm, [centre], "stump_rings")
    body = scene.make_part("Body", shapes.mesh_from_bmesh(bm), root)
    shapes.smooth_shading(body, sharp_angle_deg=50.0)
    scene.make_attach("Top", root, (0.0, 0.0, height))


# ---------------------------------------------------------------------------
# Trees
# ---------------------------------------------------------------------------
#
# The painted trees (T01, T03, T05, T11) have round, lobed, cloud-puff crowns:
# big overlapping balls of leaves, each lit on its upper side, sunlit
# yellow-green on top and deep green underneath, with an ink line where one
# puff tucks behind another. So a crown here is a cluster of separate smooth
# spheres (not one melted blob): every puff shades round on its own, the
# creases between them stay crisp, and the outline follows the scalloped edge.
# The faces of a puff buried inside its neighbours are dropped, which is what
# pays for round puffs within the 1.5k budget. Trunks are thick, curvy lofts
# with a flared, rooted foot and limbs that fork out below the crown and reach
# up into it, so the branches show under the leaves as they do in the paintings.


def puff(bm, centre, radius, squash, segments, bands, seed, lump=0.06):
    """A smooth lat-long ball (a crown puff) into `bm`; returns its faces.

    `squash` flattens it vertically; `lump` wobbles the radius a little so the
    puffs don't look machined. Poles are triangle fans, the rest quads.
    """
    r = shapes.rng(seed)
    spin = r.uniform(0.0, 2 * math.pi)
    tilt = Vector((r.uniform(-0.12, 0.12), r.uniform(-0.12, 0.12), 1.0)).normalized()
    rot = Vector((0, 0, 1)).rotation_difference(tilt)

    def point(d):
        k = 1.0 + lump * shapes.perlin(d * 1.7, seed)
        v = rot @ d
        return Vector(centre) + Vector((v.x * radius * k, v.y * radius * k,
                                        v.z * radius * squash * k))

    top = bm.verts.new(point(Vector((0, 0, 1))))
    bottom = bm.verts.new(point(Vector((0, 0, -1))))
    rings = []
    for b in range(1, bands):
        lat = math.pi * b / bands
        ring = []
        for s in range(segments):
            a = spin + 2 * math.pi * (s + 0.5 * (b % 2)) / segments
            d = Vector((math.sin(lat) * math.cos(a), math.sin(lat) * math.sin(a), math.cos(lat)))
            ring.append(bm.verts.new(point(d)))
        rings.append(ring)
    faces = []
    n = segments
    for s in range(n):
        faces.append(bm.faces.new((top, rings[0][s], rings[0][(s + 1) % n])))
    for upper, lower in zip(rings, rings[1:]):
        for s in range(n):
            faces.append(bm.faces.new((upper[s], lower[s], lower[(s + 1) % n], upper[(s + 1) % n])))
    for s in range(n):
        faces.append(bm.faces.new((bottom, rings[-1][(s + 1) % n], rings[-1][s])))
    return faces


def crown(root, puffs, seed, budget=1200):
    """The `Canopy` part: `puffs` = [(centre, radius, squash)], each a smooth ball
    (10 × 7 facets for the big ones, 9 × 6 for the small).

    A face every corner of which lies inside another puff is hidden for good
    and dropped. Faces are painted before they are split into triangles, so the
    colours follow each puff's rings: sunlit `grass_light` caps on each puff's
    upper side (the painted crowns' lighter tops, strongest high in the crown),
    `foliage_light` on the upper flanks and `foliage` below; the toon bands and
    baked AO darken the undersides and the creases between puffs.
    """
    bm = palette.new_bmesh()
    owners = []
    for k, (c, rad, squash) in enumerate(puffs):
        segments, bands = (10, 7) if rad >= 0.93 else (9, 6)
        for f in puff(bm, c, rad, squash, segments, bands, seed * 97 + k):
            owners.append((f, k))
    # A polygonal puff lies inside its sphere; test against a slightly smaller one.
    shells = [(Vector(c), rad * 0.94, squash) for c, rad, squash in puffs]

    def buried(v, own):
        for j, (c, rad, squash) in enumerate(shells):
            if j == own:
                continue
            d = v.co - c
            if (d.x * d.x + d.y * d.y) / (rad * rad) + (d.z * d.z) / (rad * rad * squash * squash) < 1.0:
                return True
        return False

    dead = [f for f, k in owners if all(buried(v, k) for v in f.verts)]
    bmesh.ops.delete(bm, geom=dead, context="FACES")
    bm.normal_update()
    lo = min(v.co.z for v in bm.verts)
    hi = max(v.co.z for v in bm.verts)
    palette.tag_all(bm, "foliage")
    light, mid = [], []
    for f in bm.faces:
        h = (f.calc_center_median().z - lo) / max(hi - lo, 1e-6)
        nz = f.normal.z
        if nz + 0.3 * h > 0.8:
            light.append(f)
        elif nz + 0.25 * h > 0.28:
            mid.append(f)
    palette.tag(bm, mid, "foliage_light")
    palette.tag(bm, light, "grass_light")
    bmesh.ops.triangulate(bm, faces=bm.faces[:], quad_method="FIXED", ngon_method="BEAUTY")
    tris = len(bm.faces)
    if tris > budget:
        raise ValueError(f"canopy has {tris} triangles, over its share of {budget}")
    obj = scene.make_part("Canopy", shapes.mesh_from_bmesh(bm), root)
    obj.data.shade_smooth()
    return obj


def trunk(root, stem, limbs, roots, segments=10, limb_segments=7, seed=0):
    """The `Trunk` part: a lofted `stem` [(centre, radius)] with a flared, rooted
    foot (`roots` = root directions, radians) and lofted `limbs` [[(centre, radius)]]
    forking off it. Bark: `trunk`, with darker `stump_bark` streaks running up it."""
    bm = palette.new_bmesh()

    def flare(amount):
        return lambda a: 1.0 + amount * sum(max(0.0, math.cos(a - c)) ** 4 for c in roots)

    flares = [0.9, 0.42, 0.14, 0.03] + [0.0] * len(stem)
    rings, sides, caps = shapes.loft(
        bm, [(c, (lambda f, r: lambda a: r * f(a))(flare(flares[i]), r))
             for i, (c, r) in enumerate(stem)], segments, cap_start=True, cap_end=True)
    for v in rings[0]:
        v.co.z = 0.0  # the foot ring sits flat on the ground
    streak = shapes.rng(seed)
    dark = {k for k in range(segments) if streak.random() < 0.3}
    palette.tag_all(bm, "trunk")
    for i, f in enumerate(sides):
        if (i % segments) in dark and i // segments >= 1:
            palette.tag(bm, [f], "stump_bark")
    for limb in limbs:
        _, limb_sides, _ = shapes.loft(
            bm, [(c, (lambda r: lambda a: r)(r)) for c, r in limb], limb_segments)
        palette.tag(bm, limb_sides, "trunk")
        for i, f in enumerate(limb_sides):
            if (i % limb_segments) in (1, 4):
                palette.tag(bm, [f], "stump_bark")
    # Every face must carry a colour, caps included.
    layer = palette.face_layer(bm)
    palette.tag(bm, [f for f in bm.faces if f[layer] == 0], "trunk")
    obj = scene.make_part("Trunk", shapes.mesh_from_bmesh(bm), root)
    shapes.smooth_shading(obj, sharp_angle_deg=70.0)
    return obj


def ring_of(n, radius, z, puff_r, squash, seed, cx=0.0, cy=0.0, jitter=0.12, phase=0.0,
            stretch=(1.0, 1.0)):
    """`n` puffs round a ring (for crowns)."""
    r = shapes.rng(seed)
    out = []
    for k in range(n):
        a = phase + 2 * math.pi * k / n + r.uniform(-jitter, jitter)
        rad = radius * (1.0 + r.uniform(-0.08, 0.08))
        out.append(((cx + math.cos(a) * rad * stretch[0], cy + math.sin(a) * rad * stretch[1],
                     z + r.uniform(-0.18, 0.2)),
                    puff_r * (1.0 + r.uniform(-0.1, 0.12)), squash))
    return out


def build_tree_a(root):
    """A round cartoon tree, about 5.8 m (T01, T05): a stout trunk with a rooted
    foot that curves up and forks into three limbs under a round, lobed crown of
    cloud puffs, sunlit on top."""
    trunk(root,
          stem=[((0.0, 0.0, 0.0), 0.4), ((0.02, 0.0, 0.16), 0.36), ((0.05, 0.01, 0.5), 0.31),
                ((0.1, 0.02, 1.05), 0.28), ((0.16, 0.03, 1.55), 0.27), ((0.15, 0.03, 1.85), 0.24),
                ((0.13, 0.03, 2.1), 0.14)],
          limbs=[
              [((0.12, 0.03, 1.65), 0.2), ((-0.35, 0.08, 2.4), 0.16), ((-0.85, 0.12, 3.0), 0.12),
               ((-1.15, 0.14, 3.45), 0.09)],
              [((0.16, 0.02, 1.65), 0.2), ((0.65, -0.12, 2.45), 0.16), ((1.1, -0.2, 3.05), 0.12),
               ((1.3, -0.24, 3.45), 0.09)],
              [((0.15, 0.05, 1.7), 0.19), ((0.2, 0.45, 2.6), 0.14), ((0.22, 0.8, 3.3), 0.1)],
              [((0.14, 0.03, 1.75), 0.17), ((0.05, -0.2, 2.8), 0.13), ((0.0, -0.3, 3.5), 0.1)],
          ],
          roots=[0.3, 1.9, 3.4, 4.9], seed=7)
    cx, cy = 0.05, 0.05
    puffs = [((cx, cy, 4.25), 1.35, 0.9)]
    puffs += ring_of(7, 1.5, 3.85, 0.98, 0.86, 41, cx, cy)
    puffs += ring_of(5, 0.9, 4.75, 0.92, 0.88, 42, cx, cy, phase=0.4)
    puffs += [((cx + 0.1, cy - 0.05, 5.25), 0.82, 0.9)]
    crown(root, puffs, seed=43, budget=1500 - 330)


def build_tree_b(root):
    """A big, broad framing tree, about 6.2 m (T01, T03, T09, T11): a thick
    trunk leaning and curving up, a big limb swinging out to one side, and a
    wide crown of cloud puffs in two masses over its limbs."""
    trunk(root,
          stem=[((0.0, 0.0, 0.0), 0.52), ((-0.02, 0.0, 0.18), 0.46), ((-0.07, 0.01, 0.55), 0.39),
                ((-0.18, 0.02, 1.15), 0.35), ((-0.3, 0.03, 1.75), 0.33), ((-0.35, 0.03, 2.1), 0.29),
                ((-0.37, 0.03, 2.4), 0.16)],
          limbs=[
              [((-0.33, 0.03, 1.95), 0.27), ((-0.62, 0.06, 2.85), 0.22), ((-0.95, 0.1, 3.5), 0.16),
               ((-1.1, 0.12, 3.95), 0.12)],
              [((-0.28, 0.02, 1.55), 0.24), ((0.3, -0.05, 2.25), 0.2), ((0.95, -0.12, 2.8), 0.16),
               ((1.45, -0.18, 3.3), 0.12)],
              [((-0.34, 0.04, 2.0), 0.2), ((-0.3, 0.55, 2.95), 0.15), ((-0.25, 0.9, 3.6), 0.11)],
          ],
          roots=[0.9, 2.5, 4.0, 5.4], seed=13)
    puffs = []
    # The main mass over the leaning stem, and a second, lower one over the limb.
    puffs += [((-0.75, 0.1, 4.55), 1.35, 0.88), ((1.2, -0.15, 3.95), 1.05, 0.86)]
    puffs += ring_of(7, 1.45, 4.15, 0.95, 0.85, 61, -0.7, 0.1, stretch=(1.0, 0.95))
    puffs += ring_of(4, 0.85, 3.8, 0.8, 0.85, 62, 1.25, -0.15, phase=0.3)
    puffs += ring_of(4, 0.85, 5.05, 0.9, 0.88, 63, -0.7, 0.1, phase=0.6)
    puffs += [((-0.6, 0.05, 5.55), 0.78, 0.9)]
    crown(root, puffs, seed=67, budget=1500 - 300)


ASSETS = [
    Asset("rock_a", "rock", build_rock_a, "big leaning boulder, 1.25 m"),
    Asset("rock_b", "rock", build_rock_b, "round boulder with a buddy rock, 1.05 m"),
    Asset("stump_a", "stump", build_stump_a, "sawn stump with rings, 0.56 m"),
    Asset("tree_a", "tree", build_tree_a, "round cartoon tree with a puff crown, about 5.8 m"),
    Asset("tree_b", "tree", build_tree_b, "big broad framing tree, about 6.2 m"),
]
