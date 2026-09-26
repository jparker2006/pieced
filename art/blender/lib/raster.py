"""A small deterministic toon rasterizer for UI images (hotbar icons, the logo).

EEVEE and Workbench render on the GPU, so their pixels can drift between
machines and drivers; the UI images are committed and `build-art.sh --check`
compares them byte for byte. So they are drawn here instead, in numpy on the
CPU: flat per-face palette colours in two toon bands (lit and the palette's
`_shadow` variant), glowing crystal faces, an orthographic camera, a thick ink
outline around the silhouette, thin ink lines where colours or depths break,
supersampled anti-aliasing, and a PNG writer with no timestamps.

    tris = gather(root, glass={"Chamber"}, glow={"Crystal"}, skip={"Shard"})
    image = render(tris, view(azimuth=35, elevation=20), (128, 128), outline_px=3.5)
    write_png(image, "assets/ui/icons/rifle.png")

Images are float arrays (height, width, 4), top row first, sRGB values in 0..1
with straight (not premultiplied) alpha, like `lib.bitmap`.
"""

import math
import os
import struct
import zlib

import numpy as np
from mathutils import Vector

from . import palette as pal
from . import scene
from .color import linear_to_srgb, srgb_to_linear

# Light direction in camera space (x right, y up, z towards the viewer): from
# the upper left, a little in front.
KEY_CAMERA = (-0.45, 0.75, 0.5)
TERMINATOR = 0.12
# Glass is drawn over what is behind it at this opacity.
GLASS_ALPHA = 0.38


class Tris:
    """Triangles in world space with their shading inputs (numpy arrays)."""

    def __init__(self, verts, normals, lit, shadow, group, glow, glass):
        self.verts = verts      # (n, 3, 3) world positions
        self.normals = normals  # (n, 3) unit face normals, world space
        self.lit = lit          # (n, 3) sRGB lit colour
        self.shadow = shadow    # (n, 3) sRGB shadow-band colour
        self.group = group      # (n,) int: faces of one part and colour share a group
        self.glow = glow        # (n,) bool: emissive (no shadow band)
        self.glass = glass      # (n,) bool: drawn translucent over the rest

    def __len__(self):
        return len(self.verts)

    @staticmethod
    def concat(parts):
        parts = [p for p in parts if len(p)]
        offset = 0
        groups = []
        for p in parts:
            groups.append(p.group + offset)
            offset += int(p.group.max()) + 1 if len(p) else 0
        return Tris(np.concatenate([p.verts for p in parts]),
                    np.concatenate([p.normals for p in parts]),
                    np.concatenate([p.lit for p in parts]),
                    np.concatenate([p.shadow for p in parts]),
                    np.concatenate(groups),
                    np.concatenate([p.glow for p in parts]),
                    np.concatenate([p.glass for p in parts]))


def _shadow_tint(p):
    ratios = []
    for n in p.names:
        s = p.srgb.get(n + "_shadow")
        if s is None or n.endswith("_shadow"):
            continue
        ratios.append([srgb_to_linear(d) / max(srgb_to_linear(l), 1e-4)
                       for d, l in zip(s, p.srgb[n])])
    return tuple(min(1.0, sum(r[i] for r in ratios) / len(ratios)) for i in range(3))


def _colour_names():
    """Maps a baked linear colour (rounded) back to its palette name."""
    p = pal.palette()
    return {tuple(round(c, 4) for c in p.linear(n)[:3]): n for n in p.names}


