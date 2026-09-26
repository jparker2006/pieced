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
