"""The cartoon palette (`art/palette.json`) and per-face palette colouring.

Modelling code tags faces with palette *names*; nothing else carries colour.
Tags live in an integer face attribute (`pal`, 1 + an index into the sorted
palette names, 0 = untagged) so they survive joins and bmesh edits, and a face
nobody tagged is caught instead of silently taking the first colour. `bake_colors` turns the tags
into the mesh's active colour attribute `Col` (face corners, linear float), which
the glTF exporter writes as `COLOR_0`. No palette texture is needed.

Lit colours go into `COLOR_0`. The `<name>_shadow` variants document the targets'
shadow band for the toon material and the preview renders; meshes never use them.
"""

import json
import os

import bmesh
import bpy

from .color import hex_to_srgb, srgb_to_linear

PAL_ATTR = "pal"
COLOR_ATTR = "Col"
UNSET = 0

_REPO = os.path.normpath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
PALETTE_PATH = os.path.join(_REPO, "art", "palette.json")


class Palette:
    def __init__(self, path=PALETTE_PATH):
        with open(path) as f:
            raw = json.load(f)
        self.names = sorted(raw)
        self.hex = {n: raw[n].upper() for n in self.names}
        self.srgb = {n: hex_to_srgb(raw[n]) for n in self.names}
        self.index = {n: i for i, n in enumerate(self.names)}

    def idx(self, name):
        try:
            return self.index[name]
        except KeyError:
            raise KeyError(f"'{name}' is not in art/palette.json") from None

    def linear(self, name):
        return tuple(srgb_to_linear(c) for c in self.srgb[name]) + (1.0,)

    def shadow_of(self, name):
        """The palette's shadow variant for a lit colour, if it has one."""
        return self.srgb.get(name + "_shadow")


_PALETTE = None


def palette():
    global _PALETTE
    if _PALETTE is None:
        _PALETTE = Palette()
    return _PALETTE


def new_bmesh():
    """A bmesh with the palette layer already present.

    Adding a face layer invalidates Python references to existing faces, so
    builders that hold face lists before tagging must start from this.
    """
    bm = bmesh.new()
    face_layer(bm)
    return bm


def face_layer(bm):
    """The `pal` int layer of a bmesh, created on first use (new faces read 0)."""
    layer = bm.faces.layers.int.get(PAL_ATTR)
    if layer is None:
        layer = bm.faces.layers.int.new(PAL_ATTR)
    return layer


def tag(bm, faces, name):
    """Tags bmesh faces with a palette colour name."""
    layer = face_layer(bm)
    i = palette().idx(name) + 1
    for f in faces:
        f[layer] = i


def tag_all(bm, name):
    tag(bm, bm.faces, name)


def paint_object(obj, name, where=None):
    """Tags an object's faces (all, or those where `where(face)` is true) with a colour.

    `where` receives the bmesh face in object space (use f.normal, f.calc_center_median()).
    """
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    layer = face_layer(bm)
    i = palette().idx(name) + 1
    for f in bm.faces:
        if where is None or where(f):
            f[layer] = i
    bm.to_mesh(obj.data)
    bm.free()


def face_names(obj):
    """Palette name per face, in face order (for checks and the sidecar)."""
    attr = obj.data.attributes.get(PAL_ATTR)
    if attr is None:
        return []
    names = palette().names
    return [names[v.value - 1] if 1 <= v.value <= len(names) else None for v in attr.data]


def bake_colors(obj):
    """Writes `Col` (corner-domain linear colour) from the face tags and drops the tags.

    Fails loudly if any face was never tagged: every surface must be a palette colour.
    """
    mesh = obj.data
    names = face_names(obj)
    if not names or any(n is None for n in names):
        missing = sum(1 for n in names if n is None) if names else len(mesh.polygons)
        raise ValueError(f"{obj.name}: {missing} face(s) have no palette colour")
    pal = palette()
    for existing in list(mesh.color_attributes):
        mesh.color_attributes.remove(existing)
    col = mesh.color_attributes.new(COLOR_ATTR, "FLOAT_COLOR", "CORNER")
    values = []
    for poly, name in zip(mesh.polygons, names):
        rgba = pal.linear(name)
        values.extend(rgba * poly.loop_total)
    col.data.foreach_set("color", values)
    mesh.color_attributes.active_color = col
    mesh.color_attributes.render_color_index = mesh.color_attributes.active_color_index
    used = sorted(set(names))
    mesh.attributes.remove(mesh.attributes[PAL_ATTR])
    return used


def export_material():
    """The one material every exported mesh uses: white, rough, non-metal, single-sided.

    Bevy's StandardMaterial multiplies base colour by COLOR_0, so the model shows
    its palette colours even before the look slice swaps in the toon material.
    """
    mat = bpy.data.materials.get("Toon")
    if mat is None:
        mat = bpy.data.materials.new("Toon")
        mat.use_nodes = True
        bsdf = mat.node_tree.nodes.get("Principled BSDF")
        bsdf.inputs["Base Color"].default_value = (1.0, 1.0, 1.0, 1.0)
        bsdf.inputs["Roughness"].default_value = 1.0
        bsdf.inputs["Metallic"].default_value = 0.0
        mat.diffuse_color = (1.0, 1.0, 1.0, 1.0)
        mat.use_backface_culling = True  # glTF doubleSided = false: closed meshes, cheaper
    return mat
