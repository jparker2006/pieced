# Design step: grilling record

The next step for Pieced is the look: cartoon up close ("Family Guy / Looney Tunes"), magical in the distance ("Harry Potter × Star Wars"), with guns that look like Fortnite guns but fire like spells. This file records each decision as Jake makes it.

## Round 1: settled 2026-09-25 (Jake: "rec on all")

| # | Decision | What it commits us to |
|---|---|---|
| D1 | **Milestone 1 is parked.** | The G2 performance bar carries into this step's done state. Jake's 10-minute feel verdict (G9) can happen at any time. The old-art sign-off (G8) is dropped because the new look replaces the old art. |
| D2 | **Concept images come from the Codex CLI in the background.** | Four style directions, two images each (`docs/design/concepts/`). Jake picks one direction, or mixes them. |
| D3 | **Spells change the visuals only.** | Gameplay stays hitscan, so time-to-kill, trackpad aiming and bot fairness don't change. Guns keep Fortnite-like silhouettes, with magical parts: crystals, runes, energy cells, brass. Every shot is a glowing spell bolt with a trail and a magical impact burst. |
| D4 | **Art is built with Blender scripts, plus code for simple things.** | Models are built by Blender Python scripts I write and iterate on without Jake, then exported for Bevy. Original assets only: no CC0 packs unless Jake approves them later. **Blender is not installed.** Installing it needs Jake's OK (see round 2). |
| D5 | **The performance line is hard.** | 60 fps on battery with Low Power Mode on the MacBook Air (G2 thresholds). The style must be cheap by design: toon shading, outlines, restrained glow and particles. |
| D6 | **Done state (draft).** | Jake picks a style from the images. The game is rebuilt in that style. Done when all four hold: Jake signs off a new 8-view gallery; the guns and spells feel "sick" to Jake; G2 holds; and the far view has life in it (e.g. ships passing, castles glowing). |

## Round 2: partial, 2026-09-25 (Jake, after seeing the 8 concepts)

| # | Decision | What it commits us to |
|---|---|---|
| D7 | **Style mix.** | **Close-up:** the C1/C2 flat TV-cartoon look (clean thin outlines, flat two-tone shading) mixed with Looney Tunes-style wizard characters. **Far view:** D1's background, a stained-glass cathedral space station over a swirling galaxy. |
| D8 | **Enemy.** | The B2 castle knight (armored battle-mage), redrawn in the close-up cartoon style with Looney-wizard flavor. |
| D9 | **Guns.** | Built like the A1/A2 chunky brass-and-crystal rifle. |
| D10 | **Spells.** | Like B1 and C1: a glowing blue orb or starburst bolt with a sparkle trail and a bright impact burst. |
| D11 | **Blender is approved.** | Jake wants help freeing disk space first; only about 5.5 GB is free. |

## Round 3: 2026-09-25/26

| # | Decision | What it commits us to |
|---|---|---|
| D12 | **Building material: open.** | Jake likes both C's cartoon brick and A's warped cartoon wood. Options for the next round: brick walls with wooden floors and ramps; wood → brick material tiers; or one mixed "brick-and-timber" look. Resolve this with the next image round. |
| D13 | **Arena: a grassy cartoon floating island in space.** | C-style grass, cliffs and a few trees. D1's stained-glass cathedral station and the swirling galaxy fill the sky. The island edges are the arena boundary. |
| D14 | **Spell colors.** | Rifle: blue starburst bolts (B1/C1). Pump: a violet-and-gold spark fan. Headshots flash gold. Shield hits flash cyan. |
| D15 | **Far-view life and HUD.** | The galaxy slowly turns. Ships glide between the station's towers. The stained glass pulses softly. Small islands bob. The HUD gets C-style clean cartoon frames and crystal icons, in the current layout. |
| D16 | **Audio.** | Magical zaps and shimmer for spells. A cartoon "bonk" plus sparkle on hits. A brick "clunk" for building. A glassy crash when a shield breaks. All still synthesized in code. |
| D17 | **Process.** | After this grill, Jake compacts the conversation. Then another grill round and more images, then a spec and a `/goal` brief with a sharply defined done state. |

