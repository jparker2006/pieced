# Milestone 3 "Waves": report

**Status:** IN PROGRESS. The goal launched 2026-09-27. The contract is `docs/M3-SPEC.md`, and the brief is `docs/M3-GOAL.md`.

Every number here names its source (session folder or test), the commit, and the power and Low Power Mode state. Failures and reruns are recorded, not deleted.

## Gates

| Gate | Status | Evidence |
|---|---|---|
| W0 M2 closed | | |
| W1 First grunt wave | | |
| W2 Endless waves | | |
| W3 Fair knights | | |
| W4 Ships and the void | | |
| W5 Castle and sky | | |
| W6 Menu and controls | | |
| W7 Performance (wave ≥ 6, battery, Low Power Mode) | | |
| W8 Launch (warm < 5 s ×3; Play → controllable < 1 s) | | |
| W9 Fun verdict (fun ≥ 4, no unfair deaths) | | |
| W10 No regressions | | |

## Castle concept

**`docs/design/concepts/M3-C4-citadel-mix.png`** (D94, 2026-09-27). Jake: "a mix of c2 and c3, c3 my fav tho". C3's starlit citadel is the base; C2 adds a taller stacked centre, an orbiting ring of golden runes, rising lantern streams and a warm golden aura. C4 was generated from C3 and C2 as the combined target. The concepts are local only (git-ignored).

## Play-tests

| # | Chunk | Commit | Fun (1–5) | Difficulty | Unfair deaths | What's off | Session / runs |
|---|---|---|---|---|---|---|---|
| 1 | One grunt wave | | | | | | |
| 2 | Endless waves | | | | | | |
| 3 | Ships and the void | | | | | | |
| 4 | Castle and sky (look check) | | | | | | |
| 5 | Menu and controls (final) | | | | | | |

## Tuning changes

Changes to spec defaults made from play-test answers (±50% allowed without asking), with the reason for each.

| Date | Number | From → to | Why |
|---|---|---|---|

## Log

