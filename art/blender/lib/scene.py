"""Scene setup and part naming.

Every asset is built in a fresh, empty scene. Its hierarchy is:

    <asset_name>        empty at the origin (the model root; pivot on the ground)
      <Part>            mesh objects, PascalCase (Body, Trunk, Canopy, Hat, ...)
        <Part>          parts may nest (e.g. Hat under Helmet) for procedural animation
      <Attach>          empties marking attach points (Muzzle, Forward, HatPivot, ...)

Orientation: models face Blender -Y (the model's front is what Blender's Front
view looks at), with +Z up and the pivot at ground level. The glTF exporter maps
that to glTF +Z forward, and the Rust spawn helper turns glTF +Z into Bevy -Z.
"""

import re

import bpy

PART_RE = re.compile(r"^[A-Z][A-Za-z0-9]*$")
ASSET_RE = re.compile(r"^[a-z][a-z0-9_]*$")


def reset():
    """Replaces the current file with an empty scene (factory settings, no objects)."""
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene
    scene.unit_settings.system = "METRIC"
    scene.unit_settings.scale_length = 1.0
    return scene


def link(obj):
    bpy.context.scene.collection.objects.link(obj)
    return obj


def check_part_name(name):
    if not PART_RE.match(name):
        raise ValueError(f"part/attach name {name!r} must be PascalCase ASCII (e.g. 'Hat')")


def make_root(asset_name):
    if not ASSET_RE.match(asset_name):
        raise ValueError(f"asset name {asset_name!r} must be snake_case ASCII")
    root = bpy.data.objects.new(asset_name, None)
    root.empty_display_type = "PLAIN_AXES"
    return link(root)


def make_part(name, mesh, parent, location=(0.0, 0.0, 0.0)):
    """A named mesh part. `mesh` is a bpy Mesh; it is renamed to the part name."""
    check_part_name(name)
    if bpy.data.objects.get(name) is not None:
        raise ValueError(f"duplicate part name {name!r}")
    mesh.name = name
    obj = bpy.data.objects.new(name, mesh)
    obj.parent = parent
    obj.location = location
    return link(obj)


def make_attach(name, parent, location, rotation_euler=(0.0, 0.0, 0.0)):
    """A named attach point (an empty). Its -Y axis is the attach point's forward."""
    check_part_name(name)
    if bpy.data.objects.get(name) is not None:
        raise ValueError(f"duplicate attach name {name!r}")
    obj = bpy.data.objects.new(name, None)
    obj.empty_display_type = "ARROWS"
    obj.empty_display_size = 0.2
    obj.parent = parent
    obj.location = location
    obj.rotation_euler = rotation_euler
    return link(obj)


def descendants(root):
    """Root's descendants in a stable order (depth-first, children sorted by name)."""
    out = []

    def walk(obj):
        for child in sorted(obj.children, key=lambda o: o.name):
            out.append(child)
            walk(child)

    walk(root)
    return out


def update():
    bpy.context.view_layer.update()
