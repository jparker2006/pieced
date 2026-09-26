# Pieced — Milestone 2 "Spellbound" spec

**Status:** AGREED with Jake on 2026-09-25. It's built from the design grill (`docs/design/DESIGN-GRILL.md`, decisions D1–D31). Jake approved all 12 targets (T01–T12) and took the recommendation on Q27–Q29. This is the build contract for Milestone 2.

**Companions:**
- `docs/SPEC.md`: the Milestone 1 contract. Its gameplay stays in force.
- `docs/research/look-stack.md`: verified Bevy, Blender and font facts.
- `docs/design/concepts/`: concept and target images. Local only, git-ignored.

**Renumbering:** bots, previously "Milestone 2", become Milestone 3.

## Problem Statement

Milestone 1 made Pieced play like "Call of Duty shooting plus Fortnite building" on Jake's MacBook Air trackpad, but its look is generic: a tasteful low-poly arena at golden hour. Jake wants a world he's excited to open in class:
- cartoon up close ("Family Guy / Looney Tunes");
- magical in the distance ("Harry Potter × Star Wars");
- guns that look like Fortnite guns but sick, and fire like spells.

He picked the direction from Codex concept images. It's a flat TV-cartoon close-up with thin ink outlines, a goofy armored knight-wizard enemy, brass-and-crystal guns held in white cartoon gloves, blue starburst spell bolts, brick walls with wooden floors and ramps, and a grassy floating island under a galaxy with a stained-glass cathedral space station (base image: `concepts/R4-M1-brick-walls-wood-floors.png`).

The risks:
- **Looks:** can a real-time game on a fanless laptop get close to AI-painted targets?
- **Performance:** can it do that while holding a steady 60 fps on battery with Low Power Mode on? Milestone 1 has **not** yet passed that bar (G2) on its simpler art.

## Solution

Milestone 2 restyles Pieced end to end **without changing how it plays**. Every feel number, damage number, hitbox, grid rule and control from Milestone 1 stays. The one exception, approved by Jake, is a handful of solid cartoon rocks and stumps in the arena, as in the targets. What changes:

- **Rendering:**
  - a custom toon material (flat two-tone shading, a rim light, no PBR);
  - ink outlines on near objects only;
  - no shadow maps; cartoon blob shadows instead;
  - a soft, glowing, outline-free far layer.
- **Models:** every model is built by Blender Python scripts in the repo and exported to glTF:
  - the rifle, the pump and the gloves;
  - the knight;
  - brick and plank pieces;
  - trees, rocks and stumps;
  - the station, ships, the far islands and the ringed planet.
- **Spells:**
  - Rifle: blue starburst bolts.
  - Pump: a violet-and-gold spark fan.
  - Headshots flash gold.
  - Shields shimmer and shatter in cyan.
  - Eliminations end in a cartoon poof, and the knight's hat drops and spins on the grass.

  Hits stay hitscan. Spells change only what you see.
- **A living sky:** a galaxy generated in code that slowly turns, ships gliding between the station's spires, stained glass that pulses, and islands that bob.
- **HUD, menu and audio:**
  - cartoon HUD frames with crystal icons, in the same layout;
  - a pause menu with a PIECED logo;
  - a new synthesized sound bank: zaps, bonks, clunks and glassy crashes.
- **A target board:** 12 fixed gallery views, each matched to a target image. Jake scores every view.

Done means all four hold:
- every gallery view scores **≥ 4/5** from Jake against its target;
- the Milestone 1 performance bar holds with the full look on battery in Low Power Mode;
- the far view visibly moves;
- Jake's 10-minute play session says the guns, spells and sounds feel sick.

## User Stories

**The overall look**

1. As Jake, I want near things to look like a flat TV cartoon, with clean thin outlines and two-tone shading, so that the game looks like the concept I picked.
2. As Jake, I want far things to be soft, glowing and magical, with no outlines, so that the world feels huge behind the cartoon foreground.
3. As Jake, I want the colors in the game to match the target images, flat and saturated rather than washed out, so that it looks like the pictures.
4. As Jake, I want no jaggies, shimmer, z-fighting or seams, so that the cartoon lines read clean.
5. As Jake, I want interiors of my boxes to stay readable, never pitch dark, so that fights inside builds are clear.

