"""Builds Pieced's models headless. Entry point for scripts/build-art.sh.

    blender -b --factory-startup --python-exit-code 1 -P art/blender/build.py -- \
        [asset ...] [--out DIR] [--previews DIR] [--no-previews] [--list]

With no asset names, builds every asset. For each one it starts a clean scene,
runs the family module's build function (art/blender/assets/*.py), bakes the
palette tags into COLOR_0, checks the triangle budget, and writes:

    <out>/<name>.glb      the model (default out: assets/models)
    <out>/<name>.json     the sidecar: part bounds, attach points (Bevy space)
    <previews>/<name>.png 3/4 + front preview render (default: art/previews)

and always rewrites <out>/manifest.json listing every registered asset.
The same scripts always produce byte-identical .glb and .json files.
"""

import importlib
import os
import pkgutil
import sys
import time
import traceback

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.normpath(os.path.join(HERE, "..", ".."))
sys.path.insert(0, HERE)

from lib import export, preview, scene  # noqa: E402
import assets as families  # noqa: E402


def registered():
    out = {}
    for mod in sorted(m.name for m in pkgutil.iter_modules(families.__path__)):
        module = importlib.import_module(f"assets.{mod}")
        for asset in getattr(module, "ASSETS", []):
            if asset.name in out:
                raise ValueError(f"asset {asset.name!r} registered twice")
            out[asset.name] = (asset, f"art/blender/assets/{mod}.py")
    return dict(sorted(out.items()))


def parse_args(argv):
    opts = {"names": [], "out": os.path.join(REPO, "assets", "models"),
            "previews": os.path.join(REPO, "art", "previews"), "no_previews": False, "list": False}
    it = iter(argv)
    for a in it:
        if a == "--out":
            opts["out"] = os.path.abspath(next(it))
        elif a == "--previews":
            opts["previews"] = os.path.abspath(next(it))
        elif a == "--no-previews":
            opts["no_previews"] = True
        elif a == "--list":
            opts["list"] = True
        elif a.startswith("-"):
            raise SystemExit(f"unknown option {a}")
        else:
            opts["names"].append(a)
    return opts


def build_one(asset, source, opts):
    t0 = time.time()
    scene.reset()
    root = scene.make_root(asset.name)
    asset.build(root)
    colors = export.finish_meshes(root)
    side = export.sidecar(root, asset.kind, source, asset.budget, colors)
    if side["triangles"] > asset.budget:
        raise ValueError(f"{asset.name}: {side['triangles']} triangles is over the "
                         f"{asset.kind} budget of {asset.budget}")
    export.export_glb(root, os.path.join(opts["out"], f"{asset.name}.glb"))
    export.write_json(side, os.path.join(opts["out"], f"{asset.name}.json"))
    height = side["bounds"]["max"][1] - side["bounds"]["min"][1]
    if not opts["no_previews"]:
        preview.render(root, os.path.join(opts["previews"], f"{asset.name}.png"),
                       [f"{height:.2f}M", f"{side['triangles']}T"])
    print(f"ART built {asset.name}: {side['triangles']}/{asset.budget} tris, "
          f"height {height:.3f} m, parts {list(side['parts'])}, attach {list(side['attach'])} "
          f"({time.time() - t0:.1f} s)")


def main():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    opts = parse_args(argv)
    known = registered()
    if opts["list"]:
        for name, (asset, source) in known.items():
            print(f"ART asset {name} ({asset.kind}, {source}): {asset.about}")
        return
    names = opts["names"] or list(known)
    unknown = [n for n in names if n not in known]
    if unknown:
        raise SystemExit(f"unknown asset(s): {', '.join(unknown)}; known: {', '.join(known)}")
    failures = []
    for name in names:
        asset, source = known[name]
        try:
            build_one(asset, source, opts)
        except Exception:
            traceback.print_exc()
            failures.append(name)
    export.write_json([{"name": n, "file": f"{n}.glb"} for n in known],
                      os.path.join(opts["out"], "manifest.json"))
    if failures:
        raise RuntimeError(f"ART FAILED: {', '.join(failures)}")
    out = opts["out"]
    shown = os.path.relpath(out, REPO) if out.startswith(REPO + os.sep) else out
    print(f"ART OK: {len(names)} asset(s) -> {shown}")


main()
