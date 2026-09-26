"""Samples the cartoon palette from the target images (headless Blender).

    scripts/sample-palette.sh
    # = blender -b --factory-startup --python-exit-code 1 -P art/tools/sample_palette.py -- \
    #       [--concepts docs/design/concepts] [--out art/palette.json] [--sheets art/previews/palette]

Reads `art/tools/palette_samples.json` (which image and pixel each colour comes
from), takes the per-channel median of a small patch around every point, and
writes `art/palette.json` (name -> "#RRGGBB", sorted). It also writes contact
sheets to `art/previews/palette/` so the sample points can be checked by eye:
one row per colour, numbered in file order, showing a zoomed crop around each
point (patch outlined in magenta) and the resulting swatch at the right.

The target images are local only (git-ignored), so this runs on Jake's Mac; the
palette it writes is committed and is the single source of truth.
"""

import json
import os
import sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.normpath(os.path.join(HERE, "..", ".."))
sys.path.insert(0, os.path.join(REPO, "art", "blender"))

from lib import bitmap  # noqa: E402
from lib.color import linear_to_srgb, srgb_to_linear, hex_to_srgb, srgb_to_hex  # noqa: E402


def parse_args(argv):
    args = {
        "concepts": os.path.join(REPO, "docs", "design", "concepts"),
        "out": os.path.join(REPO, "art", "palette.json"),
        "sheets": os.path.join(REPO, "art", "previews", "palette"),
        "samples": os.path.join(HERE, "palette_samples.json"),
    }
    it = iter(argv)
    for a in it:
        key = a.lstrip("-")
        if key not in args:
            raise SystemExit(f"unknown argument {a}")
        args[key] = os.path.abspath(next(it))
    return args


LUMA = np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)


def hue_degrees(px):
    r, g, b = px[:, 0], px[:, 1], px[:, 2]
    mx, mn = px.max(axis=1), px.min(axis=1)
    d = np.maximum(mx - mn, 1e-6)
    h = np.where(mx == r, ((g - b) / d) % 6, np.where(mx == g, (b - r) / d + 2, (r - g) / d + 4))
    return h * 60.0


def most_saturated(px, hue, name, window=25.0):
    """The patch's most saturated pixel, optionally only among hues within `window` of `hue`."""
    chroma = px.max(axis=1) - px.min(axis=1)
    if hue is not None:
        off = np.abs((hue_degrees(px) - float(hue) + 180.0) % 360.0 - 180.0)
        chroma = np.where(off <= window, chroma, -1.0)
        if chroma.max() < 0:
            raise SystemExit(f"{name}: no pixel near hue {hue} in the patch")
    return px[np.argmax(chroma)]


def patch_pixels(img, x, y, size):
    r = size // 2
    h, w = img.shape[:2]
    if not (r <= x < w - r and r <= y < h - r):
        raise ValueError(f"sample point ({x}, {y}) too close to the image edge")
    return img[y - r:y + r + 1, x - r:x + r + 1, :3].reshape(-1, 3)