def gather(root, glass=(), glow=(), skip=(), recolor=None):
    """Every triangle under `root` (after `export.finish_meshes` baked `Col`).

    `glass`, `glow` and `skip` are part names; `recolor` maps palette names to
    other palette names (e.g. to brighten a colour for a small icon).
    """
    p = pal.palette()
    tint = _shadow_tint(p)
    names = _colour_names()
    scene.update()
    verts, normals, lit, shadow, group, glows, glasses = [], [], [], [], [], [], []
    group_ids = {}
    for obj in scene.descendants(root):
        if obj.type != "MESH" or obj.name in skip:
            continue
        mesh = obj.data
        mesh.calc_loop_triangles()
        col = mesh.color_attributes.get(pal.COLOR_ATTR)
        if col is None:
            raise ValueError(f"{obj.name}: no baked colours (run export.finish_meshes first)")
        mw = obj.matrix_world
        nm = mw.to_3x3().inverted_safe().transposed()
        co = [mw @ v.co for v in mesh.vertices]
        for tri in mesh.loop_triangles:
            a, b, c = (co[i] for i in tri.vertices)
            n = (b - a).cross(c - a)
            if n.length < 1e-12:
                continue
            n.normalize()
            # Keep the mesh's own winding/normal agreement (flip if inverted).
            if n.dot(nm @ tri.normal) < 0:
                n = -n
            rgb = tuple(round(float(x), 4) for x in col.data[tri.loops[0]].color[:3])
            name = names.get(rgb)
            if recolor and name in recolor:
                name = recolor[name]
            if name is not None:
                l = p.srgb[name]
                s = p.shadow_of(name)
                if s is None:
                    s = tuple(linear_to_srgb(srgb_to_linear(x) * t) for x, t in zip(l, tint))
            else:
                l = tuple(linear_to_srgb(x) for x in rgb)
                s = tuple(linear_to_srgb(x * t) for x, t in zip(rgb, tint))
            key = (obj.name, name or rgb)
            if key not in group_ids:
                group_ids[key] = len(group_ids)
            verts.append([tuple(a), tuple(b), tuple(c)])
            normals.append(tuple(n))
            lit.append(l)
            shadow.append(s)
            group.append(group_ids[key])
            glows.append(obj.name in glow)
            glasses.append(obj.name in glass)
    if not verts:
        raise ValueError(f"{root.name}: nothing to draw")
    return Tris(np.array(verts, dtype=np.float64), np.array(normals, dtype=np.float64),
                np.array(lit, dtype=np.float64), np.array(shadow, dtype=np.float64),
                np.array(group, dtype=np.int32), np.array(glows, dtype=bool),
                np.array(glasses, dtype=bool))


class View:
    """An orthographic camera: world -> (right, up, towards-viewer) axes."""

    def __init__(self, right, up, back, roll_deg=0.0, mirror=False):
        self.right = np.array(right, dtype=np.float64)
        self.up = np.array(up, dtype=np.float64)
        self.back = np.array(back, dtype=np.float64)
        r = math.radians(roll_deg)
        self.roll = np.array([[math.cos(r), -math.sin(r)], [math.sin(r), math.cos(r)]])
        self.mirror = mirror

    def project(self, pts):
        """(..., 3) world points -> (..., 2) screen units (y up) and (...,) depth (bigger = nearer)."""
        x = pts @ self.right
        y = pts @ self.up
        xy = np.stack([x, y], axis=-1) @ self.roll.T
        if self.mirror:
            xy[..., 0] = -xy[..., 0]
        return xy, pts @ self.back

    def to_camera(self, v):
        """World directions (..., 3) in camera axes (x right, y up, z towards the viewer)."""
        v = np.asarray(v, dtype=np.float64)
        xy = np.stack([v @ self.right, v @ self.up], axis=-1) @ self.roll.T
        if self.mirror:
            xy[..., 0] = -xy[..., 0]
        return np.concatenate([xy, (v @ self.back)[..., None]], axis=-1)


def view(azimuth, elevation, roll=0.0, mirror=False):
    """Looks at the model from `azimuth` degrees round from its front (-Y, turning
    towards +X) and `elevation` degrees above; `roll` turns the picture
    counter-clockwise; `mirror` flips it left-right."""
    a, e = math.radians(azimuth), math.radians(elevation)
    back = Vector((math.sin(a) * math.cos(e), -math.cos(a) * math.cos(e), math.sin(e)))
    right = Vector((0.0, 0.0, 1.0)).cross(back).normalized()
    if right.length < 1e-6:
        right = Vector((1.0, 0.0, 0.0))
    up = back.cross(right).normalized()
    return View(tuple(right), tuple(up), tuple(back), roll, mirror)


def _shade(tris, cam):
    """Per-triangle sRGB colour: two toon bands from a camera-space key light."""
    key = np.array(KEY_CAMERA, dtype=np.float64)
    key /= np.linalg.norm(key)
    n_cam = cam.to_camera(tris.normals)
    ndl = n_cam @ key
    lit = ndl >= TERMINATOR
    rgb = np.where(lit[:, None], tris.lit, tris.shadow)
    # Crystals glow: never in shadow, and the faces towards the light shine.
    shine = np.clip((ndl - 0.55) / 0.45, 0.0, 1.0)[:, None] * 0.35
    glow_rgb = tris.lit + (1.0 - tris.lit) * shine
    rgb = np.where(tris.glow[:, None], glow_rgb, rgb)
    return rgb, n_cam


def _dilate(mask, radius):
    """Grows a boolean mask by about `radius` pixels (an octagon, near a disc)."""
    out = mask.copy()
    for i in range(int(round(radius))):
        m = out
        n = m.copy()
        n[1:, :] |= m[:-1, :]
        n[:-1, :] |= m[1:, :]
        n[:, 1:] |= m[:, :-1]
        n[:, :-1] |= m[:, 1:]
        if i % 2 == 1:
            n[1:, 1:] |= m[:-1, :-1]
            n[1:, :-1] |= m[:-1, 1:]
            n[:-1, 1:] |= m[1:, :-1]
            n[:-1, :-1] |= m[1:, 1:]
        out = n
    return out


