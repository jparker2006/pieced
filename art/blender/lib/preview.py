"""Headless EEVEE preview renders for reviewing models without opening Blender.

Writes `art/previews/<name>.png`: a 3/4 view (left) and a front view (right),
768 px each, over the targets' violet sky. Surfaces are shaded like the game's
toon material: a hard two-band step on N.L from one key light, the lit band in
the face's palette colour and the shadow band in that colour's `_shadow`
variant (or the mean lit-to-shadow tint when it has none), plus an inverted-hull
ink outline. Height (metres) and triangle count are printed in the corner.

Runs after export, so nothing done here reaches the .glb.
"""

import math
import os
import tempfile

import bmesh
import bpy
import numpy as np
from mathutils import Vector

from . import bitmap
from . import palette as pal
from .color import srgb_to_linear

SIZE = 768
KEY_DIR = Vector((-0.55, -0.7, 0.75)).normalized()  # towards the key light: front-left, above
TERMINATOR = 0.18  # N.L below this is the shadow band
OUTLINE_PX = 2.5


def _shadow_tint(p):
    ratios = []
    for n in p.names:
        s = p.srgb.get(n + "_shadow")
        if s is None or n.endswith("_shadow"):
            continue
        lit = [srgb_to_linear(c) for c in p.srgb[n]]
        dark = [srgb_to_linear(c) for c in s]
        ratios.append([d / max(l, 1e-4) for d, l in zip(dark, lit)])
    if not ratios:
        return (0.6, 0.6, 0.8)
    return tuple(min(1.0, sum(r[i] for r in ratios) / len(ratios)) for i in range(3))


def _shadow_attribute(mesh, p, tint):
    """Adds `ColShadow`: each corner's palette shadow variant (linear)."""
    col = mesh.color_attributes.get(pal.COLOR_ATTR)
    n = len(mesh.loops)
    lit = np.empty(n * 4, dtype=np.float32)
    col.data.foreach_get("color", lit)
    lit = lit.reshape(n, 4)
    by_key = {}
    for name in p.names:
        key = tuple(round(c, 4) for c in p.linear(name)[:3])
        by_key[key] = name
    out = np.empty_like(lit)
    for i in range(n):
        key = tuple(round(float(c), 4) for c in lit[i, :3])
        name = by_key.get(key)
        shadow = p.shadow_of(name) if name else None
        if shadow is not None:
            out[i, :3] = [srgb_to_linear(c) for c in shadow]
        else:
            out[i, :3] = lit[i, :3] * np.asarray(tint, dtype=np.float32)
        out[i, 3] = 1.0
    sh = mesh.color_attributes.new("ColShadow", "FLOAT_COLOR", "CORNER")
    sh.data.foreach_set("color", out.ravel())


def _socket(sockets, identifier):
    for s in sockets:
        if s.identifier == identifier:
            return s
    raise KeyError(f"no socket {identifier!r} in {[s.identifier for s in sockets]}")


def _toon_material():
    mat = bpy.data.materials.new("PreviewToon")
    mat.use_nodes = True
    nt = mat.node_tree
    nt.nodes.clear()
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    lit = nt.nodes.new("ShaderNodeAttribute")
    lit.attribute_name = pal.COLOR_ATTR
    dark = nt.nodes.new("ShaderNodeAttribute")
    dark.attribute_name = "ColShadow"
    diffuse = nt.nodes.new("ShaderNodeBsdfDiffuse")
    diffuse.inputs["Color"].default_value = (1, 1, 1, 1)
    to_rgb = nt.nodes.new("ShaderNodeShaderToRGB")
    ramp = nt.nodes.new("ShaderNodeValToRGB")
    ramp.color_ramp.interpolation = "CONSTANT"
    ramp.color_ramp.elements[0].position = 0.0
    ramp.color_ramp.elements[0].color = (0, 0, 0, 1)
    ramp.color_ramp.elements[1].position = TERMINATOR
    ramp.color_ramp.elements[1].color = (1, 1, 1, 1)
    mix = nt.nodes.new("ShaderNodeMix")
    mix.data_type = "RGBA"
    emit = nt.nodes.new("ShaderNodeEmission")
    emit.inputs["Strength"].default_value = 1.0
    links = nt.links
    links.new(diffuse.outputs["BSDF"], to_rgb.inputs["Shader"])
    links.new(to_rgb.outputs["Color"], ramp.inputs["Fac"])
    links.new(ramp.outputs["Color"], mix.inputs["Factor"])
    links.new(dark.outputs["Color"], _socket(mix.inputs, "A_Color"))
    links.new(lit.outputs["Color"], _socket(mix.inputs, "B_Color"))
    links.new(_socket(mix.outputs, "Result_Color"), emit.inputs["Color"])
    links.new(emit.outputs["Emission"], out.inputs["Surface"])
    return mat


def _ink_material(p):
    mat = bpy.data.materials.new("PreviewInk")
    mat.use_nodes = True
    nt = mat.node_tree
    nt.nodes.clear()
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    emit = nt.nodes.new("ShaderNodeEmission")
    emit.inputs["Color"].default_value = p.linear("ink")
    nt.links.new(emit.outputs["Emission"], out.inputs["Surface"])
    mat.use_backface_culling = True
    # The hull encloses the model; it must not shadow the model it outlines.
    mat.use_backface_culling_shadow = True
    return mat


