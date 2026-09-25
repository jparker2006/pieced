# Milestone 1 "Sparring": gate report

**Status: PAUSED by Jake on 2026-09-25 (02:50), before the design pass.** G2/G3/G5 screenshots/G8/G9 are unfinished; all timing runs are stopped while he uses the Mac. The build is complete: every slice is merged into `main`. The gates that need a quiet machine, or need Jake, have not run yet. A gate is PASS only when it has recorded evidence. PENDING means the gate hasn't been run yet. FAIL means a recorded run missed the gate.

| Gate | Status | Evidence |
|---|---|---|
| G1 Launch < 5 s (3 warm launches) | **PASS** | Three consecutive full-screen launches (release `b916e21`, battery, Low Power Mode on): **2.06, 0.71, 0.68 s** from process start to controllable. Five more launches in the same batch took 0.83–1.65 s. Table: `docs/evidence/m1/g1-table.md`. Summaries: `docs/evidence/m1/gate-launch{1,2,3}.summary.json`. |
| G2 Performance on battery + Low Power Mode | PENDING | — |
| G3 Latency (median ≤ 33 ms) | PENDING | — |
| G4 Building | PENDING (tests pass; native screenshots pending) | `tests/building.rs`, `tests/integration.rs` |
| G5 Weapons | PENDING (behavior verified; screenshot set pending) | `tests/combat.rs` covers fire on press, bloom and recovery, the fixed pump pattern and falloff, reloads, the 0.2 s switch and headshot ×1.5. `src/input.rs` tests cover ADS on two-finger click and on V. Native `hud_check` (`gate-hud.summary.json`): **7/7 hits showed marker, number and sound on the same frame.** Still missing: a complete visible screenshot set. |
| G6 TTK contract | **PASS** | Native `ttk` run `ttk-main-1` on `f18e8f8` (release, AC, Low Power Mode on). Rifle at 15 m: 6 kills in **1.17, 1.17, 1.50, 1.33, 1.17, 1.17 s**, each 8 body hits with no headshots. Pump point-blank at 1.5, 2.5 and 3.5 m: **100 damage, dummy survives** every time. Wall under rifle fire: **1.17 s** before it breaks. Summary: `docs/evidence/m1/ttk-main-1.summary.json`. Headless tests also cover it: `tests/combat.rs` (TTK over 12 seeds 1.0–1.5 s) and `tests/integration.rs` (real wall 1.0–2.5 s, bullets stopped by walls). Times are simulation ticks, so the covered window during the run doesn't affect them. |
| G7 Tests, clippy, fmt | PENDING (green on `8faf409`; needs a final-commit run) | see below |
| G8 Art (2+ review rounds + Jake's sign-off) | PENDING | — |
| G9 Jake's verdict after 10+ minutes | PENDING | — |

## Performance status (G2/G3), honest

- **CPU work per frame** in runs where the window was covered, so the GPU wasn't presenting: about 7–8 ms mean.
- **Visible runs** on battery with Low Power Mode on, before `0420120`: p50 16.7 ms (vsync-locked), but p95 around 34 ms in the effects-heavy HUD scenario. Only 76% of frames came in under 18 ms. **G2 would fail today.**
- **Changes in `0420120`, not yet measured on screen:**
  - no MSAA on the native-resolution UI camera (up to 7.3 MP on Jake's "More Space" display scaling);
  - the 3D render capped at 1.4 MP.
- **Tuning runs on 2026-09-25** (`tune-015608-*`: viewmodel off, shadows off, MSAA 2×, scale 0.8) are **invalid.** The window was covered for most of each run, the Mac was in use, and background processes put the load average at 14–20. The biggest was a Python service using 1.2 cores; iCloud, FSEvents and a VM added about 1 more.
- **Next valid attempt needs:**
  - battery at 50% or more;
  - heavy background services paused;
  - 25 minutes hands-off.

## Build log

- **Phase 0 (foundation), `b079050`:** the app shell, `PlayerIntent`, the headless `Sim` stepping one fixed 60 Hz tick per update, telemetry, the scenario runner and the input probe.
- **Phase 1:**
  - movement: kinematic controller, slide, crouch, ramps;
  - building: grid, targeting, turbo build, cracks, pieces;
  - combat and dummy: hitscan with analytic hitboxes, bloom, a fixed pump pattern, reloads, ADS;
  - arena look: terrain, cliffs, backdrop, sky, sun, the dummy's figure, presets, the gallery scenario.
- **Phase 2:**
  - viewmodel and effects: guns, recoil, reloads, tracers, debris, shield effects, shake and hitstop;
  - HUD, audio and menus: 20 synthesized sounds, HUD, pause menu, settings, F4 tuning panel;
  - scenarios: `perf`, `ttk`, `latency`, the frame cap and `scripts/evidence.py`.
- **Integration fixes found by cross-slice tests:**
  - During a ramp rush, an upward look targeted the slot above the player's own ramp, capping the climb.
  - Crouching sank the body hitbox below the feet; it now shortens from the top.
  - Two test assumptions broke once movement and the dummy existed.

## Known environment caveats

- The workspace shares one Cargo build cache across worktrees. `scripts/env.sh` touches the current checkout's sources before every cargo command, so builds never reuse another worktree's code.
- Timing measured while other builds compiled is not gate evidence. During Phase 1–2 the load average was 20–60 and these runs showed frame p95 of 30–90 ms. All gate timing runs happen on an idle machine with the window visible.
- Background processes that aren't part of Pieced (a VM, iCloud, a Python service) use about 1.5 cores. Timing runs report load averages as recorded.

## Pending with Jake

1. The 30-second trackpad check (Phase 0), using `--input-probe`.
2. The G2 run on battery with Low Power Mode on. It needs a charged battery; runs below about 10% charge are throttled by macOS and invalid.
3. G8: art sign-off on the gallery.
4. G9: 10 minutes of play and a verdict.
