"""Geometry helpers: primitives, rounding and bevels, deterministic noise, budgets.

Everything is deterministic: randomness comes from `random.Random(seed)` and
Blender's Perlin noise sampled at seed-derived offsets, never from global state.
Modifiers are applied immediately (`apply_modifiers`), so the exported mesh is
exactly what the build script made.
"""

import math
import random

import bmesh
import bpy
from mathutils import Vector, noise

from . import scene


# ---------------------------------------------------------------------------
# Mesh objects and modifiers
# ---------------------------------------------------------------------------

def mesh_from_bmesh(bm, name="Mesh"):
    mesh = bpy.data.meshes.new(name)
    bm.to_mesh(mesh)
    bm.free()
    return mesh


def temp_object(name, mesh):
    """A scratch object (linked to the scene) used to run modifiers on a mesh."""
    return scene.link(bpy.data.objects.new(name, mesh))


def apply_modifiers(obj):
    """Bakes the object's modifier stack into its mesh and clears the stack."""
    scene.update()
    depsgraph = bpy.context.evaluated_depsgraph_get()
    evaluated = obj.evaluated_get(depsgraph)
    mesh = bpy.data.meshes.new_from_object(evaluated, preserve_all_data_layers=True,
                                           depsgraph=depsgraph)
    old = obj.data
    obj.modifiers.clear()
    obj.data = mesh
    if old.users == 0:
        bpy.data.meshes.remove(old)
    return obj


def bevel(obj, width, segments=2, angle_deg=30.0, profile=0.5):
    """Rounds edges sharper than `angle_deg` (the cartoon 'soft corner')."""
    mod = obj.modifiers.new("Bevel", "BEVEL")
    mod.width = width
    mod.segments = segments
    mod.limit_method = "ANGLE"
    mod.angle_limit = math.radians(angle_deg)
    mod.profile = profile
    mod.harden_normals = False
    return apply_modifiers(obj)


def subdivide(obj, levels=1):
    mod = obj.modifiers.new("Subsurf", "SUBSURF")
    mod.levels = levels
    mod.render_levels = levels
    return apply_modifiers(obj)


def decimate_to(obj, max_triangles):
    """Collapses the mesh down to at most `max_triangles` (no-op if already under)."""
    tris = triangles(obj)
    if tris <= max_triangles:
        return obj
    mod = obj.modifiers.new("Decimate", "DECIMATE")
    mod.decimate_type = "COLLAPSE"
    mod.ratio = max_triangles / tris * 0.985
    mod.use_collapse_triangulate = True
    return apply_modifiers(obj)


def smooth_shading(obj, sharp_angle_deg=35.0):
    """Smooth normals, with edges sharper than the angle kept hard (soft facets)."""
    obj.data.shade_smooth()
    obj.data.set_sharp_from_angle(angle=math.radians(sharp_angle_deg))


def flat_shading(obj):
    obj.data.shade_flat()


def triangles(obj_or_mesh):
    mesh = obj_or_mesh.data if isinstance(obj_or_mesh, bpy.types.Object) else obj_or_mesh
    return sum(len(p.vertices) - 2 for p in mesh.polygons)


def join(objects, name):
    """Joins mesh objects into the first one (face attributes such as palette tags survive)."""
    base = objects[0]
    for other in objects[1:]:
        bm = bmesh.new()
        bm.from_mesh(base.data)
        other_mesh = other.data.copy()
        other_mesh.transform(base.matrix_world.inverted() @ other.matrix_world)
        bm.from_mesh(other_mesh)
        bm.to_mesh(base.data)
        bm.free()
        bpy.data.meshes.remove(other_mesh)
        mesh = other.data
        bpy.data.objects.remove(other)
        if mesh.users == 0:
            bpy.data.meshes.remove(mesh)
    base.name = name
    return base


def detach_mesh(obj):
    """Removes a scratch object and returns its (now unowned) mesh."""
    mesh = obj.data
    bpy.data.objects.remove(obj)
    return mesh


# ---------------------------------------------------------------------------
# Noise and randomness
# ---------------------------------------------------------------------------

def rng(seed):
    return random.Random(seed)


