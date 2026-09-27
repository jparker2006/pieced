"""Baked ambient occlusion, stored in the alpha of the exported colour (`COLOR_0.a`).

The game's toon shader multiplies it in (1 = open sky, 0 = fully enclosed), so
crevices, the undersides of props, the places where parts meet and where a
model touches the ground darken without any runtime cost (src/look/toon.rs).

How it is computed (deterministic: fixed ray directions, no randomness):

- Every mesh part of the model is gathered, in world space, into one BVH, so
  parts occlude each other (a gun's rings shade its barrel; a knight's arm
  shades its side).
- AO is evaluated once per (vertex, corner normal) pair: smooth vertices get one
  value, hard edges one per side, so it adds no vertex splits beyond the
  normals'. The sample point sits a hair above the surface along that normal
  (for smooth normals, the plane through the vertex is a supporting plane of a
  convex neighbourhood, so convex areas never occlude themselves).
- `rays` cosine-weighted directions (a Fibonacci spiral) around the normal are
  cast up to `radius` metres. A hit at distance d occludes by (1 - d / radius)²
  (near geometry darkens most, far geometry fades out smoothly).
- `ground`: an implicit ground plane at the model's origin (Blender z = 0) also
  occludes, which gives contact shadows where rocks, stumps, walls and the
  knight's boots meet the grass. Vertices below it read fully occluded. Models
  reaching well below their origin (debris centred on it) skip the plane.

Per-kind settings live in `KIND_AO`; a family module can override one asset
with `Asset(..., ao=AoSettings(...))` (see `registry.py`), and keep a part
fully open with `open_parts` or `obj["pieced_ao"] = False`.
"""

import math
from dataclasses import dataclass, replace

import numpy as np
from mathutils import Vector
from mathutils.bvhtree import BVHTree

from . import palette as pal
from . import scene


@dataclass(frozen=True)
class AoSettings:
    """How one model's AO is baked."""

    radius: float
    """Occluders farther than this (metres) do not darken."""
    ground: bool = False
    """An implicit ground plane at z = 0 occludes too (models that stand on the ground)."""
    rays: int = 48
    """Rays per sample."""
    open_parts: tuple = ()
    """Parts whose names start with one of these stay fully open (AO 1): features
    that must read bright however deep they sit, like eyes inside a visor. A
    part can also opt out with the custom property `obj["pieced_ao"] = False`."""


# Radii are about a tenth of each kind's size: big enough to shade crevices
# and contacts, small enough that open faces stay bright.
KIND_AO = {
    "gun": AoSettings(radius=0.05),
    "gloves": AoSettings(radius=0.035),
    "knight": AoSettings(radius=0.22, ground=True, open_parts=("Eye",)),
    "wall": AoSettings(radius=0.45, ground=True),
    "wall_tiles": AoSettings(radius=0.45, ground=True),
    "floor": AoSettings(radius=0.35),
    "floor_tiles": AoSettings(radius=0.35),
    "ramp": AoSettings(radius=0.4, ground=True),
    "ramp_tiles": AoSettings(radius=0.4, ground=True),
    "cone": AoSettings(radius=0.4),
    "tree": AoSettings(radius=0.7, ground=True),
    "rock": AoSettings(radius=0.35, ground=True),
    "stump": AoSettings(radius=0.25, ground=True),
    "station": AoSettings(radius=6.0, rays=32),
    "far_island": AoSettings(radius=3.0, rays=32),
    "ship": AoSettings(radius=1.5, rays=32),
    "planet": AoSettings(radius=6.0, rays=24),
    "probe": AoSettings(radius=0.1),
}


def settings_for(kind, override=None):
    if override is not None:
        return override
    try:
        return KIND_AO[kind]
    except KeyError:
        raise KeyError(f"no AO settings for asset kind {kind!r}; add it to lib/ao.py KIND_AO") from None


def hemisphere(n):
    """`n` unit directions around +Z, cosine-weighted (Fibonacci spiral)."""
    golden = math.pi * (3.0 - math.sqrt(5.0))
    dirs = []
    for i in range(n):
        u = (i + 0.5) / n
        r = math.sqrt(u)
        phi = i * golden
        dirs.append((r * math.cos(phi), r * math.sin(phi), math.sqrt(max(0.0, 1.0 - u))))
    return dirs


def _basis(nrm):
    helper = Vector((1.0, 0.0, 0.0)) if abs(nrm.x) < 0.9 else Vector((0.0, 1.0, 0.0))
    t = helper.cross(nrm).normalized()
    return t, nrm.cross(t)


