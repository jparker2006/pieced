# Milestone 2 "Spellbound": report

**Status:** IN PROGRESS. The goal launched 2026-09-25 (`docs/M2-GOAL.md`).

Every number here names its run folder, the commit, and the power and Low Power Mode state. Failures and reruns are recorded, not deleted.

## Gates

| Gate | Status | Evidence |
|---|---|---|
| S1 Target board (every view ≥ 4 from Jake) | **PASS** (round 3, `bb9c33a`) | Jake's round 3 scores 2026-09-27: every view ≥ 4, mean 4.67 (eight 5s, four 4s). Scored shots in `docs/evidence/m2/s1-round3/`. Rounds 1 (mean 2.5) and 2 (mean 3.4) are in the log |
| S2 Performance (battery, Low Power Mode, full look) | PENDING (method changed, D53) | To be measured from a logged real play session on battery with Low Power Mode on (same thresholds), not an away-from-the-Mac scripted run. Needs the frame-timing log for normal play first |
| S3 Launch < 5 s ×3 | PASS (native, `f5c36e7`) | `evidence/m2-20260926-231552-launch{1,2,3}`: 2564, 1208, 1396 ms; every other run 1145–2224 ms. Release build, AC, Low Power Mode on. Re-run on the final commit |
| S4 Motion | PASS (`f5c36e7`) | `tests/far.rs` plus native `evidence/m2-20260926-231552-sky`: 5 frames 5 s apart, 33.6/34.4/33.4/34.9% of sky pixels changed. Re-run on the final commit |
| S5 Knight hitbox fit | PASS (on the B3 merge; re-run on the final commit) | `tests/knight.rs` (20 tests), with the cosmetic hat excluded per Amendment B. Head: helmet +4.6 cm, eyes +0.6. Body: boots +4.1, gauntlets +3.1, torso +2.2, cape +1.6; the robe is inside by 1.4 cm (limit 5). Fill: front 8.1 / 8.0 cm (limit 10), side 18.0 (limit 21) |
| S6 Feedback timing | PASS (`f5c36e7`) | `tests/spells.rs` plus native `evidence/m2-20260926-231552-fx`: 13/13 hits with impact, hitmarker and damage number on the hit frame; 18/18 bolts on their hit point within 2 frames (worst 2). Re-run on the final commit |
| S7 No regressions (G1, G3, G6, tests, clippy, fmt) | PARTIAL | `cargo test --locked` 331 passed, 0 failed; clippy `-D warnings` and `fmt --check` clean on `12550d4`. Native G1, G3 and G6 pending (go window) |
| S8 Jake's feel verdict | **PASS** (2026-09-27, `bb9c33a`, sound on) | "OMG ITS AMAZING! IT looks FANTASTIC. The sound is GREAT!" About 4½ minutes; Jake accepted this session as S8 (D52) |
| S10 Fortnite building and controls (Amendment A) | PASS (headless, on `4d3faed`; re-run on the final commit) | `tests/building.rs`: ramp rush ≥ 10 ramps at ≥ 97% speed in 8 variants, clean stop at the 12-level limit, double ramps (11 + 11), 90s tower, 1×1 box from every corner, ramp + wall. `tests/editing.rs` (16): every edit shape's collision, invalid selections, reset, HP kept, cone places, blocks and breaks. `tests/controls.rs` (5): hold-Shift ADS, W sprint, slide, inert right click. `tests/pieces.rs`: no asset allocation over 50 edits. M1 G4 building tests green |
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
- 2026-09-26: **Jake's first impressions** (after about 3 minutes, not yet the S8 session), quoted:
  > "Okay, it plays really fun. I think the rebuild mechanics need to act a little bit more like Fortnite, like when I ramp up, I feel just infinite ramp, stuff like that. I was just shooting the robot. It was really fun. It looked really good."

  A feedback grill (Q30–Q38: ramp rush, missing Fortnite building, guns, aim, movement, visual issues, bots, scope, priority) was sent to Jake. Waiting on his answers.
- 2026-09-26: **controls merged** (D40–D42), with 340 tests passing:
  - hold Shift to aim;
  - sprint whenever W is held (diagonals too; strafe and back move at run speed, as in Fortnite), and aiming stops the sprint;
  - C while sprinting slides;
  - the right click is inert (`tests/controls.rs`).

  Also fixed a load-sensitive test (`tests/pieces.rs` pop timing now uses fixed frames). The ramp rush (targeting) and editing + cone slices are in progress.
