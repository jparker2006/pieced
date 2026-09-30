# Milestone 4 "AAA": report

**Status:** IN PROGRESS (goal launched 2026-09-28). The contract is `docs/M4-SPEC.md`, and the brief is `docs/M4-GOAL.md` (grill Round 11, D96–D118).

Every number here names its source (session folder or test), the commit, and the power and Low Power Mode state. Failures and reruns are recorded, not deleted.

## Gates

| Gate | Status | Evidence |
|---|---|---|
| A0 Carried gates closed (M3 W7/W8, M2 S2/S3, W0) | PENDING | |
| A1 Hit feedback and kills | **PASS** (`c227c3d`) | Merged `c227c3d`. On `aec1fe2`, `scripts/cargo.sh test --locked` had 629 passed, 0 failed, 16 ignored (52 suites), including `tests/kill_feedback.rs` 18/18 and the gameplay pin. Clippy `-D warnings`, `fmt --check` and `build-art.sh --check` are clean. Play-test 1 recorded: fun 4, AAA feel 3.5, about right, no unfair deaths |
| A2 Weapons | **PASS** (`3a84e94`) | Merged `3a84e94` from `4d6ccf9`. `tests/viewmodel.rs` has 24 tests: every animation inside its gameplay time, and shots and look bit-identical with and without the kick. The full suite, clippy and fmt are clean (two hud audio tests flaked once and passed on rerun); the gameplay pin passes. Play-test 2 recorded: fun 4, AAA feel 4, "the guns looks super AAA" |
| A3 Audio and music | **PASS** (`6ca867d`) | Merged `6ca867d` from `bbf0470`: 680 passed, 0 failed. `tests/music.rs` 16, 4 new in `tests/audio.rs`, and a new license audit in `tests/assets.rs`. Clippy, fmt and `build-music.sh --check` (byte-identical) are clean. Five CC BY 4.0 files are credited in `ASSETS.md`, and Jake picked them (D116, D119). Play-test 3 recorded: fun 4, AAA feel 4, "super fun with music" |
| A4 Knight animation | **PASS** (`5e13cc7`) | Merged `5e13cc7` from `4a44dac`: 709 passed, 0 failed, 22 ignored; clippy, fmt and `build-art --check` clean (the clips export byte-reproducibly). Sampled hitbox fit every 50 ms: body ≤ 9.7 cm (limit 10), helmet ≤ 8.8 cm (D121 limit 9). The wind-up is exactly 0.4 s. Play-test 4 recorded: fun 4, AAA feel 4, "it is really starting to feel like a real game!" |
| A5 Presentation, building juice and camera | PENDING | |
| A6 Target board (V1–V8 each ≥ 4) | PENDING | |
| A7 Performance (wave ≥ 6, battery, Low Power Mode, presented frames) | PENDING (close) | `20260929-215209` (`da732d7`, battery and LPM on every sample, 11 min 18 s): presented mean **16.73** ✓, **99.59% < 18** ✓, but 147 frames > 25 ms ✗, and it reached wave 5, not 6 ✗ |
| A8 Launch (warm < 5 s ×3; Play → controllable < 1 s) | PENDING | |
| A9 Final verdict (fun ≥ 4.5, AAA feel ≥ 4, no unfair deaths) | PENDING | |
| A10 No regressions | PENDING | |

## GPU budget (reported every chunk, not gated; D114)

