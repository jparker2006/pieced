# Pieced — Milestone 4 "AAA" spec

**Status:** AGREED with Jake on 2026-09-28. It's built from design grill Round 11 (D96–D118, `docs/design/DESIGN-GRILL.md`), where Jake took the recommendation on every question. This is the build contract for Milestone 4.

**Companions:**
- `docs/SPEC.md` (M1), `docs/M2-SPEC.md` (M2) and `docs/M3-SPEC.md` (M3): their gameplay, look and gates stay in force unless this spec changes them.
- `docs/evidence/M3-report.md`: what shipped, and the carried gates (W0, W7, W8).
- `docs/research/game-feel.md`: gunfeel, hitstop, camera and accessibility rules.
- `docs/design/concepts/M4-V1`–`M4-V8`: the target board (D102, D112). Local only, git-ignored.

## Problem Statement

Waves is fun. Jake rated the last play-test 4.5, with no unfair deaths ("that was actually so much fun"). Now he wants Pieced to "play and feel like a AAA game" before story and progression come.

Today:
- A kill is a number and a poof.
- The guns have springs but no hand animation.
- There's no music.
- The knight moves only through procedural springs.
- Waves start without ceremony.
- The camera never reacts.
- **The game still misses 60 fps on battery.** In the latest spike report, 86% of slow frames wait on the GPU (`acquire`). Launch has never hit 3 warm launches under 5 s in a row.

The risks:
- **GPU:** every AAA feature costs frames on a machine that already misses the bar. The budget comes first, and every feature has to fit it.
- **Feel is subjective:** stills can't show it, so each chunk goes into Jake's hands and he scores it.
- **Gameplay drift:** polish must not change TTK, movement, the grunts or building (D117).
- **Jake's Mac:** no gate may need him away from the Mac (D53). Every gate is headless, or comes from his normal play sessions.

## Solution

Milestone 4 is a **feel-led AAA pass** (D96). The references are Fortnite, CoD Zombies, GTA and Overwatch (D97). The cartoon style stays; the look gets a polish, not a new style.

- **GPU budget and launch first** (chunk 0): per-pass GPU timing in the session log, a ≤ 12 ms per-frame GPU budget, the pre-approved levers (a half-resolution far layer, dynamic resolution on Battery, overdraw caps, closer outline fade), S2 measured on presented frames, and a faster launch.
- **Hit feedback and kills:** kill confirms, directional flinches, sparks and armor chips, a headshot "ding" and dent, physical deaths, "+100" popups and multi-kill callouts.
- **Weapons:** draw, ADS, reload and rack animations with the gloves doing the work, a tiny camera kick, and layered shot sounds.
- **Audio and music:** an adaptive, open-licensed score, round and death stings, and richer effects with reverb.
- **Knight animation:** clips authored in Blender on the rigid armor parts, blended in code, with springs on top.
- **Presentation, menus, building juice and camera:**
  - a wave-start banner, a loading screen with tips, animated menus, a pause blur, results that count up, and live settings previews;
  - pieces that assemble, animated edits, tumbling debris;
  - a subtle, trackpad-safe camera and a death cam.
- **"Start at wave"** (1, 6 or 10), so a normal session can reach wave-6 load (D115).
- **A target board:** 8 AAA concept frames, matched by in-game views that Jake scores.

Done means:
- every M4 gate below is PASS with evidence;
- the carried gates W7, W8 and W0 are closed through A7 and A8;
- in the final play-test Jake rates **fun ≥ 4.5** and **AAA feel ≥ 4**, and reports no unfair deaths.

## User Stories

**Kills and hits**