## Round 4: settled 2026-09-25 (Jake: "rec on all, go with A for Q16")

The base design image is `concepts/R4-M1-brick-walls-wood-floors.png`. It combines D7–D16 in one scene.

| # | Decision | What it commits us to |
|---|---|---|
| D12 | **Building material: (a).** | Walls are chunky cartoon brick. Floors and ramps are warped cartoon wooden planks with big nail heads. One material per piece type, so there's no gameplay change and piece types read at a glance. A faint glowing build grid shows on the grass. |
| D18 | **Outlines and shading.** | Thin dark outlines (inverted hull) only on near objects: guns, gloves, the knight, built pieces, trees, rocks. The sky, station and far islands get no outlines and stay soft and glowy. Flat two-tone toon shading plus a thin rim light. **No shadow maps**; cartoon blob shadows under characters. |
| D19 | **The knight.** | As in R4-M1: big steel bucket helmet, visor eyes that blink, go wide on hits and turn to X's on elimination; oversized gauntlets and boots; small body; purple trim and cape. A **short floppy hat that stays inside the head hitbox**, and the silhouette matches the hitboxes. Reactions: a wobble on hits; on shield break, cyan glass shatters and stars circle his head; on elimination, a cartoon poof, and the hat drops and spins on the grass. |
| D20 | **Knight animation.** | Rigid armor parts (helmet, torso, gauntlets, boots, cape) animated procedurally in code with squash and stretch. No skinned skeleton. Bots reuse it later. |
| D21 | **Gloves and guns.** | White four-finger cartoon gloves. Rifle: brass SCAR-like body, dark-wood stock, a blue crystal in a glass chamber, an energy-cell magazine. Pump: stubby, with a flared brass bell muzzle, a wooden pump grip, and a violet crystal in gold rings. Code-driven animation: squash-and-stretch kick on firing; reload pops the dim crystal out and slots a glowing one in; the crystal fades as the magazine empties. |
| D22 | **Far view.** | The station fills one side of the sky. The galaxy swirl sits opposite and overhead, and a ringed planet sits low on the horizon. A ring of 4–6 bobbing waterfall islands circles the arena. Ships loop between the spires. A shimmering magic barrier marks the island edge (no falling). Warm key light from a small star near the station, cool teal fill from the galaxy. The galaxy is generated in code (baked to a cubemap, then rotated). The station and islands are low-poly 3D models. |
| D23 | **Font.** | One openly licensed (OFL) cartoon display font for the HUD and menu. Everything else (models, textures, sounds) is original. |
| D24 | **Arena.** | Keep M1's layout and size; reskin only. Cover boxes become rocks, stumps and trees. The dummy becomes the knight, with the same movement. |
| D25 | **Done, part 1: the target board.** | 12 target images, one per gallery view (`concepts/T01`–`T12`). The in-game gallery captures the same 12 views, and an evidence page shows each one beside its target. Jake scores every view 1–5. Done when **every view scores ≥ 4**. The bar is the same style and composition, not a pixel match. |
| D26 | **Done, part 2: speed and feel.** | All at once, with the full new look: the G2 bar (5 min on battery with Low Power Mode on, mean 16.4–17.0 ms, 0 frames > 25 ms, ≥ 99% < 18 ms); launch < 5 s; an automated motion check (galaxy, ships, islands and glass actually move); Jake's 10-minute play verdict that the guns, spells and sounds feel sick; M1's G1 and G6 still pass; tests, clippy and fmt clean. |
| D27 | **Order of work.** | 1. G2 baseline fix on the current art. 2. Toon material and outlines. 3. Guns and gloves. 4. Knight. 5. Island and building materials. 6. Sky and far-view life. 7. Spells and effects. 8. HUD and audio. 9. Gallery, performance run, Jake's scoring and play check. Push after every green step. Heavy or full-screen runs only when Jake says "go". |
| D28 | **Blender.** | Headless Blender Python scripts in the repo are the source of truth for every model, so assets are reproducible and need no window. Jake wants to use a Blender MCP for live, watchable iteration (installing it needs his OK). |

