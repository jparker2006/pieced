# M4: the next builder dispatches (ready to send)

Written 2026-09-29 while the builds were paused for disk. Each brief below goes to a `pieced-builder` (Opus, high effort, `isolation: "worktree"`), together with the standard rules block at the end.

## Order once disk is free (≥ 10 GB)

1. **Finish chunk 5** (`m4-presentation`, `3f92084`). Resume its builder, or dispatch a new one into a worktree on that branch:
   - re-run the full suite, and confirm the fairness suite and the gameplay pin;
   - re-run the two fixed tests (the motion unit test, and `ASSETS.md` for the blur shader);
   - run clippy;
   - capture V7 and V8 into `docs/evidence/m4/board/round5-presentation/`;
   - fix the debris chunks that fly across the line of sight when there's no piece frame.

   Then merge.
2. **Finish chunk 6A** (`m4-polish-a`, `78eff5b`):
   - `git merge main`;
   - the full suite, clippy, fmt and `build-art --check`;
   - the offscreen render test (the first compile of the sway shaders);
   - `wave_cost_offscreen` 3× with load averages.

   Then merge.
3. **Play build** from `main`: chunks 5 and 6A. Jake plays **Start at wave 6 on battery with Low Power Mode on** for A7.
4. **Chunk 6B** and **art round 4** in parallel (their files don't overlap), then the **perf pass 3** (below).
5. **The board page** for Jake's A6 scores, then **play-test 5** (A9), then A10 and A0.

## Chunk 6B: gun feel, knight movement feel, UI sound and motion (D123, D124)

Branch `m4-polish-b`. Jake named the least-premium moments: "the gun feel (not looks just feel it looks great)" and "the knights movement". Presentation only (D117). Damage, fire rate, spread, bloom, ADS and reload times, and every grunt number stay locked.

- **Gun feel:**
  - a punchier viewmodel recoil spring with a snappier recovery, plus per-shot variation;
  - heavier shot sounds (more low end and transient on the rifle and pump, a short room slap);
  - a muzzle flash and a brief light pop on the viewmodel and nearby surfaces, without a real light if that's too costly (emissive and a halo);
  - crosshair bloom animation that follows the real spread;
  - hitmarker timing and scale tuned for snap;
  - a small FOV punch on the pump (render-only, ≤ 2°).

  The aim ray stays bit-identical (there's already a test). Feel numbers move ±50% at most, and each change is logged in the report's tuning table.
- **Knight movement feel** (on the chunk 4 clips): turning into strafes (the torso leads the feet), a lean into direction changes within the sampled hitbox fit, a short anticipation squash before a sprint, a settle when he stops, and footfalls locked to the clip. The fit tests and W3's fairness suite stay green.
- **UI sound and motion:** hover, click and whoosh sounds on every menu and HUD element (synthesized, Effects slider); bouncy tickers on the score, knights-left and ammo counters. It all works muted.
- **Tests:** the aim ray is identical with and without every gun-feel effect; the flash and halo pools are capped with nothing allocated per event; the fit is sampled with the new layers; the UI sounds are wired and muted at 0; the gameplay pin holds.
- **Owns:** `src/viewmodel*`, `src/fx/**` (the flash), knight movement layering in `src/knight.rs`, `src/audio/**` (the UI and gun sounds), and `src/hud/**`/`src/menu/**` (tickers and UI sounds, once chunk 5 has merged).

## Art round 4 (from the round 3 review)

Branch `m4-art4`. Targets: V1 3 → 4, V3 2.5 → 4, V4 2 → 4, V6 2.5 → 4, V7 3 → 4, V8 3.5 → 4.

- **Ships:** airship style (V7) or a thicker saucer hull with a bronze shadow band, emissive blue windows on the rim wall and thruster glows. Scale about 0.65×, or fly them higher so they aren't cropped. The V1 beams are **violet** over a gold rune circle; the V7 beams stay gold.
- **Wand flare:** a crisp orange-gold core of about 0.8 m plus two sharp swirl-ring sprites drawn in front of the knight. It must not wash him out.
- **Board staging** (`tests/m4_board.rs`, choreography only):
  - V3: shoot at about 3 m and capture about 10 frames later, with the knight airborne, the hat and armor flying, and pellet streaks with a white core;
  - V4: launch the hat up and away, start the poof after the armor clatters, spin the popped helmet so its visor faces the camera with a rim highlight, and use smoother, fewer poof puffs in lavender;
  - V6: level the camera and back it against the rear wall, cut a bigger window with sky in it, and put orbs hitting the walls in frame;
  - V2: offset the capture so the hit knight sits left of the sight post, and add a thin blue rifle tracer;
  - V8: a brick-capped top on the wall, correct UVs, a 3/4 hero turn, and dark foreground rocks.
- **Brick edges:** rounder, with tone variation (`pieces.py`).
- **Review:** capture `round4/` and run `pieced-art-reviewer`. Iterate until every view is predicted ≥ 4, or report what still blocks it.
- **Owns:** `dropship.py`, the ship visuals, `wand.py`/`src/fx/orbs.rs` (the flare), `tests/m4_board.rs`, `pieces.py`, the menu hero wall. Doesn't touch the knight model or clips, audio, the viewmodel, or the HUD.

## Perf pass 3 (A7's last mile, and A8)

Branch `m4-perf3`. From `20260929-215209` (battery and LPM, presented 16.73 ms, 99.59% < 18, **147 frames > 25 ms**, latency to present 41 ms press and 35 ms look):

- **The CPU hitches:** 62% of spikes are `pre` (window and input events, state changes: 7.9 ms against 0.5), 20% `idle`, 15% `acquire`. Find what spikes in `First`/`PreUpdate` (event bursts? state transitions? asset events?) and flatten it.
- **Latency (D122):** log input-to-*submit* for G3 comparability, and read input as late as possible in the frame.
- **Entity growth** (7,666 → 8,448 over 10 min): confirm it's only built pieces, and fix any leak.
- **The UI pass** (1.58 ms against 0.5): cheaper HUD clears and stores at 7.6 MP.
- **A8:** check what still costs the warm launch (3.3–4.8 s), and aim for about 3 s.

## Standard rules block (paste into every dispatch)

- Run cargo **only through `scripts/cargo.sh`**. Never bare cargo; never override `CARGO_TARGET_DIR`.
- **Stay merged with `main`** (`git merge main` whenever it moves).
- **Disk:** check `df -h ~` before big builds; stop and report under 4 GB.
- **No polling loops** (`pgrep`, `ps | grep`, `until …; sleep`).
- **Quiet mode:** never touch `scripts/quiet.sh`. Before heavy work that doesn't go through `cargo.sh` or `build-art.sh`, check `$HOME/Library/Caches/pieced-target/.quiet`.
- Never open windows: no native game, no `cargo run`, never run `pieced-play` or the game binary with any flag, no scenarios, no Blender GUI or MCP.
- **No gameplay numbers change** (D117; `tests/gameplay_pin.rs`).
- Before each commit: the full test suite, clippy `-D warnings`, `fmt --check`, and `build-art --check` if art changed. Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Never push or merge.
- Report in under 300 words: what changed from a player's view, tests and counts, files and shared types, GPU and CPU cost, gaps, and the branch and final commit.
