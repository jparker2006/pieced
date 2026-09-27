# Goal brief: Pieced Milestone 3 "Waves"

**Status:** READY. Jake agreed the plan in grill rounds 8 and 9 (D52–D93) on 2026-09-27. Launch with the launcher line at the bottom.

The **contract is `docs/M3-SPEC.md`**. This brief says how to execute it, what "done" means, and when to stop and ask Jake.

## Objective

Give Pieced its first objective, as specified in `docs/M3-SPEC.md`: **survive endless waves of knights.**
- Grunts run to mid-range and fire dodgeable spell orbs; your builds block them.
- Ships beam them onto the island.
- One life per run, a score, a personal best, and a results screen with "Go again".
- A bigger, Hogwarts-like castle and a more magical sky.
- A main menu and key rebinding.

First, close Milestone 2 (chunk 0). **Fun comes first:** chunk 1 is one grunt wave in Jake's hands.

The goal is complete only when gates **W0–W10** are all **PASS**, each with recorded evidence. The report `docs/evidence/M3-report.md` lists every gate, its evidence and the exact commit measured, and it must be pushed to `main` on `github.com/jparker2006/pieced`. Any gate that is FAIL, PENDING or UNVERIFIED means the goal is not done.

## Read first

1. `docs/M3-SPEC.md`: the contract.
2. `docs/design/DESIGN-GRILL.md`, Rounds 8 and 9 (D52–D93): why each decision was made, with Jake's words.
3. `docs/M2-SPEC.md`, `docs/M2-GOAL.md` and `docs/evidence/M2-report.md`: the look, the gates that must stay green, and M2's open items (S2, S3, closing the report).
4. `docs/research/bot-ai.md` (grunt AI and fairness), `docs/research/fortnite-building.md` (the build grid the grunts path over), `docs/research/game-feel.md`.
5. The seams the knights touch: `src/shared.rs` (`PlayerIntent`, `Character`, `Health`, `AppState`, `Layer`), `src/dummy.rs`, `src/knight.rs`, `src/combat.rs`, `src/input.rs`, `src/building/**`, `src/far/**`, `src/hud/**`, `src/menu/**`, `src/telemetry.rs`.
6. `docs/design/concepts/M3-C4-citadel-mix.png`: the castle target (D94: C3 base with C2 traits; `M3-C2` and `M3-C3` are its sources). It's local only; never commit it.

## Environment facts

- **Machine:** MacBook Air 15" M4, 16 GB RAM, 60 Hz, fanless, usually on battery with Low Power Mode on. It's **Jake's only machine, and he uses it while the goal runs.**
- **Repo:** `outputs/pieced`, public `jparker2006/pieced`, branch `main`. `gh` is authenticated.
- **Rust:** always `source scripts/env.sh` first. It sets the workspace toolchain, the shared build cache in `~/Library/Caches/pieced-target` (`~/Documents` is iCloud-synced and over quota), and low-priority cargo (utility QoS, nice 10, 6 jobs).
- **Shared cache:** all worktrees share it. Before trusting a render or a binary, check that the cargo output shows `Compiling pieced (<your path>)`.
- **Blender 5.2.2 LTS:** `blender` CLI, **headless only** (`-b`). Review models through preview renders. There are no Blender windows and no Blender MCP in this milestone.
- **Codex images:** `codex exec --skip-git-repo-check --ephemeral -m gpt-5.5 -s workspace-write -C docs/design/concepts [-i <ref> --] "<prompt>" < /dev/null`. Run one at a time. The `--` after `-i` is required, and stdin **must** be `/dev/null`.
- **Disk:** about 9 GB free at launch. Run `scripts/prune-target.sh` below 6 GB. Stop and ask below 4 GB.
- **Play binary:** `~/Library/Caches/pieced-target/pieced-play`. Jake launches it himself.

## Execution protocol

**Roles**

- **Orchestrator** (main session):
  - owns the plan, the shared types (`GameMode`, the wand weapon kind, `Orb`, `PlayerDowned`, the `grunt`/`waves`/`orb` tuning sections, `AppState::Menu` and `Results`), merges, gates and pushes;
  - owns the play-test builds and every conversation with Jake.
- **`pieced-builder` subagents** (Opus, high effort, per `.claude/agents/pieced-builder.md`): one slice each, in its own worktree (`isolation: "worktree"`).
  - Builders never push or merge.
  - Builders never edit another slice's files.
  - Builders never open windows (no native game, no scenarios, no Blender GUI).
- **`pieced-researcher` subagents:** bounded API lookups (Context7 first).