The M1 `/goal` was cleared by Jake on 2026-09-25.

## Round 5: settled 2026-09-25 (Jake: "approve all targets, rec on Q27 Q28 Q29")

| # | Decision | What it commits us to |
|---|---|---|
| — | **Targets approved.** | All 12 of `concepts/T01`–`T12` are the S1 target board. |
| D29 | **Solid rocks and stumps in the arena.** | 6–8 rocks (1.0–1.4 m) and stumps (≤ 0.7 m) with static collision. Pieces can be built through them. None near the spawns, the initial cover or the dummy's strafe zone. Trees stay on the margin. This is the milestone's only gameplay change. |
| D30 | **Builds while Jake uses the Mac.** | Cargo runs at utility QoS and nice 10 with 6 jobs (`scripts/env.sh`). At most two builds at once. Full-screen, timing and Blender-window work still waits for "go". |
| D31 | **Blender MCP installed.** | `mcp-for-blender` 2.1.0 is registered as `blender` in Claude Code's user config with telemetry off and safe mode on. The add-on is enabled in Blender 5.2. It's a live preview tool; the repo scripts are the source of truth. |

The spec (`docs/M2-SPEC.md`) and goal brief (`docs/M2-GOAL.md`) are final.

## Round 6: play feedback, 2026-09-26 (after Jake's first play session)

Jake: "it plays really fun … it looked really good." His answers to Q30–Q38:

| # | Decision | What it commits us to |
|---|---|---|
| D32 | **Ramp rush is broken; fix all of it (Q30).** | Sprinting with the ramp selected and build held must ramp you up endlessly. Today "ramp started placing behind me." No dead ends, right heights, no slowdown. |
| D33 | **Fortnite building, fully (Q31: "all of those").** | Wall + ramp combos, reliable building while turning and jumping (90s), Fortnite-like reach, **editing**, and the **cone/roof** piece: all now in scope. Rules come from research on real Fortnite building (`docs/research/fortnite-building.md`). |
| D34 | **Guns stay (Q32).** | "The guns made a lot of sense." No weapon changes. Bloom is researched only to confirm we're close. |
| D35 | **Controls (Q33).** | ADS moves to a key (Shift proposed), with auto-sprint on so Shift is free. The trackpad is only for looking, aiming and clicking. Details to confirm: see Round 6 follow-ups. |
| D36 | **Movement stays (Q34).** | Sprinting and jumping are "really good." Slide untested. |
| D37 | **The look is good; make it more beautiful later (Q35).** | "Looked really good and clean … could look a little bit more beautiful." A beauty pass comes after mechanics (D39). |
| D38 | **Bots are their own milestone (Q36).** | The knight fighting back needs real gameplay decisions, so it's out of scope now (Milestone 3). |
| D39 | **Scope and order (Q37, Q38).** | Mechanics fixes go into **this** milestone. Order: **mechanics → design → playability.** |

**Round 6 follow-ups, 2026-09-26:**

| # | Decision | What it commits us to |
|---|---|---|
| D40 | **Hold Shift to aim (Q39).** | ADS is a hold on Shift, not a toggle. V and the two-finger click no longer toggle ADS. |
| D41 | **Always sprint (Q40).** | "If I'm holding W, I'm sprinting," like Fortnite. Sprint speed whenever moving; no sprint key. Slide is C while moving. |
| D42 | **No two-finger clicks (Q41).** | The trackpad only looks, aims and clicks (fire or place). |

