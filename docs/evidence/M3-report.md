# Milestone 3 "Waves": report

**Status:** CLOSED by Jake on 2026-09-28 (D95): "wrap up and close it". The goal launched 2026-09-27. Eight gates PASS. W0, W7 and W8 are **CARRIED** into the next milestone (the AAA look-and-feel pass), not passed. The contract is `docs/M3-SPEC.md`, and the brief is `docs/M3-GOAL.md`.

Every number here names its source (session folder or test), the commit, and the power and Low Power Mode state. Failures and reruns are recorded, not deleted.

## Gates

| Gate | Status | Evidence |
|---|---|---|
| W0 M2 closed | **CARRIED** (D95) | M2's S2 (battery performance) and S3 (3 warm launches in a row) still need a qualifying session; they move with W7 and W8. M2's other gates re-run green headless on `5524b1a` as part of W10 |
| W1 First grunt wave | **PASS** (`b3a4c24`) | Chunk 1 tests: `tests/grunt.rs` (10), `tests/orb.rs`, `tests/waves.rs` (7), `tests/knockback.rs` (5), with 501 passing on the merge. Play-test 1: fun 4, about right, no unfair deaths; Jake: "that was so much fun!" |
| W2 Endless waves | **PASS** (`5086a57`) | Chunk 2 tests: `tests/waves.rs` (19), `tests/potions.rs` (5), `tests/waves_ui.rs` (11) and unit tests; 544 passing on the 2B branch, which contains main. Play-test 2: fun 4, about right, no unfair deaths |
| W3 Fair knights | **PASS** (`5524b1a`) | `tests/fairness.rs`: 20 seeded runs, waves 1–10 (240 knights landed, max 8 at once). 2,327 orbs, 373 hits on the player, 2,810 wind-ups (1,918 off-screen, all warned), 91,177 orb segments checked. **Every rule 0 violations**: no-LOS 0, early first wind-up 0, > 3 shooters 0, unwarned off-screen 0, missing arrow 0, through a piece 0, stuck 0, **unfair hits 0, unfair deaths 0**. The suite found and fixed a real bug: an orb could leave the wand tip without a clear line while the eye saw the player (`confirm_shots`) |
| W4 Ships and the void | **PASS** (`fb039a3`) | `tests/ships.rs` (10), `tests/void.rs` (7), 6 unit tests; the branch's full suite, clippy, fmt and `build-art --check` are clean on top of `e1d5b9d`. Play-test 3: fun 4, about right, no unfair deaths |
| W5 Castle and sky | **PASS** (`fb039a3`) | The castle follows D94's `M3-C4`; the orchestrator reviewed the renders against C4 before merging. Sky motion and far budget tests pass (`tests/far.rs`: ≤ 90k triangles, ≤ 18 batches, ≤ 64 halos). Jake's scores, quoted: "5 and 5" (spawn vista, castle up) |
| W6 Menu and controls | **PASS** (`c3f6379`) | `tests/menu.rs` (15), `tests/bindings.rs` (10), updated `tests/waves_ui.rs`; the branch's full suite on top of `c2d4eed` had 609 passing, 0 failed (15 ignored); clippy and fmt clean. The ignored `tests/hud_offscreen.rs` review render covers the main menu and the Controls page (T12 style) |
| W7 Performance (wave ≥ 6, battery, Low Power Mode) | **CARRIED** (D95) | Not met. No qualifying session yet, and the sessions so far miss the bar (e.g. `20260928-163647`: battery + Low Power Mode, 250 s, mean 16.98 ms, 278 frames > 25 ms, 94.3% < 18). The first attributed spike report (`20260928-212450`) puts 86% of spikes in `acquire`: the GPU is behind. Carried into the AAA milestone, which starts with a GPU budget |
| W8 Launch (warm < 5 s ×3; Play → controllable < 1 s) | **CARRIED** (D95) | Play → controllable **passes** (36 ms, `20260928-212450`). Warm launches of 4,139 ms (`fb039a3`) and 4,570 ms (`c3f6379`, to a clickable menu) are under 5 s, but no 3 in a row have been logged. The knight rig (+1.3 s) and pre-window load are the first things to trim |
| W9 Fun verdict (fun ≥ 4, no unfair deaths) | **PASS** (`c3f6379`) | Play-test 5, quoted: "done playing, 4.5, about right, no unfair deaths, that was actually so much fun." Every play-test is recorded above: 4, 4, 4, castle 5/5, 4.5, with no unfair deaths in any |
| W10 No regressions | **PASS** (`5524b1a`) | `cargo test --locked --no-fail-fast`: **610 passed, 0 failed**, 15 ignored (every M1 and M2 test, including S4 motion, S5 hitbox fit, S6 spell timing and S10 building). Clippy `-D warnings` and `fmt --check` clean. `scripts/build-art.sh --check`: 66 assets byte-identical. `scripts/test_sessions.py` OK |