**Chunks** (`docs/M3-SPEC.md` → Chunks and play-tests). Slices within a chunk run in parallel where their files don't overlap.

- **Chunk 0, M2 leftovers.** One builder does the session log (spec → M2 leftovers, items 1–4): the frames CSV and session JSON, background writing, off-thread power samples, the S2 filter and verdict line, cold/warm launches and the boot breakdown. Then:
  1. Investigate the 8.7 s launch with the breakdown; fix it if cheap.
  2. Rebuild `pieced-play` and tell Jake: "every session now logs; any unplugged session ≥ 5 min with Low Power Mode on counts as S2".
  3. Re-run the M2 headless gates on the merge.
  4. Close the M2 report as soon as a qualifying session and 3 warm launches exist. **Don't wait on it:** chunk 1 starts immediately.
- **Chunk 1, one grunt wave.**
  1. The orchestrator first lands the shared types.
  2. Parallel slices:
     - **A, the grunt brain:** perception, the 8 Hz modes, A* on the build grid, aim, attack tokens, piece shooting.
     - **B, the orb and the wand:** projectile, pools, look, wind-up, the `wand.py` model and socket, orb sounds, the damage arrow, the off-screen warning.
     - **C, the run:** `GameMode` wiring, `--practice`, 3 grunts poofing in, player death, the plain results line and restart, pump knockback.
  3. Merge, build `pieced-play`, **play-test 1**.
- **Chunk 4, castle and sky**, starts in the background alongside chunk 1 (the target is D94's `M3-C4`): `far.py` castle, lanterns, glow, shooting stars, motes, shimmer, the budgets and motion tests, and offscreen castle views next to the concept. It touches only `far/`, `art/blender/`, the far tests and the look warm-up list.
- **Chunk 2, endless waves:** the wave director, the break, potions, score, best run, `runs.jsonl`, the wave HUD, the death beat, the results screen and Go again. Up to two parallel slices: rules and state, then HUD and results UI. **Play-test 2.**
- **Chunk 3, ships and the void:** the drop ship model, flight, telegraph, beam, landing and pacing, plus barrier pass-through, falling and the void bonus. **Play-test 3.** Play-test 4 (the castle look check) goes to Jake whenever chunk 4 is merged.
- **Chunk 5, menu and controls:** the main menu, Practice from the menu, Quit to menu, the Controls page, rebinding, bound-key hints, and the launch measured to the menu. **Play-test 5, the final verdict.**
- **Finish:**
  1. Run the fairness suite and the regressions (W3, W10) on the final commit.
  2. Collect W7 and W8 from Jake's session logs.
  3. Write the report.

**Play-tests** (spec → every play-test follows one protocol):
- Tell Jake in one short message: what's new, what to try, and the four questions (fun 1–5; too easy / right / too hard; any unfair death; what's off).
- **Never block on him.** Keep building the next chunk.
- When he answers, quote him in the report, and read his newest `userdata/sessions/*/session.json` and `userdata/runs.jsonl` lines into it.
- Put fixes from his answers first in the next chunk. Tuning moves of up to ±50% on spec numbers are allowed without asking; log each one.