**Round 6, part 2, 2026-09-26** (Jake: "rec for all"; based on `docs/research/fortnite-building.md`):

| # | Decision | What it commits us to |
|---|---|---|
| D43 | **Key layout (Q42).** | WASD (always sprint on W), trackpad to look, click to fire, place or select edit tiles, **hold Shift to aim**, Space jump, C crouch/slide, 1/2 guns, **Q/E/F/V wall/ramp/floor/cone**, **G edit**, **R reload or reset edit**, Esc pause. No Ctrl (Ctrl + click is right-click on macOS). |
| D44 | **Fortnite-style editing (Q43).** | G on a piece enters edit mode; click or drag tiles to select; **release confirms**; R resets. No edit delay. The edited piece keeps its HP fraction. Grids: wall 3×3, floor 2×2, ramp 2×2 path, cone 2×2 corners. |
| D45 | **Instant full piece HP (Q44).** | No Fortnite-style HP build-up, which protects "a wall soaks ≥ 1 s of rifle fire". |
| D46 | **Fortnite reach (Q45).** | Build in the 3×3 tiles around you (diagonals included, tunable to 2), from one level down to one level up. This makes double ramps and 90s possible. |

## Round 7: S1 scores and the beauty pass, 2026-09-26

Jake scored the board: T01 2, T02 2, T03 2, T04 2, T05 3, T06 2, T07 3, T08 3, T09 3, T10 2, T11 2, T12 4. The review found the gaps were shading depth, texture, character appeal, effect size and scene density. Jake: "recs are all good".

| # | Decision | What it commits us to |
|---|---|---|
| D47 | **Richer cartoon shading (Q46).** | Soft 3-tone gradients instead of flat 2-tone. Cartoon specular highlights on metal, brass and crystal. Baked ambient occlusion in the models. Outlines about 2× thicker. Punchier color grading (saturation and contrast). |
| D48 | **Painted-looking texture and density (Q47).** | Wood grain, varied brick shades, textured grass with tufts, flowers and bushes, trees framing the arena, clouds under the island. All generated by our own scripts. |
| D49 | **Cosmetic tall hat, robe, big reactions (Q48).** | The knight gets the targets' tall pointed hat, which is cosmetic: it doesn't take hits, like Fortnite cosmetics, and the S5 fit excludes it. A flowing robe replaces the small cape. Big cartoon hit reactions: arms flail, knockback. |
| D50 | **Ornate guns, bigger spells (Q49).** | More ornate brass guns with glowing crystal chambers that spark inside. Spell bursts 2–3× bigger and more saturated. |
| D51 | **Measure while building (Q50).** | The design pass starts now in the background. Jake gives a 20-minute go window soon for the first native performance numbers on the new look. |

## Round 8: closing M2 and planning Milestone 3 "Waves", 2026-09-27

**Context.**
- S1 passed in round 3: every view ≥ 4, mean 4.67.
- Jake played for about 4½ minutes with sound on: "OMG ITS AMAZING! IT looks FANTASTIC. The sound is GREAT! When there is an actual game objective this is gonna be so much fun!"
- Before that session he asked: "Can we skip the shit where I have to be away from my mac".

**Closing M2 (Q51 and the away-from-Mac request):**

| # | Decision | What it commits us to |
|---|---|---|
| D52 | **S8 passes on today's session (Q51).** | The 4½-minute session with sound on and Jake's verdict count as S8. No separate 10-minute session. |
| D53 | **No more away-from-the-Mac runs.** | **S2** is measured from a logged real play session on battery with Low Power Mode on. The game gets a frame-timing log for normal play, so any unplugged session becomes the S2 evidence (same thresholds). The native re-runs of S3–S7 are replaced by: <ul><li>the native results already recorded (go window 1);</li><li>headless tests on the final commit;</li><li>the launch time the game prints on every launch.</li></ul> The 8.7 s launch seen on 2026-09-27 (first launch after a rebuild; earlier launches took 1.1–2.6 s) must be checked. |