| Date | Commit | Session (power, LPM) | World | Outlines | Far | Effects | UI | Post | Total p95 |
|---|---|---|---|---|---|---|---|---|---|
| 1 | Hit feedback and kills | `40ef03c` | **4** | **3.5** | About right | **None** | "I think AAA games just have a certain thing about them that make them feel like real studio premium games and were not quite there yet". Smoothness: "It felt the same smoothness" | `20260929-024538` (battery 18→15%, LPM on, builds paused; 369 s; wave 4, score 3,850, 20 eliminations, 21.6% accuracy, 18 headshots) |
| 2 | Weapons (music not yet in) | `c0561c5` | **4** | **4** | About right | **None** | "the guns looks super AAA, everything else (background, knights, etc) need to now too!" | `20260929-060802` (battery, LPM on, builds paused, `--knobs pipelined=on`; 375 s; wave 4, score 4,000, 22 eliminations) |
| 3 | Audio and music | `6ca867d` | **4** | **4** | About right | **None** | Nothing named: "super fun with music" | `20260929-063236` (AC power, LPM on, pipelined on; 197 s; wave 3, score 2,400). **Not a clean performance session**: load 10 rising to 43 while headless art work that had started after quiet mode came on kept running |
| 3b | Interim: art rounds 1–3 and the perf follow-up (pipelined by default) | `da732d7` | **4** | **4** | About right | **None** | "it was super fun the guns looks great!" | `20260929-215209` (battery 25/25, LPM 25/25, builds paused; 678 s counted; wave 5, score 6,250, 32 eliminations, 35 headshots) |
| 4 | Knight animation and chibi knight | `5e13cc7` | **4** | **4** | About right | **None** | "it is really starting to feel like a real game!" | `20260930-002542` (battery, LPM on; still open when answered; wave 4 in the first run) |
| Budget | | | 4.5 | 1.5 | 2.0 | 2.0 | 0.5 | 1.0 | ≤ 12.0 |
| 2026-09-29 | `da732d7` | `20260929-215209` (battery, LPM, pipelined) | 4.33/5.25 | in world | in world | 0.61/1.32 | 1.58/1.81 | 0.93/1.42 | **7.53/8.68** |
| 2026-09-29 | `3a22f97` (offscreen, load 8–13) | offscreen full wave, 3 runs, before → after | 4.58/7.79 → 3.84/5.79 | in world | in world | 0.72/1.44 → 0.79/1.69 | 1.50/3.59 → **1.16/1.46** | 1.53/3.30 → **1.02/2.16** | 8.34/13.27 → **6.81/9.36** |
| 2026-09-28 | `c0561c5` | `20260929-060802` (battery, LPM, pipelined on) | 4.19 / 5.16 | in world | in world | 0.65 / 1.33 | **2.17 / 2.83** | **2.35 / 3.44** | 9.41 / **10.83** |

## Target board

| View | Target | Round 1 | Round 2 | Jake's score |
|---|---|---|---|---|
| V1 Spawn vista mid-wave | `M4-V1` | 3 | 3 | |
| V2 Rifle ADS | `M4-V2` | 2 | 2 | |
| V3 Pump blast | `M4-V3` | 2 | 2.5 | |
| V4 Headshot kill | `M4-V4` | 1.5 | 1.5 | |
| V5 Knight wind-up | `M4-V5` | 2 | 2.5 | |
| V6 Box-up fight | `M4-V6` | 3 | 3 | |
| V7 Wave banner | `M4-V7` | 3 (world only) | 3 (world only) | |
| V8 Main menu | `M4-V8` | 3 | 3 | |

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

## Resume here (next session, updated 2026-09-29)

Paused at the usage limit. Both builders were told to commit their work in progress and stop.

1. **Done and on `main`:**
   - chunks 0–3 are merged, and A1, A2 and A3 PASS (play-tests 1–3: fun 4/4/4, AAA feel 3.5/4/4);
   - art rounds 1–2 are merged (`83ded7f`).
2. **`m4-art`** (worktree `.claude/worktrees/agent-abacfe9e12f7760d4`): round 3 is in progress, working on the priorities in the round 2 entry of the Log:
   - mid-grey steel, an A-line robe, greaves and warm eyes;
   - saucer undersides and bank;
   - kill effects that frame the knight.

   **Round 3 is committed at `a589809`, not merged.**
   - Changes:
     - mid-grey steel `#8E93AB` and a softer rim (2.0 → 1.3);
     - a belt, greaves and an A-line robe; the wand is now the brightest part of the knight (9,490 triangles, S5 green);
     - bronze saucer undersides and a 15° bank toward the camera;
     - gold beams and landing circles;
     - a low, delayed poof and a bigger helmet pop;
     - pump bursts 1.4× bigger;
     - the V8 plinth widened into a wall, and V6 looking up through the window.
   - **Before merging:**
     - the full suite on `a589809` (the last run failed only the ship-bank test, since fixed). The one-off `tests/models.rs` failure is confirmed as timing noise under load: 3 runs on `a589809` passed 7/7 each;
     - re-run the world-pass GPU timing on a quiet machine (4.61/6.04 under load against 4.06/5.19 in round 2).
   - **Open:** V2's ADS "brown tube", "DOUBLE!" overlapping the gun in V4, V5's stance (chunk 4) and V7's banner (chunk 5).
   - Then run `pieced-art-reviewer` on `round3/`. Keep iterating until every view is predicted ≥ 4. Then publish the board page for Jake to score (A6).
