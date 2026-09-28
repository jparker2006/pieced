# Goal brief: Pieced Milestone 4 "AAA"

**Status:** READY. Jake agreed the plan in grill Round 11 (D96–D118) on 2026-09-28. Launch with the launcher line at the bottom.

The **contract is `docs/M4-SPEC.md`**. This brief says how to execute it, what "done" means, and when to stop and ask Jake.

## Objective

Make Pieced **play and feel like a AAA game** (D96: feel-led; references Fortnite, CoD Zombies, GTA, Overwatch), as specified in `docs/M4-SPEC.md`:
- the GPU budget and launch first;
- then, in order: hit feedback and kills, weapons, audio and music, knight animation, and presentation, menus, building juice and camera;
- an art polish toward the 8-frame target board, in the background.

No gameplay number changes (D117). Close the carried gates (M3's W7 and W8, and W0).

The goal is complete when **either**:
- gates **A0–A10** are all **PASS**, each with recorded evidence in `docs/evidence/M4-report.md` (with the exact commit measured), pushed to `main` on `github.com/jparker2006/pieced`; **or**
- **Jake explicitly closes the milestone** in chat (as he did for M3 in D95). Then record his words, mark the unmet gates CARRIED, and push the report.

Any gate that is FAIL, PENDING or UNVERIFIED otherwise means the goal is not done.

## Read first

1. `docs/M4-SPEC.md`: the contract.
2. `docs/design/DESIGN-GRILL.md`, Round 11 (D96–D118): why, in Jake's words. Rounds 8–10 for the Waves context.
3. `docs/evidence/M3-report.md`: what shipped, the carried gates, the first spike report, the tuning log.
4. `docs/M3-SPEC.md`, `docs/M2-SPEC.md`: the gates that must stay green and the look and performance contracts.
5. `docs/research/game-feel.md`: gunfeel, hitstop, camera and accessibility rules.
6. The seams: `src/fx.rs` and `src/fx/**` (hitstop, shake, debris, spells), `src/knight.rs`, `src/viewmodel.rs`, `src/audio/**`, `src/look/**` (toon, outline, far, halo, warm-up, settings), `src/render.rs`, `src/perf_knobs.rs`, `src/telemetry.rs`, `src/session.rs`, `src/hud/**`, `src/menu/**`, `src/waves.rs`, `src/building/**`, `scripts/sessions.py`.
7. `docs/design/concepts/M4-V1`–`M4-V8`: the target board. Local only; never commit them.

## Environment facts

- **Machine:** MacBook Air 15" M4, 16 GB RAM, 60 Hz, fanless, usually on battery with Low Power Mode on. It's **Jake's only machine, and he uses it while the goal runs.**
- **Repo:** `outputs/pieced`, public `jparker2006/pieced`, branch `main`. `gh` is authenticated.
- **Rust:** only through `scripts/cargo.sh` (e.g. `scripts/cargo.sh test --locked`). It sources `scripts/env.sh`: the workspace toolchain, the shared cache `~/Library/Caches/pieced-target` (`~/Documents` is iCloud-synced and over quota), utility QoS, nice 10, 6 jobs, `CARGO_INCREMENTAL=0`, the quiet-mode wait, and it refuses to build under 3 GB free.
- **Shared cache:** all worktrees share it. Before trusting a render or a binary, check that the cargo output shows `Compiling pieced (<your path>)`.
- **Blender 5.2.2 LTS:** the `blender` CLI, **headless only** (`-b`). Review through preview renders. No Blender windows, no Blender MCP.
- **Codex images:** `codex exec --skip-git-repo-check --ephemeral -m gpt-5.5 -s workspace-write -C docs/design/concepts [-i <ref> --] "<prompt>" < /dev/null`, one at a time. The `--` after `-i` is required, and stdin **must** be `/dev/null`. If Codex says it saved an image that isn't there, recover it from `~/.codex/generated_images/<session>/`.
- **Disk:** about 13 GB free on 2026-09-28. Run `scripts/prune-target.sh` below 6 GB; stop and ask below 4 GB.
- **Play binary:** `~/Library/Caches/pieced-target/pieced-play`.
- **Sessions:** `python3 scripts/sessions.py` (list), `--s2`, `--launches`, `--spikes <session|latest>`, and `--gpu` once chunk 0 lands.

## Execution protocol

**Roles**

- **Orchestrator** (the main session):
  - owns the plan, shared types, merges, gates, pushes, the play-test builds, the report, and every conversation with Jake;
  - may create or update agent definitions in `.claude/agents/` (D118).
- **`pieced-builder` subagents:** Opus, high effort, per `.claude/agents/pieced-builder.md`. One slice each, **in its own worktree** (`isolation: "worktree"`). Builders commit on their branch; they never push or merge. The orchestrator merges.
- **`pieced-art-reviewer` subagents:** read-only. They compare board captures with the targets and return per-view gaps and a predicted score. Used for the ≥ 2 art-review rounds.
- **`pieced-researcher` subagents:** bounded API lookups (Context7 first), e.g. Bevy 0.19 timestamp queries, `AnimationGraph`, the `vorbis` feature.

**Every builder brief includes these rules** (the M3 lessons):
1. Run cargo **only through `scripts/cargo.sh`**. Never bare `cargo` with hand-set variables: that skipped the low-priority wrapper once and dropped Jake's game to 22 fps.
2. **Stay merged with `main`**: `git merge main` before building once main moves. Worktrees on different bases compile separate ~6 GB sets of test binaries, and the disk has hit 0.9 GB.
3. **No polling:** never `pgrep`, `ps | grep` or `until …; sleep` loops. Run long commands in the foreground with a long timeout, or in the background and wait for the notification.
4. **Quiet mode is the orchestrator's job:** builders never touch `scripts/quiet.sh`, and `cargo.sh` waits by itself while it's on.
5. **Never open windows:** no native game, no scenarios, no `cargo run`, no Blender GUI or MCP. Offscreen renders and headless Blender only.
6. **Settings persistence:** new designer tuning sections are `#[serde(skip)]` or reset on load (`Tuning::load_or_default` keeps only the menu-editable sections).
7. **Disk:** check `df -h ~`. Stop and report under 4 GB.
8. **No gameplay numbers change** (D117). The gameplay pin test must stay green.
9. Report in under 300 words: what works from a player's view, tests and counts, the commands run, files touched, the GPU cost expected or measured, gaps, and the branch and commit.

**Chunks** (spec → Chunks and play-tests). Slices within a chunk run in parallel where their files don't overlap. **At most two builds at once** across all worktrees; check disk before dispatching.

1. **Start:**
   - Land the gameplay pin test (the M3 closing `Tuning` gameplay values) and any shared types first: new `FxTuning` fields, `start_wave`, music state, and the cue events for kills, callouts and banners.
   - Then dispatch in parallel: **chunk 0** (GPU budget and launch), **chunk 1** (hit feedback and kills), and the **art slice** (board captures in `tests/m4_board.rs` first, then polish).
2. **Chunk 0, the GPU budget and launch:**
   - GPU timing into the session log, `sessions.py --gpu`, presented-frame S2, max wave and start wave in `session.json`;
   - the D100 levers behind knobs, so each can be measured;
   - the launch cut (the knight rig, pre-window);
   - the budget tests.

   Then rebuild `pieced-play` and tell Jake in one line: "every session now reports GPU time; any unplugged session with Low Power Mode on counts toward A7". Turn levers on by default only when his session numbers support it.
3. **Chunk 1, hit feedback and kills. Play-test 1.** Keep it small and playable; it's the first check of the AAA direction.
4. **Chunk 2, weapons. Play-test 2.**
5. **Chunk 3, audio and music:**
   1. First the listening page: a private Artifact with 2–3 CC0 or CC-BY candidates per slot, each with its name, source, license, length and size.
   2. **Ask Jake to OK the downloads in one message;** download nothing before that.
   3. He picks one per slot.
   4. Then the adaptive score and the effects work.

   **Play-test 3.**
6. **Chunk 4, knight animation. Play-test 4.**
7. **Chunk 5, presentation, menus, building juice and camera. Play-test 5, the final verdict.**
8. **Art slice (background):**
   1. board captures;
   2. polish within the GPU budget;
   3. **two art-review rounds** against V1–V8 (the orchestrator plus `pieced-art-reviewer`);
   4. then the board page (a private Artifact), where Jake scores.
9. **Finish:**
   1. Run A10 on the final commit.
   2. Collect A7 and A8 from Jake's session logs.
   3. Close the carried gates in the M2 and M3 reports (A0).
   4. Write the report and push.

**Play-tests** (spec → every play-test follows one protocol):
- Tell Jake in one short message:
  - what's new and what to try;
  - "start at wave 6 on battery with Low Power Mode on if you can" once start at wave exists;
  - the five questions: fun 1–5, AAA feel 1–5, too easy / right / too hard, any unfair death, what's off.
- **Never block on him.** Keep building the next chunk.
- When he's about to play or asks you to launch it:
  1. Run `scripts/quiet.sh stop`.
  2. Launch `pieced-play` detached (`nohup`, logging to the scratchpad).
  3. Run `scripts/quiet.sh cont` when he says he's done.
- When he answers, quote him in the report. Read his newest `session.json`, `runs.jsonl` lines, the `PIECED_S2` line and the `--gpu` report into it.
- Fixes from his answers go first in the next chunk. Tuning moves of up to ±50% on **feel numbers only** are allowed without asking; log each one.

**Merging, pushing and evidence:**
- `--no-ff` merges after `scripts/cargo.sh test --locked`, clippy (`-D warnings`) and `fmt --check` pass on the merged result;
- **push `main` after every green step**, and keep every live builder merged with the new main;
- commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`;
- never force-push;
- `evidence/` and `userdata/` stay git-ignored. Curated summaries (board captures, session excerpts, GPU reports) go in `docs/evidence/m4/`;
- every number names its source (session folder or test), the commit, and the power and Low Power Mode state;
- failures are recorded honestly.

**Jake's Mac rule:**
- Never run full-screen, timing, native-scenario or Blender-window work while Jake is on the Mac, and **never ask him to leave it.** There are no go windows. Native numbers come only from his own logged sessions.
- Builds only through `scripts/cargo.sh` (low priority). At most two at once.
- **Never use `pgrep` or polling wait loops.**
- Codex image generation is always fine, one image at a time.

## Gates (definition of done)

| Gate | Pass condition | How it is measured |
|---|---|---|
| **A0 Carried closed** | M3's W7/W8 and M2's S2/S3 marked PASS with A7/A8's evidence; W0 closed | The M2 and M3 reports updated and pushed |
| **A1 Hit feedback and kills** | Chunk 1's tests pass; play-test 1 recorded | `tests/` output plus Jake's quoted answers |
| **A2 Weapons** | Chunk 2's tests pass; play-test 2 recorded | `tests/` output plus Jake's quoted answers |
| **A3 Audio and music** | Chunk 3's tests pass; every music file CC0 or CC-BY and credited; Jake picked them; play-test 3 recorded | `tests/` output, `assets/ASSETS.md`, Jake's quoted picks and answers |
| **A4 Knight animation** | Chunk 4's tests pass (the sampled hitbox fit, `build-art.sh --check`); play-test 4 recorded | `tests/` output plus Jake's quoted answers |
| **A5 Presentation, juice and camera** | Chunk 5's tests pass (the aim ray unchanged by camera effects, start at wave) | `tests/` output |
| **A6 Target board** | After ≥ 2 art-review rounds, Jake scores V1–V8 each **≥ 4** | Captures in `docs/evidence/m4/board/`, Jake's scores quoted |
| **A7 Performance** | A qualifying session (battery and Low Power Mode on every sample, window visible, ≥ 5 min counted, release, Battery preset) reaching **wave ≥ 6** passes S2 **on presented frames** on a final-look commit; the budget tests pass | The session folder's `session.json`, `sessions.py --s2` and `--gpu`, `tests/` output |
| **A8 Launch** | 3 warm launches in a row < 5.0 s to a clickable menu; Play → controllable < 1 s | `sessions.py --launches` |
| **A9 Final verdict** | Play-test 5: **fun ≥ 4.5**, **AAA feel ≥ 4**, no unfair deaths | Jake's words, quoted |
| **A10 No regressions** | Final commit: every M1–M3 test (incl. W3's fairness suite and the gameplay pin) passes; clippy, fmt, `build-art.sh --check` and `scripts/test_sessions.py` clean | Command output in the report |

The GPU budget (≤ 12 ms at p95, per pass) is reported in every chunk's log entry but is not a gate (D114).

**If a gate needs a session Jake hasn't played yet** (A7, A8), don't invent a run and don't ask him to leave the Mac. Tell him once, in one line, what kind of session would complete it (for example: "an unplugged session with Low Power Mode on, started at wave 6, for 5+ minutes"). Keep working, and record the gate as PENDING until the log arrives.

## Stop and ask Jake when

- **A play-test is due.** Send the one-message prompt, then continue.
- **The music downloads are ready to OK** (D116).
- **A change goes beyond the spec:**
  - any gameplay number (D117);
  - a lever that isn't pre-approved (thinner grass, anything that visibly changes the castle or a scored view);
  - shadow maps, HDR bloom or another M2 exclusion;
  - a feel tuning move of more than ±50%.
- **A gate still fails after 3 focused attempts.** Bring the evidence and options, e.g. what to cheapen to hold the GPU budget.
- **Something new is needed:** any other third-party asset, crate or service; an account, payment, system install or macOS setting change.
- **Free disk drops below 4 GB.**

Keep working on everything else while waiting on Jake. **If Jake says to close the milestone, close it:** record his words, mark the open gates CARRIED, update the backlog, and push.

## Where to launch

Launch from a **new Claude Code session opened in `outputs/pieced`**, so that `.claude/agents/` loads, `isolation: "worktree"` works, and the context starts clean.

## Launcher (paste after `/goal`, under 4,000 characters)

> Read `docs/M4-GOAL.md` and `docs/M4-SPEC.md` in the Pieced repo (`outputs/pieced`) and execute the brief end to end as orchestrator: chunk 0 (GPU budget and launch) and chunk 1 (hit feedback and kills) first, alongside the background art slice, then chunks 2–5 in the brief's order, using `pieced-builder` subagents (Opus, high effort) in parallel worktrees, with the builder rules from the brief in every dispatch. Run cargo only through `scripts/cargo.sh`, at most two builds at once; keep builders merged with `main`; merge and push to `main` after every green step. Give Jake a play-test (five questions) after each chunk without ever blocking on him. Never run full-screen, timing, native-scenario or Blender-window work, never ask Jake to leave his Mac, and never use `pgrep` or polling wait loops. Change no gameplay numbers and keep every M1–M3 gate green. Done when `docs/evidence/M4-report.md` on pushed `main` shows gates A0–A10 all PASS with evidence (A7 and A8 from Jake's own logged sessions; the A6 scores and A9 verdict from Jake in chat), or Jake explicitly closes the milestone. Stop and ask Jake for the conditions listed in the brief.