- 2026-09-26 (evening): Jake is closing his laptop. **In flight** (uncommitted, in worktrees):
  - `m2-ramp-rush`: the ramp rush, targeting, reach, 90s and the 1×1 box (5 files);
  - `m2-edit-cone`: editing and the cone (78 files).

  If their agents stop when the Mac sleeps, resume each from its worktree: commit the work in progress, `git merge main`, finish, run the gates, report. Then merge ramp-rush first, then edit-cone, rebuild the play binary, and ask Jake to test building (S8 fix loop). Jake's gates are still pending: the go window, S1 scores, the S2 battery run, S8.
- 2026-09-26: **ramp rush merged**, with 348 tests passing.
  - **Diagnosis:** looking ≥ 40° down targeted your own cell ("behind me"); looking up floated or wedged ramps; landing on a ramp cut speed from 7.5 to 3.25 m/s.
  - **Fixed:** a forward rush continues the chain at any pitch. Climbing never caps your own ramp. Walls go on your own tile's edge and push you inward. You're lifted onto a ramp built over you. Landing keeps speed.
  - **Headless results:** every variant builds the full 6-level chain at 100% speed (3.47 s sprinting). Ramp + wall holds speed. A 4-level 90s tower builds in 2.67 s. A 1×1 box builds from every corner.
  - **Next (same builder):** 3×3 reach, double ramps, and a 12-level height limit so the S10 rush reaches ≥ 10 ramps. Editing + cone is in progress.
- 2026-09-26: **editing and cone merged**, with 375 tests passing; `build-art --check` covers 24 new edit and cone models.
  - **Cone (V):** a 1.5 m plank pyramid, 170 HP, sharing a cell with a floor and a ramp, with crack stages and debris.
  - **Editing:** G opens a grid; click or drag, release confirms, R resets.
  - **Valid shapes:**
    - Wall: window, wide window, door, arch, mid and low wall, 4 triangles, 3 pillars.
    - Floor: 1–3 tiles.
    - Ramp: 8 half ramps.
    - Cone: 1–3 raised corners.
  - Collision follows the edit, the HP fraction is kept, and 50 edits create no assets.
  - `tests/editing.rs` has 16 tests.
  - **Known:** `perf_director_generates_heavy_play_deterministically` flaked once under parallel load (it passed alone).
- 2026-09-26 22:15–22:19: **Jake's second play session**, on the release build `189a0c0` (ramp rush, controls, editing, cone), muted at his request. About 3 minutes. Warm launch **2.98 s**, under the 5 s limit (informal; S3 still needs its 3-launch run). His verdict, quoted:
  > "Okay, I just played. That was a lot of fun. Obviously, there's not an objective of the game, but just looks and feel-wise, it was great. I genuinely just love that. I wanted to feel magical and just have beautiful 3D graphics, and I think we're on the way to doing that."

  S8 status: **positive on look and feel.** Still missing before PASS: a session of at least 10 minutes, and the **sounds**, which were muted in both sessions and never heard.
- 2026-09-26: **reach, double ramps and the 12-level limit merged** (`4d3faed`), with 377 tests passing:
  - floors, ramps and cones go where the aim ray lands, anywhere in the 3×3 tiles around you (`reach_tiles`, tunable to 2);
  - double ramps (11 + 11 at 100% speed);
  - `MAX_LEVELS` 12, so a rush runs the arena: 11 ramps to 32.7 m, sprint 6.10 s;
  - fixed a bug where slipping 2 cm off a ramp's side capped your own ramp.

  **Amendment A is complete; S10 passes headless.**
- 2026-09-26: **S1 scoring, round 1** (Jake, on offscreen renders of the 12 gallery views at `f5c36e7`, next to the targets), quoted:
  > "Scores: T01: 2; T02: 2; T03: 2; T04: 2; T05: 3; T06: 2; T07: 3; T08: 3; T09: 3; T10: 2; T11: 2; T12: 4"

  **FAIL:** only T12 reaches 4. Next: a design ("beauty") pass aimed at the gaps, then re-score. Per the brief, this goes to Jake with evidence and options, since S1 has now failed after two polish rounds.