**Merging, pushing and evidence:**
- `--no-ff` merges after `cargo test --locked`, clippy (`-D warnings`) and `fmt --check` pass on the merged result;
- **push `main` after every green step**;
- commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`;
- never force-push;
- `evidence/` and `userdata/` stay git-ignored. Curated summaries (session JSON excerpts, offscreen castle views, results-screen shots) go in `docs/evidence/m3/`;
- every number names its source (session folder or test), the commit, and the power and Low Power Mode state;
- failures are recorded honestly.

**Jake's Mac rule:**
- Never away from the Mac: there are **no go windows** in M3. No full-screen, native-scenario, timing or Blender-window work, ever. Native numbers come only from Jake's own sessions.
- Builds and headless tests run at low priority through `scripts/env.sh`. **At most two builds at once** across all worktrees.
- **Never use `pgrep` wait loops**; background jobs notify on completion.
- Codex image generation is always fine, one image at a time.

## Gates (definition of done)

| Gate | Pass condition | How it is measured |
|---|---|---|
| **W0 M2 closed** | `docs/evidence/M2-report.md` shows S1–S10 all PASS, and its status is DONE | S2 from a qualifying session log (D86); S3 from 3 warm printed launches in a row; S4–S6, S9, S10 and S7's test, clippy and fmt re-run headless on the closing commit; the 8.7 s launch explained, with the cold time reported |
| **W1 First grunt wave** | Chunk 1's grunt, orb and knockback tests pass, and play-test 1's answers are recorded | `tests/` output plus Jake's quoted answers |
| **W2 Endless waves** | Chunk 2's wave, potion, score, best-run, `runs.jsonl` and results tests pass, and play-test 2's answers are recorded | `tests/` output plus Jake's quoted answers |
| **W3 Fair knights** | The seeded fairness suite (20 simulated waves 1–10, 8 knights, a scripted stand-in player) holds every rule on the final commit, with **zero unfair deaths** | `tests/` output with the counts |
| **W4 Ships and the void** | Chunk 3's ship, beam, pacing, knockback and void tests pass, and play-test 3's answers are recorded | `tests/` output plus Jake's quoted answers |
| **W5 Castle and sky** | The castle follows the D94 target; the sky motion and far budget tests pass; Jake scores the spawn-vista and castle-up views **≥ 4** against the concept | Offscreen renders in `docs/evidence/m3/`, Jake's scores quoted |
| **W6 Menu and controls** | Chunk 5's menu and rebinding tests pass | `tests/` output |
| **W7 Performance** | A logged session that reaches **wave ≥ 6** qualifies (≥ 5 min of counted play, battery for every sample, Low Power Mode on, window visible, release, Battery preset) and passes the S2 bar (mean 16.4–17.0 ms, 0 frames > 25 ms, ≥ 99% < 18 ms); the full-wave budget tests pass | The session folder's `session.json` plus the wave from `runs.jsonl`, and `tests/` output |
| **W8 Launch** | 3 warm launches in a row < 5.0 s to a clickable menu, and Play → controllable < 1 s | `PIECED_LAUNCH_MS … warm` and `PIECED_PLAY_MS` from Jake's session logs |
| **W9 Fun verdict** | In play-test 5, Jake rates fun **≥ 4** and reports no unfair deaths | Jake's words, quoted from chat |
| **W10 No regressions** | On the final commit: every M1 and M2 test passes; `cargo test --locked` has 0 failures; clippy `-D warnings`, `fmt --check` and `scripts/build-art.sh --check` are clean | Command output in the report |

**If a gate needs a session Jake hasn't played yet** (W0's S2, W7, W8), don't invent a run and don't ask him to leave the Mac. Tell him once, in one line, what kind of session would complete it (for example: "an unplugged session with Low Power Mode on that reaches wave 6"). Keep working, and record the gate as PENDING until the log arrives.

## Stop and ask Jake when

- **A play-test is due.** Send the one-message prompt, then continue.
- **A change goes beyond the spec:**
  - a new mechanic or knight behaviour;
  - a tuning move of more than ±50%;
  - any change to the player's numbers, hitboxes, grid, building or controls beyond rebinding;
  - a change to D68's wave count curve.
- **A gate still fails after 3 focused attempts.** Bring the evidence and options (for example, what to cheapen to hold the performance bar with 8 knights).
- **Something new is needed:** any third-party asset, crate or service; an account, payment, system install or macOS setting change.
- **Free disk drops below 4 GB.**

Keep working on everything else while waiting on Jake.

## Where to launch

Launch from a **new Claude Code session opened in `outputs/pieced`**, for three reasons:
- the `pieced-builder` agent type and `isolation: "worktree"` then work directly;
- the session starts with a clean context;
- the repo's `.claude/agents/` definitions load.

If a session elsewhere must run it, the orchestrator creates worktrees with `git -C <pieced> worktree add` and dispatches general-purpose Opus subagents that follow `.claude/agents/pieced-builder.md`.

## Launcher (paste after `/goal`, well under 4,000 characters)

> Read `docs/M3-GOAL.md` and `docs/M3-SPEC.md` in the Pieced repo (`outputs/pieced`) and execute the brief end to end as orchestrator: chunk 0 (M2 leftovers) first, then chunks 1–5 in the brief's order, using `pieced-builder` subagents (Opus, high effort) in parallel worktrees. Source `scripts/env.sh` for every build (low priority, at most two builds at once). Merge and push to `main` after every green step. Give Jake a play-test after each chunk without ever blocking on him. Never run full-screen, timing, native-scenario or Blender-window work, never ask Jake to leave his Mac, and never use `pgrep` wait loops. Keep every M2 gate green. Done only when `docs/evidence/M3-report.md` on pushed `main` shows gates W0–W10 all PASS with evidence (W7 and W8 from Jake's own logged sessions; W5 scores and the W9 verdict given by Jake in chat). Stop and ask Jake for the conditions listed in the brief.
