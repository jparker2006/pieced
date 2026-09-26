"""Props: rocks, stumps and trees (targets T01, T03, T05, T11).

Chunky, rounded, flat-colour cartoon shapes. Rocks and stumps are the solid arena
props of D29: rocks 1.0-1.4 m tall (crouch cover), stumps at most 0.7 m (jumpable).
Trees stand on the island margin. All sit on the ground at the origin and face
Blender -Y.
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

def boulder(seed, size, points=72, exponent=2.4, lump=0.12, base=-0.45, lean=0.12,
            facet_deg=3.0):
    """A faceted cartoon boulder: the convex hull of lumpy superellipsoid points.

    The hull keeps every facet planar, the superellipsoid keeps the silhouette
    rounded and chunky, and nearly flat facets merge into bigger planes.
    """
    r = shapes.rng(seed)
    pts = []
    for d in shapes.fibonacci_sphere(points):
        p = shapes.superellipsoid_point(d, exponent)
        k = 1.0 + lump * shapes.perlin(d * 1.4, seed) + r.uniform(-0.5, 0.5) * lump * 0.4
        p = p * k
        p.z = max(p.z, base)  # flat underside, a little inside the widest point
        p.x += lean * max(0.0, p.z)  # lean the crown so it isn't symmetric
        pts.append(p)
    bm = shapes.hull_blob(pts)
    shapes.dissolve_flat(bm, facet_deg)
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


def finish_rock(root, mesh):
    body = scene.make_part("Body", mesh, root)
    shapes.flat_shading(body)
    return body


def build_rock_a(root):
    """A big leaning boulder, 1.25 m: the main crouch cover (T05, T03)."""
    main = boulder(seed=11, size=(1.9, 1.5, 1.25), points=110, exponent=2.5, lean=0.16)
    finish_rock(root, rock_mesh([(main, Vector((0, 0, 0)))]))
    scene.make_attach("Top", root, (0.0, 0.0, 1.25))


def build_rock_b(root):
    """A rounder 1.05 m boulder with a small buddy rock at its front-right (T01)."""
    main = boulder(seed=23, size=(1.45, 1.3, 1.05), points=90, exponent=2.2, lean=-0.1)
    buddy = boulder(seed=29, size=(0.7, 0.62, 0.5), points=40, exponent=2.3, lean=0.05)
    finish_rock(root, rock_mesh([(main, Vector((0, 0, 0))), (buddy, Vector((0.72, -0.42, 0)))]))
    scene.make_attach("Top", root, (0.0, 0.0, 1.05))


# ---------------------------------------------------------------------------
# Stumps
# ---------------------------------------------------------------------------

def build_stump_a(root):
    """A sawn-off stump, 0.56 m: bark plates with dark grooves, root flare, ring top (T03, T11)."""
    bm = palette.new_bmesh()
    plates = 10
    groove = 0.2  # fraction of each plate step taken by the dark groove
    step = 2 * math.pi / plates
    angles = []
    for k in range(plates):
        angles += [(k * step, "plate"), (k * step + step * (1 - groove), "groove")]
    roots = [0.4, 1.9, 3.3, 4.9]  # root lobe directions (radians)

    def lobes(a):
        return sum(max(0.0, math.cos(a - c)) ** 4 for c in roots)

    height = 0.56
    rng = shapes.rng(5)
    tops = [height - 0.02 + rng.uniform(-0.02, 0.02) for _ in angles]
    levels = [
        (lambda i, a: 0.0, lambda a: 0.52 + 0.12 * lobes(a)),
        (lambda i, a: 0.13, lambda a: 0.45 + 0.04 * lobes(a)),
        (lambda i, a: tops[i], lambda a: 0.44),
    ]
    rings = []
    for z_fn, r_fn in levels:
        ring = []
        for i, (a, kind) in enumerate(angles):
            rr = r_fn(a) * (0.955 if kind == "groove" else 1.0)
            ring.append(bm.verts.new((math.cos(a) * rr, math.sin(a) * rr, z_fn(i, a))))
        rings.append(ring)
    for lower, upper in zip(rings, rings[1:]):
        faces = shapes.bridge(bm, lower, upper)
        for i, f in enumerate(faces):
            palette.tag(bm, [f], "stump_bark_line" if angles[i][1] == "groove" else "stump_bark")
    bands, centre = shapes.cap_rings(bm, rings[-1], [0.035, 0.085, 0.024, 0.085, 0.024],
                                     z=height)
    for k, band in enumerate(bands):
        palette.tag(bm, band, "stump_ring_line" if k in (0, 2, 4) else "stump_rings")
    palette.tag(bm, [centre], "stump_rings")
    body = scene.make_part("Body", shapes.mesh_from_bmesh(bm), root)
    shapes.smooth_shading(body, sharp_angle_deg=50.0)
    scene.make_attach("Top", root, (0.0, 0.0, height))


# ---------------------------------------------------------------------------
# Trees
# ---------------------------------------------------------------------------

def build_tree_a(root):
    """A puffy cartoon tree for the island margin, about 5.6 m (T01, T05)."""
    # Trunk: a lofted tube with a lobed root flare at the foot, curving gently up
    # to a fork; three limbs grow from the fork into the canopy.
    bm = palette.new_bmesh()
    roots = [0.3, 2.2, 4.1]

    def flare(amount):
        return lambda a: 1.0 + amount * sum(max(0.0, math.cos(a - c)) ** 3 for c in roots)

    trunk_path = [
        ((0.0, 0.0, 0.0), 0.44, flare(0.75)),
        ((0.01, 0.0, 0.14), 0.38, flare(0.4)),
        ((0.03, 0.0, 0.4), 0.32, flare(0.12)),
        ((0.08, 0.01, 0.95), 0.28, flare(0.0)),
        ((0.13, 0.02, 1.6), 0.26, flare(0.0)),
        ((0.12, 0.03, 2.3), 0.25, flare(0.0)),
        ((0.1, 0.03, 2.75), 0.23, flare(0.0)),
    ]
    rings, sides, caps = shapes.loft(
        bm, [(c, (lambda f, r: lambda a: r * f(a))(f, r)) for c, r, f in trunk_path], 12)
    for v in rings[0]:
        v.co.z = 0.0  # the foot ring follows the tilted tangent; sit it flat on the ground
    limbs = [
        [((0.1, 0.03, 2.3), 0.19), ((-0.35, 0.05, 2.95), 0.15), ((-0.8, 0.06, 3.5), 0.12)],
        [((0.12, 0.02, 2.4), 0.19), ((0.6, -0.08, 3.0), 0.15), ((1.0, -0.14, 3.55), 0.12)],
        [((0.1, 0.05, 2.6), 0.17), ((0.15, 0.3, 3.2), 0.13), ((0.2, 0.45, 3.7), 0.11)],
    ]
    for limb in limbs:
        _, limb_sides, limb_caps = shapes.loft(
            bm, [(c, (lambda r: lambda a: r)(r)) for c, r in limb], 8)
        sides += limb_sides
        caps += limb_caps
    palette.tag_all(bm, "trunk")
    trunk_obj = scene.make_part("Trunk", shapes.mesh_from_bmesh(bm), root)
    shapes.smooth_shading(trunk_obj, sharp_angle_deg=60.0)

    # Canopy: a cauliflower of puffs, a ring of eight round the middle, five on
    # top, one crown, all melted together by metaballs.
    rng = shapes.rng(41)
    cx, cy = 0.1, 0.05
    balls = [((cx, cy, 3.95), 1.3)]
    for k in range(8):
        a = 2 * math.pi * k / 8 + rng.uniform(-0.15, 0.15)
        rad = 1.4 + rng.uniform(-0.1, 0.12)
        balls.append(((cx + math.cos(a) * rad, cy + math.sin(a) * rad * 0.9,
                       3.75 + rng.uniform(-0.2, 0.3)), 0.92 + rng.uniform(-0.1, 0.12)))
    for k in range(5):
        a = 2 * math.pi * k / 5 + 0.4 + rng.uniform(-0.2, 0.2)
        balls.append(((cx + math.cos(a) * 0.8, cy + math.sin(a) * 0.75,
                       4.7 + rng.uniform(-0.1, 0.15)), 0.85 + rng.uniform(-0.08, 0.08)))
    balls.append(((cx, cy, 5.2), 0.8))
    canopy_mesh = shapes.metaball_mesh("Canopy", balls, resolution=0.14, threshold=0.6)
    canopy = shapes.temp_object("CanopyTmp", canopy_mesh)
    shapes.decimate_to(canopy, 1500 - shapes.triangles(trunk_obj) - 20)
    bm = bmesh.new()
    bm.from_mesh(canopy.data)
    palette.tag_all(bm, "foliage")
    mesh = shapes.mesh_from_bmesh(bm)
    shapes.detach_mesh(canopy)
    canopy_obj = scene.make_part("Canopy", mesh, root)
    shapes.smooth_shading(canopy_obj, sharp_angle_deg=60.0)


ASSETS = [
    Asset("rock_a", "rock", build_rock_a, "big leaning boulder, 1.25 m"),
    Asset("rock_b", "rock", build_rock_b, "round boulder with a buddy rock, 1.05 m"),
    Asset("stump_a", "stump", build_stump_a, "sawn stump with rings, 0.56 m"),
    Asset("tree_a", "tree", build_tree_a, "puffy margin tree, about 5.6 m"),
]