def noise_offset(seed):
    r = random.Random(seed * 7919 + 17)
    return Vector((r.uniform(-500, 500), r.uniform(-500, 500), r.uniform(-500, 500)))


def perlin(p, seed, freq=1.0):
    """Perlin noise in about -1..1 at point p, decorrelated by seed."""
    return noise.noise(Vector(p) * freq + noise_offset(seed), noise_basis="PERLIN_ORIGINAL")


# ---------------------------------------------------------------------------
# Shapes
# ---------------------------------------------------------------------------

def fibonacci_sphere(n):
    """n roughly even unit vectors (deterministic)."""
    pts = []
    golden = math.pi * (3.0 - math.sqrt(5.0))
    for i in range(n):
        z = 1.0 - 2.0 * (i + 0.5) / n
        r = math.sqrt(max(0.0, 1.0 - z * z))
        a = golden * i
        pts.append(Vector((math.cos(a) * r, math.sin(a) * r, z)))
    return pts


def superellipsoid_point(d, exponent):
    """Scales unit direction d onto the superellipsoid |x|^e + |y|^e + |z|^e = 1.

    exponent 2 is a sphere; higher values make a rounded box (chunky cartoon rock).
    """
    s = (abs(d.x) ** exponent + abs(d.y) ** exponent + abs(d.z) ** exponent) ** (1.0 / exponent)
    return d / s


def hull_blob(points):
    """A convex hull bmesh around the points (for faceted boulders)."""
    bm = bmesh.new()
    verts = [bm.verts.new(p) for p in points]
    result = bmesh.ops.convex_hull(bm, input=verts, use_existing_faces=False)
    # Drop interior points the hull did not use.
    unused = {v for key in ("geom_interior", "geom_unused") for v in result.get(key, [])
              if isinstance(v, bmesh.types.BMVert)}
    if unused:
        bmesh.ops.delete(bm, geom=sorted(unused, key=lambda v: v.index), context="VERTS")
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    return bm


def dissolve_flat(bm, angle_deg):
    """Merges nearly coplanar faces into larger facets."""
    bmesh.ops.dissolve_limit(bm, angle_limit=math.radians(angle_deg), use_dissolve_boundaries=False,
                             verts=bm.verts, edges=bm.edges)


def fit_to_box(bm, size, ground=True):
    """Scales a bmesh non-uniformly to the given (x, y, z) extents, centred in XY.

    With `ground`, the lowest point sits on z = 0 (the model pivot is on the ground).
    """
    lo = Vector((min(v.co.x for v in bm.verts), min(v.co.y for v in bm.verts), min(v.co.z for v in bm.verts)))
    hi = Vector((max(v.co.x for v in bm.verts), max(v.co.y for v in bm.verts), max(v.co.z for v in bm.verts)))
    ext = hi - lo
    centre = (lo + hi) / 2
    for v in bm.verts:
        p = v.co - centre
        v.co = Vector((p.x * size[0] / ext.x, p.y * size[1] / ext.y, p.z * size[2] / ext.z))
        if ground:
            v.co.z += size[2] / 2


def ring(bm, n, radius_fn, z_fn, angle0=0.0):
    """A closed ring of n verts; radius_fn(i, angle) and z_fn(i, angle) shape it."""
    verts = []
    for i in range(n):
        a = angle0 + 2.0 * math.pi * i / n
        r = radius_fn(i, a)
        verts.append(bm.verts.new((math.cos(a) * r, math.sin(a) * r, z_fn(i, a))))
    return verts


def bridge(bm, lower, upper):
    """Quads between two rings of equal length (lower -> upper, outward normals)."""
    n = len(lower)
    faces = []
    for i in range(n):
        j = (i + 1) % n
        faces.append(bm.faces.new((lower[i], lower[j], upper[j], upper[i])))
    return faces