## Castle concept

**`docs/design/concepts/M3-C4-citadel-mix.png`** (D94, 2026-09-27). Jake: "a mix of c2 and c3, c3 my fav tho". C3's starlit citadel is the base; C2 adds a taller stacked centre, an orbiting ring of golden runes, rising lantern streams and a warm golden aura. C4 was generated from C3 and C2 as the combined target. The concepts are local only (git-ignored).

## Play-tests

| # | Chunk | Commit | Fun (1–5) | Difficulty | Unfair deaths | What's off | Session / runs |
|---|---|---|---|---|---|---|---|
| 1 | One grunt wave | `b3a4c24` | **4** | About right ("maybe a little hard to kill them in the 'early rounds'") | **None** | Early grunts a bit tanky → wave-1 HP 100 → 80 | `20260928-045508` (AC, 62 s), `20260928-045801` (AC, 387 s, measured under builder load) |
| 2 | Endless waves | `5086a57` | **4** | About right | **None** | "the pump should hit a little harder" → pump falloff and knockback tuned; "map should be bigger with more progression / stuff to do... but thats out of scope" → backlog | `20260928-085054` (battery + LPM, 206 s, reached wave 3, score 1,900, 10 eliminations) |
| 3 | Ships and the void | `fb039a3` | **4** | About right | **None** | Nothing named: "it was actually so much fun" | `20260928-163647` (battery + LPM, 250 s, wave 3, score 2,250, 12 eliminations); an earlier launch closed after 3 s when the display went away (`20260928-095221`) |
| 4 | Castle and sky (look check) | `fb039a3` | Castle views **5 and 5** against C4 | — | — | — | Jake saw the castle in play-tests 1–3; he scored the offscreen spawn and up views in `docs/evidence/m3/castle/` next to `M3-C4` |
| 5 | Menu and controls (final) | `c3f6379` | **4.5** | About right | **None** | Nothing named. Jake: "that was actually so much fun … one time, they did break my ramps, which was just awesome" | `20260928-212450` (AC, reached wave 5, score 5,250, 25 eliminations, 363 s; fresh seed) |

## Tuning changes

Changes to spec defaults made from play-test answers (±50% allowed without asking), with the reason for each.

| Date | Number | From → to | Why |
|---|---|---|---|
| 2026-09-28 | Pump falloff (`GunTuning::pump`) | full damage to 8 → 10 m; falloff end 15 → 18 m; floor 30% → 40% | Play-test 2, Jake asked for it: "the pump should hit a little harder". The point-blank maximum (10 × 10 = 100) is unchanged, so M1's G6 (no pump kill from full) still holds |
| 2026-09-28 | `GruntTuning::pump_knockback` | 4 → 5 m | Same request |
| 2026-09-27 | `GruntTuning::hp` (wave-1 grunt health; later waves scale from it) | 100 → 80 (−20%) | Play-test 1: "maybe a little hard to kill them in the 'early rounds'". Now 3 rifle body shots or 2 headshots; wave 25 is ≈ 157 HP |

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
- 2026-09-27: **chunk 4 merged** (`7547463`): the castle and sky, built to D94's `M3-C4`.
  - `far.py` rebuilds the station as C4's castle city:
    - a columnar crag rock with waterfalls;
    - a town of cone-roofed turrets around the rim;
    - the great terrace and glowing gate;
    - the hall with a gold-and-teal rose window;
    - the keep stacked like a wedding cake;
    - star-tipped spires and hundreds of warm windows in one part;
    - bridges and satellite castle rocks.
  - `src/far/magic.rs` adds the rotating rune ring, instanced lanterns (a swarm plus rising streams), gold embers, drifting motes (≤ 64), seeded shooting stars, galaxy twinkles and aurora ribbons. All are pure functions of the sky clock, warmed during Boot.
  - The far budget test is raised to 90k triangles, 18 batches and 64 halos.
  - Offscreen review renders are in `docs/evidence/m3/castle/`. The builder stopped at the usage limit before reporting, so the orchestrator reviewed the renders against C4: the rune ring, the lantern swarms and streams, the stained-glass hall, the aurora and the shooting stars all read.
  - Branch verification: 479 tests passing; clippy and fmt clean; `build-art --check` byte-identical (64 assets).