**Milestone 3: what the game is (Q52–Q58):**

| # | Decision | What it commits us to |
|---|---|---|
| D54 | **Survive endless waves of knights (Q52).** | Jake: "survive a wave of knights where you get to use building, editing, shooting, etc to defeat them." This replaces the original 1v1 build fight as the first real objective. |
| D55 | **Knights fight back, with no difficulty settings (Q53, Q54).** | The knight is the enemy. The waves are the difficulty: there's no Easy/Medium/Hard menu. Knights that build (walls, ramps) are wanted, but they come with the later knight types (D60). |
| D56 | **Match flow (Q55).** | <ul><li>A fixed rifle + pump loadout and unlimited materials.</li><li>A results screen after each run.</li><li>No loot, unlocks or progression yet.</li></ul> |
| D57 | **Gameplay first; rebinding and menu ride along (Q56).** | "Make the bot and gameplay fun first." Then key rebinding (backlog) and a main menu. A second arena and more weapons wait. |
| D58 | **Endless random waves, like CoD Zombies (Q57).** | Runs are infinite, with some randomness in each wave. The results screen shows your personal best. New maps and guns later make runs fresher. |
| D59 | **Nothing bugged Jake; the background could be more magical (Q58).** | See D64 and D67. |

**Milestone 3: the wave mode (Q59–Q69):**

| # | Decision | What it commits us to |
|---|---|---|
| D60 | **Knight roster, later (Q59).** | A new type unlocks every few waves, and late waves mix all four at random:<ul><li>**Grunt:** runs and shoots bolts.</li><li>**Brute:** bigger and slower; smashes walls.</li><li>**Mage:** lobs spells over walls.</li><li>**Builder knight:** ramps up to you.</li></ul>**M3 starts with the grunt only** (D68). |
| D61 | **One life per run (Q60).** | Death ends the run; the score is the wave reached. Knights sometimes drop shield potions, and you heal a little between waves. |
| D62 | **Unlimited ammo and materials, for now (Q61).** | "Lets just make fighting a basic wave fun before adding new things." Limited materials and ammo, knight drops and a Zombies-style points shop are for later. |
| D63 | **Knights arrive by ship from the station (Q62).** | Ships fly down from the station and drop knights onto the island, "with a good graphic / animation." About a 10 s break between waves to build. Knights can be knocked off the edge into the void. |
| D64 | **A more magical sky (Q63).** | Shooting stars, drifting glowing motes, and a slow shimmer on the galaxy. The castle in the back is bigger and more magical, "more like Harry Potter" (D67). |
| D65 | **Run screens (Q64).** | <ul><li>**HUD:** wave number, knights left, score.</li><li>**Results screen:** wave reached, eliminations, accuracy, headshots, best run next to it, and a "Go again" button.</li></ul> |
| D66 | **The mode is called "Waves" (Q65).** | Real names are picked when the whole game is renamed. |
| D67 | **A bigger, more magical castle, concepts first (Q66).** | Keep the stained-glass heart and add Hogwarts traits: <ul><li>many towers with pointed cone roofs;</li><li>hundreds of warm, candlelit windows;</li><li>floating lanterns;</li><li>a soft golden glow, with sparkles drifting off it.</li></ul>2–3 concept paintings come from Codex first (see the handoff for the command), and Jake picks one before it's built. |
| D68 | **Difficulty curve, grunts only to start (Q67).** | Wave 1 is 3 grunts, and each wave adds about 2. Knights get slightly faster and tougher every wave. An average run lasts 10–15 minutes; a good run passes wave 20. Brutes (wave 3), mages (wave 5) and builder knights (wave 8) come in a later step. |
| D69 | **Short loops (Q68).** | The first chunk is **one basic grunt wave Jake can play**, to check the fun early. Then ship arrivals, the castle and the sky, then rebinding and the menu. Jake play-tests after each chunk. |
| D70 | **M2 leftovers are step 1 of M3 (Q69).** | <ul><li>Add the frame-timing log for normal play (D53).</li><li>Check the 8.7 s launch.</li><li>Close the M2 report.</li></ul>Jake may start M3 from a new session. |

