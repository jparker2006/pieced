# Milestone 4 "AAA": report

**Status:** IN PROGRESS (goal launched 2026-09-28). The contract is `docs/M4-SPEC.md`, and the brief is `docs/M4-GOAL.md` (grill Round 11, D96–D118).

Every number here names its source (session folder or test), the commit, and the power and Low Power Mode state. Failures and reruns are recorded, not deleted.

## Gates

| Gate | Status | Evidence |
|---|---|---|
| A0 Carried gates closed (M3 W7/W8, M2 S2/S3, W0) | PENDING | |
| A1 Hit feedback and kills | PENDING (tests pass; play-test 1 due) | Merged `c227c3d`. On `aec1fe2`, `scripts/cargo.sh test --locked` had 629 passed, 0 failed, 16 ignored (52 suites), including `tests/kill_feedback.rs` 18/18 and the gameplay pin. Clippy `-D warnings`, `fmt --check` and `build-art.sh --check` are clean |
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
  - The board targets (`M4-V1`–`V8`) are in use as generated.
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
- 2026-09-28: **music direction (D119).** Jake: "I'm sure your recs on the music are good. I want it to feel like John Williams composed this game."
  - The first shortlist leaned festival and comic, so a researcher is re-shortlisting symphonic, brass-fanfare and celesta candidates (CC0 or CC-BY only).
  - Jake gets one final file list to OK before anything is downloaded.
