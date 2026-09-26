"""Gridded, labelled crops of the target images, for choosing palette sample points.

    blender -b --factory-startup --python-exit-code 1 -P art/tools/grid_crop.py -- \
        --concepts DIR --out art/previews/palette KEY:x0:y0:w:h [...]

KEY is an image key from palette_samples.json (T01, R4, ...). Each crop is
upscaled 2x with faint lines every 10 px, strong lines every 50 px, and the
absolute x (top) and y (left) coordinates printed at the strong lines.
"""

import json
import os
import sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.normpath(os.path.join(HERE, "..", ".."))
sys.path.insert(0, os.path.join(REPO, "art", "blender"))

from lib import bitmap  # noqa: E402


def main():
    argv = sys.argv[sys.argv.index("--") + 1:]
    concepts = os.path.join(REPO, "docs", "design", "concepts")
    out = os.path.join(REPO, "art", "previews", "palette")
    crops = []
    it = iter(argv)
    for a in it:
        if a == "--concepts":
            concepts = next(it)
        elif a == "--out":
            out = next(it)
        else:
            key, x0, y0, w, h = a.split(":")
            crops.append((key, int(x0), int(y0), int(w), int(h)))
    with open(os.path.join(HERE, "palette_samples.json")) as f:
        images = json.load(f)["images"]
    os.makedirs(out, exist_ok=True)
    cache = {}
    k = 2
    margin = 40
    for key, x0, y0, w, h in crops:
        if key not in cache:
            cache[key] = bitmap.load_png(os.path.join(concepts, images[key]))
        img = cache[key]
        crop = bitmap.upscale_nearest(img[y0:y0 + h, x0:x0 + w], k)
        canvas = np.zeros((h * k + margin, w * k + margin, 4), dtype=np.float32)
        canvas[..., 3] = 1.0
        canvas[margin:, margin:] = crop
        for x in range(x0 - x0 % 10 + 10, x0 + w, 10):
            strong = x % 50 == 0
            px = margin + (x - x0) * k
            col = np.array((1, 0, 1) if strong else (1, 1, 1), dtype=np.float32)
            a = 0.8 if strong else 0.25
            canvas[margin:, px, :3] = canvas[margin:, px, :3] * (1 - a) + col * a
            if strong:
                bitmap.text(canvas, px - 12, 4, str(x), (1, 1, 1), scale=2)
        for y in range(y0 - y0 % 10 + 10, y0 + h, 10):
            strong = y % 50 == 0
            py = margin + (y - y0) * k
            col = np.array((1, 0, 1) if strong else (1, 1, 1), dtype=np.float32)
            a = 0.8 if strong else 0.25
            canvas[py, margin:, :3] = canvas[py, margin:, :3] * (1 - a) + col * a
            if strong:
                bitmap.text(canvas, 2, py - 5, str(y), (1, 1, 1), scale=2)
        path = os.path.join(out, f"grid_{key}_{x0}_{y0}.png")
        bitmap.save_png(canvas, path)
        print("GRID", path)


main()