**Guns and hands**

6. As Jake, I want to see white four-finger cartoon gloves holding my gun, so that I feel like a cartoon wizard.
7. As Jake, I want the rifle to look like the brass-and-crystal rifle in the base image, with a blue crystal glowing in a glass chamber, so that it looks sick.
8. As Jake, I want the pump to be stubby, with a flared brass bell muzzle, a wooden pump grip and a violet crystal in gold rings, so that it reads as a different gun at a glance.
9. As Jake, I want each gun's crystal to fade as its magazine empties, so that I can read my ammo from the gun itself.
10. As Jake, I want reloading to pop the dim crystal out and slot a glowing one in, so that reloads are fun to watch.
11. As Jake, I want the pump's gold rings to spin when it racks, so that the pump feels chunky.
12. As Jake, I want every shot to kick the gun with a squash-and-stretch bounce, so that firing feels cartoon-punchy.

**Spells and hits**

13. As Jake, I want the rifle to fire a glowing blue starburst bolt with a sparkle trail, so that it shoots like a spell.
14. As Jake, I want the pump to fire a wide violet-and-gold spark fan, so that close-range blasts look huge.
15. As Jake, I want a bright impact burst where every shot lands, on the same frame as the hitmarker, so that hits feel instant.
16. As Jake, I want a headshot to flash gold, bounce the knight's hat and pop a big gold number, so that headshots feel special.
17. As Jake, I want hits on the knight's shield to shimmer cyan, and the shield to shatter into cyan glass with stars circling his head when it breaks, so that I know when he's cracked.
18. As Jake, I want an elimination to end in a cartoon poof, with his hat dropping out and spinning on the grass, so that kills are funny and satisfying.
19. As Jake, I want damage numbers in a cartoon font (white on health, cyan on shield, gold on headshots), so that they match the look.
20. As Jake, I want bolts that miss to fizzle with a small puff where they land, so that misses still look magical.

**The knight**

21. As Jake, I want the enemy to be the goofy knight-wizard from the base image, so that he's funny and readable.
   - big steel bucket helmet with cartoon eyes in the visor;
   - short floppy purple hat with a gold star;
   - oversized gauntlets and boots;
   - small body;
   - purple trim and cape.
22. As Jake, I want his eyes to blink, go wide when hit and turn to X's when he's eliminated, so that he has personality.
23. As Jake, I want him to bob, run, jump and land with bouncy squash and stretch, so that he moves like a cartoon.
24. As Jake, I want him to wobble when hit, so that every hit visibly lands.
25. As Jake, I want his visible body to match his hitboxes exactly, including his hat, so that shots I see land do land.
26. As Jake, I want him to pop back in with a sparkle when he respawns, so that respawns are clear.
27. As Jake, I want him to stand out against the purple sky, so that I never lose him.

**Building**

28. As Jake, I want walls to be chunky cartoon brick and floors and ramps to be warped wooden planks with big nails, so that I can tell pieces apart at a glance.
29. As Jake, I want new pieces to pop in with a quick squash, so that building feels snappy.
30. As Jake, I want damaged pieces to crack cartoonishly at two stages, with bricks missing at the second, so that I can see how hurt a piece is.
31. As Jake, I want broken pieces to burst into chunky bricks or planks and a dust poof, so that breaking feels great.
32. As Jake, I want the build ghost to glow blue when valid and red when invalid, so that I know what will happen.
33. As Jake, I want a faint glowing build grid on the grass, so that placing pieces is easy.

**The island and the sky**

34. As Jake, I want to fight on a grassy cartoon floating island with puffy trees, rocks and stumps, so that the arena feels like a place.
35. As Jake, I want the island to end in rounded cliffs dropping into space, behind a shimmering magic barrier, so that the edge is clear and I can't fall off.
36. As Jake, I want a stained-glass cathedral space station on one side of the sky and a swirling galaxy opposite, so that the sky is magical.
37. As Jake, I want the galaxy to slowly turn, ships to glide between the spires, the stained glass to pulse and small waterfall islands to bob, so that the far view is alive.
38. As Jake, I want a ringed planet low on the horizon, so that the sky has scale.

