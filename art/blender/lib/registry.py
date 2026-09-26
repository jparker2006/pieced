"""Asset declarations and the spec's triangle budgets (docs/M2-SPEC.md, Asset pipeline)."""

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
