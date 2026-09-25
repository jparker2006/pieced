# Goal brief: Pieced Milestone 1 "Sparring"

Launch this with `/goal` (see the launcher line at the bottom). The **contract is `docs/SPEC.md`**. This brief defines how to execute it, what "done" means, and when to stop and ask Jake.

## Objective

Build Pieced Milestone 1 exactly as specified in `docs/SPEC.md`: a first-person arena where Jake can move, shoot a SCAR and a pump, and build walls, ramps and floors against a strafing training dummy. It has to look gorgeous and run at a steady 60 fps on his MacBook Air on battery with Low Power Mode on.

The goal is complete only when gates **G1–G9** below are all **PASS**, each with recorded evidence. The report `docs/evidence/M1-report.md` must list every gate and its evidence and name the exact commit measured, and it must be pushed to `main` on `github.com/jparker2006/pieced`. Any gate that is FAIL or UNVERIFIED means the goal is not done.

## Read first

1. `docs/SPEC.md`: the contract. Its settled decisions are not up for renegotiation without Jake:
   - first person;
   - Fortnite pieces, not voxels;
   - two guns only;
   - a 1–2 s time-to-kill;
   - local only;
   - Bevy 0.19.1 and avian3d 0.7.0.
2. `docs/research/game-feel.md`, `docs/research/bevy-stack.md` and `docs/research/bot-ai.md`: the principles and verified facts behind the spec. The bot research only constrains Milestone 1 through the `PlayerIntent` seam.

## Environment facts (verified 2026-09-24)

- **Machine:** MacBook Air 15" M4 (Mac16,13), 16 GB RAM, 2880×1864 Retina panel, 60 Hz, fanless. Low Power Mode is on (`pmset -g` shows `lowpowermode 1`).
- **Repo:** `outputs/pieced` in Jake's Codex workspace, next to Chunky's `outputs/voxel-game`. Public remote `jparker2006/pieced`, branch `main`. `gh` is authenticated as `jparker2006`.
- **Rust:** there is no Rust on PATH. **Always `source scripts/env.sh` first.** It points `CARGO_HOME`/`RUSTUP_HOME` at the workspace's isolated toolchain (Rust 1.98.1) and sets a **shared `CARGO_TARGET_DIR` outside the repo**, so every worktree reuses one build cache.
- **Disk:** only about 12 GB free (98% used). Check `df -h` before large builds. Never create a per-worktree target directory. Chunky's `target/` is 4.8 GB and could be deleted, but **only if Jake agrees**.
- **Power checks for evidence:** `pmset -g batt` (AC or battery) and `pmset -g | grep lowpowermode`.
- **Chunky** (`../voxel-game`) is read-only reference material. Useful patterns to reuse:
  - cursor lock;
  - pausing on focus loss;
  - clearing input on state changes;
  - ignoring the look delta on the frame the cursor is recaptured;
  - evidence and frame-log discipline;
  - synthesized WAVs.

  Never modify it.

## Execution protocol

**Roles**

- **Orchestrator** (the main session, Opus): owns the plan, shared types, merges into `main`, running the gates, pushes, the art review and every conversation with Jake. Keeps a running checklist in the conversation.
- **`pieced-builder` subagents** (Opus, medium effort; defined in `.claude/agents/`): implement one slice each in an isolated git worktree (`isolation: "worktree"`, or `git worktree add .claude/worktrees/<slice> -b m1/<slice>`). Builders:
  - run `source scripts/env.sh`, then `cargo test --locked` for their slice;
  - commit on their branch with clear messages;
  - **never push, never merge, and never edit files owned by another slice**;
  - request changes to shared types through the orchestrator.
- **`pieced-researcher` subagents** (Opus, medium effort): bounded doc and API lookups, such as Bevy 0.19 or avian 0.7 API questions. Use Context7 first. Research never blocks unrelated work.

**Phases**

Run independent slices in parallel. Merge sequentially, testing after each merge.

- **Phase 0, Foundation** (sequential; the orchestrator or one builder):
  - Cargo project with exact pins and a committed lockfile.
  - Profiles as the spec describes.
  - App shell: states, window and present settings, cursor lock, pause on focus loss.
  - `PlayerIntent` plus the keyboard/trackpad adapter.
  - A fixed 60 Hz step and the headless test harness.
  - Frame log and launch-time telemetry.
  - A flat placeholder floor with a first-person camera and basic look.
  - Declare the plugin and module skeleton for every slice, including the shared types: intent, health, damage event, collision layers, tuning resource, grid coordinates.
  - Build, test, commit and push.

  **Then ask Jake for the 30-second trackpad check:** hold W and swipe on the trackpad, then hold W and press to click. Do both register? Record his answer in the report.
- **Phase 1** (parallel builders):
  - **A: movement controller**, with tests.
  - **B: build grid and pieces**, with tests: targeting, ghost, turbo, rebuild lock, damage, cracks, destruction.
  - **C: combat and dummy**, with tests: weapons, hitscan, headshots, shield, reloads, switch, TTK contract, dummy pattern and respawn.
  - **D: arena and look**: environment, sky, sun, shadows, fog, palette materials, dummy rim material, quality presets, and the **gallery** scenario that writes a greyscale copy of every shot.
