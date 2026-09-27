# Pieced — Milestone 3 "Waves" spec

**Status:** AGREED with Jake on 2026-09-27. It's built from the design grill (`docs/design/DESIGN-GRILL.md`): Round 8 (D52–D70) set the plan, and Round 9 (D71–D93, Jake: "rec is good" on every question) filled in what the spec needs. This is the build contract for Milestone 3.

**Companions:**
- `docs/SPEC.md` (Milestone 1) and `docs/M2-SPEC.md` (Milestone 2): their gameplay, look and gates stay in force unless this spec changes them.
- `docs/research/bot-ai.md`: the bot architecture, aim model and fairness rules the grunt follows.
- `docs/research/fortnite-building.md`: the building the grunts path over.
- `docs/design/concepts/M3-C4-citadel-mix.png`: the castle target (D94: C3's starlit citadel with C2's traits). Local only, git-ignored.

## Problem Statement

Milestone 2 made Pieced look and sound the way Jake wanted. After his last session: "OMG ITS AMAZING! … When there is an actual game objective this is gonna be so much fun!" The game still has no objective. The only enemy is a training dummy that strafes and never shoots back, so building never matters and nothing can go wrong.

Jake wants a reason to use building, editing and shooting together (D54): **survive endless waves of knights**, like CoD Zombies, with one life per run and a personal best to beat.

The risks:
- **Fun:** will fighting a basic grunt be fun? Nothing else matters until that's true, so the first chunk is one playable wave (D69).
- **Fairness:** with 8 knights on the island, damage from nowhere feels cheap. The grunts must be readable and beatable (D76).
- **Performance:** 8 animated, outlined knights, their spell orbs and ships must hold M2's 60 fps bar on battery with Low Power Mode on.
- **Jake's Mac:** no gate may need him away from the Mac (D53). Every gate is headless, or comes from his normal play sessions.

## Solution

Milestone 3 adds the **Waves** mode, the new default:

- **Grunts:** knights that run to mid-range, strafe and fire slow, dodgeable orange-red spell orbs from a crystal wand. They walk up your ramps and shoot your walls, but never build or smash. They're driven through `PlayerIntent`, like the player (the "virtual controller" rule).
- **Endless waves:** 3 grunts in wave 1, about 2 more each wave, a little tougher every wave, at most 8 alive. A 10 s break between waves, shield potions, and one life per run.
- **Arrivals:** ships peel off the station and beam knights down onto the island edge. The pump knocks knights back, and a knock-off into the void counts as a kill.
- **Score and results:** a HUD with the wave, knights left and score, and a results screen with your personal best and "Go again".
- **A bigger, magical castle and sky:** the station becomes a Hogwarts-like castle from the concept Jake picks, and the sky gets shooting stars, drifting glowing motes and a shimmering galaxy.
- **A main menu and key rebinding:** Waves, Practice (the M2 sandbox with the dummy), Settings with a Controls page, and Quit.
- **M2 closes first:** a frame-timing log in every normal play session turns any unplugged session into S2 evidence, and the 8.7 s launch gets checked.

Done means:
- the M2 report is closed with every gate PASS;
- every M3 gate below is PASS with evidence;
- in the final play-test Jake rates the fun ≥ 4 out of 5 and reports no unfair deaths.

## User Stories

**The fight**

1. As Jake, I want knights that shoot back, so that there's something to survive.
2. As Jake, I want to see every enemy shot coming as a glowing orange-red orb after a wand wind-up, so that I can dodge it or build against it.
3. As Jake, I want my walls to stop orbs and take chip damage, so that building is the answer to incoming fire.
4. As Jake, I want grunts to spread around me at mid-range and strafe, so that fights feel like fights and not a conga line.
5. As Jake, I want grunts to walk up my ramps and shoot my walls when I box up, so that turtling isn't a free win.
6. As Jake, I want grunts to go down fast (4 rifle body shots, 3 headshots, one close pump), so that mowing through a wave feels powerful.
7. As Jake, I want my pump to blast knights backwards, and to send them screaming off the island edge, so that close fights are hilarious.
8. As Jake, I never want to take damage from a knight that can't see me, or from off-screen without a warning sound first, so that every death feels fair.
9. As Jake, I want a red arrow showing where every hit came from, so that I know where to turn or build.