3. **`m4-perf2`** (worktree `.claude/worktrees/agent-ae2f9c1d477f37f63`): in progress:
   - input-to-present latency in the log;
   - pipelined rendering as the default;
   - cutting the UI and post passes.

   **Status (`a775c91`, not merged):**
   - Done:
     - The UI camera now draws the HUD and writes the window in one pass; the old 2D pass and the full-screen image are gone. HUD offscreen captures match (SSIM ≥ 0.9989).
     - Pipelined rendering is the default (`--knobs pipelined=off` turns it off). The presented cadence and `acquire_ms` are now logged under pipelining; before, they read 0.
     - Input-to-present latency for presses and look goes into `frames.csv`, `session.json`, `PIECED_S2` and `sessions.py --latency`. It isn't yet measured with real input.
   - Left:
     - The GPU marks on the UI camera's new schedule never get written, so `ui` reads 0 and `gpu_timing_offscreen` fails. Fix that, then re-measure. Offscreen totals fell from 6.58/10.37 to about 5.4/7.0 without the HUD pass.
     - Remove the temporary `debug_last_marks` code (`src/gpu_timing.rs`, `tests/wave_cost_offscreen.rs`).
     - Run the full test suite, clippy and fmt.
   - Then merge, build `pieced-play`, and give Jake a session (battery, LPM, Start at wave 6 once chunk 5 lands) for A7 and the latency check (≤ 33 ms).
4. **Not started:**
   - chunk 4 (knight animation), after the art slice merges, since both edit `knight.py`;
   - chunk 5 (presentation and Start at wave; the banner reads `audio::music::WaveStarted`).
   - Then play-tests 4 and 5 (A4, A5, A9), A8 (3 warm launches in a row), A10, and A0.
5. **Rules learned** (also in the builder agent and in memory):
   - builders check the `.quiet` flag before heavy non-cargo work;
   - never run the game binary to read its flags;
   - watch disk: prune, and ask Jake before deleting `release/`.

## Earlier resume notes (2026-09-28, superseded)

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
- 2026-09-28 19:38: **`pieced-play` built for play-test 1** (release from `main` `40ef03c`, which is chunk 1 at `c227c3d` plus docs; 22 min 47 s at low priority; 126 MB). Chunk 0 was resumed on its branch, and Jake OK'd the `wgpu` dependency.
- 2026-09-28 19:45–19:52: **play-test 1** (release `pieced-play` from `40ef03c`, launched by the orchestrator with quiet mode on; battery at 18% falling to 15%, Low Power Mode on). Jake, quoted: "done playing, 4, 3.5, about right, no unfair deaths". He reached wave 4 (score 3,850).
  - **Launch:** 2,953 ms **cold** (the first launch of a new binary), faster than M3's warm 4.1–4.6 s. Play → controllable took 72 ms.
  - **Frames: a regression.** `PIECED_S2 FAIL`: mean 25.63 ms, p99 41.26, 7,509 frames > 25 ms, 41.3% < 18 ms (45.2% by the drawable cadence). 86.5% of spikes are `acquire` (18.0 ms on spikes against 6.0 on normal frames), so the GPU is far behind.
  - Low battery doesn't explain it: M3's `20260928-085054` (`5086a57`, battery 13%, LPM) got mean 16.91 ms and 94.45% < 18, at higher background load (7–25 against 4.5–5.8 now).
  - **Next:** the chunk 0 builder was redirected to bisect the GPU cost between `fb039a3` and `c227c3d` offscreen with the per-pass timer (chunk 1 is the prime suspect), fix it, and pin it with a test.
- 2026-09-28: **play-test 1, the rest of Jake's answers.** On smoothness: "It felt the same smoothness". On what's off: "I think AAA games just have a certain thing about them that make them feel like real studio premium games and were not quite there yet".
  - The builder was told he felt no choppiness, so it checks whether the frame "regression" is a measurement or pacing change from chunk 1's time handling (the hitstop now uses `HitstopFrozen`), not only GPU cost.
