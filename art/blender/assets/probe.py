"""Pipeline fixtures.

`axis_probe` pins the orientation chain (Blender -> glTF -> Bevy). It is a 0.4 m
block with a gold nose on its front (Blender -Y) and an empty named `Forward`
exactly 1 m in front of the block's front face. The Rust orientation test reads
`Forward` from the .glb, applies the spawn helper's fix and checks it lands on
Bevy -Z, and that the sidecar agrees.
"""

import bmesh
import bpy
from mathutils import Matrix

from lib import palette, scene, shapes
from lib.registry import Asset

SIZE = 0.4
FRONT_Y = -SIZE / 2  # the block's front face (Blender -Y is the model's front)


def build_axis_probe(root):
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=SIZE, matrix=Matrix.Translation((0, 0, SIZE / 2)))
    palette.tag_all(bm, "station_stone")
    nose = bmesh.new()
    # A four-sided cone along +Z, turned to point along -Y, on the front face.
    bmesh.ops.create_cone(nose, cap_ends=True, cap_tris=False, segments=4,
                          radius1=0.12, radius2=0.0, depth=0.16)
    nose.transform(Matrix.Translation((0, FRONT_Y - 0.08, SIZE / 2)) @ Matrix.Rotation(1.5707963, 4, "X"))
    palette.tag_all(nose, "spell_gold")
    mesh = shapes.mesh_from_bmesh(nose, "Nose")
    bm.from_mesh(mesh)
    bpy.data.meshes.remove(mesh)
    body = scene.make_part("Body", shapes.mesh_from_bmesh(bm), root)
    shapes.flat_shading(body)
    scene.make_attach("Forward", root, (0.0, FRONT_Y - 1.0, SIZE / 2))


ASSETS = [
    Asset("axis_probe", "probe", build_axis_probe, "orientation test fixture"),
]