def cap_rings(bm, rim, insets, z=None):
    """A flat cap inside a closed CCW rim: concentric bands, then a centre n-gon.

    `insets` are band widths from the rim inwards (metres). Inner rings shrink the
    rim towards its centroid and sit at height `z` (default: the rim's mean), so a
    wobbly rim gets a flat top. Returns (bands, centre): `bands[k]` is the list of
    quads of band k (outermost first), `centre` the final face; all face +Z.
    """
    n = len(rim)
    centre = Vector((sum(v.co.x for v in rim) / n, sum(v.co.y for v in rim) / n,
                     sum(v.co.z for v in rim) / n))
    if z is None:
        z = centre.z
    radius = sum((Vector((v.co.x, v.co.y, 0)) - Vector((centre.x, centre.y, 0))).length
                 for v in rim) / n
    bands = []
    outer = rim
    inset = 0.0
    for w in insets:
        inset += w
        s = max(0.05, 1.0 - inset / radius)
        inner = [bm.verts.new((centre.x + (v.co.x - centre.x) * s,
                               centre.y + (v.co.y - centre.y) * s, z)) for v in rim]
        bands.append(bridge(bm, outer, inner))
        outer = inner
    return bands, bm.faces.new(outer)


def loft(bm, path, segments, cap_start=True, cap_end=True, angle0=0.0):
    """A tube through `path` = [(centre, radius_fn)], radius_fn(angle) -> metres.

    Rings are perpendicular to the path (parallel-transported frames, so the tube
    does not twist). Returns (rings, side_faces, caps).
    """
    pts = [Vector(c) for c, _ in path]
    tangents = []
    for i in range(len(pts)):
        a = pts[max(i - 1, 0)]
        b = pts[min(i + 1, len(pts) - 1)]
        tangents.append((b - a).normalized())
    ref = Vector((1, 0, 0)) if abs(tangents[0].x) < 0.9 else Vector((0, 1, 0))
    normal = (ref - tangents[0] * ref.dot(tangents[0])).normalized()
    rings = []
    for i, (c, radius_fn) in enumerate(path):
        t = tangents[i]
        normal = (normal - t * normal.dot(t)).normalized()
        binormal = t.cross(normal)
        ring = []
        for k in range(segments):
            a = angle0 + 2.0 * math.pi * k / segments
            r = radius_fn(a)
            ring.append(bm.verts.new(pts[i] + (normal * math.cos(a) + binormal * math.sin(a)) * r))
        rings.append(ring)
    sides = []
    for lower, upper in zip(rings, rings[1:]):
        sides += bridge(bm, lower, upper)
    caps = []
    if cap_start:
        caps.append(bm.faces.new(list(reversed(rings[0]))))
    if cap_end:
        caps.append(bm.faces.new(rings[-1]))
    return rings, sides, caps


def metaball_mesh(name, balls, resolution=0.25, threshold=0.6):
    """Polygonises metaballs [(centre, radius), ...] into a mesh (the puffy canopy look)."""
    mb = bpy.data.metaballs.new(name + "Meta")
    mb.resolution = resolution
    mb.render_resolution = resolution
    mb.threshold = threshold
    for centre, radius in balls:
        el = mb.elements.new()
        el.co = centre
        el.radius = radius
        el.stiffness = 2.0
    obj = scene.link(bpy.data.objects.new(name + "Meta", mb))
    scene.update()
    depsgraph = bpy.context.evaluated_depsgraph_get()
    mesh = bpy.data.meshes.new_from_object(obj.evaluated_get(depsgraph), depsgraph=depsgraph)
    bpy.data.objects.remove(obj)
    bpy.data.metaballs.remove(mb)
    mesh.name = name
    return mesh


def skin_mesh(name, nodes, edges, subdivisions=1):
    """A branching tube (trunks, roots, limbs) from a skeleton.

    nodes: [(co, radius)], edges: [(i, j)]; node 0 is the root.
    """
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata([n[0] for n in nodes], edges, [])
    obj = temp_object(name + "Skin", mesh)
    mod = obj.modifiers.new("Skin", "SKIN")
    mod.use_smooth_shade = True
    mod.branch_smoothing = 0.5
    skin = mesh.skin_vertices[0].data
    for i, (_, r) in enumerate(nodes):
        skin[i].radius = (r, r)
    skin[0].use_root = True
    if subdivisions:
        sub = obj.modifiers.new("Subsurf", "SUBSURF")
        sub.levels = subdivisions
        sub.render_levels = subdivisions
    apply_modifiers(obj)
    return obj