- 2026-09-28: **D120:** chunks 2 (weapons) and 3 (music) run together after the regression check, and play-test 2 covers both. Jake: "yes do weapons and music together".
- 2026-09-28: **chunk 0 merged** (`3ebb2aa`, from `ce97381`: 646 passed, 0 failed, 19 ignored; clippy, fmt and `test_sessions.py` clean).
  - **Play-test 1's slow frames were pacing, not a GPU regression.** Offscreen, a full wave (wave 6, 8 knights, UI at Retina size) costs the same on `fb039a3` and on current code:
    - CPU 11.3–11.6 against 10.5–11.6 ms;
    - GPU 8.7–9.0 against 8.1–9.5 ms.
  - The CPU and GPU run one after the other (pipelined rendering off, frame latency 1, both since M1), so a heavy wave with 12.7 ms of CPU and an 11.4 ms drawable wait lands at about 25 ms. It isn't a logging artifact: `vsync_dt_ms` matches `dt_ms`.
  - **Next:** a session with `--knobs pipelined=on` (and one with `latency=2`); a default changes only on Jake's numbers.
  - **Per-pass GPU for a full wave, offscreen (mean/p95 ms):** world 4.31/5.28, effects 0.88/1.54, UI 1.46/2.44 (over its 0.5 budget), post 1.88/3.12 (over its 1.0). Total 8.5/10.2, within 12. UI and post run over because of full-screen passes at Retina size. Outlines draw inside the world pass.
  - **Levers:**
    - `farres=half` works but saves nothing (world 4.00 + far 1.08 against 4.31), so it stays off.
    - `dynres` should save about 36% of fragment work at 0.8×.
    - `overdraw=cap` covers halos only.
    - New defaults: outline fade 15–25 m, and far knights animated at 30 Hz.
  - **Launch:** the "+1.3 s knight rig" is one boot frame compiling 57–59 pipelines (1,051 ms on `c3f6379`, 374 ms in the latest cold launch). No launch cut was made; the cold launch already reached the menu in 2,953 ms.
  - **Mistake, recorded:** while checking the knob syntax, the orchestrator ran `pieced-play --help`, which launched the game (session `20260929-040612`, about 2 minutes, not a play session). It was closed as soon as it was noticed; the rule is never to run the game binary without Jake asking.
- 2026-09-28: **chunks 2 (weapons, `m4-weapons`) and 3 (audio and music, `m4-audio-music`) dispatched in parallel** (D120), after pruning (8 → 16 GB free).
  - Chunk 3 owns all audio, including the layered gun shots.
  - Chunk 3 will add Bevy's `vorbis` feature (the `lewton` crate) for OGG. That needs Jake's OK before merging.
- 2026-09-28: Jake: "yes to lewton, and turn on pipelined for play-test 2".
  - Bevy's `vorbis` feature (the `lewton` decoder) is OK'd for chunk 3.
  - Play-test 2 launches with `--knobs pipelined=on`. The default changes only if the presented-frame numbers improve and input latency stays ≤ 33 ms.
- 2026-09-28: **chunk 2 merged** (`3a84e94`, from `4d6ccf9`).
  - What's in it:
    - the draw on swap rises and settles by 0.2 s;
    - the 0.12 s ADS blend overshoots about 3.7% and lands on the sights;
    - rifle reload: the glove flicks out the dim crystal and slots a glowing one, which charges up;
    - pump: shards pushed in with the fingertips, and a heavier rack (longer pull, slam, clack; `PUMP_RACK_TRAVEL` 0.09 → 0.125, a presentation-only number);
    - breathing sway, off when M1's sway is off;
    - a render-only camera kick (0.12° rifle, 0.3° pump, gone within 0.11 s; overlapping kicks take the max; off when shake is 0);
    - `WeaponCue` beats for chunk 3's sounds.
  - No new meshes or pipelines.
  - **Still off, for the art slice:**
    - the ADS poses read as "a brown tube" next to V2;
    - the glove is a thumbless fist, and a pinch variant won't fit the 2k glove budget (1,904 now);
    - the pump looks slightly small next to V3.
- 2026-09-28 23:08–23:15: **play-test 2** (release `pieced-play` from `c0561c5`: chunks 0–2 without music; launched with `--knobs pipelined=on` and quiet mode; battery, Low Power Mode on). Jake, quoted: "done playing, 4, 4, about right, no unfair deaths / the guns looks super AAA, everything else (background, knights, etc) need to now too!" He reached wave 4 (score 4,000). **A2 PASS.**
  - **Pipelined rendering fixes most of the pacing.** `PIECED_S2 FAIL`: mean 17.40 ms, p99 34.65, 956 frames > 25 ms, 92.21% < 18. Play-test 1 had 25.63 ms, 7,509 frames and 41.3%.
  - **GPU** (`sessions.py --gpu`, the first on-hardware per-pass numbers): the total is 9.41 mean and 10.83 p95, within 12. It's over 12 ms on 0.6% of timed frames. The acquire wait averages 0.00 ms.
  - **Over budget:** UI 2.17/2.83 (the full-screen composite of the 3D image at Retina size) and post 2.35/3.44 (FXAA and the copies into each output). The timer costs 0.39 ms per timed frame.
  - **Launch:** 4,834 ms cold; Play → controllable 55 ms.
  - **Next:**
    - Make `pipelined=on` the default once input-to-present latency is logged and shown to be ≤ 33 ms (G3). This session suggests it's worth it, and Jake's feel was unchanged.
    - Cut the UI and post full-screen passes: composite once at render size, and run FXAA on the render target, not the output.
    - The art slice starts now, on Jake's direction that the world and the knights need to reach the guns' level.