1. As Jake, I want a kill to feel unmistakable (a special sound, an "X" hitmarker, a tiny hitstop), so that every elimination lands.
2. As Jake, I want knights to flinch toward where I hit them, with sparks and armor chips flying, so that every hit reads.
3. As Jake, I want a headshot to ring the helmet with a "ding" and dent it, so that headshots feel special.
4. As Jake, I want knights to come apart when they die (the helmet pops off, the armor clatters) before the poof, so that kills are physical and funny.
5. As Jake, I want "+100" popups and "Double!" / "Triple!" callouts, like CoD, so that streaks feel rewarding.

**Weapons**

6. As Jake, I want my guns to draw, aim, reload and rack with real hand animation, so that they feel weighty.
7. As Jake, I want each shot to kick a little and sound layered (mechanism, magic, tail), so that firing feels powerful without spoiling my trackpad aim.

**Audio and music**

8. As Jake, I want music that's calm in the break and builds in a fight, so that waves feel tense.
9. As Jake, I want a round-start sting and a death sting, like Zombies, so that the moments feel big.
10. As Jake, I want the sounds to feel richer and roomier, so that the world sounds expensive.

**Knights**

11. As Jake, I want knights to run, strafe, wind up, flinch, die, land from the beam and celebrate with real animation, so that they feel alive.

**Presentation and building**

12. As Jake, I want every wave to start with a big banner and a sting, so that each wave feels like an event.
13. As Jake, I want a loading screen with tips, animated menus, a blurred background when paused, results that count up and settings that preview live, so that the game feels finished.
14. As Jake, I want pieces to visibly assemble, edits to animate and broken pieces to tumble apart, so that building feels juicy.
15. As Jake, I want subtle camera reactions and a death cam that shows who got me, so that moments feel cinematic without hurting my aim.
16. As Jake, I want to start a run at wave 6 or 10, so that I can jump into a big fight.

**Speed and process**

17. As Jake, I want a locked 60 fps on battery with Low Power Mode on, even in a full wave, so that it plays smoothly.
18. As Jake, I want the game ready in about 3 s, so that I can play the moment I'm bored.
19. As Jake, I want to score the new look against AAA concept frames, so that "done" is my call.
20. As Jake, I want a play-test after every chunk, and never to leave my Mac for a test, so that I only ever play.

## Implementation Decisions

### Scope and the gameplay line (D117)

- **Unchanged:** every gameplay number in `docs/SPEC.md`, `docs/M2-SPEC.md` and `docs/M3-SPEC.md`: movement, guns (damage, fire rate, bloom, swap, ADS and reload times), TTK, pieces and their HP, building, editing, hitboxes, the grid, grunt stats and scaling, the wave count curve, orbs, knockback, score and the fairness rules.
- **The only gameplay addition** is "Start at wave" (D115).
- **Everything else is presentation:** flinches, death animations, debris, camera effects and hitstop never move a hitbox, the aim ray, a timer or a gameplay number.
- **A new test pins the gameplay numbers:** the gameplay sections of `Tuning` equal their M3 closing values (`d722340`). A deliberate change must update the pin and needs Jake.
- **Tuning:** the orchestrator may tune **feel numbers only** by up to ±50% on Jake's play-test answers, logging each change in the report. Feel numbers are: hitstop, kick, camera nudges, FOV kicks, flinch sizes, popup timing, music and effect levels, and animation speeds. Anything else needs Jake.
- **New settings are menu-editable or skipped:** the `settings.json` rule from M3 still holds (see the Working rules below).

### Chunk 0: the GPU budget and the launch (D98–D101, D114)

1. **Per-pass GPU timing:**
   - The session log records GPU time per pass on every frame, from timestamp queries: world opaque, outlines, far, effects/halos/transparent, UI, and post (FXAA, sharpening, composite).
   - This extends `--knobs gpu=on` into the default, **if its cost is measured at ≤ 0.3 ms**; otherwise it samples 1 frame in 8.
   - `scripts/sessions.py --gpu <session>` prints each pass's mean and p95 against its budget.
   - The first real numbers come from Jake's next normal session. The chunk isn't blocked on it.