- **Phase 2** (parallel builders):
  - **E: viewmodel and effects**: guns, recoil and sway springs, reload motion, muzzle flash, tracers, sparks, debris, shield shimmer, elimination burst, and optional hitstop.
  - **F: audio, HUD, menus and settings**: synthesized sound bank, crosshair and bloom, bars, ammo, hotbar, hitmarkers, damage numbers, piece HP bar, combat readout, pause and settings, performance overlay, tuning panel, settings persistence.
  - **G: scenarios and telemetry**: the perf, ttk and latency scenarios, the evidence folder layout and the summary computation (with tests).
- **Phase 3, Integration and gates** (orchestrator):
  - Tune the feel numbers against the spec.
  - Optimize to the performance contract.
  - Run the art review loop: gallery → inspect every PNG and its greyscale copy against the spec's quality bar → fix → repeat. **At least two review rounds.**
  - Collect evidence and write the report.
  - Run the Jake-assisted gates.

**Merging and pushing**

- Merge each finished slice into `main` with `--no-ff` after `cargo test --locked` passes on the merged result.
- Push `main` after **every** green merge or working step; Jake likes frequent pushes.
- End every commit message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Remove merged worktrees and branches.
- Never force-push or rewrite `main`.

**Evidence**

- Scenario output goes to `evidence/<run-name>/`, which git ignores.
- The curated report and selected screenshots go in `docs/evidence/` and are committed.
- Every number in the report names its run folder, the commit, and the power and Low Power Mode state.
- Never delete failed runs to make the numbers look better; record failures and reruns honestly.

## Gates (definition of done)

| Gate | Pass condition | How it is measured |
|---|---|---|
| **G1 Launch** | Warm launch to controllable in **< 5.0 s**, in 3 consecutive launches | Launch-time telemetry from process start to the first frame in `Playing` with input live; release build |
| **G2 Performance** | **Battery power, Low Power Mode on**, Battery preset, window visible: over a 5-min `perf` scenario (after the first 10 s), mean frame interval 16.4–17.0 ms, **0 frames > 25 ms**, **≥ 99% < 18 ms** | Frame-log CSV plus the summary JSON from the release build. A development proxy on AC with Low Power Mode on is allowed while iterating. **The gate run must be on battery**, so ask Jake to unplug. |
| **G3 Latency** | Input-to-frame latency measured, method and limits documented, **median ≤ 33 ms** | `latency` scenario. The report states exactly what the number includes and excludes (display scanout is excluded) |
| **G4 Building** | In headless tests and a native run: wall, ramp and floor snap correctly for all four facings; turbo build places continuously while moving; a scripted **1x1 box completes in ≤ 1.0 s** of intents; pieces take damage, show cracks at 66% and 33%, shatter, and free their cell after the 0.15 s lock; placement never traps the player | Tests plus native scenario screenshots of a built 1x1 box, a ramp rush and a breaking piece |
| **G5 Weapons** | In tests and a native run, both guns work: fire on press, rifle bloom and recovery, fixed pump pattern with falloff, ADS toggle (two-finger click **and** V), reloads (magazine; shell-by-shell and interruptible), 0.2 s switch, headshot ×1.5; hitmarker, sound and damage number appear **on the same frame** the hit registers | Tests plus a native scenario frame log showing hit and feedback on the same frame index, plus screenshots |
| **G6 TTK** | Standing dummy at 15 m, full 200: continuous rifle fire kills in **1.0–2.0 s**; one close pump body shot **never** kills from full; a wall soaks **≥ 1.0 s** of continuous rifle fire | Headless tests plus the native `ttk` scenario |
| **G7 Tests** | `cargo test --locked` passes with **0 failures**. `cargo clippy --locked --all-targets -- -D warnings` and `cargo fmt --check` are clean. Headless tests cover every module listed in the spec's Testing Decisions | Command output recorded in the report, on the final commit |
| **G8 Art** | The gallery scenario produces the 8 views: arena overview, sunset backdrop, inside a 1x1 box, ramp-top view, rifle ADS on the dummy, pump hit effects, piece-break debris, and the HUD mid-fight. The orchestrator verifies every view and its greyscale copy against the spec's quality bar over at least 2 review rounds, and **Jake says the look is good** | Gallery PNGs in `docs/evidence/m1/` plus written review notes in the report |
| **G9 Verdict** | Jake plays at least 10 minutes on the trackpad and says he wants to keep playing, **or** names what feels off. If something feels off, fix it, re-verify any affected gates and ask again. G9 passes only on a positive verdict from Jake **in chat** | Jake's words quoted in the report |

## Stop and ask Jake when

- Free disk drops below **4 GB**, or a build needs more room than is available.
- A pinned crate won't work with Bevy 0.19.1 and the fix changes a pinned version or a spec decision.
- A gate still fails after **3 focused attempts**. Bring the evidence and options.
- A change would contradict a settled spec decision, or add something the spec excludes.
- Anything needs accounts, payments, system software installs, macOS setting changes (including Low Power Mode) or touching Chunky.
- A Jake-assisted step is due: the trackpad check after Phase 0, the battery run for G2, the art sign-off for G8, and the play session for G9.

Keep working on everything else while waiting on Jake.

## Launcher (paste after `/goal`, well under 4,000 characters)

> Read `docs/GOAL.md` and `docs/SPEC.md` in the Pieced repo and execute the brief end to end as orchestrator, using `pieced-builder` subagents in parallel worktrees, merging and pushing to `main` after every green step. Done only when `docs/evidence/M1-report.md` on pushed `main` shows gates G1–G9 all PASS with evidence (G2 measured on battery with Low Power Mode on; G8 art sign-off and G9 verdict given by Jake in chat). Stop and ask Jake for the conditions listed in the brief.