def main():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    args = parse_args(argv)
    with open(args["samples"]) as f:
        spec = json.load(f)
    images = {}

    def image(key):
        if key not in images:
            path = os.path.join(args["concepts"], spec["images"][key])
            if not os.path.exists(path):
                raise SystemExit(f"missing target image {path} (targets are local only)")
            images[key] = bitmap.load_png(path)
        return images[key]

    default_patch = int(spec["patch"])
    srgb = {}
    how = {}
    # Pass 1: sampled and authored colours.
    for name, entry in spec["colors"].items():
        if "at" in entry:
            size = int(entry.get("patch", default_patch))
            mode = entry.get("mode", "median")
            patches = [patch_pixels(image(k), x, y, size) for k, x, y in entry["at"]]
            if mode == "median":
                pooled = np.concatenate(patches)
            elif mode in ("min", "max", "vivid"):
                # Thin lines: the darkest (or brightest) pixel of each patch; glows
                # and small glass cells: the most saturated one. Then the median of
                # those across the sample points.
                if mode == "vivid":
                    hue = entry.get("hue")
                    pooled = np.stack([most_saturated(p, hue, name) for p in patches])
                else:
                    pick = np.argmin if mode == "min" else np.argmax
                    pooled = np.stack([p[pick(p @ LUMA)] for p in patches])
            else:
                raise SystemExit(f"{name}: unknown mode {mode!r}")
            srgb[name] = tuple(float(v) for v in np.median(pooled, axis=0))
            how[name] = "sampled"
        elif "hex" in entry:
            srgb[name] = hex_to_srgb(entry["hex"])
            how[name] = "authored"
    # Pass 2: shadow variants derived from the mean lit -> shadow ratio of the
    # sampled pairs (the targets' cool violet shadow band).
    ratios = []
    for name in srgb:
        shadow = name + "_shadow"
        if how.get(name) == "sampled" and how.get(shadow) == "sampled":
            lit = np.array([srgb_to_linear(c) for c in srgb[name]])
            dark = np.array([srgb_to_linear(c) for c in srgb[shadow]])
            ratios.append(dark / np.maximum(lit, 1e-4))
    tint = np.clip(np.mean(ratios, axis=0), 0.0, 1.0) if ratios else np.array([0.6, 0.6, 0.8])
    print("PALETTE shadow tint (linear lit->shadow ratio):", ", ".join(f"{v:.3f}" for v in tint))
    for name, entry in spec["colors"].items():
        if "derive" in entry:
            lit = np.array([srgb_to_linear(c) for c in srgb[entry["derive"]]])
            srgb[name] = tuple(linear_to_srgb(v) for v in lit * tint)
            how[name] = "derived"

    palette = {name: srgb_to_hex(srgb[name]) for name in sorted(srgb)}
    os.makedirs(os.path.dirname(args["out"]), exist_ok=True)
    with open(args["out"], "w") as f:
        json.dump(palette, f, indent=2, sort_keys=True)
        f.write("\n")
    print(f"PALETTE wrote {len(palette)} colours to {os.path.relpath(args['out'], REPO)}")

    write_sheets(spec, srgb, image, args["sheets"], default_patch)


def write_sheets(spec, srgb, image, out_dir, default_patch):
    """Contact sheets: per colour row, zoomed crops around each sample, then the swatch."""
    os.makedirs(out_dir, exist_ok=True)
    names = list(spec["colors"].keys())
    rows_per_sheet = 12
    zoom, half = 3, 20  # 41x41 source pixels -> 123 px tiles
    tile = (2 * half + 1) * zoom
    label_w = 60
    max_samples = max(len(e.get("at", [])) for e in spec["colors"].values())
    width = label_w + (max_samples + 1) * (tile + 6)
    for s in range(0, len(names), rows_per_sheet):
        chunk = names[s:s + rows_per_sheet]
        sheet = np.zeros(((tile + 6) * len(chunk), width, 4), dtype=np.float32)
        sheet[..., :3] = 0.12
        sheet[..., 3] = 1.0
        for r, name in enumerate(chunk):
            y0 = r * (tile + 6) + 3
            bitmap.text(sheet, 6, y0 + 4, str(s + r), (1, 1, 1), scale=4)
            entry = spec["colors"][name]
            x0 = label_w
            for k, x, y in entry.get("at", []):
                img = image(k)
                h, w = img.shape[:2]
                crop = np.zeros((2 * half + 1, 2 * half + 1, 4), dtype=np.float32)
                ys, ye = max(0, y - half), min(h, y + half + 1)
                xs, xe = max(0, x - half), min(w, x + half + 1)
                crop[ys - (y - half):ye - (y - half), xs - (x - half):xe - (x - half)] = img[ys:ye, xs:xe]
                crop = bitmap.upscale_nearest(crop, zoom)
                crop[..., 3] = 1.0
                p = int(entry.get("patch", default_patch)) // 2
                c0 = (half - p) * zoom
                c1 = (half + p + 1) * zoom
                bitmap.frame(crop, c0 - 2, c0 - 2, c1 + 2, c1 + 2, (1, 0, 1), t=2)
                sheet[y0:y0 + tile, x0:x0 + tile] = crop
                x0 += tile + 6
            x0 = width - tile - 6
            bitmap.fill(sheet, x0, y0, x0 + tile, y0 + tile, srgb[name])
        path = os.path.join(out_dir, f"sheet_{s // rows_per_sheet}.png")
        bitmap.save_png(sheet, path)
        print(f"PALETTE sheet {os.path.relpath(path, REPO)}: rows {s}..{s + len(chunk) - 1} = "
              + ", ".join(chunk))


main()
