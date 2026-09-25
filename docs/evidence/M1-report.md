# Milestone 1 "Sparring": gate report

**Status: IN PROGRESS.** The build is complete: every slice is merged into `main`. The gates that need a quiet machine, or need Jake, have not run yet. A gate is PASS only when it has recorded evidence. PENDING means the gate hasn't been run yet. FAIL means a recorded run missed the gate.

| Gate | Status | Evidence |
|---|---|---|
| G1 Launch < 5 s (3 warm launches) | PENDING | — |
| G2 Performance on battery + Low Power Mode | PENDING | — |
| G3 Latency (median ≤ 33 ms) | PENDING | — |
| G4 Building | PENDING (tests pass; native screenshots pending) | `tests/building.rs`, `tests/integration.rs` |
| G5 Weapons | PENDING (tests pass; native same-frame check pending) | `tests/combat.rs`, `tests/hud.rs` |
| G6 TTK contract | PENDING (headless tests pass; native `ttk` run pending) | `tests/combat.rs`, `tests/integration.rs` |
| G7 Tests, clippy, fmt | PENDING (green on `8faf409`; needs a final-commit run) | see below |
| G8 Art (2+ review rounds + Jake's sign-off) | PENDING | — |
| G9 Jake's verdict after 10+ minutes | PENDING | — |

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