- 2026-09-28: **art slice dispatched** (`m4-art`). Jake: "I want you to seriously wow me on this art slice and blender 3d stuff."
  - The builder was told to treat V1, V4, V5 and V8 as hero shots, with the knight as the star, and to run at least 3 review rounds into `docs/evidence/m4/board/roundN/` with `NOTES.md`, plus model turntables in `docs/evidence/m4/art/`.
  - **Orchestrator decision on look budgets** (not gameplay, not the GPU budget): per-model triangle budgets may rise (knight to 12k, props and trees to 2×, far layer to 120k triangles) **only** while the offscreen full-wave timer keeps the world pass at ≤ 4.5 ms mean and the total at ≤ 12 ms p95. Draw counts, overdraw and full-screen passes may not grow.
  - Shadows, bloom and SSAO still need Jake.
- 2026-09-28: **chunk 3 merged** (`6ca867d`, from `bbf0470`).
  - **Score:**
    - the menu plays Space Fanfare;
    - the break, results and Practice play an **original A-minor celesta waltz** in 3/4 (celesta, harp, strings, flute), synthesized and looping seamlessly;
    - combat plays the adventure cue, switching to the Star Wars-style battle cue at wave ≥ 6 or ≥ 6 knights alive;
    - 1.5 s crossfades; the round-start fanfare on every wave; a death sting made from a brass chord bent down 8 semitones; the victory fanfare on NEW BEST;
    - stings, big hits and pause duck the music.
  - **Settings → Audio:** Master, Music, Effects and Mute, plus a credit line.
  - **Effects:** room reverb baked into every sample at load, pitch spread, layered rifle and pump shots, and all 12 `WeaponCue` beats voiced.
  - **Judged by analysis only** (loop seams, levels, key and meter, spectral balance); nobody has listened yet.
  - **Encoding:** Homebrew's ffmpeg has no libvorbis, so the files use ffmpeg's built-in Vorbis encoder at q6 (about 170 kbps).
  - **Cost:** about 33 MB of RAM for decoded music, 4.5 ms on the main thread at launch, and no GPU.
  - Disk dropped to 3.9 GB; pruning took it to 5.5. The art builder paused its cargo builds and kept doing Blender work.
  - The art builder may now edit the main-menu layout for V8.
- 2026-09-28 23:32–23:36: **play-test 3** (release `pieced-play` from `6ca867d`, chunks 0–3; pipelined on; AC power, Low Power Mode on). Jake, quoted: "done playing, 4, 4, about right, no unfair deaths super fun with music". He reached wave 3. **A3 PASS.**
  - **Launch:** 1,445 ms cold (window at 832 ms, menu at 1,445). Play → controllable took 158 ms.
  - **Frames are not valid for performance:** on AC, mean 39.67 ms and 2.66% < 18.
    - The load average rose from 10 to 43 about a minute in. Spikes sit in `idle` (the event loop starved: 20.6 ms against 4.4 on normal frames), not in the GPU.
    - Cause: the art builder started headless Blender work *after* `quiet.sh stop`, which only pauses processes already running.
    - **Fix:** builders now check the quiet flag (`$CARGO_TARGET_DIR/.quiet`) before any heavy command that bypasses `cargo.sh` or `build-art.sh`. The rule goes into every builder brief and the builder agent definition.