- 2026-09-27: **chunk 1B merged** (`b3a4c24`): the wand and orbs.
  - Holding fire starts a 0.4 s wind-up (`WandWindup` cue, wand glow); releasing fire cancels it. The orb leaves the wand tip.
  - Orbs are pooled and sphere-cast each tick: 12 damage (18 to the head) to the player, 12 chip damage to pieces, and they pass through knights.
  - The restart recycles orbs into the pool.
  - Orb look, sounds (fwoom, near-miss whoosh, the bonk when you're hit), the off-screen warning and the red damage arrow.
  - The wand model (`wand.glb`).
  - After merging main, the grunt tests use a player the orbs can't kill (they watch movement and tokens for minutes of real fire), and orbs may break pieces.
- 2026-09-27: **chunk 1 is complete on `main` (`b3a4c24`).** 501 tests pass, 0 failed; clippy and fmt are clean; `build-art --check` matches (65 assets). The release `pieced-play` for **play-test 1** is building.
- 2026-09-27 21:55–22:05: **play-test 1** (release `pieced-play` from `b3a4c24`, AC power, Low Power Mode on, two sessions). Jake's answers, quoted: "4, about right maybe a little hard to kill them in the 'early rounds', no unfair deaths", then after a second run: "launch it again i wanna play again … done playing that was so much fun!"
  - **Tuning:** wave-1 grunt HP 100 → 80 (see Tuning changes).
  - **Session `20260928-045508`** (62 s, builds paused): mean 16.77 ms, p99 20.29, 25 frames > 25 ms, 96.35% < 18 ms. That's much better than the first logged session, which ran under build load, but still short of the S2 bar.
  - **Session `20260928-045801`** (387 s): mean 45.2 ms and a 23.5 s launch. **Not a measurement of the game:** load was 57 at launch and still 12–16 later. The chunk-2 builder was compiling at full priority, because hand-set environment variables skip `env.sh`'s wrapper, so neither low priority nor quiet mode applied.
  - **Fix:** `scripts/cargo.sh` is now the only way builders run cargo; it applies low priority, the quiet-mode wait and the disk guard.
- 2026-09-28: **chunk 2A merged** (`cf6366e`): the endless wave director.
  - Wave n brings 3 + 2(n − 1) knights, at most 8 at once; the rest poof in at seeded edge points as knights go down. Stats from `GruntStats::for_wave(n)` with ±10% speed.
  - The 10 s break: +50 shield (capped); `SkipBreak` ends it early.
  - Shield potions: a seeded 10% drop, a pool of 12, +25 shield spilling into health, gone after 20 s.
  - Score: 100 per knight, +50 for a headshot kill, +150 for a void kill (the hook for chunk 3), +250 × n per wave.
  - The personal best (wave, then score) in `userdata/best.json`, and one line per run in `userdata/runs.jsonl` with the cause of death.
  - The death beat (`Dying`, then `Over`, with the survivors' victory hops); `EndRun` and `RestartRun`; `--seed N`.
  - `RunSummary` feeds the UI. Headless runs write no files.
  - Tests: `tests/waves.rs` (19), `tests/potions.rs` (5), 6 unit tests. The branch's full suite on top of `0f9472c` had 523 passing and 0 failed; clippy and fmt clean. The orchestrator reviewed the file-writing code (`src/waves/record.rs`) and the `app.rs` changes before merging, because the safety classifier was down while the builder worked.
  - **Chunk 2B dispatched** (`m3-waves-ui`): the wave HUD, the break countdown, the potion look and sound, the death-beat slow motion, and the results screen with Go again.
- 2026-09-28: **chunk 2B merged** (`5086a57`):
  - the run HUD (wave, knights left, score);
  - the break banner and countdown, with the bound Enter key;
  - the potion look and gulp-chime;
  - the death beat (0.3× only while Dying, the view drops to the grass, a vignette);
  - the results card with every stat, the best run, "NEW BEST!", the seed, Go again and Quit;
  - pause → Quit mid-run shows the results.

  The results use `RunPhase` inside `Playing`, not a separate `AppState::Results`.
- 2026-09-28 01:50–01:55: **play-test 2** (release `pieced-play` from `5086a57`, battery at 13%, Low Power Mode on, builds paused). Jake, quoted: "done playing, 4, about right, no unfair deaths, I think the pump should hit a little harder. map should be bigger with more progression / stuff to do... but thats out of scope". He reached wave 3 (score 1,900, 10 eliminations, 21% accuracy, 206 s), and his first personal best was saved.
  - **Launch:** 5,992 ms, **cold** (the window appeared at 4,799 ms).
  - **Frames (S2 N/A, 206 s < 300 s):** mean 16.91 ms, p99 31.98, 168 frames > 25 ms, 94.45% < 18 ms. The spikes run all through the session and are denser in the heaviest fights. Background load from other apps was 7–25.
  - **Next:** a performance builder was dispatched (`m3-perf`). It adds per-frame attribution to the session log (CPU by schedule, GPU time if cheap, event counts, pipeline compiles), plus a spike report, and fixes per-event costs.
- 2026-09-28: **settings file:** `Tuning::load_or_default` now takes only the menu-editable settings from `userdata/settings.json` (look, audio, graphics, HUD, feedback, aim friction). Every section is saved, so designer numbers had been frozen by Jake's first save; tuning changes (the pump) would otherwise never reach his game.
- 2026-09-28: **chunk 3 merged** (`fb039a3`): drop ships and the void.
  - Knights arrive by drop ship (`dropship.glb`, 1,632 triangles). It swoops from the station in about 4 s and hovers 12–16 m over one of 6 edge drop points.
  - A rune circle lights 2 s before the first landing, with a hum. 1–3 knights come down a violet beam 0.4 s apart and can't be hit or act until they land.
  - Knights alive or beaming never exceed 8, and the ship schedule is seeded.
  - **The void:** a pump shove carries a knight through the barrier, and it is flung clear of the grass. It counts as a kill once 3 m below the island top (+100 +150), credited to the shover. The player can't cross.
  - **Accepted decision:** "off the island" is judged at the barrier line, not the grass edge. The grass runs about 9 m past the barrier, more than a 5 m shove.
  - **Known gaps:** a player standing on a lit circle can get knights landing closer than 12 m, and the 2 s warning is a hum then a shimmer rather than one sound.
- 2026-09-28 09:36–09:41: **play-test 3** (release `pieced-play` from `fb039a3`, battery, Low Power Mode on, builds paused). Jake, quoted: "done playing, 4, about right, no unfair deaths it was actually so much fun. I think when we finish this pass, when we start making it look like a true AAA game, and then after that, when we start doing the story behind it and actual progression, it's going to be so fun." He reached wave 3 again (score 2,250).
  - **Warm launch 4,139 ms**, with the window at 3,188 ms. More than 3 s goes before the window exists, and it grows with each build (cold launches of 6.8 s, the 8.7 s one, and 2.4–4.8 s windows). This is the W8 risk; the performance builder is looking at pre-window work.
  - **Frames (S2 N/A, 250 s < 300 s):** mean 16.98 ms, p99 33.2, 278 frames > 25 ms, 94.31% < 18 ms.
  - **Bug found:** the first run of every launch uses the same seed (272065245258904011 in play-tests 2 and 3). D83 wants each run different unless `--seed` is given; being fixed.
- 2026-09-28: **play-test 4, the castle look check.** Jake scored the two castle views against C4: "5 and 5, keep building". **W5 PASS.**
- 2026-09-28: **performance slice merged** (`62a2920`). Branch: 573 passing, 0 failed; clippy and fmt clean; the Python tests pass.
  - **Attribution:**
    - Every `frames.csv` row splits the frame into pre, fixed (physics and tick count), update and post, plus the previous frame's render: extract, prepare, acquire (waiting for a drawable), graph (compile, encode, submit) and render end, idle time, and the drawable cadence.
    - Event counters per frame: knights, orbs, pieces, particles, debris, potions, damage numbers, voices, pipelines compiled, entities.
    - `session.json` gets a spike report (cause bucket, events over-represented on spikes, worst frames, jitter pairs), printed by `scripts/sessions.py --spikes`.
    - A counting-allocator test pins zero per-frame allocation.
    - GPU time is behind `--knobs gpu=on` (not verified on hardware).
  - **Fixes from code reading** (not yet measured):
    - pooled effect slots rewrite a mesh or material only when it changes;
    - path smoothing is staggered by brain phase;
    - building raycasts skip knights;
    - path validation reuses one list;
    - the blob-shadow probe no longer clones a filter per frame;
    - the results card's box-shadow pipeline is warmed.
  - A new test runs 5 simulated minutes of waves (reaching wave ≥ 6 with 8 alive) with every visual: no asset or entity growth.
  - **Launch:** new `PIECED_BOOT` phases (app_built, plugins_ready, startup_start, startup_end). Sound synthesis (76 ms) moved to a thread. **The release build now strips symbols** (190 → about 115 MB), accepted by the orchestrator: faster to load, at the cost of function names in panic backtraces. Startup measures about 110 ms headless, so the ~3.2 s before the window is probably binary load or renderer init; the new phases will say which.
  - Pacing knobs `latency=1|2|3` and `pipelined=on` (defaults unchanged).
  - **Open options:** 6 far materials are touched every frame; one entity per sound; the orb spark pool saturates; damage-number text restyles; knight line of sight is checked at 60 Hz; A* allocates per plan.
- 2026-09-28: **chunk 5 merged** (`c3f6379`).
  - **The main menu:** the logo over the orbiting island; Waves (with the best run), Practice, Settings and Quit. Modes start in place, resetting cover, knights, the dummy and the player.
  - **Quit to menu** from pause, which ends a live run through the results first, and from the results card. `--waves`, `--practice` and scenarios skip the menu.
  - **The Controls page:** every action rebinds to a key or click; Esc cancels; a conflict swaps; Reset restores D43; Cmd, Cmd combos, Esc and F3/F4 are refused. Bindings save in `settings.json` and survive `load_or_default`. Hints show the bound key.
  - **Launch timing:** `PIECED_LAUNCH_MS` marks the clickable menu (`menu_ready`); `PIECED_PLAY_MS <ms> <mode>` prints each mode start and is logged under `plays`.
  - **Fresh seeds:** without `--seed`, each launch seeds from OS entropy and the clock.
- 2026-09-28: **play-test 5 launched** (release `pieced-play` from `c3f6379`, 126 MB after stripping). The first launch at 11:13 on battery recorded 0 s of play (session `20260928-181344`). Relaunched at 14:24 on AC (5%, charging).
- 2026-09-28 14:24–14:31: **play-test 5, the final verdict** (release `pieced-play` from `c3f6379`, AC power, Low Power Mode on). Jake, quoted: "done playing, 4.5, about right, no unfair deaths, that was actually so much fun. My strategy was I would just build up really high, and eventually they would start chasing me up my ramps. Something I really liked: one time, they did break my ramps, which was just awesome." **W9 PASS.** He reached wave 5 (score 5,250, 25 eliminations), with a fresh seed.
  - **Launch:** 4,570 ms **warm** to a clickable menu (app built 721 ms, window 2,002 ms, knight rig +1.3 s, menu 4,570 ms). Play → controllable took 36 ms.
  - **First attributed spike report** (`scripts/sessions.py --spikes`): 2,121 frames > 25 ms, 81.8% < 18 ms, not on battery. **86% of the spikes are `acquire`**, waiting for the swapchain drawable, so **the GPU is behind**, not gameplay CPU (fixed 4%, prepare 6%). Orb and damage frames are slightly over-represented (lift 1.7–1.8).
  - **Caveat:** from about 14:28 the fairness suite compiled and ran at low priority beside the game, so this session's frame numbers are pessimistic. The attribution still points at GPU cost, which matters for W7 and for the look pass.
- 2026-09-28: **W3 merged** (`5524b1a`): the fairness suite (see the gate table). The builder stopped when the app quit; the orchestrator committed its wand-tip fix and ran the suite (46 s, 0 violations).
- 2026-09-28: **W10 on `5524b1a`**: 610 passing, 0 failed; clippy, fmt, `build-art --check` (66 assets) and the Python tests clean.
- 2026-09-28: **Milestone 3 closed by Jake (D95).** After play-test 5 he asked whether this was a good stopping point, because he wants to start making the game "play and feel like a AAA game, and then from there we'll add more progression". He chose to wrap up and close: "wrap up and close it".
  - **Passed:** W1, W2, W3, W4, W5, W6, W9, W10.
  - **Carried into the AAA milestone:** W7 (60 fps on battery with Low Power Mode, to wave 6), W8 (3 warm launches in a row < 5 s) and W0 (M2's S2/S3, which ride on the same sessions).
  - **Reason:** the attribution shows the GPU is the bottleneck, and the AAA pass rebuilds the look anyway, so a GPU budget belongs at its start.
  - **Tools in place for that milestone:** the session log with spike attribution (`scripts/sessions.py --spikes`), `--launches`, quiet mode and `scripts/cargo.sh`.
