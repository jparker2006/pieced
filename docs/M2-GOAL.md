# Goal brief: Pieced Milestone 2 "Spellbound"

**Status:** READY. Jake approved the target board and Q27–Q29 on 2026-09-25. Launch with the launcher line at the bottom.

The **contract is `docs/M2-SPEC.md`**. This brief says how to execute it, what "done" means, and when to stop and ask Jake.

## Objective

Restyle Pieced as specified in `docs/M2-SPEC.md`:
- a flat TV-cartoon close-up with ink outlines;
- the goofy knight-wizard;
- brass-and-crystal guns in white gloves that fire spells;
- brick and plank building;
- a grassy floating island under a living galaxy sky with a stained-glass station;
- a cartoon HUD and a new synthesized sound bank.

**Gameplay does not change**, except for the solid rocks and stumps Jake approved (D29).

The goal is complete only when gates **S1–S9** are all **PASS**, each with recorded evidence. The report `docs/evidence/M2-report.md` lists every gate, its evidence and the exact commit measured, and it must be pushed to `main` on `github.com/jparker2006/pieced`. Any gate that is FAIL or UNVERIFIED means the goal is not done.

## Read first

1. `docs/M2-SPEC.md`: the contract. `docs/SPEC.md`: Milestone 1's gameplay contract, still in force.
2. `docs/design/DESIGN-GRILL.md`: why each look decision was made (D1–D28).
3. `docs/research/look-stack.md`: verified Bevy 0.19, Blender 5.2 and font facts.
4. `docs/design/concepts/`: `R4-M1-brick-walls-wood-floors.png` is the base design. `T01`–`T12` are the gallery targets. They're local only; never commit them.

## Environment facts