**HUD, menu and sound**

39. As Jake, I want the HUD in the same places as now but in cartoon frames with crystal icons and a cartoon font, so that it matches without me relearning it.
40. As Jake, I want hotbar icons that show the actual rifle, pump, brick wall, plank ramp and plank floor, so that slots are obvious.
41. As Jake, I want the pause menu to show a PIECED logo in brass-and-crystal letters with a tiny wizard hat, so that the game feels finished.
42. As Jake, I want spells to zap and shimmer, hits to bonk and sparkle, walls to clunk, planks to thock and shields to shatter like glass, so that the sound matches the look.
43. As Jake, I want every sound to stay optional, with every hit still visible on screen, so that I can play muted in class.

**Speed**

44. As Jake, I want the new look to hold a steady 60 fps on battery with Low Power Mode on, so that it plays as smoothly as it looks.
45. As Jake, I want it to still launch into the arena in under 5 seconds, so that I can play the moment I'm bored.
46. As Jake, I want shooting and building to feel exactly as responsive as before, so that the new look costs no feel.

**Process**

47. As Jake, I want to score each gallery view against its target image on one page, so that "done" is my call and is clear.
48. As Jake, I want the Mac left alone while I'm using it, with heavy or full-screen runs only when I say "go", so that the build never gets in the way of class.
49. As Jake, I want every model to be regenerable from scripts in the repo, so that the art can keep improving without losing anything.
50. As Jake, I want to be able to watch models come together live in Blender when I feel like it, so that making the art is fun too.

## Implementation Decisions

### Scope rule: look and sound only

- **Unchanged from Milestone 1:**
  - every gameplay number in `docs/SPEC.md` (movement, weapons, pieces, the TTK contract);
  - the hitbox constants in `player.rs` (body capsule r 0.33 m from 0.05 to 1.45 m; head sphere r 0.20 m centred at 1.62 m);
  - the grid (4 m cells, 3 m levels, 12 × 12 arena, 6 levels);
  - the initial cover layout (`building::initial_cover`), spawns, controls and the `PlayerIntent` seam.
- **Models are made to fit the hitboxes, never the reverse.** Any hitbox change is a gameplay change and needs Jake.
- **One approved gameplay change (Q27, D29): solid props in the arena.**
  - **What:** 6–8 cartoon rocks (1.0–1.4 m tall, crouch cover) and stumps (at most 0.7 m, jumpable) stand inside the 48 m square, as in the targets.
  - **Collision:** static colliders on the World layer, fixed positions listed in `arena/mod.rs` (headless). They block movement and shots.
  - **Building:** pieces may intersect props, like building through terrain in Fortnite. Placement ignores props.
  - **Placement rules:** no prop within 3 m of either spawn or an initial cover piece, and none in the dummy's strafe zone.
  - **Trees** stay on the island margin outside the barrier.
  - **Otherwise** the arena is unchanged: a flat 48 m square with an invisible boundary.

### Toolchain and dependencies

- Unchanged: Rust 1.98.1 via `source scripts/env.sh`, bevy =0.19.1, avian3d =0.7.0, bevy_egui =0.42.0, Metal, pipelined rendering off, Fifo present, and the build cache in `~/Library/Caches/pieced-target`.
- **One new crate: `bevy_mod_outline =0.13.0`** (supports bevy 0.19; see `look-stack.md`). It extrudes vertices with a screen-space pixel width, draws in its own pass and generates outline normals. It's kept only if it measures cheaper than, or no worse than, a hand-rolled inverted hull (an inflated mesh copy drawn with `StandardMaterial { unlit: true, cull_mode: Some(Face::Front) }`). The loser is removed. The toon shader is our own WGSL.
- **Blender 5.2.2 LTS** (`/Applications/Blender.app`, CLI `blender`) runs headless for the asset build. It's only needed to *regenerate* art: the exported `.glb` files are committed, so the game builds without Blender.

### Rendering

