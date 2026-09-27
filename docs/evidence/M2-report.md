# Milestone 2 "Spellbound": report

**Status:** IN PROGRESS. The goal launched 2026-09-25 (`docs/M2-GOAL.md`).

Every number here names its run folder, the commit, and the power and Low Power Mode state. Failures and reruns are recorded, not deleted.

## Gates

| Gate | Status | Evidence |
|---|---|---|
| S1 Target board (every view ≥ 4 from Jake) | PENDING | — |
| S2 Performance (battery, Low Power Mode, full look) | PENDING | — |
| S3 Launch < 5 s ×3 | PENDING | — |
| S4 Motion | PARTIAL | `tests/far.rs` passes (galaxy angle, ships ≥ 5 m on their loops, islands bob, glass pulse) on `12550d4`. Native `sky_check` frames pending (go window) |
| S5 Knight hitbox fit | PASS (on `12550d4`; re-run on the final commit) | `tests/knight.rs`. Worst part outside its hitbox: Helmet +4.6 cm (limit 5). Front fill ≤ 9.2 cm (limit 10) |
| S6 Feedback timing | PARTIAL | `tests/spells.rs`: impact, hitmarker and damage number on the hit tick; bolt ≤ 2 frames, with the real `HudPlugin`. Native `fx_check` `scenario.s6` pending (go window) |
| S7 No regressions (G1, G3, G6, tests, clippy, fmt) | PARTIAL | `cargo test --locked` 331 passed, 0 failed; clippy `-D warnings` and `fmt --check` clean on `12550d4`. Native G1, G3 and G6 pending (go window) |
| S8 Jake's feel verdict | PENDING | — |
| S9 Original and reproducible | PASS (on `12550d4`; re-run on the final commit) | `scripts/build-art.sh --check`: every model and UI image rebuilt headless, byte-identical. `tests/assets.rs` audit passes. `tests/models.rs` loads every glb headless. The only third-party files are Lilita One and its OFL license |

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
- 2026-09-26: **knight merged** (`2bdfbd6`), with 238 tests passing:
  - the model at 7,824 triangles, with four eye states and joint pivots;
  - in game: toon, outline, a warm rim and a blob shadow that sits exactly under the boots;
  - blinking, wide and X eyes; idle, run, air, squash, wobble, hat bounce, the respawn pop;
  - **S5 evidence:** `tests/knight.rs`. Worst vertex outside its hitbox: Helmet +3.8 cm, Hat +4.2, Gauntlets +3.9, Boots +4.1, Torso +1.0 (the limit is 5). From the front the capsule sticks out at most 9.2 cm past the model, the head sphere 1.4 cm.
  - **Rules the builder set, accepted by the orchestrator:** below 0.12 m the capsule counts as a cylinder, so the boots stand on the ground as in M1. The 10 cm fill rule is applied from the front, because the dummy always faces the player; the side fill is held to ≤ 21 cm.
  - **Art-review notes:** the arms read thin, and the helmet could be rounder and chunkier, closer to T05.
- 2026-09-26: **guns merged** (`f67211f`), with 252 tests passing:
  - the rifle (5,036 triangles) and pump (5,604) in brass and dark wood, with a blue crystal in a glass chamber and a violet crystal in gold rings; the gloves (1,960);
  - crystal glow = 0.25 + 0.75 × magazine fraction (the `CrystalGlow` resource);
  - the crystal-swap reload, pump shards and ring whirr, the squash kick, ADS on the sights.
- 2026-09-26: **sky merged** (`40740a0`), with 271 tests passing:
  - a seeded 6 × 1024² galaxy skybox, turning once per 10 minutes and generated on a background thread (97–400 ms in debug);
  - the station (12,689 triangles) at 640 m and azimuth 30°, fully in view from spawn;
  - a ringed planet;
  - 18 bobbing islands, 11 with solid waterfalls;
  - 5 ships crossing the view in about 16–18 s;
  - glass pulsing on a 5 s period.

  **S4 evidence:** `tests/far.rs`.
- 2026-09-26: the disk was pruned again (`scripts/prune-target.sh`), leaving 23 GB free. The **spells** builder was dispatched. The island is re-merging `main`, and the gallery is in progress.
- 2026-09-26: **island merged** (`f6d5527`), with 292 tests passing:
  - brick wall and plank floor and ramp, with 66% and 33% crack stages, debris and a dust poof;
  - pieces share one mesh per kind and stage and one toon material. **Placing, cracking and breaking 50 pieces leaves the mesh, material and image asset counts unchanged** (`tests/pieces.rs`);
  - the D29 props: 8 rocks and stumps with collision (`tests/props.rs`, 8 tests, including a 5-minute no-stall run and a rock blocking a shot);
  - the grass top with a faint world-space build grid (fading between 12 and 40 m), a margin of 46 trees, cliffs, and the rune barrier.

  This is the first time the whole look is together offscreen (spawn view against T01). The **HUD and menu** builder was dispatched.
