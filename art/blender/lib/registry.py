"""Asset declarations and the spec's triangle budgets (docs/M2-SPEC.md, Asset pipeline).

A family module lists its models in `ASSETS` and its UI images (hotbar icons,
the logo) in `IMAGES`.
"""

from dataclasses import dataclass
from typing import Callable, Optional

# Worst-case triangles per asset kind, before outlines. `probe` is a test fixture.
BUDGETS = {
    # S1 round 3: rounder, smooth-shaded guns (up from the spec's 6k).
    "gun": 8000,
    "gloves": 2000,
    "knight": 11000,
    "wall": 600,
    "floor": 400,
    "ramp": 400,
    # M2 Amendment A: the cone, and the edit tile sets (every tile of a piece
    # plus the diagonal half tiles and opening trims; an edited piece draws a
    # subset, never more than about its full piece).
    "cone": 700,
    "wall_tiles": 2600,
    "floor_tiles": 700,
    "ramp_tiles": 800,
    "tree": 1500,
    "rock": 300,
    "stump": 300,
    "station": 25000,
    "far_island": 1500,
    "ship": 500,
    # M3: the grunt's crystal wand (docs/M3-SPEC.md → The grunt, item 10).
    "wand": 400,
    "probe": 100,
}


@dataclass(frozen=True)
class Asset:
    """One model. `build(root)` adds named parts and attach points under `root`.

    `ao` overrides the kind's baked ambient-occlusion settings (`lib/ao.py`
    `KIND_AO`) for this asset, e.g. `ao=AoSettings(radius=0.3, ground=True)`.
    """

    name: str
    kind: str
    build: Callable
    about: str = ""
    ao: Optional[object] = None

    @property
    def budget(self):
        return BUDGETS[self.kind]

    @property
    def ao_settings(self):
        from .ao import settings_for
        return settings_for(self.kind, self.ao)


@dataclass(frozen=True)
class UiImage:
    """One committed UI picture. `render(path)` builds its own scene and writes
    the PNG to `path` (deterministically: see `lib/raster.py`). `path` here is
    where it lives under `assets/ui/`."""

    name: str
    path: str
    render: Callable
    about: str = ""