2. **The budget:** on battery with Low Power Mode at wave-6 load, **≤ 12 ms of GPU per frame at p95**. Starting split (the orchestrator may rebalance it, and logs each change):

   | Pass | Budget (ms) |
   |---|---|
   | World opaque (toon, props, pieces, knights, viewmodel) | 4.5 |
   | Outlines | 1.5 |
   | Far layer (castle, sky, galaxy, composite) | 2.0 |
   | Effects, halos, transparents | 2.0 |
   | UI | 0.5 |
   | Post (FXAA, sharpening) | 1.0 |
   | Slack | 0.5 |

   The budget **steers the work but isn't a gate** (D114). Each chunk's report lists every pass against it. Every feature must fit its slice or pay for itself by cutting something.
3. **Pre-approved levers** (D100), used as the numbers demand:
   - **Half-resolution far layer:** the castle, sky and galaxy render into their own half-resolution target, which the world pass composites behind everything nearer than the far layer. The castle views must still read (A6).
   - **Dynamic resolution on Battery only:**
     - A controller moves the render scale between **0.8 and 1.0** from the GPU time, with hysteresis: at most one step every 0.5 s, so it never visibly pumps.
     - A sharpening pass (contrast-adaptive) runs whenever the scale is below 1.0.
     - Plugged-in is untouched.
   - **Overdraw caps:** halos, particles and spell bursts are capped by estimated screen coverage (the sum of their projected areas). Past the cap the oldest fade first, and the nearest are kept.
   - **Outline fade** moves to 15–25 m. Knights beyond 20 m animate at 30 Hz.
   - **Not pre-approved** (Jake decides): thinner grass or flowers, and any change that visibly alters the castle or a scored view.
4. **S2 on presented frames** (D98): the qualifying rules and thresholds are unchanged (mean 16.4–17.0 ms, 0 frames > 25 ms, ≥ 99% < 18 ms, ≥ 5 min counted play, battery and Low Power Mode on for every sample, window visible, release build, Battery preset). But they're computed over **presented-frame intervals** (the drawable cadence already in `frames.csv`), not CPU `dt`.
   - `PIECED_S2` and `sessions.py` print both, and the verdict uses the presented one.
   - `session.json` records the highest wave reached in the session and the starting wave of each run, so A7's "wave ≥ 6" is read from the log.