def _raster(xy, depth, ids, width, height):
    """Z-buffers triangles (screen px, y down) into depth and triangle-index buffers."""
    zbuf = np.full((height, width), -np.inf)
    ibuf = np.full((height, width), -1, dtype=np.int64)
    for t in range(len(xy)):
        (x0, y0), (x1, y1), (x2, y2) = xy[t]
        area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0)
        if abs(area) < 1e-9:
            continue
        lo_x = max(int(math.floor(min(x0, x1, x2))), 0)
        hi_x = min(int(math.ceil(max(x0, x1, x2))), width - 1)
        lo_y = max(int(math.floor(min(y0, y1, y2))), 0)
        hi_y = min(int(math.ceil(max(y0, y1, y2))), height - 1)
        if lo_x > hi_x or lo_y > hi_y:
            continue
        px = np.arange(lo_x, hi_x + 1) + 0.5
        py = (np.arange(lo_y, hi_y + 1) + 0.5)[:, None]
        w0 = ((x1 - px) * (y2 - py) - (x2 - px) * (y1 - py)) / area
        w1 = ((x2 - px) * (y0 - py) - (x0 - px) * (y2 - py)) / area
        w2 = 1.0 - w0 - w1
        inside = (w0 >= 0) & (w1 >= 0) & (w2 >= 0)
        if not inside.any():
            continue
        z = w0 * depth[t, 0] + w1 * depth[t, 1] + w2 * depth[t, 2]
        region = zbuf[lo_y:hi_y + 1, lo_x:hi_x + 1]
        win = inside & (z > region)
        region[win] = z[win]
        ibuf[lo_y:hi_y + 1, lo_x:hi_x + 1][win] = ids[t]
    return zbuf, ibuf


def fit(tris, cam, size, margin_px, ss):
    """Screen transform (scale, offset) that centres the model in `size` px."""
    xy, _ = cam.project(tris.verts.reshape(-1, 3))
    lo, hi = xy.min(axis=0), xy.max(axis=0)
    w, h = size
    span = hi - lo
    scale = min((w - 2 * margin_px) / max(span[0], 1e-9), (h - 2 * margin_px) / max(span[1], 1e-9))
    centre = (lo + hi) / 2
    return scale * ss, centre


def render(tris, cam, size, ss=4, outline_px=3.0, line_px=1.1, margin_px=None,
           ink=None, depth_break=0.02, frame=None):
    """Draws `tris` from `cam` into an RGBA image `size` = (width, height) px.

    `outline_px` is the ink outline round the silhouette; `line_px` the inner
    ink lines where colours meet or the surface jumps in depth by more than
    `depth_break` x the model's size. `frame`, if given, is a (scale, centre)
    from `fit` to share between images.
    """
    p = pal.palette()
    ink = np.array(ink if ink is not None else p.srgb["ink"], dtype=np.float64)
    w, h = size
    if margin_px is None:
        margin_px = outline_px + 1.5
    W, H = w * ss, h * ss
    scale, centre = frame if frame is not None else fit(tris, cam, size, margin_px, ss)
    flat = tris.verts.reshape(-1, 3)
    xy, depth = cam.project(flat)
    sx = (xy[:, 0] - centre[0]) * scale + W / 2
    sy = (centre[1] - xy[:, 1]) * scale + H / 2
    xy = np.stack([sx, sy], axis=-1).reshape(-1, 3, 2)
    depth = depth.reshape(-1, 3)
    rgb, n_cam = _shade(tris, cam)

    opaque = np.nonzero(~tris.glass & (n_cam[:, 2] > 0))[0]
    zbuf, ibuf = _raster(xy[opaque], depth[opaque], opaque, W, H)
    covered = ibuf >= 0
    colour = np.zeros((H, W, 3))
    colour[covered] = rgb[ibuf[covered]]

    # Inner ink lines: colour groups meeting, or a jump in depth.
    extent = float(np.ptp(flat, axis=0).max())
    group = np.where(covered, tris.group[np.maximum(ibuf, 0)], -1)
    edge = np.zeros((H, W), dtype=bool)
    zc = np.where(covered, zbuf, 0.0)
    jump = depth_break * extent
    for axis in (0, 1):
        g0 = np.take(group, np.arange(group.shape[axis] - 1), axis=axis)
        g1 = np.take(group, np.arange(1, group.shape[axis]), axis=axis)
        z0 = np.take(zc, np.arange(zc.shape[axis] - 1), axis=axis)
        z1 = np.take(zc, np.arange(1, zc.shape[axis]), axis=axis)
        both = (g0 >= 0) & (g1 >= 0)
        brk = both & ((g0 != g1) | (np.abs(z0 - z1) > jump))
        if axis == 0:
            edge[:-1, :] |= brk & (z0 >= z1)
            edge[1:, :] |= brk & (z1 > z0)
        else:
            edge[:, :-1] |= brk & (z0 >= z1)
            edge[:, 1:] |= brk & (z1 > z0)
    if line_px > 0:
        edge = _dilate(edge, line_px * ss / 2)
        colour[edge & covered] = ink

    # Glass: the nearest glass face in front of the opaque surface, blended over.
    glass = np.nonzero(tris.glass)[0]
    alpha = covered.astype(np.float64)
    if len(glass):
        gz, gi = _raster(xy[glass], depth[glass], glass, W, H)
        over = (gi >= 0) & (gz > zbuf)
        tint = rgb[np.maximum(gi, 0)]
        colour[over] = colour[over] * (1 - GLASS_ALPHA) + tint[over] * GLASS_ALPHA
        new = over & ~covered
        colour[new] = tint[new]
        alpha[new] = GLASS_ALPHA
        covered = covered | over

    # The outline round everything drawn.
    ring = _dilate(covered, outline_px * ss) & ~covered
    colour[ring] = ink
    alpha[ring] = 1.0

    return downsample(colour, alpha, ss)