def _mesh_parts(root):
    return [o for o in scene.descendants(root) if o.type == "MESH" and len(o.data.polygons) > 0]


def _world_geometry(parts):
    """All parts' triangles in world space: (vertices, triangles)."""
    verts, tris = [], []
    for obj in parts:
        mesh = obj.data
        mesh.calc_loop_triangles()
        mw = obj.matrix_world
        base = len(verts)
        verts.extend(mw @ v.co for v in mesh.vertices)
        tris.extend(tuple(base + i for i in t.vertices) for t in mesh.loop_triangles)
    return verts, tris


def occlusion(bvh, origin, nrm, dirs, radius, ground):
    """Visibility (1 open .. 0 enclosed) of the hemisphere around `nrm` at `origin`."""
    t, b = _basis(nrm)
    occluded = 0.0
    for dx, dy, dz in dirs:
        d = t * dx + b * dy + nrm * dz
        hit = bvh.ray_cast(origin, d, radius)
        dist = hit[3] if hit[0] is not None else None
        if ground and d.z < -1e-6:
            g = origin.z / -d.z if origin.z > 0.0 else 0.0
            if g < radius and (dist is None or g < dist):
                dist = g
        if dist is not None:
            k = 1.0 - dist / radius
            occluded += k * k
    return 1.0 - occluded / len(dirs)


def bake(root, settings):
    """Writes AO into the alpha of every part's `Col` (after `palette.bake_colors`).

    Returns {part name: mean AO} for the log.
    """
    scene.update()
    parts = _mesh_parts(root)
    if not parts:
        return {}
    verts, tris = _world_geometry(parts)
    bvh = BVHTree.FromPolygons(verts, tris, all_triangles=True, epsilon=0.0)
    lo = Vector((min(v.x for v in verts), min(v.y for v in verts), min(v.z for v in verts)))
    hi = Vector((max(v.x for v in verts), max(v.y for v in verts), max(v.z for v in verts)))
    # Lift sample points off the surface by a hair (bigger on big models,
    # where float precision is coarser).
    lift = max(1e-4, (hi - lo).length * 2e-5)
    # Models centred on their origin (debris that tumbles) don't stand on the
    # ground, whatever their kind: only use the plane for models resting on it.
    if settings.ground and lo.z < -0.2 * (hi.z - lo.z):
        settings = replace(settings, ground=False)
    dirs = hemisphere(settings.rays)
    means = {}
    for obj in parts:
        mesh = obj.data
        col = mesh.color_attributes.get(pal.COLOR_ATTR)
        if col is None or col.domain != "CORNER":
            raise ValueError(f"{obj.name}: bake palette colours before AO")
        mw = obj.matrix_world
        nm = mw.to_3x3().inverted_safe().transposed()
        n_loops = len(mesh.loops)
        vidx = np.empty(n_loops, dtype=np.int64)
        mesh.loops.foreach_get("vertex_index", vidx)
        normals = np.empty(n_loops * 3, dtype=np.float64)
        mesh.corner_normals.foreach_get("vector", normals)
        normals = normals.reshape(n_loops, 3)
        world = [mw @ v.co for v in mesh.vertices]
        cache = {}
        ao = np.empty(n_loops, dtype=np.float64)
        stays_open = (not obj.get("pieced_ao", True)
                      or any(obj.name.startswith(p) for p in settings.open_parts))
        for i in range(0 if stays_open else n_loops):
            v = int(vidx[i])
            nx, ny, nz = normals[i]
            key = (v, round(float(nx), 4), round(float(ny), 4), round(float(nz), 4))
            value = cache.get(key)
            if value is None:
                nrm = (nm @ Vector((nx, ny, nz))).normalized()
                if nrm.length < 0.5:
                    value = 1.0
                else:
                    origin = world[v] + nrm * lift
                    if settings.ground and origin.z < -lift:
                        value = 0.0
                    else:
                        value = occlusion(bvh, origin, nrm, dirs, settings.radius,
                                          settings.ground)
                # Quantise so the result never depends on float noise.
                value = round(value, 4)
                cache[key] = value
            ao[i] = value
        if stays_open:
            ao[:] = 1.0
        rgba = np.empty(n_loops * 4, dtype=np.float32)
        col.data.foreach_get("color", rgba)
        rgba = rgba.reshape(n_loops, 4)
        rgba[:, 3] = ao
        col.data.foreach_set("color", rgba.ravel())
        means[obj.name] = float(ao.mean()) if n_loops else 1.0
    return means
