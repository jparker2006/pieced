# Milestone 2 "Spellbound": report

**Status:** IN PROGRESS. The goal launched 2026-09-25 (`docs/M2-GOAL.md`).

Every number here names its run folder, the commit, and the power and Low Power Mode state. Failures and reruns are recorded, not deleted.

## Gates

| Gate | Status | Evidence |
|---|---|---|
| S1 Target board (every view ≥ 4 from Jake) | PENDING | — |
| S2 Performance (battery, Low Power Mode, full look) | PENDING | — |
| S3 Launch < 5 s ×3 | PENDING | — |
| S4 Motion | PENDING | — |
| S5 Knight hitbox fit | PENDING | — |
| S6 Feedback timing | PENDING | — |
| S7 No regressions (G1, G3, G6, tests, clippy, fmt) | PENDING | — |
| S8 Jake's feel verdict | PENDING | — |
| S9 Original and reproducible | PENDING | — |

## Phase 0: performance baseline (Milestone 1 art)

- **Commit:** tag `m2-baseline` (`5249c9b`).
- **Attempt 1** (2026-09-25 21:50, `evidence/perf-20260925-215019-*`): battery, Low Power Mode on, 57% → 51%. **INVALID.** The window was occluded for the whole run (303 s of 303 s occluded in the full run; 22–68 s of each 60 s knob run), so frames weren't paced by the display. Load averages ran 5.9 → 11.4.
  - The unpaced means (6–11 ms) only say the frame work fits well inside 16.7 ms when nothing has to present. Two knob runs had the fewest spikes and are worth checking first on the new look: `vmmsaa=1` (4 frames > 25 ms, max 36.7 ms) and `sky=off` (max 103 ms).
  - An earlier unmuted start at 21:46 was stopped at Jake's request (the sound was annoying). Baseline runs are now muted through the baseline worktree's `userdata/settings.json`.
- **Next:** rerun with the game window uncovered for the whole run (Jake away from the Mac).

## Log

- 2026-09-25: goal launched. The baseline was tagged, and the shared foundations (`BootGate`, the `look` and `models` plugin skeletons) were added. Phase 1 builders were dispatched.
- 2026-09-25: fonts downloaded with Jake's OK. Luckiest Guy (Apache 2.0) and Lilita One (OFL) are in `assets/fonts/`, with their licenses.
- 2026-09-25: **audio merged** (`d586c29`): the new synthesized bank, with 12 audio tests; 171 tests pass on the merge. Open item: the rifle reload's last click lands about 0.12 s after the gun is ready.
- 2026-09-25: **art pipeline merged** (`d7e9bdd`):
  - the headless Blender pipeline, deterministic (`build-art.sh --check`);
  - a 66-color palette sampled from the targets;
  - `rock_a`, `rock_b`, `stump_a` and `tree_a`;
  - `ModelsPlugin` (embedded glbs, the BootGate hold, part lookup, the forward fix);
  - the asset audit.

  181 tests pass. Colors ship as vertex colors (the spec was updated). Art-review notes for Phase 4: the rock is a little boxy and its shadow too saturated blue; the foliage shadow is teal where the targets show a darker green.
- 2026-09-25: Phase 2 **guns** and **knight** builders dispatched (models first; their in-game integration waits for the look slice).
- 2026-09-25: **look foundations merged** (`0bc8a27`):
  - the toon material (vertex colors, violet shadow band, rim), with `Tonemapping::None`;
  - inverted-hull ink outlines, the default. `bevy_mod_outline` sits behind `outline=mod`, because offscreen it drew outlines through the ground, couldn't fade merged scenery and adds an MSAA blit;
  - the far material, halos, blob shadows;
  - shadow maps and fog removed;
  - shader warm-up behind a loading overlay, with Boot ending about 1.3 s after start in a debug build with a warm cache.

  The orchestrator added model dressing (`eb2f6cf`): Blender models get toon, far or viewmodel materials on spawn. 227 tests pass. Risk: the first launch after new shaders took about 12 s while Metal's shader cache was cold. S3 measures warm launches.
- 2026-09-25: Phase 2 **island** (pieces, D29 props, island, barrier, grid) and **sky** (galaxy, station, ships, far islands, planet) builders dispatched. Guns and knight were told to continue into in-game integration.