def downsample(colour, alpha, ss):
    """Box-filters a supersampled sRGB image (averaging in linear light)."""
    H, W = alpha.shape
    h, w = H // ss, W // ss
    lin = np.where(colour <= 0.04045, colour / 12.92, ((colour + 0.055) / 1.055) ** 2.4)
    pre = lin * alpha[..., None]
    pre = pre.reshape(h, ss, w, ss, 3).mean(axis=(1, 3))
    a = alpha.reshape(h, ss, w, ss).mean(axis=(1, 3))
    lin = np.where(a[..., None] > 0, pre / np.maximum(a[..., None], 1e-9), 0.0)
    srgb = np.where(lin <= 0.0031308, lin * 12.92, 1.055 * np.power(np.clip(lin, 0, 1), 1 / 2.4) - 0.055)
    out = np.zeros((h, w, 4))
    out[..., :3] = np.clip(srgb, 0.0, 1.0)
    out[..., 3] = np.clip(a, 0.0, 1.0)
    return out


def over(top, bottom):
    """Straight-alpha `top` over `bottom` (same shape)."""
    ta, ba = top[..., 3:4], bottom[..., 3:4]
    a = ta + ba * (1 - ta)
    rgb = np.where(a > 0, (top[..., :3] * ta + bottom[..., :3] * ba * (1 - ta)) / np.maximum(a, 1e-9), 0)
    return np.concatenate([rgb, a], axis=-1)


# ---------------------------------------------------------------------------
# PNG
# ---------------------------------------------------------------------------

def _chunk(kind, data):
    return (struct.pack(">I", len(data)) + kind + data
            + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF))


def encode_png(image):
    """8-bit RGBA PNG bytes: adaptive row filters, zlib level 9, no metadata."""
    rgba = np.clip(np.round(np.asarray(image, dtype=np.float64) * 255.0), 0, 255).astype(np.int16)
    # Fully transparent pixels carry no colour (smaller, and stable).
    rgba[rgba[..., 3] == 0] = 0
    h, w = rgba.shape[:2]
    rows = rgba.reshape(h, w * 4)
    zero_row = np.zeros((1, w * 4), dtype=np.int16)
    zero_px = np.zeros((h, 4), dtype=np.int16)
    up = np.vstack([zero_row, rows[:-1]])
    left = np.hstack([zero_px, rows[:, :-4]])
    upleft = np.hstack([zero_px, up[:, :-4]])
    pa = np.abs(up - upleft)
    pb = np.abs(left - upleft)
    pc = np.abs(left + up - 2 * upleft)
    paeth = np.where((pa <= pb) & (pa <= pc), left, np.where(pb <= pc, up, upleft))
    filtered = [rows, rows - left, rows - up, rows - ((left + up) >> 1), rows - paeth]
    filtered = [(f % 256).astype(np.uint8) for f in filtered]
    scores = np.stack([np.abs(f.astype(np.int8).astype(np.int32)).sum(axis=1) for f in filtered])
    best = np.argmin(scores, axis=0)
    raw = bytearray()
    for y in range(h):
        raw.append(int(best[y]))
        raw += filtered[best[y]][y].tobytes()
    header = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + _chunk(b"IHDR", header)
            + _chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + _chunk(b"IEND", b""))


def write_png(image, path):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(encode_png(image))