1. **Toon material** (our own `Material`, not `StandardMaterial`):
   - Two bands: lit and shadow, split by a hard step on N·L from one key light.
   - The shadow band is tinted cool violet, never black.
   - A thin rim light.
   - An emissive channel for crystals, glass, spells and waterfalls.
   - Color comes from the shared palette (`art/palette.json`, sampled from the targets) as **per-face vertex colors** (glTF `COLOR_0`). This was decided during Phase 1 instead of a palette texture, because it's simpler and needs no UVs. Small detail textures (mortar lines, plank grain, nail heads) generated by our own scripts can be added on top.
   - The same material serves the world, pieces, props, the knight and the viewmodel. Pieces of one kind share a mesh and a material, so they batch.
2. **Lighting:**
   - A warm key light from a small star near the station, and a teal fill from the galaxy side.
   - Light direction and colors come from one global resource, so every toon surface agrees.
   - **No shadow maps.** Soft cartoon blob shadows (decal quads) sit under the knight, trees, rocks and stumps.
   - Interiors stay readable through the shadow-band floor color: no pitch-dark areas.
3. **Outlines** (inverted hull):
   - Each outlined mesh gets a second draw with front-face culling: `bevy_mod_outline`, or the hand-rolled hull, whichever wins the measurement.
   - Vertices are pushed out along a **smoothed normal** averaged by vertex *position*, which keeps hard-edged low-poly meshes from cracking. Bevy's `compute_smooth_normals` does not merge split vertices, so use our own averaging or `bevy_mod_outline`'s outline-normal generation.
   - Width is about 1.5 px at the reference resolution and stays constant in screen space. It fades out between 25 and 40 m.
   - The ink color is a dark, desaturated version of each object's base color, not pure black.
   - **Outlined:** viewmodel guns and gloves (in the viewmodel pass), the knight, pieces, trees, rocks and stumps.
   - **Not outlined:** the ground, grass tufts, the sky, the station, ships, far islands, the planet and effects.
4. **The far layer:**
   - Objects beyond about 150 m use an unlit "far" material with haze toward the galaxy colors.
   - Glow comes from **additive halo billboards** (crystals, stained glass, bolts, waterfalls), not bloom. Bevy's `Bloom` needs `Hdr` (double the bandwidth, 16 small passes, and the two-camera risk), so it's out of both presets unless Jake asks.
   - The sky is a `Skybox` whose `rotation` is updated every frame, with brightness set explicitly (the default of 0 renders black).
5. **Color pipeline:**
   - Tonemapping is off (`Tonemapping::None`), or neutral if the gallery comparison says so, so authored palette colors land on screen as authored and match the targets.
   - Non-HDR cameras tone-map inside the shader, so the custom toon shader must apply the same mapping as every other material.
   - Both cameras stay non-HDR, with the same MSAA setting. Mixing HDR and non-HDR between the world and viewmodel cameras can overwrite instead of blend.