- 2026-09-26 23:15: **go window 1** (native, release `f5c36e7`, AC with the battery at 2%, Low Power Mode on, muted, window visible, builders paused; background load 6–14, high):
  - **S3 PASS:** launches 2.56 / 1.21 / 1.40 s.
  - **G3 PASS:** input-to-submit median 24.3 ms, p95 32.2 (n = 120).
  - **G6 PASS:** rifle TTK 1.167–1.500 s ×6; pump max 100, no kill; wall soak 1.167 s.
  - **S6 PASS:** 13/13 hits same frame; bolts ≤ 2 frames.
  - **S4 PASS:** the sky changes about 34% per 5 s.
  - **S2 (AC proxy, not the gate):** mean 16.72 ms, p99 18.68, max 165.6, 40 frames > 25 ms, 96.42% < 18 ms (`evidence/m2-20260926-231552-perf`).
  - **Knob breakdown** (`evidence/perf-20260926-232342-*`, 45–60 s runs), frames > 25 ms: full 22, `outline=off` 31, `far=off` 17, `halos=off` 9, `blobs=off` 29, **`msaa=1` 1** (p99 18.57).

  **MSAA 4× is the main spike source**, as in Phase 0. The shading builder is switching the Battery preset to MSAA off plus cheap edge smoothing. The native gallery was captured (`evidence/m2-20260926-231552-gallery`, pre-beauty-pass).
- 2026-09-27: **beauty B1 (shading) merged**, with 387 tests passing:
  - 3-tone toon with sky and ground fill;
  - cartoon specular from `art/surfaces.json` (brass, steel, crystal, glass);
  - baked per-vertex AO (`art/blender/lib/ao.py`, `COLOR_0` alpha);
  - 3 px ink tapering to 65% between 8 and 25 m;
  - a colour grade (`pieced::grade`);
  - the **Battery preset now uses MSAA off plus one FXAA pass** (the Phase 0 and go-window-1 spike source); Plugged-in keeps MSAA 4×.

  Before/after renders were in the B1 worktree. The environment, knight and guns/spells slices are still in progress.
- 2026-09-27: **beauty B3 (knight) merged**, with 397 tests passing:
  - a tall floppy cosmetic wizard hat with a star and a flopping tip;
  - a purple coat and robe with gold trim, and a bigger cape;
  - bigger oval eyes and a grille;
  - open gloves;
  - visual-only hit reactions (arms fling, boot kick, lean back, hop, slide, bigger for headshots and shield breaks);
  - a bouncy lean on the run and a goofy idle.

  S5 was re-verified with the hat excluded.
- 2026-09-27: **beauty B4 (guns and spells) merged**, with 397 tests passing:
  - **Guns:** an ornate gold-brass rifle (5,656 triangles) with beaded collars, rivets, scroll ridges, grained wood and a fluted muzzle. The pump is reworked to T04 (5,752): a fluted bell, a ribbed pump grip, a violet glass chamber and gem-studded spinning rings. A `Runes` glow follows `CrystalGlow`.
  - **Chamber energy** (`src/fx/chamber.rs`): lightning arcs and sparkles that dim with the magazine and freeze under `FreezableTime`.
  - **Spells 2–3× bigger:** rifle trail 36 sparkles, pump fan 56 sparks plus 20 sparkles, body 22 sparks, headshot 26, shield break 14 shards and 40 chips. The particle cap went from 400 to 600, with fixed pools and nothing allocated per event.
  - **T08:** the tall hat lands 1.2 m in front of the poof at 1.45×.

  **Performance of the extra glow is unmeasured**; the next go window checks S2.
- 2026-09-27: **beauty B2 (environment) merged** (`caee4ca`), with 405 tests passing and `build-art --check` clean:
  - a painted grass texture (512², mipmapped) with blade strokes and patches, graded at 0.5;
  - tufts, flowers, about 110 margin bushes, bushes hugging the props, and framing trees (plus a new `tree_b`);
  - a cloud sea under the rim, the far islands and the station;
  - brick shade variation and lit tops, two wood grains, bigger nails;
  - moss rocks and bark stumps.

  Merged scenery on Battery is 116.7k triangles (130k cap), in 16 culled chunks with 6 shared materials. **All four beauty slices are merged.** The round 2 scoring board (offscreen renders of `caee4ca`) was sent to Jake.