- 2026-09-27: **goal launched** in the grill session.
  - **Chunk 0** (session frame log, cold/warm launch, boot breakdown) was dispatched to a builder (`m3-session-log`).
  - **Chunk 4** (castle to D94's `M3-C4`, and the magical sky) was dispatched as the background art slice (`m3-castle-sky`).
- 2026-09-27: **shared skeleton merged** (`9db8ae4`):
  - `GameMode` (Practice stays the default in tests and scenarios; `--waves` / `--practice`);
  - `grunt.rs` with `GruntTuning` and the `GruntStats::for_wave` scaling (tested: wave 25 ≈ 196 HP, speed capped below the sprint, aim reaches "Hard" at wave 15);
  - `orb.rs` with `Wand` and `Orb` (the gun step skips wand carriers);
  - `waves.rs` with `WavesTuning` (tested: 440 grunts through wave 20);
  - the new tuning sections are never saved to `settings.json`, since it already stores every other gameplay number and would freeze these;
  - `GameCue::WandWindup` / `OrbFired`, and `Sim::waves`.

  `cargo test --locked`: 0 failures; clippy and fmt are clean.
- 2026-09-27: **chunk 1 dispatched** as three parallel slices: A, the grunt brain (`m3-grunt-brain`); B, the orb, wand and feedback (`m3-orb-wand`); C, the run, pool, death, restart and pump knockback (`m3-run`).
- 2026-09-27: **chunk 0 merged** (`3f9a213`), with 429 tests passing (0 failed), clippy and fmt clean, and `scripts/test_sessions.py` OK.
  - Every native session writes `userdata/sessions/<stamp>/` with `frames.csv` (frame, t_ms, dt_ms, state, occluded) and `session.json` (commit, build id, preset, launch with cold/warm and boot phases, power samples, play and counted time, occlusion, S2 verdict).
  - Rows stream to a writer thread through a bounded channel, and `pmset` runs off the main thread.
  - At quit the game prints `PIECED_S2 …` and `PIECED_SESSION <dir>`.
  - `PIECED_LAUNCH_MS <ms> cold|warm` and `PIECED_BOOT …` print on every launch.
  - `scripts/sessions.py` lists sessions, qualifying S2 runs (`--s2`) and the launch series (`--launches`).
  - Tests: `tests/sessions.rs` (16).
  - **Launch findings (read from the code):** Bevy 0.19 builds render pipelines one at a time on macOS, and pipelined rendering is off, so the 33 warm-up sites compile back to back on the main thread. After a rebuild, Metal's shader cache misses. That fits the cold launches of 8.7, 10.7 and ~12 s against warm launches of 1.1–2.6 s. The next cold launch's `PIECED_BOOT` line will confirm it (a big gap before `warmup=`); if it does, the fix is fewer pipeline variants.
  - Every fresh copy of `pieced-play` counts as cold on its first launch.
- 2026-09-27: **disk:** it fell to 2.4 GB during the parallel builds. Old gate binaries, a stale set of test binaries (chunk 0's `build.rs` had given its worktree a second set of about 6 GB) and stale incremental caches were deleted, bringing it back to 8.3 GB.
- 2026-09-27 16:19–16:21: **Jake's first logged session** (release `pieced-play` from `3f9a213`, the M2 sandbox, on battery at 83%, Low Power Mode on; session `userdata/sessions/20260927-231906`).
  - Verdict, quoted: "Its super fun and looks great! Keep building".
  - **Launch:** 3,271 ms **cold** (the first launch of a new binary), under 5 s even cold. The window appeared at 2,446 ms; after that, island +308 ms, knight +339 ms, far +85 ms, warm-up +33 ms, playing at 3,246 ms. The first 2.4 s come before the window exists, not from shader warm-up.
  - **S2: N/A** (127 s of play, under 300 s). The frames were also poor: mean 20.25 ms, p50 16.94, p95 34.1, 1,313 frames > 25 ms, 72.0% < 18 ms. It was steady through play and pause: about 20% of frames missed a vblank (≈ 33 ms) in every 10 s bucket.
  - **Not a clean measurement:** `cargo`/`rustc` were paused, but builder test binaries (including offscreen GPU renders) and headless Blender kept running.
  - **Fix:** `scripts/quiet.sh stop|cont` pauses every build process (cargo, rustc, test binaries in the cache, headless Blender), and new cargo and `build-art` commands wait while it's on. Power samples now record the load average. The next session will separate game cost from background load.
- 2026-09-27: **disk hit 0.9 GB** mid-build (the grunt slice stopped).
  - Incremental caches (5.9 GB) and stale test binaries were deleted, back to 11 GB.
  - `scripts/env.sh` now sets `CARGO_INCREMENTAL=0`.
- 2026-09-27: **chunk 1C merged** (`67cd26d`), with 442 tests passing (0 failed); clippy and fmt clean.
  - The native game now opens in **Waves** (`--practice` gives the sandbox; scenarios stay Practice).
  - A pool of 8 grunt characters is parked at start. Wave 1's 3 grunts pop in 0.4 s apart at seeded spots 2 m inside the edge, ≥ 12 m from the player, each with ±10% speed.
  - "Wave 1 cleared!" loops the wave for now.
  - The player's death stops input, freezes the knights 1 s later, and shows "Wave 1 — N eliminations — press Enter to go again". Enter restarts in place: pieces and cover reset, orbs cleared, knights parked, the player respawned, a new seed.
  - **Pump knockback:** about 4 m at point blank, stopped by walls, knights only (it skips the Practice dummy, so M1/M2 time-to-kill is unchanged).
  - Tests: `tests/waves.rs` (7), `tests/knockback.rs` (5).
  - Until slice A merges, knights stand still.
- 2026-09-27: **all builders stopped at the account's weekly usage limit** (it resets Oct 1, 4 pm PT). Their work was saved on their branches, and the orchestrator is finishing the merges directly.
- 2026-09-27: **chunk 1A merged** (`2cd56c1`): the grunt brain.
  - Perception at 30 Hz; utility modes at 8 Hz (Approach, Strafe, Reposition, ShootPiece), staggered at most 2 per tick.
  - Hand-rolled A* over (cell, level, ground/floor/ramp), capped at 2,000 expansions.
  - Spots 10–18 m out with a spread penalty.
  - Aim leads a lagged snapshot, with error re-rolled per shot, blending Normal → Hard by wave 15.
  - At most 3 attack tokens; walls in the way get shot.
  - Pool wiring: `spawn_grunt`, brain reset and reseed per activation.
  - **Orchestrator fixes:**
    - A floor's top stands 0.1 m above a ramp's top edge, and movement's step-up missed it at some approach phases, so a climbing grunt stalled. Grunts now hop after 0.2 s of pushing without moving; the player's movement is unchanged.
    - Grunt tests run in `Sim::grunt_lab`, which is Waves without the wave director.
    - Wave tests freeze the brains to check landing spots (D83: the seed fixes where, when and how fast knights land).
  - Tests: `tests/grunt.rs` (10) plus 22 unit tests; the full suite has 0 failures, and clippy and fmt are clean.
  - **Known:** where grunts walk after landing can drift slightly between two runs with the same seed. The likely source is physics-query ordering; the wave itself replays exactly.