6. **Anti-aliasing** is chosen by measurement: MSAA 4×, 2× or off plus FXAA/SMAA, on the world and viewmodel passes. The viewmodel pass must not double-resolve or write back MSAA (the prime suspect for Milestone 1's G2 failure).
7. **Quality presets:** keep Battery (default) and Plugged in. Battery must pass the performance bar. **Perf knobs** extend to cover:
   - outlines;
   - the far layer;
   - halos;
   - the particle cap;
   - sky rotation;
   - MSAA level;
   - render scale.

   This lets the cost of each feature be measured on its own.

### Asset pipeline (Blender, scripted)

- **Scripts:**
  - `art/blender/` holds one Python script per asset family:
    - `guns.py`, `gloves.py`, `knight.py`;
    - `pieces.py` (brick wall, plank floor and ramp, crack stages and debris chunks);
    - `props.py` (trees, rocks, stumps, grass tufts, flowers);
    - `island.py` (the cliff skirt beyond the arena);
    - `far.py` (station, ships, far islands with waterfalls, ringed planet);
    - `icons.py` (hotbar icon renders), `logo.py` (the PIECED logo render).
  - Shared helpers set up the palette UVs, bevels and naming.
- **Build command:** `scripts/build-art.sh [asset…]` runs `blender -b --factory-startup --python-exit-code 1 -P art/blender/build.py -- <assets>`. The glTF export uses `export_vertex_color='ACTIVE'`, `export_apply=True` and `export_extras=True`, with Y-up. It writes:
  - `assets/models/<name>.glb`, committed;
  - `assets/models/<name>.json` sidecars, committed. These hold named attach points (muzzle, crystal socket, eye sockets, hat pivot) and **per-part bounds** used by tests;
  - `art/previews/<name>.png` preview renders, git-ignored, for review.
- **Orientation:** glTF forward is +Z and Bevy's is −Z, so either the models are authored facing +Z or the loader flips them. A test pins the rifle's muzzle pointing forward.
- **Named parts:** models use named nodes, e.g. `Helmet`, `Hat`, `EyeL`, `GauntletL`, `BootR`, `Cape`, `Torso` for the knight, and `Crystal`, `Rings`, `PumpGrip`, `Muzzle` for the guns. Rust finds them after spawn and animates them procedurally.
- **Triangle budgets** (worst case, before outlines):

  | Asset | Triangles |
  |---|---|
  | Gun viewmodel (each) | ≤ 6k |
  | Gloves | ≤ 2k |
  | Knight | ≤ 8k |
  | Wall | ≤ 600 |
  | Floor or ramp | ≤ 400 |
  | Tree | ≤ 1.5k |
  | Rock or stump | ≤ 300 |
  | Station | ≤ 25k |
  | Far island (each) | ≤ 1.5k |
  | Ship | ≤ 500 |

  Brick and plank silhouettes come from geometry at the edges (bumpy brick outlines, bent plank ends). Interior detail comes from textures.
- **Original assets only:**
  - Every model, texture, palette and sound is generated by project scripts.
  - The one exception is a single openly licensed cartoon display font in `assets/fonts/`, with its license file: **Luckiest Guy** (Apache 2.0) or **Lilita One** (OFL). Pick one from a HUD mockup.
  - `assets/ASSETS.md` lists every file in `assets/` and its source script or license.
  - No external asset services, including Blender MCP's asset-download features, AI 3D generators and CC0 packs, unless Jake approves.
- **Blender MCP** (installed 2026-09-25, D31) is a live, watchable preview tool only:
  - What it builds must be ported into the repo scripts before it counts.
  - It opens a Blender window, so it runs only when Jake wants to watch or has said "go".
- **Loading:**
  - All models, the palette and icons load during `Boot`. glTF scenes spawn through `WorldAssetRoot` and signal readiness with `WorldInstanceReady` (0.19 names).
  - Launch-to-controllable must stay under 5 s.
  - The galaxy cubemap is generated at load within a budget (about 300 ms), or cached on disk after first launch.
- **Pipeline warm-up (required for S2):**
  - On macOS, Bevy compiles each new material and mesh-layout pipeline synchronously the first time it's drawn, which causes a stutter.
  - During `Boot`, draw every combination once behind the loading screen: each effect type, crack stage, eye state, ghost color and the barrier.
  - Result: the first bolt, shatter or poof never drops a frame.

### The areas, in detail

1. **Guns and gloves** (viewmodel):
   - **Rifle and pump:** shaped as in D21 and the base image.
   - **Crystal glow:** emissive = 0.25 + 0.75 × (rounds in magazine ÷ magazine size).
   - **Rifle reload (2.0 s):** the crystal pops up and spins away, a fresh one slides in, and its glow ramps up at the moment the reload completes.
   - **Pump reload:** each 0.5 s shell is a violet crystal shard pushed into the rings. The rings spin on the rack.
   - **Firing kick:** a squash-and-stretch scale spring along the barrel, on top of the existing recoil springs.
   - **ADS:** the gun centers on the existing ADS pose, with chunky brass sights.
   - Gloves have four fingers and a cuff, and grip each gun at named attach points.
2. **Spells and effects:** these replace the Milestone 1 tracers, sparks and bursts, with the same pooled system and caps.
   - **Rifle bolt:** a star-shaped head billboard plus a sparkle ribbon. It **reaches its hit point within 2 rendered frames**. Impact effects, the hitmarker and the damage number spawn **on the hit frame**, which keeps Milestone 1's same-frame feedback rule (G5).
   - **Misses:** a small fizzle puff where the bolt lands.
   - **Pump:** 10 sparks along the real pellet paths, plus a fan-shaped flash at the muzzle.
   - **Impacts by type:**
     - Body: a white-blue starburst.
     - Head: a gold flash, the hat bounces, and a larger gold number.
     - Shield: cyan hex shimmer.
     - Shield break: cyan glass shards, and stars circle the helmet for 1.0 s.
     - Piece: brick chips or wood splinters.
   - **Elimination:** a puffy poof cloud with sparkles. The hat becomes a prop that drops, spins, settles on the grass and stays until respawn.
3. **The knight** (replaces the dummy's figure; the dummy's behavior is unchanged):
   - **Parts:** Helmet (with visor and eye sockets), Hat, Torso (with trim), Cape, GauntletL/R and BootL/R.
   - **Hitbox fit:**
     - The body parts' bounds lie inside the body capsule, and the Helmet and Hat bounds inside the head sphere, each with **≤ 5 cm tolerance**. The hat stays short and floppy.
     - The silhouette also fills the hitboxes, so the capsule never sticks out more than about 10 cm past the model.
     - A headless test checks both, from the sidecar bounds.
   - **Eyes:** open, blink (every 2–5 s at random), wide (0.3 s after a hit) and X (on elimination). Each is a mesh or material swap.
   - **Procedural animation** driven by the dummy's velocity, grounded state and events:
     - an idle bob;
     - a run cycle (boots swing, gauntlets pump, torso bobs, cape sways);
     - squash on jump takeoff and on landing;
     - a hit-wobble spring;
     - a respawn pop-in (scale overshoot) with a sparkle.
   - **Readability:** outline plus a warm rim light, so he reads against the purple sky. He must stay distinguishable in the greyscale copy of every gallery view.
4. **Building:**
   - **Wall:** a brick slab with bumpy brick edges; mortar lines come from the detail texture.
   - **Floor and ramp:** warped planks with nail heads and slightly bent ends.
   - **Crack stages:** at 66% HP, cartoon cracks. At 33%, bigger cracks and missing bricks or split planks.
   - **Breaking:** chunky brick or plank debris plus a dust poof, within the existing effect caps.
   - **Placing:** a 0.12 s squash pop.
   - **Ghost:** translucent glowing blue when valid, red when invalid.
   - **Build grid:** faint glowing lines drawn by the ground shader in world space. No geometry.
5. **The island:**
   - The playable 48 m square becomes the top of a grassy island, with low, non-colliding grass tufts, flowers and pebbles, plus the solid rocks and stumps (D29).
   - Beyond the arena bounds, a margin of island carries the puffy trees, big rocks and stumps, then rounded cliffs drop into space.
   - A shimmering, translucent rune barrier stands on the arena boundary. It fades in within about 4 m and brightens where the player touches it.
   - The initial cover pieces render as brick and plank pieces, as in T01.
6. **The sky and far view:**
   - **Galaxy:** a seeded procedural cubemap (spiral arms, star field, nebula clouds) that rotates once every 10 minutes.
   - **Station:** fixed so that it's visible from spawn, to the front-right as in T01.
   - **Stained glass:** emissive pulses on a 4–6 s period.
   - **Ringed planet:** low on the horizon.
   - **Far islands:** 4–6 islands with waterfalls (a scrolling emissive strip plus a mist halo). They bob 1–3 m over 6–10 s, out of phase with each other.
   - **Ships:** 3–5 on looping splines between the spires. Each crosses the view in 10–20 s.
   - **Motion systems** are pure transform and parameter updates in a module that runs headless, so tests can check them.
7. **HUD and menu:**
   - The layout is unchanged from `hud/layout.rs`. The restyle adds:
     - rounded cartoon frames with ink borders;
     - a cyan shield bar with a crystal icon;
     - a green health bar with a heart icon;
     - ammo with a crystal icon;
     - hotbar icons rendered by `icons.py`;
     - the chosen open-license font.
   - The pause menu keeps Resume, Settings and Quit, under a PIECED logo rendered by `logo.py`.
8. **Audio:** a new bank in `audio/synth.rs`, all synthesized:
   - rifle zap (bright sweep plus sparkle);
   - pump whoomp-zap (low body plus chime);
   - body hit: bonk plus sparkle;
   - headshot: bonk plus ding;
   - shield hit: glassy tick;
   - shield break: glass crash plus chime;
   - reload: crystal clink and hum;
   - pump rack: ring whirr;
   - wall: clunk;
   - floor and ramp: wooden thock;
   - piece break: crumble;
   - elimination: poof plus a short slide whistle;
   - soft grass footsteps.

### The target board and gallery

- **Targets:** `docs/design/concepts/T01`–`T12`, local only.

  | ID | View |
  |---|---|
  | T01 | Spawn vista |
  | T02 | Rifle idle |
  | T03 | Rifle bolt in flight |
  | T04 | Pump spark fan |
  | T05 | Knight body hit at 10 m |
  | T06 | Headshot |
  | T07 | Shield break |
  | T08 | Elimination poof |
  | T09 | 1×1 fort with ghost |
  | T10 | Looking up at the station |
  | T11 | Island edge and barrier |
  | T12 | Pause menu |
- **Gallery scenario:** it replaces Milestone 1's 8 views with these 12, each scripted:
  - through intents, `teleport`, `set_look` and a scenario-only hook that freezes the knight's pose and effect timing at the right moment;
  - each writes a PNG and a greyscale copy.
- **Board page:** a local HTML page (`evidence/m2-board/index.html`, git-ignored) shows each target beside its game view, with a 1–5 score and a notes box for each. It can also be published as a **private** Artifact so Jake can score from his phone.
- **Committed record:** scores and notes go in the report. Game screenshots go in `docs/evidence/m2/`. Targets are never committed.

### Performance and latency contract (unchanged bar)

- **Conditions:** release build, **on battery with Low Power Mode on**, window visible, Battery preset, the full new look.
- **Bar:** the 5-minute `perf` scenario (after the first 10 s) must hold:
  - mean frame interval 16.4–17.0 ms;
  - **0 frames > 25 ms**;
  - **≥ 99% < 18 ms**.
- **Launch:** under 5 s, warm, 3 times in a row.
- **Latency:** input-to-frame median ≤ 33 ms (Milestone 1's G3, never yet measured as passing).
- **Baseline first:** before any new art, measure Milestone 1's current art against the bar and break down each feature's cost with the knobs. Look-independent fixes (viewmodel MSAA, render scale, present mode) land then.
- **Proxy runs:** plugged-in runs with Low Power Mode on are allowed while iterating. The gate run is on battery.

### Working on Jake's Mac

- **Jake's rule:** full-screen scenario runs, timing runs and Blender windows (including Blender MCP) happen only when Jake has said "go" for that window of time.
- Timing runs are also invalid when the window is covered.
- **Code builds and headless tests may run while Jake uses the Mac** (Q28, D30), at low priority:
  - `scripts/env.sh` runs cargo under `taskpolicy -c utility` and `nice -n 10`, with `CARGO_BUILD_JOBS=6`;
  - at most two builds run at once across all worktrees;
  - `PIECED_FULL_SPEED=1` lifts this during a "go" window;
  - never time a game started through `cargo run`, because it inherits the low priority.
- Codex image generation (network only) is always fine.

## Testing Decisions

- **Seams:** the same as Milestone 1: headless simulation (primary), native scenarios (evidence), and Jake (feel and scoring). Tests assert behavior a player would notice, deterministically.
- **New headless tests:**
  - **Hitbox fit:** every knight part's sidecar bounds lie inside its hitbox within 5 cm, and the silhouette fills the hitboxes within 10 cm.
  - **Motion:**
    - after 10 simulated seconds the galaxy has rotated by the expected angle;
    - every ship has moved at least 5 m and stays on its loop;
    - every island's height oscillates within its amplitude;
    - the stained-glass emissive varies over its period.
  - **Crystal ammo:** crystal emissive follows the magazine fraction, including at empty, full, mid-reload and after reload.
  - **Spell timing:** on a scripted hit, the impact effect, hitmarker and damage number exist on the hit tick, and the bolt reaches its hit point within 2 frames.
  - **Elimination hat:** the hat prop spawns on elimination, settles on the ground within 2 s and despawns on respawn.
  - **Props (D29):**
    - the player collides with rocks and can jump onto a stump;
    - a wall places through a rock;
    - both spawns and the initial cover are at least 3 m from every prop;
    - over 5 simulated minutes (seeded), the dummy never stalls on a prop;
    - a rock blocks a rifle shot.
  - **Asset audit:** every file under `assets/` appears in `assets/ASSETS.md` with a source script or license, and every glb in the manifest loads.
- **Kept:** every Milestone 1 test, including the TTK contract, building and combat. They must stay green untouched: the only gameplay change is the props, and no Milestone 1 test scenario touches them.
- **Native scenarios:**
  - `gallery`: the 12 views;
  - `perf`, `latency`, `fx_check` (same-frame feedback) and `ttk`: re-run on the new look;
  - a new `sky_check`: frames 5 s apart, for Jake to see the motion.

**Gates for Milestone 2** (used by `docs/M2-GOAL.md`):

| ID | Gate |
|---|---|
| S1 | Target board: every view scores ≥ 4 from Jake |
| S2 | Performance bar with the full look, on battery with Low Power Mode on |
| S3 | Launch under 5 s, 3 times in a row |
| S4 | Motion check: tests plus `sky_check` frames |
| S5 | Knight hitbox fit |
| S6 | Same-frame feedback and spell timing |
| S7 | No regressions: Milestone 1 G1, G3 and G6 pass; `cargo test`, clippy and fmt are clean |
| S8 | Jake's 10-minute feel verdict |
| S9 | Original and reproducible: `build-art.sh` regenerates every model headless; the asset audit passes |

## Out of Scope

- Any gameplay change beyond the D29 props: numbers, hitboxes, grid, controls, other arena collision.
- Bots (now Milestone 3), rounds and scoring.
- Skinned skeletal animation and motion capture.
- Third-party models, textures, sounds or asset packs. The single open-license font is the only exception.
- HDR bloom, SSAO, SSR, TAA, volumetrics and real-time shadows in the Battery preset.
- Character customization, skins and cosmetics.
- New maps or a map selection.
- Changes to Chunky.

## Further Notes

**Known risks:**
1. **The AI targets are paintings.** "Same style and composition" is the bar, and the scoring makes it Jake's call.
2. **G2 has never passed**, even on the old art. The baseline step finds out early whether the bar is reachable on this machine. The new look drops shadow maps and PBR, which should cost less than Milestone 1 did.
3. **Outline cost:** the outline draw doubles draw calls for outlined meshes. It's measured with a knob, and the fade distance is tuned.
4. **Scripted Blender art** may need several iterations to look as good as the targets. Preview renders let Claude review the art without opening a window.
5. **Hitbox proportions:** a 40 cm head sphere caps how big the helmet can be. The "big head" look comes from the helmet's shape and the small body, not a bigger hitbox.

**Resolved questions** (2026-09-25, Jake: "approve all targets, rec on Q27 Q28 Q29"):
- **Q27 → D29:** solid rocks and stumps inside the arena (see the scope rule).
- **Q28 → D30:** builds and headless tests may run at low priority while Jake uses the Mac (see Working on Jake's Mac).
- **Q29 → D31:** Blender MCP is installed.
  - It's `ahujasid/mcp-for-blender` (`mcp-for-blender` 2.1.0 via `uvx`).
  - It's registered in Claude Code's user config with `DISABLE_TELEMETRY=true` and `BLENDER_MCP_SAFE_MODE=1`.
  - The add-on is `blender_mcp.py`, enabled in Blender 5.2's user add-ons. It listens on localhost:9876 only, when Blender is open.
  - External asset services stay off.