## Round 9: the M3 spec grill, 2026-09-27

Questions only the spec needs and Round 8 doesn't answer. Context from reading the code:
- Combat already gives every character the same loadout and hit resolution, so a grunt carrying the player's rifle would deal 28 damage six times a second.
- D68's count (wave 1 = 3, about +2 per wave) means wave 20 alone is 41 grunts, and passing it takes about 440 kills. Kills must be quick for a 10–15-minute average run.
- D63 (knock knights into the void) conflicts with D22's barrier (no falling). Q77 resolves it.

**Batch 1: the grunt (Q70–Q77).** Jake: "rec is good" on every question.

| # | Decision | What it commits us to |
|---|---|---|
| D71 | **Grunt attack: a slow, visible spell orb (Q70).** | The grunt fires an orange-red orb (about 30 m/s, 12 damage, one shot every ~1.5 s) after a 0.4 s wand-glow wind-up. The orb is a real projectile, so you can dodge it, and builds block it (it chips the piece). Incoming fire becomes the reason to build. The player's guns stay hitscan (D3). The grunt carries a small crystal wand, built by a new Blender script. |
| D72 | **Movement: mid-range and spread out (Q71).** | Grunts close to about 10–18 m, then strafe and shoot. Each picks its own spot, so they spread around the player instead of forming a conga line. |
| D73 | **Against builds: path and shoot, never smash (Q72).** | Grunts always know where the player is (horde rules, no stealth). They path over the build grid, so they walk up ramps and floors, and they shoot whatever piece stands between them and the player. No smashing or building: that's the brute's and the builder knight's job later (D60). |
| D74 | **Wave-1 grunt: 100 HP, no shield (Q73).** | That's 4 rifle body shots, 3 headshots or one close pump. Quick kills, as the ~440-kill run needs. |
| D75 | **Scaling per wave (Q74).** | Each wave adds +4% HP, +1.5% move speed and +2% fire rate, and aim improves slightly. Speed stays below the player's sprint. Scaling stops at wave 25 (a grunt then has about 196 HP). |
| D76 | **Aim and fairness (Q75).** | <ul><li>Wave 1 aims like bot-ai.md's "Normal" tier: 0.33 s reaction after first sight, leading the player's movement with some error. Aim creeps toward "Hard" by wave 15.</li><li>**At most 3 grunts shoot at once** (attack tokens); the others reposition.</li><li>No shot without line of sight.</li><li>Every hit on the player shows a red damage-direction arrow on the HUD. A shot from off-screen plays a warning sound first.</li></ul> |
| D77 | **At most 8 knights alive (Q76).** | The rest wait for the next ship as slots open. Eight knights fit the M2 triangle budget. |
| D78 | **Pump knockback and the void (Q77).** | <ul><li>The pump knocks knights back for real, up to about 4 m at point blank (distance-scaled). The player takes no knockback.</li><li>The barrier stops the player but lets knights through outward.</li><li>A knight past the edge falls with a cartoon yelp, and it counts as an elimination with bonus points.</li><li>The player can never fall.</li></ul> |

**Batch 2: waves, score, ships and runs (Q78–Q84).** Jake: "rec is good for all".