- 2026-09-28: **board approved.** Jake: "approve all 8 board images". `M4-V1`–`V8` are the A6 targets.
- 2026-09-28: **Williams-style picks proposed** (waiting on Jake's download OK):
  - Menu: humanoide9000 "Space Fanfare" (freesound 744049).
  - Combat low: "Cinematic orchestral adventure music" (689177).
  - Combat high: "Cinematic Battle Music (Star Wars Style)" (685841).
  - Round start: Sheyvan "Orchestral Victory Fanfare" (470083).
  - New best: humanoide9000 "Victory Fanfare" (466133).
  - All five are CC-BY 4.0, about 24 MB together. They would come from Freesound's high-quality previews, since originals need a login.
  - No CC0 or CC-BY celesta cue exists; the ideal ones are non-commercial. So the break is a code-built celesta ostinato over a string drone, and the death sting is a downward bend of a brass chord from the battle cue.
- 2026-09-28: **music downloaded with Jake's OK** ("yes, download them").
  - The five CC-BY 4.0 Freesound high-quality OGG previews (about 180 kbps, 4.9 MB in total), each license re-checked on its page.
  - Committed as sources in `art/music/src/` with `CREDITS.md` (`98730d9`). Chunk 3 trims, loops and credits them in `assets/ASSETS.md`.
- 2026-09-28: **disk ran out** (1.7 GB free); `scripts/cargo.sh` refuses to build under 3 GB.
  - Pruning freed nothing, because all 15 GB of the debug cache was live builder output.
  - With Jake's OK ("you run it for me be careful"), the stale M3 `release/` cache (4.7 GB) was deleted, after checking that `pieced-play` sits outside it. Free disk went to 6.2 GB. The next play build recompiles the release dependencies.
- 2026-09-28: **chunk 1 nearly done, on branch `m4-hit-feedback`** (WIP commit `2c3f18d` and later). The builder reported:
  - kill confirm, the X marker and a 2-frame hitstop that holds only `FreezableTime`;
  - directional flinch, sparks and 2–4 armor chips;
  - the headshot ding and a dented helmet (`knight_helmet_dent`, 1,516 triangles);
  - physical deaths through the reusable chunk sim `fx/chunks.rs`;
  - popups and callouts.

  The full suite passed (51 suites, 0 failed, pin and TTK unchanged), and `tests/kill_feedback.rs` is 16/16. Clippy and fmt are clean. Still open: the dent and wave-bonus tests and `tests/kill_offscreen.rs` (compile and run), the review render against V2 and V4, and a rerun of `build-art.sh --check` after a determinism fix.
- 2026-09-28: **paused** (Jake losing connection). Both builders were asked to commit their work in progress and stop.

## Resume here (next session)

1. `git worktree list`. Chunk 1 is on `m4-hit-feedback` (`.claude/worktrees/agent-a1a8ca69e7eb45ea8`); chunk 0 is on the `.claude/worktrees/agent-ab9440989154b3d5b` worktree's branch. Read each branch's last commit message for what's left.
2. `df -h ~`: need more than 4 GB free. Run `scripts/prune-target.sh` first.
3. **Chunk 1** is paused at `9763ad0` on `m4-hit-feedback`. Since `2c3f18d`, the dent and armor-hiding bug on the rigged knight is fixed (the helmet mesh had no visibility component to toggle), `tests/kill_feedback.rs` is 18/18, and the dent asset builds identically every time. Left: run and review `tests/kill_offscreen.rs` against V2 and V4, then rerun `build-art.sh --check` and the full test suite, clippy and fmt on `9763ad0`. Dispatch a `pieced-builder` into its worktree for these. Then `--no-ff` merge to `main` after the test suite, clippy, fmt and `build-art --check`. Build `pieced-play` (release, low priority) and send Jake **play-test 1** with the five questions.
4. **Chunk 0** is on branch `worktree-agent-ab9440989154b3d5b`, at `20cca05`.
   - **Done:**
     - Per-pass GPU timing through `src/gpu_timing.rs`. Metal can't timestamp inside a pass, so the marks sit between passes. It was checked offscreen on the real GPU: world 4.4 ms, effects 0.8, UI 0.35, post 1.2.
     - The timing costs about 0.5 ms per timed frame, which is over the 0.3 ms limit, so it samples 1 frame in 8 by default.
     - Outlines can't be split from world.
     - Per-pass GPU columns in the session log, and `sessions.py --gpu`.
     - S2 on presented frames, with the starting and highest wave recorded, and `--s2` showing the first A7 pass.
   - **Tests:** `tests/sessions.rs` 27/27 and `test_sessions.py` 10/10. The full suite and clippy haven't been re-run on the final edits.
   - **Left:** the levers (step 4), the launch cut (step 5), and the budget and allocation tests (step 6).
   - **Needs Jake's OK before merging:** it adds `wgpu = "=29.0.4"` (default features off) as a direct dependency. It's the same wgpu Bevy 0.19.1 already builds, so nothing new compiles. It's needed because Bevy doesn't re-export the timestamp query types.
5. Then the art slice (board captures in `tests/m4_board.rs`) and chunk 2 (weapons), at most two builds at once.
6. Music sources are ready in `art/music/src/` for chunk 3. The board V1–V8 is approved.
- 2026-09-28: **Jake OK'd the direct `wgpu` dependency** for chunk 0: "yes, wgpu is fine". It is `=29.0.4` with default features off, the same wgpu Bevy 0.19.1 builds, for the timestamp query types.
- 2026-09-28: **chunk 1 merged** (`c227c3d`, from `aec1fe2`).
  - What's in it:
    - kill confirm: the cha-ching sound, the X marker (gold on a headshot kill) and a 2-frame hitstop that holds only `FreezableTime`;
    - directional flinch, metal sparks and 2–4 armor chips;
    - the headshot ding and the dented helmet (`knight_helmet_dent`);
    - physical deaths: the helmet and limbs fly off, bounce and clatter, gone by 1.5 s, through the reusable chunk sim `fx/chunks.rs`;
    - "+100", "+50 HEADSHOT" and "+150 VOID" popups (Waves only);
    - the wave bonus pops under the score;
    - "DOUBLE!" to "RAMPAGE!" callouts, drawn below-right of the crosshair after the review render showed them overlapping the popups.
  - Shared changes: `HitstopFrozen`, the `ScoreAwarded` message, and `Tuning::kills` (`#[serde(skip)]`). `load_or_default` now resets the hitstop numbers.
  - Review render (`tests/kill_offscreen.rs`), compared with V2 and V4 by the builder:
    - the body hit and the headshot kill read well;
    - the dent is subtle at 3.5 m;
    - on the kill frame the poof hides the flying armor, which only reads a few frames later.

    Both are candidates for the art slice.
  - Before the release build, the chunk 1 worktree was removed and stale test binaries pruned (5.7 → 12 GB free). `pieced-play` for play-test 1 is building.