def _ground(p, radius, tint):
    bm = bmesh.new()
    bmesh.ops.create_circle(bm, cap_ends=True, radius=radius, segments=48)
    mesh = bpy.data.meshes.new("PreviewGround")
    bm.to_mesh(mesh)
    bm.free()
    col = mesh.color_attributes.new(pal.COLOR_ATTR, "FLOAT_COLOR", "CORNER")
    col.data.foreach_set("color", list(p.linear("grass")) * len(mesh.loops))
    obj = bpy.data.objects.new("PreviewGround", mesh)
    obj.location.z = -0.002
    bpy.context.scene.collection.objects.link(obj)
    return obj


def _look_at(cam, target):
    direction = target - cam.location
    cam.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()


def render(root, out_path, label_lines):
    scene = bpy.context.scene
    p = pal.palette()
    tint = _shadow_tint(p)
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x = SIZE
    scene.render.resolution_y = SIZE
    scene.render.resolution_percentage = 100
    scene.render.film_transparent = True
    scene.render.image_settings.file_format = "PNG"
    scene.render.image_settings.color_mode = "RGBA"
    scene.eevee.taa_render_samples = 16
    scene.view_settings.view_transform = "Standard"
    scene.view_settings.look = "None"
    scene.view_settings.exposure = 0.0
    scene.view_settings.gamma = 1.0
    world = bpy.data.worlds.new("PreviewWorld")
    world.use_nodes = True
    bg = world.node_tree.nodes.get("Background")
    bg.inputs["Color"].default_value = (0, 0, 0, 1)
    bg.inputs["Strength"].default_value = 0.0
    scene.world = world

    # Shade every mesh with the preview toon material and an ink hull.
    toon = _toon_material()
    ink = _ink_material(p)
    meshes = [o for o in [root] + list(root.children_recursive) if o.type == "MESH"]
    points = []
    for obj in meshes:
        points += [obj.matrix_world @ v.co for v in obj.data.vertices]
    lo = Vector((min(v.x for v in points), min(v.y for v in points), min(v.z for v in points)))
    hi = Vector((max(v.x for v in points), max(v.y for v in points), max(v.z for v in points)))
    centre = (lo + hi) / 2
    radius = max((v - centre).length for v in points)
    fov = 2 * math.atan(18.0 / 50.0)
    distance = radius / math.sin(fov / 2) * 1.08
    outline = distance * math.tan(fov / 2) * 2 / SIZE * OUTLINE_PX
    for obj in meshes:
        _shadow_attribute(obj.data, p, tint)
        obj.data.materials.clear()
        obj.data.materials.append(toon)
        obj.data.materials.append(ink)
        mod = obj.modifiers.new("Outline", "SOLIDIFY")
        mod.thickness = outline
        mod.offset = 1.0
        mod.use_flip_normals = True
        mod.use_rim = False
        mod.material_offset = 1
    ground = _ground(p, max(hi.x - lo.x, hi.y - lo.y) * 1.1 + 0.3, tint)
    _shadow_attribute(ground.data, p, tint)
    ground.data.materials.append(toon)

    sun_data = bpy.data.lights.new("Key", "SUN")
    sun_data.energy = math.pi
    sun_data.angle = math.radians(1.0)
    sun = bpy.data.objects.new("Key", sun_data)
    sun.rotation_euler = (-KEY_DIR).to_track_quat("-Z", "Y").to_euler()
    scene.collection.objects.link(sun)

    cam_data = bpy.data.cameras.new("PreviewCam")
    cam_data.lens = 50.0
    cam_data.sensor_width = 36.0
    cam_data.clip_start = 0.05
    cam_data.clip_end = distance * 10
    cam = bpy.data.objects.new("PreviewCam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam

    views = [(35.0, 22.0), (0.0, 6.0)]  # (azimuth to the model's front-right, elevation)
    panels = []
    with tempfile.TemporaryDirectory() as tmp:
        for k, (az, el) in enumerate(views):
            a, e = math.radians(az), math.radians(el)
            cam.location = centre + distance * Vector((math.sin(a) * math.cos(e),
                                                       -math.cos(a) * math.cos(e),
                                                       math.sin(e)))
            _look_at(cam, centre)
            path = os.path.join(tmp, f"view{k}.png")
            scene.render.filepath = path
            bpy.ops.render.render(write_still=True)
            panels.append(bitmap.load_png(path))

    top = np.array(p.srgb["galaxy_deep"], dtype=np.float32)
    bottom = np.array(p.srgb["cloud"], dtype=np.float32) * 0.55 + top * 0.45
    ramp = np.linspace(0.0, 1.0, SIZE, dtype=np.float32)[:, None, None]
    sky = (top[None, None, :] * (1 - ramp) + bottom[None, None, :] * ramp)
    sky = np.broadcast_to(sky, (SIZE, SIZE, 3))
    composed = np.concatenate([bitmap.over(panel, sky) for panel in panels], axis=1)
    y = 14
    for line in label_lines:
        bitmap.text(composed, 14, y, line, (1.0, 1.0, 1.0), scale=5)
        y += 34
    os.makedirs(os.path.dirname(out_path), exist_ok=True)
    bitmap.save_png(composed, out_path)