| # | Decision | What it commits us to |
|---|---|---|
| D79 | **The break between waves (Q78).** | A 10 s countdown on the HUD. Enter skips it. At the start of each break the player's shield refills +50; health comes back only from potions. |
| D80 | **Shield potions (Q79).** | 10% of knights drop one. It bobs and glows cyan; walking over it gives +25 shield, spilling into health when the shield is full. It vanishes after 20 s. |
| D81 | **Score and personal best (Q80).** | <ul><li>100 per knight, +50 for a headshot kill, +150 for a void knock-off (D78), and +250 × wave number for each wave cleared.</li><li>The **personal best is the wave reached**, with score as the tiebreak.</li><li>The best is saved locally in the git-ignored `userdata/`.</li></ul> |
| D82 | **Ship arrivals (Q81).** | <ul><li>A bigger version of the M2 ship peels off from the station and swoops down in about 4 s to hover over one of 4–6 drop points at the island edge.</li><li>A glowing circle marks the landing spot 2 s ahead.</li><li>Knights slide down a beam of light 1–3 at a time and land with a sparkle poof; the ship flies back up.</li><li>Knights can't be shot until they land (about 1 s).</li></ul> |
| D83 | **Seeded randomness (Q82).** | Each run has a seed that drives drop points, ship timing, how knights split between ships, and ±10% speed per knight. The results screen shows the seed, so a run can be replayed. |
| D84 | **Death and results (Q83).** | <ul><li>About 1 s of slow motion, the view drops to the grass, and the knights do a goofy victory hop.</li><li>The results screen (D65) shows the wave reached, eliminations, accuracy, headshots, run time and the best run beside them, with "NEW BEST!" when it is one.</li><li>"Go again" restarts instantly with no reload, and Quit goes to the menu.</li><li>Quitting a run from the pause menu also goes to results.</li></ul> |
| D85 | **Practice mode stays (Q84).** | The M2 sandbox with the training dummy becomes **Practice** on the main menu. Waves is the default. The M1/M2 tests and scenarios (TTK, gallery, fx_check, perf) keep running against Practice, which keeps the M2 gates green. |

**Batch 3: gates, M2 leftovers, order and menus (Q85–Q92).** Jake: "rec is good for all".