- **Machine:** MacBook Air 15" M4, 16 GB RAM, 60 Hz, fanless, usually on battery with Low Power Mode on. It's **Jake's only machine, and he uses it while the goal runs.**
- **Repo:** `outputs/pieced`, public `jparker2006/pieced`, branch `main`. `gh` is authenticated.
- **Rust:** always `source scripts/env.sh` first. It sets the workspace toolchain and the shared build cache in `~/Library/Caches/pieced-target` (`~/Documents` is iCloud-synced and over quota).
- **Blender 5.2.2 LTS:** `blender` CLI, run headless only (`-b`), unless Jake has said "go".
- **Blender MCP:** installed (server `blender` in Claude Code's user config, safe mode on, telemetry off). It needs the Blender GUI open with "Start MCP Server" clicked, so use it only in a "go" window or when Jake wants to watch. Whatever it builds must be ported into `art/blender/` scripts.
- **Codex images:** `codex exec --skip-git-repo-check --ephemeral -m gpt-5.5 -s workspace-write -C docs/design/concepts [-i <ref.png> --] "<prompt>" < /dev/null`. Run one at a time.
- **Disk:** about 21 GB free. Stop and ask below 4 GB.

## Execution protocol

**Roles**

- **Orchestrator** (main session): owns the plan, shared types (the toon material, palette, outline plumbing, named-part lookup), merges, gates, pushes, art review and every conversation with Jake.
- **`pieced-builder` subagents** (Opus, medium effort): one slice each in its own worktree of this repo, following `.claude/agents/pieced-builder.md`.
  - Builders never push or merge.
  - Builders never edit another slice's files.
  - Builders never open windows (no native scenarios, no Blender GUI).
  - Blender work runs headless and is checked through preview renders.
- **`pieced-researcher` subagents:** bounded API lookups (Context7 first).

**Phases**

- **Phase 0, baseline** (orchestrator): Milestone 1's G2 on the current art.
  1. Run the perf scenario with the knobs breakdown (this needs a "go" window). Fix what doesn't depend on the look: the viewmodel MSAA writeback, render scale, present mode.
  2. Record the baseline in the report. It doesn't need to pass; it tells us the budget.
  3. Delete the unused shadow and PBR paths only once the toon material replaces them.
- **Phase 1, foundations** (orchestrator plus one builder):
  - the toon material and its global light resource;
  - the palette texture;
  - outlines: `bevy_mod_outline =0.13.0` vs a hand-rolled inverted hull, measured, keep the cheaper, with position-averaged normals and distance fade;
  - the far material and halo billboards;
  - tonemapping off;
  - the Blender pipeline: `art/blender/build.py`, helpers, `scripts/build-art.sh`, glb and sidecar export, preview renders;
  - `assets/ASSETS.md` and the audit test;
  - named-part lookup (0.19: `WorldAssetRoot`, `WorldInstanceReady`; fix glTF +Z vs Bevy −Z forward);
  - pipeline warm-up behind the loading screen (macOS compiles pipelines on first draw);
  - new perf knobs.

  Convert the existing arena and pieces to the toon material as the first proof. Build, test, push.
- **Phase 2** (parallel builders):
  - **A:** guns and gloves: models, crystal ammo glow, reload and rack animation, squash kick.
  - **B:** the knight: model, sidecar bounds, hitbox-fit test, eyes, procedural animation, respawn pop.
  - **C:** building and island: brick and plank pieces, crack stages, debris, ghost colors, grid lines, the solid rocks and stumps in the arena (D29, with their tests), the island margin with trees, cliffs, barrier.
  - **D:** sky and far view: galaxy cubemap, station, ships, far islands and waterfalls, planet, motion systems and their tests.
- **Phase 3** (parallel builders):
  - **E:** spells and effects: bolts, pump fan, impacts by type, shield shimmer and break, elimination poof and hat prop, cartoon damage numbers, spell-timing tests.
  - **F:** HUD, menu and audio: cartoon frames, icons, the open-license font (Luckiest Guy or Lilita One), logo, pause menu, the new sound bank.
  - **G:** scenarios: the 12-view gallery with pose freezing, `sky_check`, the board page, updated perf, fx_check and ttk runs.
- **Phase 4, integration and gates** (orchestrator):
  1. Art review loop: gallery → compare every view with its target and its greyscale copy → fix → repeat. **At least two rounds before Jake scores.**
  2. Performance tuning to the bar.
  3. Evidence and the report.
  4. The Jake-assisted steps.

**Merging, pushing and evidence** work as in Milestone 1:
- `--no-ff` merges after `cargo test --locked` passes on the merged result;
- push `main` after every green step;
- commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`;
- never force-push;
- evidence folders stay git-ignored, and the curated report and game screenshots go in `docs/evidence/m2/`;
- every number names its run folder, commit, and power and Low Power Mode state;
- failures are recorded honestly.

**Jake's Mac rule:**
- Full-screen scenarios, timing runs and Blender windows run only inside a window Jake has opened by saying "go". Stop the moment he asks.
- Code builds and headless tests may run while he uses the Mac, at low priority: `scripts/env.sh`'s cargo wrapper (utility QoS, nice 10, 6 jobs). **At most two builds at once** across all worktrees.
- Codex image generation is always fine.

## Gates (definition of done)

| Gate | Pass condition | How it is measured |
|---|---|---|
| **S1 Target board** | All 12 gallery views score **≥ 4/5** from Jake against their targets | Board page. Jake's scores and notes are quoted in the report, and the game screenshots are committed |
| **S2 Performance** | Battery power, Low Power Mode on, Battery preset, full look, window visible. Over the 5-min `perf` scenario (after the first 10 s): mean 16.4–17.0 ms, **0 frames > 25 ms**, **≥ 99% < 18 ms** | Frame-log CSV plus summary JSON from the release build |
| **S3 Launch** | Warm launch to controllable **< 5.0 s**, 3 times in a row, with all new assets | Launch telemetry, release build |
| **S4 Motion** | The motion tests pass (galaxy, ships, islands, glass), and the `sky_check` frames show visible change | Test output plus frames in `docs/evidence/m2/` |
| **S5 Hitbox fit** | Every knight part lies inside its hitbox within 5 cm, and the silhouette fills the hitboxes within 10 cm | Headless test on the sidecar bounds |
| **S6 Feedback timing** | The hitmarker, damage number and impact effect appear on the hit frame. The bolt reaches its hit point within 2 frames | Spell-timing tests plus the native `fx_check` frame log |
| **S7 No regressions** | Milestone 1's G1, G3 (median ≤ 33 ms) and G6 pass on the final commit. `cargo test --locked` has 0 failures, and `cargo clippy --locked --all-targets -- -D warnings` and `cargo fmt --check` are clean | Command output and scenario summaries in the report |
| **S8 Feel verdict** | Jake plays at least 10 minutes and says the guns, spells and sounds feel sick, **or** names what's off. Fix it, re-verify the affected gates and ask again | Jake's words, quoted from chat |
| **S9 Original and reproducible** | `scripts/build-art.sh` regenerates every model headless without errors, and the game loads them. The asset audit test passes. The only third-party file is the open-license font and its license | Script output plus the test in the report |

## Stop and ask Jake when

- A Jake-assisted step is due:
  - a "go" window for the Phase 0 baseline and later timing and gallery runs;
  - the S2 battery run;
  - S1 scoring;
  - the S8 play session.
- A change would alter gameplay: numbers, hitboxes, collision, controls.
- A gate still fails after **3 focused attempts**. Bring the evidence and options, e.g. which look feature to cut or cheapen to hold the performance bar.
- Any new third-party asset, crate or service is needed beyond `look-stack.md`, or any account, payment, system install or macOS setting change is needed.
- Free disk drops below 4 GB.

Keep working on everything else while waiting on Jake.

## Launcher (paste after `/goal`, well under 4,000 characters)

> Read `docs/M2-GOAL.md` and `docs/M2-SPEC.md` in the Pieced repo (`outputs/pieced`) and execute the brief end to end as orchestrator, using `pieced-builder` subagents in parallel worktrees, merging and pushing to `main` after every green step. Never run full-screen, timing or Blender-window work unless Jake has said "go". Done only when `docs/evidence/M2-report.md` on pushed `main` shows gates S1–S9 all PASS with evidence (S2 on battery with Low Power Mode on; S1 scores and S8 verdict given by Jake in chat). Stop and ask Jake for the conditions listed in the brief.