**Waves and runs**

10. As Jake, I want waves to keep coming, each a bit bigger and tougher, so that a run always ends in a good fight.
11. As Jake, I want a 10 s break between waves with a countdown, and Enter to skip it, so that I can rebuild and breathe.
12. As Jake, I want my shield to top up +50 each break and knights to sometimes drop shield potions, so that a run isn't over after one bad fight.
13. As Jake, I want knights to arrive by ship with a beam and a marked landing spot, so that I know where the next fight comes from.
14. As Jake, I want each run to be a little different (drop points, ship timing, knight speeds), so that endless runs stay fresh.
15. As Jake, I want the HUD to show the wave, the knights left and my score, so that I always know where I stand.
16. As Jake, I want a short, funny death beat, then a results screen with my wave, eliminations, accuracy, headshots, run time and my best run, so that I want to go again.
17. As Jake, I want "Go again" to restart instantly, so that the next run is one click away.
18. As Jake, I want my personal best saved between launches, so that I have something to beat.

**The look**

19. As Jake, I want the castle in the sky to be much bigger and more magical, like Hogwarts, with many cone-roofed towers, hundreds of candlelit windows, floating lanterns and a golden glow, so that the world feels magical.
20. As Jake, I want shooting stars, drifting glowing motes and a shimmering galaxy, so that the sky is alive.
21. As Jake, I want the grunt to carry a little crystal wand, so that he reads as a caster.

**Menus and controls**

22. As Jake, I want the game to open on a main menu with the logo over the live island, so that it feels like a real game.
23. As Jake, I want Practice mode to keep the M2 sandbox with the dummy, so that I can warm up.
24. As Jake, I want to rebind every action, so that the controls fit my hands.
25. As Jake, I want the HUD and menus to show the keys I bound, so that the hints are never wrong.

**Speed and process**

26. As Jake, I want 60 fps with 8 knights on the island, on battery with Low Power Mode on, so that the waves play as smoothly as the sandbox.
27. As Jake, I want the game ready to click in under 5 s, so that I can play the moment I'm bored.
28. As Jake, I never want to leave my Mac for a test, so that I only ever play.
29. As Jake, I want to play-test after every chunk, so that fun is checked early and often.

## Implementation Decisions

### Scope and the seams