- **Handoff (2026-09-27, before compaction):**
  - `main` is `3f39f6f`: all M2 build work (look, Amendment A mechanics, Amendment B beauty) is merged; 405 tests pass and `build-art --check` is clean. The release play binary is `~/Library/Caches/pieced-target/pieced-play` (built from `3f39f6f`).
  - The round 2 scoring page is in the session scratchpad (`board-r2/pieced-scoring-round2.html`). To regenerate: `PIECED_GALLERY_OUT=<dir> cargo test --locked --test gallery_offscreen -- --ignored --nocapture`, then build a side-by-side page.
  - **Jake's steps left:**
    - S1 round 2 scores (≥ 4 on every view);
    - the S2 battery run (charged, Low Power Mode on; `PIECED_BIN=<play binary> scripts/m2-gates.sh --only-perf` under `timed-run`, builds paused);
    - S8, a 10-minute session with sound on.
  - **After that:** re-run S3–S10 on the final commit, then final report.
  - **Worktrees:** only `m2-baseline` remains.
- 2026-09-27: **S1 scoring, round 2** (Jake, on offscreen renders of the 12 gallery views at `caee4ca`, next to the targets), quoted:
  > "Round 2 scores: T01: 3; T02: 3; T03: 3; T04: 3; T05: 3; T06: 4; T07: 4; T08: 4; T09: 4; T10: 3; T11: 3; T12: 4"

  **FAIL, improved:** the mean went from 2.5 to 3.4; T06–T09 and T12 pass. The seven views at 3 share four gaps, compared view by view with their targets:
  - **Viewmodel framing:** our gun fills about a third of the frame from centre-right and is blocky (flat facets, a big square rear block, a pump bell that reads as a flat disc). The targets show a slimmer, rounder gun tucked into the lower-right corner, angled toward the crosshair, leaving the vista clear. This affects all seven.
  - **Station and sky:** our station reads as a toy (candy glass panels fanning outward around a small keep). The targets show a massive dark gothic cathedral with many spires, tall glass set into its walls, a ring, and a jagged rock underside pouring several waterfalls. The targets' sky is packed with floating islands at every scale, most with waterfalls; ours has 18 sparse ones. The planet is candy-striped, where the targets have a soft banded lavender giant. This affects T01–T05, T10 and T11, and T10 most of all (the station is small in frame).
  - **Foreground framing:** every target frames the shot with a big tree, bushes, rocks and stumps near the camera. Ours open onto bare grass. Our tree crowns are faceted low-poly blobs, where the targets have round lobed crowns. The targets' build grid is a faint glowing green, and their island edge has grassy overhangs over brown rock faces (ours is a flat tan cut). This affects T01, T03, T05, T10 and T11.
  - **Knight framing:** T05's knight is farther and smaller than in the target, and T03's bolt impact reads beside him rather than on him.

  **Next:** round 3, the third focused attempt the brief allows before S1 goes back to Jake with options. Three parallel slices: viewmodel framing and silhouette, station and sky, ground and framing. The five passing views must not regress.
- 2026-09-27: **round 3, sky slice merged** (`f347c58`), with 407 tests passing and `build-art --check` clean:
  - **Station:** rebuilt as a massive dark slate gothic cathedral: about 20 towers with spires and pinnacles, buttresses, flush pointed stained glass (a 44×118 m great window), a lit arcade, a ring walkway, and a jagged rock with six crags and seven waterfalls. It fills the upper-right quarter of T01 and T02.
  - **Island field:** 60 islands, up from 18 (a near ring of 14 plus 46 at 300–900 m), with 47 pouring waterfalls. They share five meshes, including a new castle island and a pebble.
  - **Planet:** soft lavender with subtle bands and a thin ring.
  - **T10:** the camera is 250 m from the station, looking up 24°.
  - **Budget:** far layer 25.4k → 53.4k triangles, opaque batches 15 → 14, halos 25 → 47. A new `tests/far.rs` budget test enforces ≤ 75k triangles, ≤ 16 batches and ≤ 50 halos.
  - **Open item:** the scenery cloud bank (ground slice) now hides the station's rock and the bottom of T10; the ground builder is re-anchoring it under the station.
- 2026-09-27: **round 3, viewmodel slice merged** (`e14a6bc`), with 407 tests passing and `build-art --check` clean:
  - **Framing:** the rifle sits lower, nearer and further right, with the muzzle just under the crosshair; the T02 inspect pose is a big diagonal from the lower right. The viewmodel FOV is unchanged.
  - **Feel kept:** recoil, sway, bob, reload, swap and fire timings are unchanged; motion offsets and the muzzle flash are scaled by the new hold depth so they move as far on screen as before. ADS sights still line up, bolts still leave the muzzle, and S6 timing tests pass.
  - **Rifle:** round, smooth-shaded barrel, collars and chamber; a warm red-brown stock and fore-grip; a small rear sight in place of the square block; clearer glass over a deep inner chamber so the crystal reads.
  - **Pump:** a long trumpet bell that reads as a horn from the side, a ribbed wooden grip, and the violet chamber in view.
  - **Gloves:** bigger and puffier, with dark sleeves.
  - **Budget:** rifle 6,776, pump 6,984, gloves 1,904 triangles. The gun budget is raised from 6k to 8k in the spec, `registry.py` and `tests/models.rs`.
