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
- **Attempt 2** (2026-09-26 08:20, `evidence/perf-20260926-082043-*`): **VALID as a development proxy.** AC power (the battery was at 6%, too low for a battery run), Low Power Mode on, occluded 0 ms, muted, builder compiles paused (`timed-run.sh`), load 8.4 → 4.4.

  | Run | Mean ms | p99 ms | Max ms | Frames > 25 ms | < 18 ms |
  |---|---|---|---|---|---|
  | full (300 s) | 16.70 | 18.34 | 69.8 | 28 | 97.71% |
  | shadows=off (60 s) | 16.69 | 18.50 | 34.0 | 6 | 97.54% |
  | msaa=1 | 16.67 | 18.19 | 29.5 | 1 | 98.09% |
  | vmmsaa=1 | 16.68 | 18.49 | 30.7 | 2 | 96.82% |
  | viewmodel=off | 16.67 | 18.52 | 24.9 | 0 | 96.92% |
  | scale=0.8 | 16.67 | 18.45 | 32.5 | 2 | 96.90% |
  | fog=off | 16.79 | 18.58 | 125.8 | 16 | 96.96% |
  | sky=off | 16.68 | 18.40 | 39.5 | 3 | 97.23% |
  | no vsync, 60 fps cap (120 s) | 16.67 | 21.29 | 22.8 | 0 | 78.23% |

  Launch was 0.61–1.24 s (G1 passes).

  **What it says:**
  1. **The GPU isn't the bottleneck.** No knob moves the < 18 ms share meaningfully.
  2. **The < 18 ms misses are pacing jitter.** In the full run, 2.04% of frames land at 18–20 ms and a matching 2.04% below 15 ms: a late frame followed by an early one, still presented on a vblank.
  3. **The > 25 ms spikes are real, and they come in bursts** at 98–99 s, 195–197 s and 230–234 s, during heavy scripted building and breaking. That points to per-event costs (spawning pieces, debris, audio voices) or pipeline compiles on first use. Without vsync the same moments cost only ≤ 23 ms, so each is a frame that overruns 16.7 ms by a few ms and then waits for the next vblank.
  4. **The fixes to pursue in S2 tuning:**
     - piece and debris spawns reuse meshes and materials, with pooled debris;
     - pipeline warm-up (now in);
     - a steadier frame start: a pacing experiment with vsync plus the spin limiter.

     If jitter alone still keeps the < 18 ms share under 99% on CPU-side intervals, raise with Jake whether to measure presented-frame intervals instead. That would change the method only, not the threshold.
- **Still needed:** the S2 gate run itself is on battery, and on the new look.

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
- 2026-09-26: the disk hit 2.3 GB free. Stale split-debuginfo `.o` files (18.7 GB) and old caches were cleared, leaving 15 GB free. The dev profile now has `debug = false` (`6cddddd`).
- 2026-09-26: **paused at the usage limit.** The Phase 2 builders were stopped mid-work. Their progress is on disk, uncommitted or partly committed, in `.claude/worktrees/`:
  - `m2-guns`: models done, integration in progress;
  - `m2-knight`: model done, integration in progress;
  - `m2-island`: pieces and props in progress;
  - `m2-sky`: galaxy, station, ships and islands working. Composition fixes are pending: show the whole station from spawn, a readable ringed planet, bigger and more far islands, solid waterfalls.

  **To resume:**
  1. In each worktree, `git merge main`, finish, then run the gates and commit.
  2. Merge in this order: sky, island, knight, guns.
  3. Then Phase 3 (spells, HUD, gallery) and Phase 4.

  The Phase 0 baseline still needs a "go" window.