- 2026-09-29: **art round 1 merged** (`e10ea50`, from `fdc7e73`: 681 passed, 0 failed; clippy, fmt and `build-art --check` clean).
  - What changed:
    - the knight got a detail pass (11k triangles);
    - rounder tree puffs, rounder rocks, a brass drop ship;
    - V8 is now a hero shot;
    - a wand orientation fix;
    - halos fade when they would cover the screen.
  - Budget pins were raised for art (knight 11k, tree 3k, rock 600, scene 665k triangles), and the draw pins were tightened (1,500 meshes, 160 batches). Offscreen full-wave world pass: 4.15/5.57 against 4.20/5.42.
  - **`pieced-art-reviewer` on round 1:** V1 3, V2 2, V3 2, V4 1.5, V5 2, V6 3, V7 3 (world only), V8 3. The two big misses:
    - orb halos washing out V2, V4 and V6;
    - a knight that reads as a stick figure at 6–9 m (helmet about 20% of his height against about 35% in the targets, grille bars reading as a skull, trims turning into value noise).
  - Round 2 priorities were sent to the builder:
    - the halo near-fade;
    - a chibi knight within the fixed head sphere (hitboxes don't change);
    - saucer and capsule ships with rune decals under the beams;
    - a hollow pump starburst, and the kill pop held in front of the poof;
    - a bigger wind-up flare;
    - lawn value variation and subtler grid lines;
    - a clean ADS sight;
    - the V6 and V8 compositions.
  - The spec's knight budget now reads ≤ 12k, the orchestrator's art-slice decision.
- 2026-09-29: **art round 2 merged** (`83ded7f`, from `271f883`: 683 passed, 0 failed; clippy, fmt and `build-art --check` clean).
  - What changed:
    - a chibi knight within the head sphere;
    - gilded saucer drop ships with rune circles;
    - halo near-fade 0.42–0.8;
    - hollow pump starbursts and a smaller poof;
    - wind-up swirl sparks;
    - a subtler grid, denser flowers and a warmer grade.
  - World pass 4.06/5.19 ms.
  - **Reviewer on round 2:** V1 3, V2 2, V3 2.5, V4 1.5, V5 2.5, V6 3, V7 3, V8 3. The biggest remaining problem is the knight's steel rendering nearly white (a white egg on a narrow purple column).
  - Round 3 priorities:
    - mid-grey steel with a narrow highlight, an A-line robe, a belt and greaves, and warm eyes;
    - saucer undersides and a bank toward the player;
    - kill effects that frame the knight (a delayed low poof and a big helmet pop);
    - then the V5 flare, the V8 framing, the V2 sight and the V6 window.
- 2026-09-29: **the perf follow-up was dispatched** (`m4-perf2`), with disk back at 14 GB. It covers:
  - input-to-present latency in the session log;
  - pipelined rendering as the default;
  - cutting the UI (2.17 ms) and post (2.35 ms) full-screen passes toward their 0.5 and 1.0 budgets;
  - a pin on the number of full-screen passes.
- 2026-09-29: **resumed. Art round 3 merged** (`c9b8ac9`, from `7a02464`).
  - Checks: 684 passed, 0 failed, 21 ignored; clippy, fmt and `build-art --check` clean.
  - Offscreen full-wave world pass (mean/p95): 3.31/4.82, 4.15/5.90 and 4.55/6.55 ms, at load 6.4, 6.8 and 7.6. The time rises with machine load, and the quietest run is well inside budget.
  - The worktrees of the merged weapons and music branches were removed.
  - The perf follow-up (`m4-perf2`) was resumed to fix the missing GPU marks on the new UI schedule. `pieced-art-reviewer` is running on round 3.
- 2026-09-29: **reviewer on art round 3:** V1 3, V2 2, V3 2.5, V4 2, V5 2.5, V6 2.5 (it regressed: the window framing shows only the lintel), V7 3 (world only), V8 3.5 (the best view).
  - **More rounds of the same kind** should lift V8, and maybe V1 and V7, to about 3.5–4.
  - **Three structural limits:**
    - (1) **Proportions:** the fixed head sphere blocks the targets' chibi helmet (about a third of his height). Asked Jake as Q121.
    - (2) **Chunk 4 poses:** V1, V3, V5 and V8 cap near 3.5 without them.
    - (3) **Board staging:** V3, V4 and V6 are choreography problems in `tests/m4_board.rs`.
  - **Next art round:**
    - airship-style ships, smaller or higher, with violet beams in V1;
    - a crisp, smaller wand flare;
    - restaging V3, V4 and V6;
    - the V8 cap and UVs;
    - the knight's silhouette, depending on Q121.
- 2026-09-29: **D121:** the helmet may visually overhang the head hitbox by up to +45% of its radius (about 9 cm). It's cosmetic, and the hitboxes and numbers are unchanged. Jake: "rec". S5's helmet tolerance becomes 0.09 m, and the body keeps 5 cm.
- 2026-09-29: **perf follow-up merged** (`da732d7`, from `3a22f97`: 700 passed, 0 failed, 22 ignored; clippy, fmt and `test_sessions.py` 12/12 clean).
  - **Pipelined rendering is the default** (`--knobs pipelined=off` turns it off).
  - **One UI pass at window size:** the old 2D pass and the full-screen image of the 3D view are gone, and the world camera skips its copy when it shares the viewmodel's texture.
  - **Input-to-present latency** for presses and look is in the session log (`sessions.py --latency`), not yet measured with real input.
  - **Timing-mark bug fixed:** the UI camera's `CameraOutputMode::Skip` made Bevy drop the view.
  - The offscreen totals are in the GPU table. UI (1.16) is still over its 0.5 budget: clearing and storing the HUD at 7.6 MP. Post is at budget by mean.
- 2026-09-29: **chunk 4 dispatched with the chibi knight** (`m4-knight`: D121 silhouette, then the authored clips), and a play build for Jake is building from `da732d7`.
- 2026-09-29 14:52–15:04: **interim play session** (release from `da732d7`: art rounds 1–3 plus the perf follow-up; pipelined by default; launched with quiet mode; battery, Low Power Mode on). Jake, quoted: "done playing, 4, 4, about right, no unfair deaths it was super fun the guns looks great!" He reached wave 5 (score 6,250, 32 eliminations, 35 headshots).
  - **Frames (presented): nearly qualifying.** `PIECED_S2`: mean 16.73 ✓, p99 17.00, **99.59% < 18 ✓**, 147 frames > 25 ms ✗. By CPU dt: 16.73 mean, 96.81%, 393 frames. Battery and Low Power Mode were on for all 25 samples, and the load stayed at 4.2–6.4.
  - **GPU:** total 7.53/8.68 ms, well inside 12; UI 1.58 and world 4.33 are slightly over their slices.
  - **The remaining spikes are CPU hitches, not GPU.** Of the 393 CPU spikes, 62% are `pre` (window and input events, state changes; 7.9 ms on spikes against 0.5 on normal frames), 20% are `idle` (the OS) and 15% `acquire`. The entity count grew from 7,666 to 8,448 over 10 minutes, probably built pieces; to be checked.
  - **Input-to-present latency** (the first real numbers): presses 40.9 ms median, 50.0 p95; trackpad look 34.6 median, 39.6 p95. The spec's ≤ 33 ms comes from M1's G3, which measured input-to-*submit* (24.3 ms then). This measure ends at *present*, so it isn't directly comparable. Asked Jake as Q122 whether to keep pipelining.
  - **Launch:** 3,332 ms cold; Play → controllable 35 ms.
- 2026-09-29: **D122:** pipelined rendering stays on. Input-to-submit is to be logged for comparison with G3, and latency trimmed where possible. Jake: "rec on Q122".
- 2026-09-29: **chunk 4 merged** (`5e13cc7`, from `4a44dac`).
  - **The chibi knight (D121):**
    - a flat-topped bucket helm 8 cm over the head sphere, a dark visor band with warm eyes;
    - a belt with a gold buckle, a flared robe, round pauldrons, bigger gauntlets, chunky boots;
    - 3-value steel; 9,406 triangles.
  - **Clips on one shared `AnimationGraph`** (D104): Idle 2.0 s, Run 0.6, Strafe L/R 0.6, WindUp exactly 0.4, 4 additive flinches 0.3, BeamLand 0.5, VictoryHop 0.5, VoidFall 0.6, 3 deaths 0.35 each, Hero 2.4. The springs are layered on top.
  - The fit keeps swings to about ±0.1–0.3 rad, so the motion reads subtle.
  - No new draws. The wand flare is capped at 0.85 m (was 1.7), a feel change that fixes V5's pink wash.
  - `knight.glb` grew from 298 to 506 KB.
  - The builder's predicted scores: V1 3.5, V4 3, V5 3, V8 3.5 (`round4-knight/`).
  - Play-test 4 build under way.
- 2026-09-29 17:25: **play-test 4** (release `pieced-play` from `5e13cc7`, chunk 4; battery, Low Power Mode on). Jake, quoted: "done playing, 4, 4, about right, no unfair deaths / it is really starting to feel like a real game! what is left on the AAA feel todo list... how can we give it that final polish before we make it a real game with real objectives". **A4 PASS.**
  - Launch: 4,326 ms cold.
  - The orchestrator ran `quiet.sh cont` too early (the game was still open); Jake said builds were fine while he plays.
- 2026-09-29: **D123: chunk 6 (final polish)** added before the final verdict: knight voice barks and footsteps, a living world, UI sound and motion, and a feel-tuning pass on Jake's 3 least-premium moments. Jake: "rec, do chunk 6 with all four". Play-test 5 (A9) now follows chunk 6.
- 2026-09-29: **D124:** Jake's 3 least-premium moments: "the background / trees, the gun feel (not looks just feel it looks great), and the knights movement and smartness (though that might be a pass for later)". The trees go to chunk 6A (in progress); gun feel and knight movement feel go to chunk 6B; knight smartness goes to the backlog for the next milestone.
- 2026-09-29: Jake, still playing play-test 4: "ok its so so so fun! i thnik the background being still is the biggest issue. it is really fun though. what separates this from like a cod zombies AAA feel".
  - The still background is now the top priority of chunk 6A, which also gains an **ambient soundscape** (a wind bed following the sway, birdsong, a distant castle choir and bells, lantern crackle, a combat drone).
  - The orchestrator's answer on CoD Zombies: the biggest gap is the in-run economy loop (wall buys, box, perks, doors, special rounds). That's next milestone, and already in the backlog.
- 2026-09-29: Jake: "once we polish a little more we will move on to the run loop / economy / actual story of the game that will make it much more fun. as it is now i make it to wave 3-5 then die like every time". This is noted in the backlog as the next milestone's direction. No difficulty tuning in M4 (D117).
- 2026-09-29: **chunk 6A committed on `m4-polish-a`** (`78eff5b`, not merged; the full checks and GPU timing wait for disk).
  - **Voices and footsteps:** synthesized gibberish knight barks (hup, taunts, 4 yelps, hoo-HAH, whaaaa), with per-knight pitch; footsteps on the run clip's footfalls by surface, from the nearest 3 knights.
  - **Ambience:** a wind bed that follows the sway gusts, a castle choir and bells, a combat drone, birdsong.
  - **A living world:** grass, flowers, bushes and tree crowns sway in one travelling gust; the cloud sea circles every 15 min; clouds bob; 4 flocks of birds circle the castle; fuller trees in warm and cool tones.
  - **Bug found and fixed:** rodio 0.22 gives the far ear the louder share, so every spatial sound (including M3's off-screen wind-up warning) has been playing from the wrong side since M3. The listener's ears are swapped in `src/audio/spatial.rs`, with a test.
  - **Fairness:** a warning always wins. It cuts every bark, no bark starts within 0.55 s of it, and the fairness suite asserts it.
- 2026-09-29: **chunk 5 written and committed on `m4-presentation`** (`3f92084`, main merged at `95e9755`). It is **not verified**: the builder stopped when disk fell to 3.2 GB.
  - What's in it:
    - the wave banner (V7) on `WaveStarted`;
    - Start at wave 1/6/10 (not ranked; `start_wave` logged for A7);
    - loading tips; menu motion; the pause blur, with the 3D passes off while paused;
    - live settings, with a new saved Camera effects slider; the results count-up;
    - building assembly, edit flips and debris (pool of 48);
    - camera FOV kick, landing dip, slide tilt and damage nudge, with the aim ray bit-identical;
    - the death cam that frames the killer.
  - Tests passed: menu 20/20, waves_ui 15/15, camera 2/2, pieces 5/5, plus building, editing and waves.
  - Still to run:
    - the rest of the full suite (fairness, gameplay pin and the files after `far.rs` are unconfirmed);
    - two fixes not yet re-run (a motion unit test, and the `ASSETS.md` entry for the blur shader);
    - clippy, and the V7/V8 captures.
  - Known: debris chunks without a known piece frame fly across the line of sight.
- 2026-09-29: **the full-speed switch.** `scripts/env.sh` now honours `$CARGO_TARGET_DIR/.fullspeed` (no low priority, 10 jobs). It's on, with Jake's say-so: "full speed build im not playing rn".
- 2026-09-29: **disk at 3.2 GB, so the orchestrator stopped and asked Jake** (the brief's stop-and-ask line is below 4 GB).
  - The space is mostly `~/.codex/worktrees/ecf9/glox-brain-lab/data/cloud-migration` (51 GB): another project's Codex worktree data, not Pieced's.
  - Waiting on Jake to say whether it can be removed. Chunks 5 and 6A resume their checks when there's room.
- 2026-09-29: **disk freed.** Jake: "dont delete stuff from glox brain … doesnt have to belong to pieced... legit j not glox brain".
  - Nothing in glox-brain or glox-brain-lab was touched: every Codex worktree turned out to be glox, so all were left alone.
  - Only regenerable caches were cleared: pip, pnpm, Homebrew, node-gyp, the Claude desktop ShipIt, the opencode updater, the Codex app, CodexBar, Antigravity, Spotify, Google. Free disk went from 3.2 to 14 GB.
  - The rule is saved in memory. Chunk 5's builder resumed its final checks at full speed; chunk 6A follows.