- **Unchanged:** every gameplay number in `docs/SPEC.md` and `docs/M2-SPEC.md` (the player's movement, guns, damage, TTK, pieces, building, editing), the hitboxes, the grid, the props, the look, and `PlayerIntent` as the only input seam.
- **What changes in gameplay:**
  - knights fight back (grunts);
  - the player can die (one life in Waves);
  - the pump knocks knights back (never the player);
  - knights can fall through the barrier into the void (the player still can't).
- **Modes:** a `GameMode { Waves, Practice }` resource.
  - The native game defaults to **Waves**. Until the main menu lands (chunk 5), `--practice` picks Practice.
  - **The headless test app and every existing scenario default to Practice**, so every M1 and M2 test, scenario and gate runs unchanged. Waves tests opt in.
  - Practice is today's game: the dummy, no death, no waves.
- **Knights are characters:** a grunt is a [`Character`] spawned through `player::spawn_character`, with the knight model and animation from M2, a `Grunt` marker, a `GruntBrain` and `Health::full(100, 0)`. It writes only its own `PlayerIntent` and `LookAngles`; movement, combat and building apply them exactly as for the player.
- **Shared-type changes** (the orchestrator lands these first, additively, before dispatching chunk 1 slices):
  - `GameMode`;
  - the wand as a new weapon kind that only knights carry, and an `Orb` projectile;
  - a `PlayerDowned` signal (the player's elimination ends a Waves run);
  - `Tuning` sections for `grunt`, `waves` and `orb`;
  - later, `AppState::Menu` and `AppState::Results` (chunks 2 and 5).
- **Tuning:** every number in this spec is a starting default in `Tuning`. The orchestrator may tune any of them by up to ±50% in response to Jake's play-test answers, logging each change in the report. Anything bigger, or a new mechanic, needs Jake.

### The grunt (D71–D78)

1. **Loops** (bot-ai.md):
   - perception at 30 Hz: line of sight to the player's head and chest, and last-known position;
   - decisions at 8 Hz: a utility-scored mode (`Approach`, `Strafe`, `Reposition`, `ShootPiece`), with a stickiness bonus so it doesn't flip-flop;
   - movement and aim every fixed tick.
2. **Knowing where you are:** grunts always know your position (horde rules, D73). They **only fire with line of sight** to you, or at the piece blocking it (item 5).
3. **Range and spread:**
   - Each grunt picks a standing spot 10–18 m from the player.
   - Spots are spread around the player: each grunt scores candidate spots by range, a spread penalty for being near other grunts' spots, and line of sight.
   - It strafes around its spot (like the dummy's strafe) while shooting.
4. **Navigation:** a hand-rolled A* over the build grid (no new crate).
   - A node is a cell, level and surface (ground, floor, ramp). Walls on edges block; ramps link level n to n+1; floors add walkable nodes.
   - The successor function reads the live piece map, so nothing is rebuilt. A path crossing a changed piece is re-planned.
   - Grunts climb the player's ramps and floors. They never build, break walls in melee, or edit.
   - Props and the arena bounds are obstacles; ship drop points (at the edge) are valid starts.
5. **Shooting pieces:** when a piece blocks the line to the player, a grunt with nothing better to do shoots that piece.
6. **Aim** (D76):
   - A **reaction delay** of 0.33 s after line of sight is gained, before the first shot.
   - **Leading:** it aims at a delayed snapshot of the player (lag 180 ms), extrapolated by a noisy velocity estimate over the orb's flight time.
   - **Error:** about 35 cm at 10 m, re-rolled per shot.
   - Wave 1 is bot-ai.md's "Normal" tier; reaction, lag and error blend linearly toward "Hard" (0.26 s, 140 ms, 20 cm) by wave 15.
7. **Attack tokens:** at most **3 grunts may be winding up or firing at once**. A grunt without a token repositions or strafes. Tokens go to grunts with line of sight, nearest first, and are held through one shot.
8. **Stats at wave 1** (D74): 100 HP, no shield; move speed 4.5 m/s (the player runs 5.5 and sprints 7.5); one orb every 1.5 s.
9. **Per-wave scaling** (D75), with wave n counting from 1:
   - HP × (1 + 0.04 (n − 1));
   - speed × (1 + 0.015 (n − 1)), capped at 6.5 m/s (below the player's 7.5 sprint);
   - fire rate × (1 + 0.02 (n − 1));
   - aim per item 6;
   - every curve stops at wave 25 (a grunt then has about 196 HP).
10. **Wand and wind-up:**
    - A small crystal wand in the right gauntlet: a new `art/blender/assets/wand.py` model (≤ 400 triangles), attached at a named socket.
    - Before each shot, a **0.4 s wind-up**: the wand's crystal glows orange, and the knight's eyes squint. The orb leaves from the wand tip.
    - Pieces of the M2 knight look are unchanged: the grunt is the knight, plus the wand.
11. **Pump knockback** (D78): each pump hit on a knight adds an impulse away from the shooter, scaled by pellets landed and distance, up to about 4 m of travel at point blank. The rifle adds none. It applies to knights only.
12. **The void** (D78):
    - The barrier's collider applies to the player only; knights can pass outward.
    - A knight whose feet leave the island top falls with a cartoon yelp and spin, and counts as an elimination (with the void bonus) the moment it drops 3 m below the island top.
    - It never counts for the player: the player can't cross the barrier.

### The orb (D71)

- **A real projectile**, stepped in the fixed tick: 30 m/s, a 0.15 m sphere cast each tick, 12 damage to the player, 12 structure damage to pieces, 60 m range.
  - It hits the player's body or head hitbox (a head hit does ×1.5, like the player's guns).
  - It stops on world geometry and pieces, and passes through knights (no friendly fire).
  - The player can't shoot it down.
- **Look:** a hot orange-red orb with a short flame-sparkle trail and a halo. On impact it makes a small orange burst; on a piece it adds chips of that piece. It must never read as the player's blue bolts.
- **Pooling:** orbs, trails and bursts come from fixed pools (at least 8 knights × 2 orbs in flight), and nothing is allocated per shot.
- **Sound:** a synthesized "fwoom" when an orb is fired, a doppler whoosh when one passes within 3 m of the player, and a crunchy "bonk" when one hits you.

### Fairness (D76)

These are rules, and each one is tested headless:
- No knight fires without line of sight to the player or to the piece it's shooting.
- No first shot sooner than the reaction delay after line of sight is gained.
- At most 3 knights hold attack tokens at any moment.
- **Off-screen shots are telegraphed:** when a knight outside the player's view starts a wind-up, a directional warning sound plays at the start of the 0.4 s wind-up.
- **Every hit on the player shows a red damage-direction arrow** around the crosshair, pointing at the orb's source, for 1 s.
- Orbs never pass through pieces.
- **Stuck:** a knight moving toward a goal makes at least 0.5 m of progress in every 3 s, or it re-plans (and a stuck event is logged).
- **An "unfair death"** is one where the final damage came from an orb fired without line of sight, from off-screen without the warning sound, or through a piece. The tests assert zero; Jake's play-tests report any he felt.

### Waves (D79–D85)

- **Count:** wave n has 3 + 2 (n − 1) grunts (D68). At most **8 alive** (D77); the rest wait for ships as slots open.
- **Randomness** (D83): a per-run seed drives drop points, ship timing, how knights split between ships, and a ±10% speed jitter per knight. The seed shows on the results screen, and `--seed <n>` replays it.
- **Break** (D79):
  - When the last knight of a wave dies, a **10 s countdown** shows on the HUD. Enter (rebindable) skips it.
  - At the start of each break, the player's shield refills by 50 (capped at max). Health comes back only from potions.
- **Shield potions** (D80): 10% of knights drop one where they die (seeded). It bobs and glows cyan. Walking within 1 m gives +25 shield, spilling into health when the shield is full. It vanishes after 20 s.
- **Score** (D81): 100 per knight, +50 when the killing hit was a headshot, +150 for a void knock-off, +250 × n for clearing wave n.
- **Personal best** (D81): the highest wave reached, with score as the tiebreak. It's saved in the git-ignored `userdata/best.json`.
- **Run log** (D89): each run appends one line to `userdata/runs.jsonl`: seed, commit, wave reached, score, run time, eliminations, accuracy, headshots, and the cause of death (orb source, wave, time since last hit). It's used to tune toward D68's 10–15-minute average run, a target and not a gate.
- **Death and results** (D84):
  - The player's elimination slows time to 0.3× for 1 s. The view drops to the grass, and the remaining knights do a goofy victory hop.
  - Then the results screen (`AppState::Results`): wave reached, eliminations, accuracy, headshots, run time and score, the best run beside them, and **"NEW BEST!"** when it is one. It also shows the seed.
  - **"Go again"** restarts the run in place, with no reload: pieces cleared, knights and orbs despawned, the player respawned at full health, a new seed. **"Quit"** goes to the main menu (or quits the game before chunk 5).
  - Quitting a run from the pause menu ends it and shows results.
- **Chunk 1 placeholder:** until ships land (chunk 3), knights appear with a sparkle poof at seeded points along the island edge, and death shows a plain results line with a restart key.

### Ship arrivals (D82)

- **Drop ship:** a bigger version of the M2 ship (`art/blender/assets/far.py` or a new `dropship` model, ≤ 2k triangles) with a glowing hull crystal.
- **Flight:** it peels off from the station and swoops down along a spline in about 4 s. It hovers 12–16 m above one of **4–6 fixed drop points** just inside the island edge, then flies back up.
- **Telegraph:** a glowing rune circle marks the landing spot **2 s** before the first knight lands, with a rising hum.
- **Drop:** knights slide down a beam of light, 1–3 per ship, about 0.4 s apart. They land with a sparkle poof. **They can't be hit and don't act until they land** (about 1 s in the beam).
- **Pacing:** a wave's knights come on several ships. Ships launch as alive slots open, never exceeding 8 knights alive or in the beam.
- **Headless:** the flight, beam and landing are pure functions of the ship's clock, tested headless, like M2's far motion.

### HUD and results UI (D65)

- **In a run:** wave number, knights left (alive plus still to come), and score, in M2's cartoon frames at the top of the screen. The existing layout doesn't move.
- **Break:** a big countdown with "Press ⟨Enter⟩ to start", showing the bound key.
- **Damage arrow:** red, cartoon-styled, around the crosshair.
- **Results screen:** a cartoon card in the pause menu's style (T12), with a "NEW BEST!" burst, the stats, the best-run comparison and the seed. It has "Go again" and "Quit" buttons, clickable with the trackpad.
- Everything shows on the hit frame where it relates to a hit (M2's same-frame rule), and everything works muted.

### The castle and the sky (D64, D67, D91)

- **The castle:** the target is `M3-C4-citadel-mix.png` (D94). C3's starlit citadel is the base: a vast dark-indigo castle city on a jagged floating rock with waterfalls, star-tipped needle spires, and a great stained-glass hall glowing gold and teal. From C2 it takes a taller centre stacked like a wedding cake of towers, an orbiting ring of golden runes (one rotating emissive mesh), rising lantern streams and a warm golden aura. The station model (`far.py`) is rebuilt to match, keeping its stained-glass heart and the Hogwarts traits:
  - many towers with pointed cone roofs;
  - hundreds of warm, candlelit windows, as emissive faces in one shared material;
  - floating lanterns, as one instanced mesh or pooled halos, bobbing;
  - a soft golden glow with sparkles drifting off it.
- **The sky:**
  - **shooting stars:** a streak across the sky every 8–20 s (seeded), from a small pool;
  - **drifting glowing motes:** soft specks floating around the arena and the far islands, a fixed pool of at most 64;
  - **a galaxy shimmer:** a slow twinkle over the galaxy on a 4–8 s cycle;
  - **aurora ribbons:** soft teal-violet bands that drift slowly around the galaxy, as in C3 (D94).
- **Budget:** the far layer may grow to **≤ 90k triangles, ≤ 18 opaque batches and ≤ 64 halos** (from 75k, 16 and 50). The `tests/far.rs` budget test is updated to match. Anything past that needs the performance gate to hold first.
- **Motion tests:** the M2 motion tests keep passing, and new ones check that shooting stars fire and cross, motes drift within their volume, lanterns bob, and the shimmer varies over its period.
- **The bar (Jake, on seeing C4: "make it that but 100x in the 3d"):** the in-game castle should look as spectacular as C4, not like a simplified stand-in. That means:
  - the silhouette, spire count, window density, rune ring, lanterns, waterfalls and glow all read at C4's scale and richness from spawn;
  - **at least two art-review rounds** comparing offscreen views with C4 (and its greyscale) before Jake scores, as in M2's S1;
  - the whole far budget (below) spent where it shows, with emissive windows, instanced lanterns and halos doing the heavy lifting cheaply.
- **Look check:** two offscreen views, the spawn vista and looking up at the castle (like T01 and T10), shown next to C4. Jake scores them in play-test 4; W5 needs **≥ 4 on both, aiming for 5**.

### Main menu and rebinding (D92, D93)

- **Main menu** (`AppState::Menu`):
  - The game opens here after Boot: the PIECED logo over the live island and sky, with the camera slowly orbiting the arena.
  - Buttons: **Waves** (showing the best run), **Practice**, **Settings** and **Quit**, in the pause menu's cartoon style.
  - Esc in a run still pauses; the pause menu gets "Quit to menu".
- **Rebinding** (`docs/BACKLOG.md`):
  - A **Controls** page in Settings lists every `PlayerIntent` action.
  - Click an action, then press a key or click to bind it. The trackpad look is fixed.
  - A conflict swaps the two bindings.
  - "Reset to defaults" restores D43's layout.
  - Bindings save with the other settings in `userdata/settings.json`.
  - `src/input.rs` stays the only code that reads devices; it maps bindings to intents.
  - Every on-screen key hint (break, edit, results) shows the bound key.
  - macOS-dangerous keys (Cmd combinations) can't be bound.

### M2 leftovers (chunk 0; D53, D70, D86, D87)

1. **Session frame log** (always on, D86):
   - Every native session writes to `userdata/sessions/<stamp>/`:
     - `frames.csv` (frame, t_ms, dt_ms, state);
     - `session.json` (commit, preset, launch time with cold or warm, power and Low Power Mode samples every 30 s, occluded time, play time, the S2 verdict).
   - Rows stream from a background thread, flushed every 5 s, so there's no hitch at quit and a crash keeps the data.
   - Power is sampled off the main thread (`pmset` must never block a frame).
   - The commit is embedded at build time, since the play binary doesn't run inside git.
   - The last 30 sessions are kept.
   - At quit the game prints one line: `PIECED_S2 PASS|FAIL|N/A <mean> <p99> <n>25ms <pct>18ms <why>`.
2. **Which frames count for S2:**
   - only frames in `Playing` (menus, pause and results excluded);
   - not the first 10 s after launch, nor the first 1 s after each return to `Playing`;
   - everything else counts, including wave starts, deaths and Go again.
3. **When a session qualifies for S2:** ≥ 5 min of counted play, on battery for every power sample, Low Power Mode on for every sample, the window never occluded during play, and a release build on the Battery preset. The thresholds are M2's: mean 16.4–17.0 ms, 0 frames > 25 ms, ≥ 99% < 18 ms.
4. **Launch** (D87):
   - A launch is **cold** when the binary's build id differs from the previous session's, and **warm** otherwise.
   - The game prints `PIECED_LAUNCH_MS <ms> cold|warm` plus a boot-phase breakdown (window, assets, galaxy, warm-up draws).
   - The 8.7 s launch is investigated with that breakdown and fixed if cheap (for example, warming fewer pipelines or caching the galaxy). The cold time is reported, not gated.
   - After chunk 5, "ready" means the main menu can be clicked, and `PIECED_PLAY_MS` reports Play → controllable.
5. **Closing the M2 report:**
   - S2 from the first qualifying session on any commit at or after chunk 0 (M3 only adds to the M2 look).
   - S3 from 3 warm launches in a row printed in Jake's sessions.
   - S4–S6, S9, S10 and S7's test, clippy and fmt re-run headless on the M2 closing commit.
   - The report's status becomes DONE with every gate PASS.

### Performance (D88)

- **Budgets with a full wave:**
  - 8 knights (7.8k triangles each, plus wands), 16 orbs, one ship and the M2 scene;
  - scenery, far and effect budgets as above;
  - the particle cap raised only if the budget test and S2 hold.
- **Knight cost:** knights share meshes and materials (the M2 knight already does). Beyond 30 m, the outline tapers per M2, and the procedural animation may drop to 30 Hz.
- **AI cost:** decisions at 8 Hz, staggered across knights so at most 2 re-plan in one tick. A* is capped at 2,000 node expansions per plan. Line-of-sight checks at 30 Hz.
- **Pipeline warm-up:** the orb, wand glow, beam, rune circle, drop ship, potion, damage arrow, results card and every new sky effect are warmed behind the loading screen (M2's rule), so the first of each never drops a frame.

### Working on Jake's Mac

- **Never away from the Mac** (D53): no gate needs a "go" window. Every gate is headless (tests, offscreen renders) or comes from Jake's normal play sessions (the session log, printed launch times, his play-test answers).
- **Never while Jake uses the Mac:** no full-screen runs, timing runs, native scenarios or Blender windows (including Blender MCP). There are no go windows in this milestone.
- **Low-priority builds:** always `source scripts/env.sh` (utility QoS, nice 10, 6 jobs). At most two builds at once across all worktrees.
- **Play builds:** the orchestrator builds the release play binary at low priority and copies it to `~/Library/Caches/pieced-target/pieced-play`, then tells Jake it's ready. Jake launches it himself.
- **No `pgrep` wait loops.** Background jobs notify on completion.
- **Disk:** run `scripts/prune-target.sh` below 6 GB free; stop and ask below 4 GB.
- Codex image generation is always fine (one image at a time).

## Chunks and play-tests (D90)

| Chunk | What lands | Play-test |
|---|---|---|
| **0. M2 leftovers** | The session frame log, cold/warm launch and the boot breakdown, the launch investigation, then the M2 report closed as soon as the evidence arrives | None; Jake's next unplugged session feeds S2 |
| **1. One grunt wave** | `GameMode`, the grunt (AI, A*, aim, tokens, wand, wind-up), the orb, pump knockback, the damage arrow and orb sounds, the player's death, 3 grunts poofing in at the edge, and a plain results line with restart | **1** |
| **2. Endless waves** | The wave director (counts, scaling, 8-alive cap, seed), the break and Enter skip, potions, score, best run, `runs.jsonl`, the wave HUD, the death beat and the results screen with Go again | **2** |
| **3. Ships and the void** | The drop ship, flight, telegraph circle, beam and landing, ship pacing, and the void (barrier pass-through, falling, the void bonus) | **3** |
| **4. Castle and sky** | The castle from Jake's concept, lanterns and glow, shooting stars, motes, the galaxy shimmer, the budgets. **This runs as a background art slice from chunk 1**, since it touches only `far/`, `art/blender/` and far tests | **4: a look check** |
| **5. Menu and controls** | The main menu, Practice from the menu, Quit to menu, the Controls page and rebinding, bound keys in hints, the launch measured to the menu | **5: the final verdict** |

**Every play-test** follows one protocol:
1. The orchestrator builds `pieced-play` from the merged `main` at low priority and tells Jake what's new and what to try.
2. Jake plays whenever he likes, as long as he likes.
3. Then he answers four questions (D89):
   - Fun, 1–5?
   - Too easy, about right, or too hard?
   - Did any death feel unfair?
   - What's off?
4. The answers, his `runs.jsonl` lines and the session's S2 line go in the report.
5. Fixes from the answers go first in the next chunk. The build never waits on Jake: while he hasn't played, the next chunk proceeds.

## Testing Decisions

- **Seams:** the same as M1 and M2. Headless simulation is primary: scripted `PlayerIntent` in, observable state out, fixed ticks, seeded randomness. Offscreen renders are for looks. Jake's sessions give feel, fun and native performance.
- **New headless tests:**
  - **Grunt:**
    - it approaches to 10–18 m and strafes;
    - grunts spread out: with 8 alive, no two stand within 3 m for more than 2 s;
    - it climbs a ramp to reach a player on a floor one level up;
    - it shoots the wall between them and breaks it in the expected time;
    - it never builds or edits;
    - the stats and every scaling curve at waves 1, 10 and 25, including the speed cap.
  - **Fairness** (seeded, over 20 simulated waves 1–10, against a scripted stand-in player that strafes, boxes up, ramps and peeks): no shot without line of sight, no first shot before the reaction delay, never more than 3 token holders, every off-screen wind-up plays its warning, every hit raises a damage arrow on the hit tick, no orb through a piece, no stuck knight beyond 3 s, and **zero unfair deaths**.
  - **Orb:** its speed and range, damage to the player (×1.5 on the head), chip damage to pieces, passing through knights, pooled with nothing allocated over 5 simulated minutes.
  - **Knockback and void:** a point-blank pump pushes a knight about 4 m and the rifle doesn't; the player is never pushed; a knight pushed through the barrier falls and scores the void bonus; the player can't cross.
  - **Waves:** counts per wave, the 8-alive cap, the break timer and skip, the +50 shield refill, potion drops (seeded), pickup and despawn, score arithmetic, the personal best save and load, `runs.jsonl`, Go again resetting everything, the same seed giving the same run.
  - **Ships:** a drop ship reaches its hover point in about 4 s, the telegraph leads the landing by 2 s, knights in the beam can't be hit and don't act, and ships never exceed the alive cap.
  - **Menu and controls:** every action rebinds, a conflict swaps, reset restores D43, bindings persist, hints show bound keys, Cmd combinations are refused, Practice and Waves start from the menu.
  - **Session log:** a simulated session produces the CSV and JSON; the S2 frame filter and qualification rules; the verdict on known inputs; retention of 30 sessions; cold versus warm from the build id.
  - **Budgets:** a full wave (8 knights, 16 orbs, a ship) within the triangle, batch and particle budgets; no asset allocation over 5 simulated minutes of waves.
  - **Sky:** the motion tests for shooting stars, motes, lanterns and shimmer; the far budget (≤ 90k triangles, ≤ 18 batches, ≤ 64 halos).
- **Kept:** every M1 and M2 test, unchanged, running in Practice mode.

## Gates (used by `docs/M3-GOAL.md`)

| ID | Gate | Pass condition |
|---|---|---|
| **W0** | M2 closed | `docs/evidence/M2-report.md` shows S1–S10 all PASS: S2 from a qualifying logged session, S3 from 3 warm launches in a row, and the headless gates re-run on the closing commit. The 8.7 s launch is explained, with the cold time reported |
| **W1** | First grunt wave | Chunk 1's headless grunt, orb and knockback tests pass, and play-test 1's answers are recorded |
| **W2** | Endless waves | Chunk 2's wave, score, best-run and results tests pass, and play-test 2's answers are recorded |
| **W3** | Fair knights | The fairness suite passes on the final commit: zero unfair deaths, and every rule holds |
| **W4** | Ships and the void | Chunk 3's ship, knockback and void tests pass, and play-test 3's answers are recorded |
| **W5** | Castle and sky | The castle follows the D94 target after ≥ 2 art-review rounds; the motion and budget tests pass; Jake scores both castle views ≥ 4 against C4 in play-test 4 |
| **W6** | Menu and controls | Chunk 5's menu and rebinding tests pass |
| **W7** | Performance | A logged session reaching **wave ≥ 6** qualifies and passes the S2 bar (battery, Low Power Mode on, window visible, ≥ 5 min of counted play); the full-wave budget tests pass |
| **W8** | Launch | 3 warm launches in a row < 5.0 s to a clickable menu, printed in Jake's sessions; Play → controllable < 1 s |
| **W9** | Fun verdict | In play-test 5, Jake rates fun **≥ 4** and reports no unfair deaths. Every play-test's answers are in the report |
| **W10** | No regressions | On the final commit: every M1 and M2 test passes; S4, S5, S6, S9 and S10 re-run headless; `cargo test --locked`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo fmt --check` and `scripts/build-art.sh --check` are clean |

## Out of Scope

- The brute, the mage and the builder knight (D60), and knights that build, smash or edit.
- Limited ammo or materials, loot, drops other than shield potions, a points shop, upgrades and unlocks (D62).
- New maps, new guns, a 1v1 build-fight mode, cosmetics (D57, D58).
- Difficulty settings (D55): the waves are the difficulty.
- Renaming the game or the mode (D66).
- Multiplayer, online leaderboards, controller support.
- Any change to the player's movement, guns, damage, hitboxes, grid, building or editing numbers.
- HDR bloom, real-time shadows, SSAO and the other M2 exclusions.

## Further Notes

**Known risks:**
1. **Fun is unproven.** That's why chunk 1 is one small wave in Jake's hands, and why the tuning rule lets the orchestrator move numbers quickly on his answers.
2. **D68's count is big.** Wave 20 is 41 grunts, and getting past it takes about 440 kills. If runs drag, the first lever is kill speed (HP), then the count curve. A count change needs Jake.
3. **S2 has never passed on battery.** The go-window proxies were close but spiky (MSAA was the main source, since fixed). Eight knights and their orbs add per-event costs, so pools and warm-up matter even more. If a wave session fails the bar, the knobs and the session CSV show where.
4. **Qualifying sessions depend on Jake playing unplugged.** The spec never asks him to; the log just waits for one. If none arrives, the goal says so plainly rather than inventing a run.
5. **Projectile orbs are new code** beside hitscan. They run in the fixed step with sphere casts, never the render frame.
6. **The castle concept is a painting.** As with M2's targets, the bar is the same style and composition, judged by Jake.
