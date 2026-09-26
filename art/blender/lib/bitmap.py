"""Tiny numpy image helpers shared by the palette sampler and the preview renders.

Images are float32 arrays shaped (height, width, 4), top row first, values in 0..1
(sRGB-encoded, as stored in 8-bit PNGs). Blender stores pixels bottom row first,
so `load_png` and `save_png` flip rows.
"""

import bpy
import numpy as np

# 3x5 glyphs, one string per row, '#' = ink.
_GLYPHS = {
    "0": ["###", "#.#", "#.#", "#.#", "###"],
    "1": [".#.", "##.", ".#.", ".#.", "###"],
    "2": ["###", "..#", "###", "#..", "###"],
    "3": ["###", "..#", ".##", "..#", "###"],
    "4": ["#.#", "#.#", "###", "..#", "..#"],
    "5": ["###", "#..", "###", "..#", "###"],
    "6": ["###", "#..", "###", "#.#", "###"],
    "7": ["###", "..#", ".#.", ".#.", ".#."],
    "8": ["###", "#.#", "###", "#.#", "###"],
    "9": ["###", "#.#", "###", "..#", "###"],
    ".": ["...", "...", "...", "...", ".#."],
    "-": ["...", "...", "###", "...", "..."],
    "M": ["#.#", "###", "###", "#.#", "#.#"],
    "T": ["###", ".#.", ".#.", ".#.", ".#."],
    " ": ["...", "...", "...", "...", "..."],
}


def load_png(path):
    """Loads a PNG as a top-down float array. No colour conversion is applied."""
    img = bpy.data.images.load(path, check_existing=False)
    try:
        img.colorspace_settings.name = "Non-Color"
    except TypeError:
        pass
    w, h = img.size
    px = np.empty(w * h * 4, dtype=np.float32)
    img.pixels.foreach_get(px)
    bpy.data.images.remove(img)
    return px.reshape(h, w, 4)[::-1].copy()


def save_png(arr, path):
    """Saves a top-down float array (h, w, 4) as an 8-bit RGBA PNG."""
    h, w = arr.shape[:2]
    img = bpy.data.images.new("pieced_out", w, h, alpha=True)
    img.pixels.foreach_set(np.ascontiguousarray(arr[::-1], dtype=np.float32).ravel())
    img.filepath_raw = path
    img.file_format = "PNG"
    img.save()
    bpy.data.images.remove(img)


def fill(arr, x0, y0, x1, y1, rgb):
    h, w = arr.shape[:2]
    x0, x1 = max(0, x0), min(w, x1)
    y0, y1 = max(0, y0), min(h, y1)
    if x0 < x1 and y0 < y1:
        arr[y0:y1, x0:x1, :3] = rgb
        arr[y0:y1, x0:x1, 3] = 1.0


def frame(arr, x0, y0, x1, y1, rgb, t=2):
    fill(arr, x0, y0, x1, y0 + t, rgb)
    fill(arr, x0, y1 - t, x1, y1, rgb)
    fill(arr, x0, y0, x0 + t, y1, rgb)
    fill(arr, x1 - t, y0, x1, y1, rgb)


def text(arr, x, y, s, rgb, scale=4):
    """Draws `s` (digits, '.', '-', 'M', 'T', ' ') with its top-left corner at (x, y)."""
    for ch in s.upper():
        glyph = _GLYPHS.get(ch, _GLYPHS[" "])
        for gy, row in enumerate(glyph):
            for gx, cell in enumerate(row):
                if cell == "#":
                    fill(arr, x + gx * scale, y + gy * scale,
                         x + (gx + 1) * scale, y + (gy + 1) * scale, rgb)
        x += 4 * scale
    return x


def upscale_nearest(arr, k):
    return np.repeat(np.repeat(arr, k, axis=0), k, axis=1)


def over(fg, bg_rgb):
    """Composites a premultiplied-free RGBA image over a flat or per-pixel background."""
    a = fg[..., 3:4]
    out = fg.copy()
    out[..., :3] = fg[..., :3] * a + np.asarray(bg_rgb, dtype=np.float32)[..., :3] * (1.0 - a)
    out[..., 3] = 1.0
    return out