- 2026-09-27: **round 3, ground slice merged** (`402e7e8`), with 410 tests passing; clippy (`-D warnings`), `fmt --check` and `build-art --check` are clean:
  - **Trees:** round, lobed crowns of separate smooth puffs with sunlit tops; curvy trunks with rooted feet and forking limbs.
  - **Rocks and stumps:** smooth rounded boulders (warm tops, cool grey flanks); stumps with grooved bark, four roots and a ring top. Footprints and heights are unchanged, so D29 colliders are unchanged.
  - **Framing:** 8 hand-placed margin trees, rocks and stumps; low non-colliding bushes, flowers and tufts just inside the edge, kept 3 m from spawns; a small floating knoll with trees and rocks under T10.
  - **Grid:** a pale core with a soft green glow. **Island edge:** a grassy overhang over brown stratified rock. **Barrier:** fainter face-on so the cliff shows through. **Cloud bank:** anchored 110 m under the station's platform.
  - **Cameras:** T01, T03, T04, T05 and T11 moved to framed spots (T05's knight now 3.6 m out; T03 captures 3 frames after the shot so the burst lands on him). T02, T06–T10 and T12 keep their cameras.
  - **Budget:** merged scenery 116.7k → 122.2k triangles (cap 130k), still 6 shared materials. New scenery tests: edge cover stays low and off the spawns, every view is framed by a tree, and the knoll sits under T10.
  - **Known gap:** T11's cliff face is one wide slab with horizontal strata, where the target paints vertical rock columns.

  **All three round 3 slices are merged.** Next: the round 3 board for Jake.
- 2026-09-27: **S1 scoring, round 3** (Jake, on offscreen renders of the 12 gallery views at `bb9c33a`, next to the targets), quoted:
  > "Round 3 scores: T01: 5; T02: 5; T03: 5; T04: 5; T05: 4; T06: 5; T07: 5; T08: 5; T09: 5; T10: 4; T11: 4; T12: 4"

  **S1 PASS.** Every view is at least 4, with a mean of 4.67 (2.5 → 3.4 → 4.67 over three rounds). All seven round 2 3s rose to 4 or 5, and the five round 2 passes held or rose. The shots Jake scored (960 px JPEGs) and the capture metadata are committed in `docs/evidence/m2/s1-round3/`; the targets stay uncommitted.

  **Performance still to be measured:** round 3 raised the far layer to 53.4k triangles, merged scenery to 122.2k, and each gun to about 7k. S2 on battery must be measured on this look.

  **Still open:** S2 (the battery run with Low Power Mode on), S8 (10 minutes with sound on), and re-running S3–S10 on the final commit. `pieced-play` is built from `bb9c33a`.
- 2026-09-27 14:26–14:31: **Jake's play session on `bb9c33a`** (release `pieced-play`, sound on, Battery preset; AC power at 88% charging, Low Power Mode on). About 4½ minutes. Verdict, quoted:
  > "OMG ITS AMAZING! IT looks FANTASTIC. The sound is GREAT! When there is an actual game objective this is gonna be so much fun!"

  **S8 PASS** (Jake accepted this session in place of 10 minutes, D52). The printed launch time was **8,721 ms**, over the 5 s S3 limit. It was the first launch after a release rebuild; earlier launches took 1.1–2.6 s. It needs checking before the final report.
- 2026-09-27: **Gate method change (D53, Jake):** "Can we skip the shit where I have to be away from my mac".
  - **S2:** measured from a logged real play session on battery with Low Power Mode on, with the same thresholds. The game needs a frame-timing log in normal play for this.
  - **S3–S7:** no native re-runs. They rely on the go-window-1 native results, headless tests on the final commit, and each launch's printed time.

  These leftovers are step 1 of Milestone 3 (D70). The M3 plan is Round 8 of `docs/design/DESIGN-GRILL.md`.