5. **Pacing:**
   - The `latency` and `pipelined` knobs are measured in Jake's sessions.
   - A default changes only if the presented-frame numbers improve and input-to-frame latency stays ≤ 33 ms (M1's G3).
   - First, confirm from the GPU timings that `acquire` really means the GPU is behind, rather than display pacing. If it's pacing, the levers above are the wrong fix: bring the evidence to the next chunk report.
6. **Launch** (D101):
   - Cut the knight rig's +1.3 s: spawn the knight scene once and clone it, or build it while the window is appearing.
   - Cut the pre-window time (`app_built` → `window`).
   - Target about **3 s warm** to a clickable menu. The gate stays < 5 s (A8).
   - Cold launches are reported, not gated.
7. **Budget tests (headless):** the full-wave scene (8 knights, 16 orbs, a ship) plus every M4 addition at its cap (debris chunks, popups, callouts, armor pieces, the music voices) stays within the triangle, draw and particle budgets, and 5 simulated minutes allocate nothing per event.

### Chunk 1: hit feedback and kills (D105)

Everything shows **on the hit frame** (M2's same-frame rule), works muted, and is pooled.

- **Kill confirm:**
  - a distinct kill sound (a bonk-chime "cha-ching" layer over the hit sound);
  - an **"X" kill hitmarker**, bigger than the hit marker, white, or gold on a headshot kill;
  - **hitstop of 2 frames** on kills (`FxTuning::hitstop_frames` = 2). It freezes only `FreezableTime` (effects, knight animation, the viewmodel). The fixed gameplay step and input never pause, so aim and TTK are untouched.
- **Directional flinch:** a knight that's hit flinches away from the hit, by region (head, chest, left, right, legs). It's a spring impulse on the rigid parts, replaced by the authored flinch clips in chunk 4. Visual only.
- **Sparks and armor chips:** body hits throw metal sparks and 2–4 small armor chips (pooled), plus the existing starburst.
- **Headshot "ding" and dent:** a bright metallic ding layered on the headshot sound. The helmet swaps to a **dented variant** (from `knight.py`) that stays until the knight dies or respawns.
- **Physical deaths:**
  - On elimination the helmet pops off with spin, and 4–6 armor parts (gauntlets, boots, pauldrons) come off with ballistic arcs and bounce on the grass, the build pieces or the island top. They clatter (a sound per first bounce, capped at 4 voices), then shrink and fade within 1.5 s.
  - The existing poof and hat drop follow.
  - It's a cheap per-part simulation (gravity, a ground plane and piece tops, restitution), not avian bodies. It never collides with the player.
  - Void deaths keep the yelp and spin; the armor comes apart on the fall.
- **Score popups:**
  - World-space "+100" at the kill point, rising and fading over 0.8 s;
  - "+50 HEADSHOT" in gold and "+150 VOID" in violet, stacked;
  - the wave-clear bonus pops on the HUD score.
- **Multi-kill callouts:** kills within 1.5 s of the last chain. "Double!", "Triple!", "Quad!" and "Rampage!" (5+) pop near the crosshair with a sting. Visual and sound only; no score change.
- **Tests:**
  - every feedback element exists on the kill tick;
  - hitstop freezes `FreezableTime` for exactly 2 frames while the fixed step advances;
  - chains of 2–5 kills give the right callouts, and a gap over 1.5 s resets;
  - the armor parts land and are gone within 2 s;
  - pools stay capped over 5 simulated minutes;
  - hitboxes and TTK are unchanged (the M1 TTK suite).

### Chunk 2: weapons (D106)

- **Animations per gun,** keyframed in code, or as clips on the viewmodel's named parts. **Every one fits inside the existing gameplay time:** swap, ADS in and out, rifle reload 2.0 s, pump shells 0.5 s each.
  - **Draw on swap:** the gun rises in from below with a small settle.
  - **ADS:** in and out with weight (slight overshoot and settle). The existing ADS pose and zoom times stay.
  - **Rifle reload:** the left glove pops the dim crystal out, grabs a fresh glowing one from below, and slots it in with a click and a glow ramp.
  - **Pump:** each shell is a violet shard the glove pushes into the rings. The rack is heavier (a bigger pull, the rings spin, a clack).
  - **An idle breathing sway** (with M1's sway off-switch).
- **Camera kick:** render-only, **≤ 0.3° per shot**, recovering in ≤ 120 ms. The aim ray, bloom and recoil numbers are M1's; the kick never changes where a shot goes.
- **Shot sounds:** layered per gun (mechanism click + magic body + a tail that decays with the room reverb from chunk 3). Pump: a low whoomp + chime + tail.
- **Tests:**
  - every animation completes within its gameplay time;
  - the aim ray is identical with and without the kick;
  - the M1 gun and TTK tests pass unchanged;
  - the viewmodel budget stays ≤ 8k triangles per gun.

### Chunk 3: audio and music (D107, D116)

- **Adaptive score:**

  | Slot | When |
  |---|---|
  | Menu | the main menu |
  | Break | the 10 s break |
  | Combat low | a wave in progress |
  | Combat high | wave ≥ 6 or ≥ 6 knights alive |
  | Round-start sting | each wave start, synced with the banner |
  | Death sting | the player's elimination |
  | New-best sting | on "NEW BEST!" |

  - Loops crossfade in 1–2 s, and stings duck the music.
  - Music and effect levels are separate sliders in Settings (Audio), and both work at 0.
- **The music is openly licensed** (CC0 or CC-BY only), from OpenGameArt, Incompetech or Freesound:
  1. The orchestrator searches and builds a **listening page** (a private Artifact) with 2–3 candidates per slot. Each shows its name, source, license, length and size.
  2. **Jake OKs the downloads in one message.** Nothing is downloaded before that.
  3. He picks one per slot by ear.

  Tracks are trimmed and looped with scripts, encoded as OGG (Bevy's `vorbis` feature), and credited in `assets/ASSETS.md` with a license file. The asset audit is updated to accept these licenses.
- **Richer effects:** the synthesized bank stays original.
  - Each sound is layered (transient + body + tail).
  - A generated room reverb is **baked into each sample at load** on a background thread, so it costs nothing per frame.
  - A slight random pitch (±4%) on repeated sounds (steps, hits, placements).
  - Big hits duck the music for 150 ms.
- **Tests:**
  - the state → slot mapping, crossfade timing and sting ducking, simulated;
  - the sliders at 0 mute exactly;
  - every music file is in `ASSETS.md` with a CC0 or CC-BY license;
  - synthesis stays off the main thread, and the launch doesn't grow by more than 150 ms.

### Chunk 4: knight animation (D104)

- **Clips authored in Blender** (`art/blender/assets/knight.py` or a new `knight_anim.py`), keyframed on the named rigid parts (Helmet, Hat, Torso, Robe, GauntletL/R, BootL/R, the wand):

  | Clip | Notes |
  |---|---|
  | Idle | a breathing bob |
  | Run | blended with Strafe L/R by velocity direction |
  | Wand wind-up | exactly 0.4 s, the gameplay wind-up |
  | Flinch | head, chest, left, right: short additive clips |
  | Death | 3 variants (fall back, spin, crumple), feeding D105's armor break |
  | Beam landing | the drop down the beam and a squash on landing |
  | Victory hop | D84's death-beat celebration |
  | Void fall | the yelp and spin |

- **Exported and blended:**
  - The clips are exported in `knight.glb` and stay reproducible (`build-art.sh --check`).
  - They play through Bevy's `AnimationGraph`, with blend weights from velocity and state. The procedural springs (hit wobble, squash) are layered on top after the clips.
- **Cost:**
  - Knights share one graph and one set of clips.
  - Beyond 20 m they sample at 30 Hz (D100).
  - The knight triangle budget is unchanged (≤ 8k).
- **Hitbox fit (S5 extended):**
  - In every non-death clip, sampled every 50 ms, the body parts stay inside the body capsule (and the helmet inside the head sphere) within 10 cm.
  - The hitboxes never move with the clips.
- **Tests:**
  - every clip exists with its duration (the wind-up is exactly 0.4 s);
  - blend weights follow the velocity;
  - the sampled hitbox fit;
  - 8 knights animate within the budget;
  - W3's fairness suite still passes, since the wind-up telegraph is unchanged in timing.

### Chunk 5: presentation, menus, building juice and camera (D108–D110, D115)

- **Wave-start banner:**
  - A big cartoon "WAVE 6" (brass-and-crystal lettering) sweeps in over the island for 1.5 s with the round sting, then shrinks into the HUD's wave counter.
  - It never covers the crosshair for more than 0.3 s.
- **Loading screen:** the Boot overlay gets a progress bar and a rotating tip ("Hold Shift to aim", "Pump knights off the edge for +150", …) showing the bound keys.
- **Menus:**
  - Animated transitions: buttons slide and scale in 0.2 s, and hover pops.
  - The pause menu blurs the frozen world: one downsampled blur of the last frame, **and the 3D world stops rendering while paused**, which also saves battery.
- **Results:** stats count up with ticks; "NEW BEST!" bursts after the count.
- **Settings:** changes preview live behind a translucent panel. Graphics shows the world, the audio sliders play a sample, and the camera-effects slider shows a nudge.
- **Start at wave** (D115):
  - The Waves button offers 1, 6 or 10. The run starts at wave n with `GruntStats::for_wave(n)`, counts and scaling exactly as if it had got there.
  - Runs that start above wave 1 never update the personal best; they're marked on the results screen, and `runs.jsonl` and `session.json` record `start_wave`.
- **Building juice** (D109):
  - Pieces visibly assemble in 0.15 s: bricks stack and planks slap down. **Collision and full HP exist from the first tick** (D45); only the mesh animates.
  - Edit tiles flip with a 0.1 s animation; collision follows the edit at once.
  - Broken pieces burst into physics-lite chunks (the same simulation as D105's armor) that tumble, settle and fade within 2 s, within a cap of 48 chunks.
  - Placement sounds vary in pitch ±5%.
  - The ghost pulses (a 1 Hz glow).
- **Camera** (D108): every effect is **FOV, roll or a render-only offset**, so the aim ray never moves.
  - FOV kick: ≤ +4° on slides and landings from ≥ 3 m, over 250 ms.
  - Landing dip: ≤ 6 cm, render-only.
  - Slide tilt: 2–3° of roll.
  - Damage nudge: ≤ 0.5° of roll toward the hit, with a "Camera effects" slider (0–100%) in Settings.
  - **Death cam:** replaces D84's drop to the grass. The camera pulls out to third person over 0.6 s, turns to frame the knight that got you (the orb's source), holds while the knights hop, then goes to the results.
  - No head bob.
- **Tests:**
  - the banner timing and crosshair rule;
  - start at wave 6 and 10 gives the right stats and counts, doesn't touch the best, and is logged;
  - an assembling piece blocks a shot and a player on its first tick;
  - an edit's collision is immediate;
  - the debris cap;
  - the aim ray is identical with every camera effect at 100% and at 0;
  - the death cam frames the killer;
  - the paused frame stops the 3D passes;
  - the menu timings.

### The target board (D102, D112)

- **Targets:** `docs/design/concepts/M4-V1`–`M4-V8`, generated by Codex from in-game renders and approved by Jake in the grill:

  | ID | View |
  |---|---|
  | V1 | Spawn vista mid-wave, with ships arriving |
  | V2 | Rifle in ADS, firing at a knight |
  | V3 | The pump blasting a knight backwards |
  | V4 | A headshot kill, the helmet popping off |
  | V5 | A knight's wand wind-up at 10 m |
  | V6 | A box-up fight, orbs hitting the walls |
  | V7 | The wave-start banner over the island |
  | V8 | The main menu |

- **Capture:** a new ignored offscreen test (`tests/m4_board.rs`, like `hud_offscreen.rs`: headless, no window) scripts each view through intents, `teleport`, `set_look` and the freeze hook, and writes a PNG and a greyscale copy.
- **Art polish:** a background slice from the start works toward the board inside the GPU budget. It covers lighting and rim, material and texture richness, effect shapes, the knight's detail (the dented helmet, armor parts), and HUD polish. M2's exclusions (shadow maps, HDR bloom, SSAO) stay out unless the GPU numbers show room **and** Jake approves.
- **Review:** at least **two art-review rounds** (the orchestrator plus the `pieced-art-reviewer` agent) compare each capture with its target and greyscale before Jake scores.
- **Scoring:** a board page (a private Artifact) shows each target beside its capture. Jake scores 1–5; **each view needs ≥ 4**. The captures go in `docs/evidence/m4/board/`; the targets are never committed.

### Working rules on Jake's Mac

- **Never away from the Mac** (D53). There are no go windows. **Never run full-screen, timing, native-scenario or Blender-window work** (including Blender MCP) while Jake is on the Mac, and never ask him to leave it. Performance evidence comes **only from Jake's own logged sessions** (`userdata/sessions/`, `scripts/sessions.py`).
- **Builds only through `scripts/cargo.sh`**, which gives low priority, the shared cache, `CARGO_INCREMENTAL=0`, the quiet-mode wait and the disk guard. No hand-set cargo variables, and at most two builds at once across all worktrees.
- **Quiet mode:** when Jake is about to play (or asks the orchestrator to launch the game), run `scripts/quiet.sh stop`, then launch `~/Library/Caches/pieced-target/pieced-play` detached (`nohup`, logging to the scratchpad). Run `scripts/quiet.sh cont` when he's done.
- **Never use `pgrep` or polling wait loops.** Background jobs notify on completion.
- **Disk:** check `df -h ~` before dispatching parallel builders. Run `scripts/prune-target.sh` below 6 GB free; stop and ask below 4 GB. Keep every worktree merged with `main`, because worktrees on different bases compile separate ~6 GB sets of test binaries.
- **Settings persistence:** `Tuning::load_or_default` keeps only the menu-editable sections from `userdata/settings.json`. New designer sections (feel, music mix, camera defaults) are `#[serde(skip)]` or reset on load; new menu sliders are saved.
- **Codex images:** always fine, one at a time: `codex exec --skip-git-repo-check --ephemeral -m gpt-5.5 -s workspace-write -C docs/design/concepts [-i ref --] "<prompt>" < /dev/null`.
- **Downloads:** only the music Jake OKs (D116). Nothing else third-party without asking.

## Chunks and play-tests (D111)

| Chunk | What lands | Play-test |
|---|---|---|
| **0. GPU budget and launch** | Per-pass GPU timing, `--gpu` reports, the D100 levers, S2 on presented frames, max wave in the session log, pacing checks, the launch cut, budget tests. **Runs alongside chunk 1** | None; Jake's normal sessions feed it |
| **1. Hit feedback and kills** | Kill confirm, the "X" marker, hitstop, flinches, sparks and chips, the headshot ding and dent, physical deaths, popups, callouts | **1** |
| **2. Weapons** | Draw, ADS weight, reloads, the pump rack, sway, camera kick, layered shots | **2** |
| **3. Audio and music** | The listening page and Jake's picks, the adaptive score, stings, sliders, layered and reverbed effects | **3** (includes his picks) |
| **4. Knight animation** | Blender clips, the graph and blending, springs on top, the hitbox fit | **4** |
| **5. Presentation, menus, building juice and camera** | Wave banner, loading tips, menu motion, pause blur, results count-up, live settings, start at wave, assembling pieces, edit animation, debris, camera effects, the death cam | **5, the final verdict** |
| **Art (background)** | Board captures, art polish toward V1–V8, two review rounds, then Jake scores the board | Board scoring, any time after 2 rounds |

**Every play-test** follows one protocol:
1. The orchestrator builds `pieced-play` from the merged `main` at low priority and tells Jake, in one message, what's new and what to try (for chunks 0–5: "start at wave 6 on battery with Low Power Mode on if you can; it counts toward A7").
2. Jake plays whenever and however long he likes.
3. He answers **five questions** (D113):
   - Fun, 1–5?
   - AAA feel, 1–5?
   - Too easy, about right, or too hard?
   - Did any death feel unfair?
   - What's off?
4. The answers, his `runs.jsonl` lines, the session's `PIECED_S2` line and the `--gpu` report go in the report.
5. Fixes from the answers go first in the next chunk. **The build never waits on Jake.**

## Testing Decisions

- **Seams:** the same as M1–M3. Headless simulation is primary; offscreen renders are for looks; Jake's sessions give feel, fun and native performance.
- **New headless tests:** listed per chunk above, plus:
  - **The gameplay pin:** the `Tuning` gameplay sections equal the M3 closing values.
  - **Presented-frame S2:** the verdict on known inputs, both measures printed, the max wave and start wave recorded.
  - **GPU report:** `sessions.py --gpu` on a fixture session (in `scripts/test_sessions.py`).
  - **Budgets:** a full wave plus every M4 effect at its cap stays within the triangle, draw and particle budgets, and nothing allocates per event over 5 simulated minutes.
- **Kept:** every M1, M2 and M3 test, including the W3 fairness suite, unchanged.

## Gates (used by `docs/M4-GOAL.md`)

| ID | Gate | Pass condition |
|---|---|---|
| **A0** | Carried gates closed | When A7 and A8 pass, M3's W7 and W8 and M2's S2 and S3 are marked PASS in their reports with the same evidence, and W0 closes (D101) |
| **A1** | Hit feedback and kills | Chunk 1's tests pass, and play-test 1's answers are recorded |
| **A2** | Weapons | Chunk 2's tests pass, and play-test 2's answers are recorded |
| **A3** | Audio and music | Chunk 3's tests pass; every music file is licensed CC0 or CC-BY and credited; Jake picked the tracks; play-test 3's answers are recorded |
| **A4** | Knight animation | Chunk 4's tests pass, including the sampled hitbox fit and `build-art.sh --check`; play-test 4's answers are recorded |
| **A5** | Presentation, building juice and camera | Chunk 5's tests pass, including the aim-ray invariance and start at wave |
| **A6** | Target board | After ≥ 2 art-review rounds, Jake scores every one of V1–V8 **≥ 4** |
| **A7** | Performance | A logged session qualifies (battery and Low Power Mode on every sample, window visible, ≥ 5 min counted play, release, Battery preset), reaches **wave ≥ 6** (a start-at-wave run counts), and passes S2 **on presented frames** (mean 16.4–17.0 ms, 0 frames > 25 ms, ≥ 99% < 18 ms), on a commit with the final look. The full-wave budget tests pass |
| **A8** | Launch | 3 warm launches in a row < 5.0 s to a clickable menu, printed in Jake's sessions; Play → controllable < 1 s |
| **A9** | Final verdict | In play-test 5, Jake rates **fun ≥ 4.5** and **AAA feel ≥ 4**, and reports no unfair deaths |
| **A10** | No regressions | On the final commit: every M1–M3 test passes (including W3's fairness suite and the gameplay pin); `cargo test --locked` has 0 failures; clippy `-D warnings`, `fmt --check`, `scripts/build-art.sh --check` and `scripts/test_sessions.py` are clean |

The GPU budget is reported every chunk but is not a gate (D114).

## Out of Scope

- Story, progression, unlocks, the economy, new knight types, new maps or guns, and cosmetics (after this milestone).
- Any gameplay number change (D117), beyond start at wave.
- A skinned skeleton (D104).
- Head bob, or any camera effect that moves the aim ray.
- Shadow maps, HDR bloom, SSAO, SSR and TAA, unless the GPU numbers show room and Jake approves.
- Third-party assets other than the OFL font and the music Jake picks.
- Multiplayer, controller support, renaming the game.

## Further Notes

**Known risks:**
1. **The GPU diagnosis is still an inference.** `acquire` can mean the GPU is behind or display pacing. Chunk 0's GPU timings settle it before the levers are tuned.
2. **Qualifying sessions depend on Jake playing unplugged.** The spec never asks him to. Start at wave makes wave-6 load easy, and the log just waits. If none arrives, the goal says so plainly.
3. **Feel additions cost frames.** Pools, caps and warm-up (every new pipeline is warmed behind the loading screen, M2's rule) are mandatory, and the budget report shows each chunk's cost.
4. **Music quality depends on what's out there under CC0 or CC-BY.** If a slot has no good candidate, it stays a synthesized sting, and Jake is told.
5. **The concept frames are paintings.** As with M2, the bar is the same style and composition, judged by Jake.
6. **Authored clips are new pipeline work.** glTF animation export from the Blender scripts must stay byte-reproducible for `build-art.sh --check`.