| # | Decision | What it commits us to |
|---|---|---|
| D86 | **Frame-timing log in normal play (Q85).** | <ul><li>Always on, and cheap.</li><li>Every session writes a frame CSV plus a summary to the git-ignored `userdata/sessions/`.</li><li>It records power and Low Power Mode every 30 s, the preset, time the window was covered, and time actually in play (menus and pause excluded).</li><li>At quit, the game prints a one-line S2 verdict.</li><li>**Any session with ≥ 5 min of play, fully unplugged, Low Power Mode on the whole time and the window visible counts as S2**, with the same thresholds.</li><li>The last 30 sessions are kept.</li></ul> |
| D87 | **Cold and warm launches (Q86).** | <ul><li>The first launch after a rebuild compiles Metal shaders from scratch. It's logged as **cold** and isn't gated, but it's investigated and fixed if it's cheap.</li><li>The launch gate stays at **warm launches < 5 s**, taken from the time the game prints in Jake's normal sessions: 3 in a row.</li><li>With the main menu, "ready" means the menu can be clicked, and Play → arena must take < 1 s.</li></ul> |
| D88 | **Performance with a full wave (Q87).** | <ul><li>S2's bar (mean 16.4–17.0 ms, 0 frames > 25 ms, ≥ 99% < 18 ms) must hold in a logged battery session that reaches **wave 6 or higher**, so 8 knights are alive for long stretches.</li><li>Headless checks:<ul><li>8 knights, 8 orbs each and ship effects stay within the triangle, draw and particle budgets;</li><li>5 simulated minutes of waves allocate nothing per event;</li><li>AI thinks at 8 Hz with a per-tick work cap.</li></ul></li></ul> |
| D89 | **How fun is judged (Q88).** | <ul><li>After each chunk Jake plays and answers four quick questions: fun 1–5, too easy / right / too hard, any death that felt unfair, and what's off.</li><li>**The final gate:** fun ≥ 4 and no unfair deaths on the last chunk.</li><li>Headless fairness tests:<ul><li>no damage without line of sight;</li><li>no first shot sooner than the reaction time;</li><li>at most 3 shooters at once;</li><li>no knight stuck > 3 s.</li></ul></li><li>Each run appends a line to `userdata/runs.jsonl` (wave reached, run time, cause of death), used to tune toward D68's 10–15-minute average run. That average is a tuning target, not a gate.</li></ul> |
| D90 | **Chunk order and play-tests (Q89).** | <ol start="0"><li>**M2 leftovers:** the session log, the launch check, closing the M2 report. No play-test; Jake's next normal session feeds S2.</li><li>**One grunt wave:** 3 grunts poof in at the edge, orbs, dodge and build, death, and a plain results line. **Play-test 1.**</li><li>**Endless waves:** scaling, the break, potions, score, the HUD, the results screen with the best run, and Go again. **Play-test 2.**</li><li>**Ship arrivals, pump knockback and the void.** **Play-test 3.**</li><li>**The castle** (Jake's concept pick) **and the magical sky.** It runs as a background art slice from chunk 1, since it touches no gameplay files. **Play-test 4, a look check.**</li><li>**The main menu and key rebinding.** **Play-test 5, the final verdict.**</li></ol> |
| D91 | **Castle concepts now (Q90).** | Three images, generated in this session one at a time with Codex, using the M2 station render as the reference. Jake picks one before the goal launches, so the spec names the target. |
| D92 | **Main menu (Q91).** | <ul><li>The game opens on the PIECED logo over the live island and sky, with the camera slowly orbiting.</li><li>Buttons: **Waves** (showing the best run), **Practice**, **Settings** and **Quit**, in the pause menu's cartoon button style.</li><li>Esc in a run still pauses.</li></ul> |
| D93 | **Key rebinding (Q92).** | <ul><li>A Controls page in Settings.</li><li>Every `PlayerIntent` action can bind to a key or the click; trackpad look is fixed.</li><li>Click an action, then press a key. A conflict swaps the two bindings. "Reset to defaults" is there.</li><li>Bindings save with the other settings.</li><li>The HUD and menus show the bound keys (the break's "Enter to start", edit hints and so on).</li></ul> |

## Handoff: where to pick up

- **State (2026-09-27):**
  - M2 main is `7f8d962` or later; S1 passes (round 3 mean 4.67), and S8 passes per D52.
  - Remaining for M2: S2 (from a logged battery play session, D53), the launch check, and closing the report (`docs/evidence/M2-report.md`).
  - `pieced-play` (`~/Library/Caches/pieced-target/pieced-play`) is built from `bb9c33a`.
- **Next:** turn Round 8 (D52–D70) into `docs/M3-SPEC.md` and `docs/M3-GOAL.md`, with gates and short play-test loops (D69). Then Jake launches it, maybe from a new session.
- **Images (castle concepts, D67):** use Codex CLI, one image at a time:
  ```
  codex exec --skip-git-repo-check --ephemeral -m gpt-5.5 -s workspace-write -C docs/design/concepts [-i ref.png --] "<prompt>" < /dev/null
  ```
  - The `--` after `-i` is required, because `-i` takes several values.
  - Stdin **must** be `/dev/null`, or the job hangs.
  - If Codex says it saved an image that isn't there, recover it from `~/.codex/generated_images/<session>/`.
  - Generated images stay local and are git-ignored.
- **Environment gotchas:**
  - `source scripts/env.sh`: builds run at low priority, and the shared cache is `~/Library/Caches/pieced-target`. `~/Documents` is iCloud-synced and over quota.
  - All worktrees share one build cache. Before trusting a render, check that the cargo output shows `Compiling pieced (<your path>)`.
  - Never use `pgrep` wait loops. Run `scripts/prune-target.sh` if disk drops below 6 GB.
  - Never run full-screen or timing work while Jake is on the Mac, and D53 removes the need for it.
