# Milestone 4 "AAA": report

**Status:** IN PROGRESS (goal launched 2026-09-28). The contract is `docs/M4-SPEC.md`, and the brief is `docs/M4-GOAL.md` (grill Round 11, D96–D118).

Every number here names its source (session folder or test), the commit, and the power and Low Power Mode state. Failures and reruns are recorded, not deleted.

## Gates

| Gate | Status | Evidence |
|---|---|---|
| A0 Carried gates closed (M3 W7/W8, M2 S2/S3, W0) | PENDING | |
| A1 Hit feedback and kills | PENDING | |
| A2 Weapons | PENDING | |
| A3 Audio and music | PENDING | |
| A4 Knight animation | PENDING | |
| A5 Presentation, building juice and camera | PENDING | |
| A6 Target board (V1–V8 each ≥ 4) | PENDING | |
| A7 Performance (wave ≥ 6, battery, Low Power Mode, presented frames) | PENDING | |
| A8 Launch (warm < 5 s ×3; Play → controllable < 1 s) | PENDING | |
| A9 Final verdict (fun ≥ 4.5, AAA feel ≥ 4, no unfair deaths) | PENDING | |
| A10 No regressions | PENDING | |

## GPU budget (reported every chunk, not gated; D114)

| Date | Commit | Session (power, LPM) | World | Outlines | Far | Effects | UI | Post | Total p95 |
|---|---|---|---|---|---|---|---|---|---|
| Budget | | | 4.5 | 1.5 | 2.0 | 2.0 | 0.5 | 1.0 | ≤ 12.0 |

## Target board

| View | Target | Round 1 | Round 2 | Jake's score |
|---|---|---|---|---|
| V1 Spawn vista mid-wave | `M4-V1` | | | |
| V2 Rifle ADS | `M4-V2` | | | |
| V3 Pump blast | `M4-V3` | | | |
| V4 Headshot kill | `M4-V4` | | | |
| V5 Knight wind-up | `M4-V5` | | | |
| V6 Box-up fight | `M4-V6` | | | |
| V7 Wave banner | `M4-V7` | | | |
| V8 Main menu | `M4-V8` | | | |

## Play-tests

| # | Chunk | Commit | Fun (1–5) | AAA feel (1–5) | Difficulty | Unfair deaths | What's off | Session / runs |
|---|---|---|---|---|---|---|---|---|

## Tuning changes

Feel numbers only (D117), ±50% without asking, with the reason for each.

| Date | Number | From → to | Why |
|---|---|---|---|

## Log

- 2026-09-28: grill Round 11 (D96–D118) settled; `docs/M4-SPEC.md`, `docs/M4-GOAL.md` and this report written.
- 2026-09-28: **goal launched** (Jake pasted the launcher into the grill session).
  - Housekeeping: the 11 merged M3 worktrees were removed (branches kept), and stale test binaries were pruned. Free disk went from 12 to 18 GB.
  - **Gameplay pin landed** (`327e4c5`): `tests/gameplay_pin.rs` compares every gameplay section of `Tuning`, the hitboxes and the grid with `tests/fixtures/gameplay-pin.json`, taken from M3's closing values (D117).
  - **Dispatched** in parallel (the two-build cap):
    - chunk 0, GPU budget and launch (`m4-gpu-launch`);
    - chunk 1, hit feedback and kills (`m4-hit-feedback`).
  - The art slice waits for a free build slot.
  - A researcher is shortlisting CC0 and CC-BY music for chunk 3's listening page. Nothing is downloaded before Jake's OK (D116).
  - The board targets (`M4-V1`–`V8`) are in use as generated; Jake's explicit approval is still pending.
- 2026-09-28: **the music listening page is ready** (private Artifact https://claude.ai/artifact/6PhbUgWT9KYcLFX8r5E3Dx).
  - It lists 3 candidates for each of the 7 slots, all from OpenGameArt or Freesound, with every license checked on its page. Incompetech was left out: its pages wouldn't render, so its license couldn't be verified, and it offers MP3 only.
  - Recommended picks:
    - Enchanted Festival (menu);
    - Heavenly Loop (break);
    - Heroic Demise (combat low);
    - Battle March (combat high);
    - Just a random fanfare (round start);
    - wah wah sad trombone (death);
    - Won! (new best).
  - Waiting on Jake's picks and his OK to download (D116). `ffmpeg` is available for trimming and OGG encoding.
