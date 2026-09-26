"""Asset declarations and the spec's triangle budgets (docs/M2-SPEC.md, Asset pipeline).

A family module lists its models in `ASSETS` and its UI images (hotbar icons,
the logo) in `IMAGES`.
"""

from dataclasses import dataclass
from typing import Callable

# Worst-case triangles per asset kind, before outlines. `probe` is a test fixture.
BUDGETS = {
    "gun": 6000,
    "gloves": 2000,
    "knight": 8000,
    "wall": 600,
    "floor": 400,
    "ramp": 400,
    "tree": 1500,
    "rock": 300,
    "stump": 300,
    "station": 25000,
    "far_island": 1500,
    "ship": 500,
    "probe": 100,
}


@dataclass(frozen=True)
class Asset:
    """One model. `build(root)` adds named parts and attach points under `root`."""

    name: str
    kind: str
    build: Callable
    about: str = ""

    @property
    def budget(self):
        return BUDGETS[self.kind]


@dataclass(frozen=True)
class UiImage:
    """One committed UI picture. `render(path)` builds its own scene and writes
    the PNG to `path` (deterministically: see `lib/raster.py`). `path` here is
    where it lives under `assets/ui/`."""

    name: str
    path: str
    render: Callable
    about: str = ""