- 2026-09-26: **gallery merged** (`1c3e59e`), with 303 tests passing:
  - one table of 12 views drives both the native `gallery` scenario and `tests/gallery_offscreen.rs`;
  - `shared::GalleryFreeze` and `FreezableTime` freeze the knight, the dummy and effects for exact captures;
  - the `sky_check` scenario (frames every 5 s, with a frame-difference measure, S4);
  - `scripts/board.py`: the S1 scoring page, with side-by-side views, a greyscale toggle, 1–5 scores, notes and a copy button.

  The orchestrator added the far view to the offscreen gallery app (`be0adff`).
- 2026-09-26: a network outage stopped the spells and HUD builders mid-work. Both were resumed from their worktrees.
- **Art-review round 1 notes** (offscreen board on `be0adff`):
  - T10 matches its target well.
  - T01 composition is close.
  - The knight reads **smaller than in the paintings** in T03–T08, because the targets paint him 2–3× closer than the planned distances. Move him closer to match the painted framing.
  - The HUD and spells are still M1 style (their slices are in flight).
  - Offscreen damage numbers render top-left (no window), which is an artifact of the offscreen setup.
- 2026-09-26: **HUD and menu merged**, with 311 tests passing:
  - **Font:** Lilita One. Luckiest Guy was removed, so one font ships (S9).
  - **Art:** hotbar icons and the PIECED logo rasterized deterministically from the real models (`assets/ui`, covered by `build-art --check`).
  - **HUD:** cartoon frames with crystal and heart icons; the crystal ammo readout pulses with `CrystalGlow`.
  - **Damage numbers:** white for health, cyan for shield, gold for headshots, with an ink outline and a squash pop. Pooled at 24, they show on the hit frame and run on `FreezableTime`.
  - **Pause menu:** logo buttons, with the HUD hidden, as in T12.
  - **Accepted extras:** hotbar slots 66×60 without text labels, and bigger numbers.
- 2026-09-26: **spells merged** (`afddf0f`), with 326 tests passing:
  - the rifle starburst bolt landing ≤ 2 frames after the hit frame;
  - the pump fan with 10 sparks on the real pellet paths;
  - impacts by type: body, head, shield hex, shield break with stars, brick chips and wood splinters;
  - the poof and the `knight_hat` prop;
  - fixed pools, warm-up and `FreezableTime`;
  - `fx_check` writes `scenario.s6`.

  `scripts/evidence.py` now has S6 and S4 rows (`a962936`). **All Phase 2 and 3 slices are merged.**
- **Art-review round 1** (offscreen board, `afddf0f`):
  - **Close:** T04 and T10 match their targets closely; T01, T09, T11 and T12 are close.
  - **Gaps:**
    - the knight is framed too far away in T03–T08;
    - T02 lacks the tilted close-up;
    - the T03 bolt is small;
    - the knight's arms are thin;
    - the T08 hat is small.

  Polish round 1 was dispatched.
- 2026-09-26: **polish round 1 merged**, with 331 tests passing:
  - **Knight framing:** gallery framing re-matched to the paintings (T03 8 m, T04 4 m, T05 5 m, T06 6 m, T07 4.5 m, T08 closer, with a crouched 1.05 m eye).
  - **T02:** a gallery-only viewmodel inspect pose.
  - **Bolt:** twice the size, with more sparkles.
  - **Knight model:** chunkier arms and fists, and a rounder helmet (worst overshoot +4.6 cm, under the 5 cm limit).
  - **Hat prop:** 2.1× the size.
  - **Pause:** the gun hides while paused.

  Jake was shown the full offscreen board (target next to game for all 12). Open: the planet sits left of the station, where the targets paint it to the right.
- 2026-09-26: **polish round 2 merged** (`713feb0`), with 333 tests passing:
  - **T06:** a bold opaque gold headshot starburst, and the hat pops 0.44 m up and falls back onto the head (the view is mirrored so the damage number doesn't cover the hat).
  - **T11:** the east edge (z 0–8) ends 0.9 m past the barrier, so the cliff drop shows. This is visual only; colliders are unchanged.
  - **T03:** the knight faces three-quarters toward the camera.

  **Two art-review rounds are complete** (the brief's minimum before Jake scores). The release play build is ready at `713feb0`.
- **Known, left as is:**
  - the planet sits left of the station (the targets paint it right);
  - T01's foreground has no prop, because prop positions are fixed gameplay;
  - the T12 background isn't blurred (a dark overlay only, by the spec's cost rule).
- 2026-09-26 17:13: **Jake's first play session**, on the release build `713feb0`, muted at his request: about 2–3 minutes, and the game quit cleanly (exit 0, no errors). This isn't the S8 session, which needs at least 10 minutes and a verdict; his impressions are pending.
  - Launch took **10.7 s**, but it was the first launch of a new build with a cold Metal shader cache. S3 measures warm launches (in the go window). If warm launches aren't well under 5 s, the warm-up and pipeline count are the first suspects.
