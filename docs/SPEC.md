# Pieced — Milestone 1 "Sparring" spec

**Status:** Agreed with Jake in the 2026-09-24 grilling session. This is the build contract for Milestone 1. It supersedes nothing inside Chunky: Chunky stays as it is, and its Minecraft-fidelity spec is retired as historical.

**Companion research:** `docs/research/game-feel.md`, `docs/research/bevy-stack.md`, `docs/research/bot-ai.md`.

## Problem Statement

Jake wants a game he actually reaches for when he's bored in class. It's played alone on his MacBook Air (M4, 15", 60 Hz display, fanless, usually on battery with Low Power Mode on), with **WASD and the built-in trackpad**, no mouse and no internet. His favorite game is Fortnite. What he loves is building under pressure, the drop, winning the match, and shredding an opponent with a pump and an assault rifle and hunting them down. Getting better, and every fight being a little different, is what brings him back.

Nothing he has does this. Chunky is a Minecraft-style creative sandbox with no combat. Fortnite needs a network, a strong machine and a mouse to feel good. Its first-person mode had no building and has reportedly been shut down. Existing laptop-friendly shooters don't combine fast building with a good pump/rifle sandbox, and none is tuned for a trackpad.

The risk isn't whether such a game could exist. It's whether it **feels good**: whether first-person building plus two guns can feel fast, crunchy and fair on a trackpad, at a steady 60 fps on a fanless laptop in Low Power Mode, and whether it looks good enough that he wants to keep playing. Milestone 1 exists to answer that before any bot is built.

## Solution

Pieced is a solo first-person shooter that **shoots like Call of Duty and builds like Fortnite**, set in a chunky, low-poly, flat-color world. Later milestones add smart local bots, one-minute rounds, and eventually a drop and a mini battle royale.

Milestone 1, **"Sparring"**, is a feel test. Launching Pieced drops Jake straight into a small, handsome arena within five seconds. He has:

- the full movement set (walk, sprint, jump, crouch, slide);
- a SCAR-style assault rifle and a pump shotgun, with aim-down-sights, hitmarkers, damage numbers, headshots, recoil and reloads;
- instant Fortnite-style building of walls, ramps and floors on a grid, with hold-to-keep-building, pieces that take damage, crack and shatter into chunky debris;
- a vividly colored training dummy that strafes around, soaks damage like a real opponent (100 health + 100 shield), dies with a satisfying burst, and respawns.

It holds a steady 60 fps with low input delay on battery in Low Power Mode. A tuning panel lets Jake and Claude tweak the feel live. After ten minutes, Jake either wants to keep playing or can say exactly what feels off. That verdict decides what Milestone 2 builds.

## User Stories

**Launch and session**

1. As Jake, I want Pieced to open straight into the arena in under five seconds, so that I can play the moment I'm bored.
2. As Jake, I want the game to pause instantly when I switch away, hide the window or lose focus, so that nothing happens while I'm not looking.
3. As Jake, I want the cursor locked and hidden while I play and released when paused, so that the trackpad controls only the camera during play.
4. As Jake, I want returning from pause not to jolt my view, so that resuming never flings the camera.
5. As Jake, I want a pause menu with Resume, Settings and Quit, so that I control the session without keyboard shortcuts I might hit by accident.
6. As Jake, I want the game to play fine muted, with every hit also visible on screen, so that I can play in class without sound.
7. As Jake, I want my settings to be remembered between launches, so that I tune sensitivity once.

**Looking and aiming on a trackpad**

8. As Jake, I want camera look to use raw trackpad motion with no smoothing, so that aiming feels direct.
9. As Jake, I want a high default sensitivity, where one full-width swipe turns about 180°, so that I can turn around without lifting and re-swiping.
10. As Jake, I want separate sensitivity multipliers for aiming down sights and for build mode, so that precise aiming and fast building each feel right.
11. As Jake, I want an optional acceleration curve (fast swipes turn more, slow swipes aim finely), so that one surface handles both flicks and tracking.
12. As Jake, I want an optional mild aim friction on the rifle, off by default, so that I can decide whether trackpad aiming needs help.
13. As Jake, I want to set the field of view between 60° and 90° (vertical), so that the view suits the screen.
14. As Jake, I want to fire with a physical trackpad press and have it register while I hold movement keys, so that shooting while strafing works.
15. As Jake, I want to toggle aim-down-sights with a two-finger click or a key, so that I never have to hold an awkward two-finger press.

**Movement**

16. As Jake, I want to run with WASD, reaching full speed almost instantly and stopping crisply, so that movement feels responsive.
17. As Jake, I want to sprint while holding Shift, so that I can cover ground and rotate fast.
18. As Jake, I want to jump, with jump buffering and a short grace period after walking off an edge, so that jumps never feel eaten.
19. As Jake, I want a jump that can't clear a wall, so that walls mean something.
20. As Jake, I want to crouch while holding C, so that I can make myself smaller and move quietly.
21. As Jake, I want pressing crouch while sprinting to slide with a burst of speed that decays, so that I get the Call of Duty slide.
22. As Jake, I want to walk up ramps smoothly and never snag on piece edges, so that ramp rushing flows.
23. As Jake, I want to never clip into walls or see through them, however close I stand, so that boxes feel solid.
24. As Jake, I want to stay inside the arena and never fall out of the world, so that a mistake never ends the session.

**Weapons**

25. As Jake, I want a SCAR-style assault rifle on 1 and a pump shotgun on 2, switching quickly, so that I can do pump-then-rifle combos.
26. As Jake, I want the rifle to be accurate on the first shot and spread as I keep spraying, so that tapping and bursting are rewarded.
27. As Jake, I want the pump to fire a fixed pellet pattern that hits hard up close and falls off with distance, so that the pump is the close-range finisher.
28. As Jake, I want both guns to fire on press, with the pump accepting a press made slightly early, so that shots never feel dropped.
29. As Jake, I want to reload with R, with auto-reload on empty, a rifle magazine reload and a shell-by-shell pump reload that firing can interrupt, so that ammo management matters without busywork.
30. As Jake, I want aim-down-sights to zoom in and tighten spread, so that mid-range fights feel like COD.
31. As Jake, I want headshots to do bonus damage, with a distinct marker and sound, so that aiming for the head is rewarded.
32. As Jake, I want the gun visible in my view, with recoil kick, sway, reload motion and a muzzle flash, so that shooting feels physical.
33. As Jake, I want tracers and impact bursts where my bullets land, so that I can see where I'm shooting.
34. As Jake, I want a crisp hitmarker, a distinct hit sound and a floating damage number on the same frame every hit lands, so that I always know I connected.
35. As Jake, I want killing a target in about one to two seconds of steady rifle fire from full health, so that fights last long enough to build in them.
36. As Jake, I want one close pump shot to the body never to kill from full health, so that no fight ends in a single click.
37. As Jake, I want camera shake kept minimal and adjustable down to zero, so that it never ruins my aim.

**Building**

38. As Jake, I want Q, E and F to select wall, ramp and floor and put me in build mode, with 1 or 2 returning to my guns, so that building feels like Fortnite on PC.
39. As Jake, I want a see-through ghost preview of exactly where the piece will go, turning red when it can't be placed, so that I never misplace a piece.
40. As Jake, I want walls to snap to the grid edge I'm facing, ramps to rise away from me, and floors to go at my feet or one level up when I look up, so that placement is predictable.
41. As Jake, I want to place a piece with a click and keep placing while I hold the click and move ("turbo build"), so that I can ramp rush and throw up walls fast.
42. As Jake, I want to be able to build a 1x1 box (four walls around me, plus a ramp or floor) in about a second, so that I can box up under pressure.
43. As Jake, I want pieces to take damage from my guns, show cracks as they weaken, show their health when I aim at them, and shatter into chunky debris when destroyed, so that building and shooting interact.
44. As Jake, I want a short lock before a destroyed spot can be rebuilt, so that building has the Fortnite rhythm.
45. As Jake, I want unlimited building materials, so that Milestone 1 tests feel, not economy.
46. As Jake, I want to build up to a sensible height limit, so that I can take height without breaking the arena.
47. As Jake, I want the inside of a box to stay bright and readable, so that being boxed in never feels claustrophobic or dark.

**Training dummy**

48. As Jake, I want a training dummy in a vivid color no other object uses, with a rim light, so that it pops out at a glance.
49. As Jake, I want the dummy to strafe unpredictably, sometimes jump, and move at player speed, so that I practice tracking a moving target.
50. As Jake, I want the dummy to have 100 health and 100 shield, with shield hits and health hits looking and sounding different, so that I learn real damage values.
51. As Jake, I want the dummy to die with a satisfying burst and respawn a couple of seconds later somewhere else, so that practice never stops.
52. As Jake, I want to switch the dummy to "stand still" from the tuning panel, so that I can test time-to-kill fairly.
53. As Jake, I want a small combat readout (time to kill, accuracy, headshot rate), so that I can see myself getting better.

**Looks**

54. As Jake, I want the arena to look gorgeous: a warm, stylized sky, soft sunlight and shadows, distance fog, a chunky low-poly landscape beyond the arena, and a clear, harmonious palette, so that I want to be in it.
55. As Jake, I want every surface to have clean, flat-shaded facets and crisp anti-aliased edges, so that the style reads as deliberate, not unfinished.
56. As Jake, I want my built pieces to look like chunky crafted panels in their own material color, clearly different in brightness from the ground and the dummy, so that the battlefield is readable.
57. As Jake, I want satisfying effects (muzzle flashes, tracers, sparks, debris chunks, a shield shimmer and an elimination burst) that stay light enough for 60 fps, so that the game feels alive without stutter.

**HUD and settings**

58. As Jake, I want a clean HUD with the crosshair, health and shield bars, ammo, my selected weapon or piece, and a build-mode indicator, so that I read my state instantly.
59. As Jake, I want the crosshair to show current spread, with a toggle to hide the bloom, so that I understand accuracy.
60. As Jake, I want a performance overlay (fps, frame time, worst frame) on a key, so that I can see if the game is running smoothly.
61. As Jake, I want a live tuning panel on a key, covering sensitivity, FOV, weapon numbers, movement numbers, piece health and effects, and saving what I change, so that Claude and I can dial in the feel together.
62. As Jake, I want graphics presets ("Battery" by default, "Plugged in" optional), so that the game looks as good as the power budget allows.

**Development and evidence**

63. As Claude building the game, I want all gameplay driven by a player-intent structure that keyboard and trackpad, scripted scenarios and future bots all write, so that tests, scenarios and bots use the same code path as the player.
64. As Claude, I want gameplay to run on a fixed 60 Hz step that can run headless with scripted intents, so that movement, building and combat are testable and deterministic.
65. As Claude, I want a scenario mode that plays scripted sessions with the real renderer and writes frame logs, screenshots and a summary, so that performance and looks are measured, not guessed.
66. As Claude, I want a frame-time log and launch-time measurement written on every run when requested, so that performance claims have evidence.
67. As Claude, I want a measured input-to-frame latency figure, with its method and limits stated, so that "low delay" is a number.
68. As Jake, I want the repo pushed to GitHub after every working step, so that I can follow progress and nothing is lost.

## Implementation Decisions

### Platform, toolchain and dependencies

- Native macOS, Apple Silicon only. Rust **1.98.1** pinned via a rust-toolchain file, using the toolchain already installed in the parent workspace (Chunky's isolated `work/cargo` and `work/rustup`).
- **Bevy `=0.19.1`** (latest stable; 0.20 is only a release candidate) with the `wav` feature. **avian3d `=0.7.0`** for collision shapes and spatial queries. **bevy_egui `=0.42.0`** for the dev tuning panel only. **bevy_framepace `=0.22.0`** is allowed only as a measured latency A/B experiment. No other gameplay crates in Milestone 1. `pathfinding` is reserved for Milestone 2.
- Dependencies are pinned exactly, with a committed lockfile.
- Build profiles:
  - **dev:** own crate at opt-level 1, dependencies at opt-level 3, minimal debug info to save disk.
  - **release:** used for every performance gate.
- The machine has about 12 GB of free disk, so builds share a single target directory and avoid extra profiles.
- The renderer backend is Metal. Pipelined rendering is disabled. The present mode starts as vsync (`Fifo`) with the maximum frame latency set to 1. `AutoNoVsync` with a 60 fps cap is an allowed A/B experiment, and the measured winner becomes the default.
- The window uses the built-in display's logical resolution with a scale-factor override of 1, so it renders at logical pixels rather than full Retina density. Borderless fullscreen is the default; windowed mode is available from settings.

### Architecture (Bevy plugins, one responsibility each)

1. **App shell.** Window and present setup, app states (`Boot → Playing ⇄ Paused`), cursor lock, pause on focus loss or hide, clearing held input on state changes, and ignoring the look delta on the recapture frame. This carries over Chunky's proven patterns.
2. **Intent.** The single boundary between devices and gameplay. A **`PlayerIntent`** per controlled character holds:
   - a move vector;
   - jump pressed and held;
   - sprint, crouch and fire (held and pressed edges);
   - ADS toggle, reload;
   - select rifle, select pump;
   - select wall, ramp or floor;
   - a look delta in yaw and pitch.

   Three writers exist: the keyboard/trackpad adapter, the scenario script driver and, in Milestone 2, bots. Gameplay systems never read devices directly. Look is applied every rendered frame; everything else is consumed in the fixed step.
3. **Movement.** A kinematic first-person controller in the fixed 60 Hz step, using avian3d's move-and-slide:
   - ground detection and ground snapping on ramps;
   - acceleration, friction and air control;
   - jump with buffer and coyote time;
   - crouch height change with a headroom check;
   - slide state with a decaying speed boost;
   - depenetration, and arena bounds.

   Render transforms are interpolated between fixed steps.
4. **Build grid.**
   - A logical **piece map** keyed by grid coordinates is the source of truth. Colliders and meshes are derived from it.
   - Piece kinds are Wall, Floor and Ramp. Each piece has an orientation, HP and a crack stage.
   - The module provides:
     - a targeting function from camera ray, player cell and piece kind to a candidate placement, marked valid or invalid;
     - placement rules: no overlap, player not trapped inside a new wall, height limit, arena bounds, and the rebuild lock after destruction;
     - turbo placement cadence;
     - damage intake and destruction events.
   - The grid is also the future bot navigation graph, so its API exposes cell and edge queries without rendering.
5. **Combat.**
   - Weapon definitions are data: fire mode, fire interval, damage, headshot multiplier, falloff curve, spread and bloom, ADS spread and zoom, magazine, reload, pellet pattern, and structure damage.
   - Hitscan uses avian3d ray casts against separate collision layers for world, pieces, target bodies and target heads.
   - A single **damage event** stream carries source, target (character or piece), amount, headshot flag and hit point, feeding health, pieces, HUD and effects.
   - **Health** is a component with 100 HP and 100 shield. Shield absorbs damage first.
6. **Dummy.** A target character sharing the character components (health, hit volumes, movement) and driven through `PlayerIntent` by a simple strafe/jump pattern with seeded randomness. A tuning option makes it stand still. It respawns about 2 s after elimination at a random valid arena spot away from the player.
7. **Arena and look.**
   - Generated in code from low-poly primitives with flat normals and palette-driven materials:
     - an arena floor with subtle faceted variation;
     - a few pre-placed cover pieces;
     - a boundary that reads as terrain: low cliffs, rocks and a visible edge;
     - a backdrop of low-poly hills, trees and clouds;
     - a stylized gradient sky with a sun disc.
   - Lighting is one directional sun with a single shadow cascade sized to the arena, plus ambient light and distance fog.
   - Forward rendering with MSAA 4×. Heavy post effects (SSAO, SSR, TAA, contact shadows, volumetrics) are off.
   - The target material adds a rim or fresnel term so the dummy reads against any background.
   - Two quality presets, **Battery** (default) and **Plugged in**, change shadow resolution and optional effects. They are chosen by measurement, not guesswork.
8. **Viewmodel and effects.**
   - Guns are built from low-poly primitives in code and drawn by a separate viewmodel camera or layer with its own FOV, so they never clip into walls.
   - Spring-based recoil, sway and reload animations.
   - Pooled effects: muzzle flash, tracers, impact sparks, piece debris chunks, shield shimmer and elimination burst.
   - Effects have fixed caps so they never threaten the frame budget.
   - A small, optional hitstop on eliminations only.
9. **Audio.** A small bank of **original, synthesized** sounds generated by our own code:
   - rifle shot, pump shot, pump rack, reload;
   - hitmarker tick, headshot ding, shield hit, shield break;
   - piece place, piece crack, piece break;
   - footstep, slide, jump/land, elimination.

   Spatialized where it matters, with master volume and a mute setting. No third-party or Minecraft assets.
10. **HUD and menus.**
    - Crosshair with bloom (toggleable), health and shield bars, ammo, weapon and piece hotbar, and a build-mode indicator.
    - Hitmarkers and damage numbers (headshot and shield variants), plus an HP bar for the piece under the crosshair.
    - A combat readout: last TTK, accuracy and headshot rate.
    - A pause menu with Settings: sensitivities, acceleration curve, aim friction, FOV, bloom toggle, shake slider, volume and mute, quality preset, and windowed or fullscreen.
    - A performance overlay and a dev tuning panel.
11. **Tuning.** One tunable resource holds every feel number in this spec. It's editable live in the tuning panel and persisted to a local settings file that git ignores. Defaults in code match this spec.
12. **Telemetry and scenarios.**
    - An optional frame log: one row per frame with frame index, real frame delta, fixed ticks run, state and preset.
    - Launch-to-controllable time.
    - An input-to-frame latency probe.
    - A CLI scenario runner that drives `PlayerIntent` from scripts with the real renderer, captures screenshots at set points and writes an evidence folder with raw logs and a summary.
    - Scenarios:
      - **perf:** sustained mixed play — running, sliding, building boxes and ramps, spraying and pumping the dummy.
      - **gallery:** fixed art-review views.
      - **ttk:** a standing dummy at a set range.
      - **latency:** the input-to-frame probe.
    - The runner records the power source and Low Power Mode state.

### World units and grid

- Units are meters. **Grid cell 4 m × 4 m, level height 3 m.**
- A wall fills a cell edge (4 m wide, 3 m tall, about 0.2 m thick). A floor fills a cell at a level (about 0.2 m thick). A ramp spans a cell, rising 3 m over 4 m in one of four directions.
- The arena is about **12 × 12 cells (48 m square)** with a build height limit of **6 levels**.
- The player capsule is about 1.8 m tall and 0.35 m in radius, with eyes at about 1.62 m. Crouching drops height to about 1.2 m.
- The camera near plane is about 0.05 m.

### Starting feel numbers (all tunable live)

**Movement**
- Run 5.5 m/s, sprint 7.5 m/s, crouch 2.8 m/s.
- Slide: entry boost to about 9 m/s, decaying over about 0.8 s, with a cooldown.
- About 0.1 s to reach full speed, and about 0.06 s to stop.
- Jump apex about 1.2 m under gravity of about 20 m/s².
- Air control about 40%.
- Jump buffer and coyote time 0.1 s each.

**Look**
- Default sensitivity: about 180° per full-width trackpad swipe.
- ADS multiplier 0.7, build multiplier 1.0.
- Acceleration curve off by default.
- Vertical FOV 70°. Rifle ADS zoom ×0.75, pump ADS zoom ×0.9.

**SCAR-style rifle**
- 6 shots/s, 28 body damage, ×1.5 headshot.
- Full damage to 25 m, falling to 70% at 50 m.
- 30-round magazine, 2.0 s reload.
- First-shot accuracy, bloom growing per shot and recovering after about 0.3 s. ADS reduces spread by about 60%.
- 28 structure damage per hit.

**Pump**
- 10 hitscan pellets in a fixed pattern at 10 damage each (100 max to the body), ×1.5 per headshot pellet.
- Falloff beyond about 8 m, down to about 30% by 15 m.
- 0.9 s between shots, 5 shells, 0.5 s per shell reload that firing can interrupt.
- 150 ms early-press buffer.
- 10 structure damage per pellet.

**Weapon switch** about 0.2 s.

**Pieces**
- Wall 200 HP, floor and ramp 170 HP.
- Crack stages at 66% and 33%.
- Rebuild lock 0.15 s after destruction.
- Turbo placement interval 0.05 s.

**TTK contract**
- From full 200 (100 HP + 100 shield), a standing dummy at 15 m takes **8 body hits, about 1.17 s**, of continuous rifle fire.
- Any time-to-kill between **1.0 and 2.0 s** passes.
- One close pump body shot (at most 100) never kills from full.
- A wall soaks **at least 1 s** of continuous rifle fire.

**Dummy**
- 100 HP + 100 shield, moving at player run speed, respawning after about 2 s.

### Controls (Milestone 1; rebinding is out of scope)

| Input | Action |
|---|---|
| W A S D | Move |
| Trackpad move | Look |
| Trackpad physical click | Fire / place piece (hold for full-auto rifle or turbo build) |
| Two-finger click, or V | Toggle aim-down-sights |
| Space | Jump |
| Shift (hold) | Sprint |
| C (hold) | Crouch; slide when pressed while sprinting |
| 1 / 2 | SCAR / pump (also leaves build mode) |
| Q / E / F | Wall / ramp / floor (enters build mode) |
| R | Reload |
| Esc | Pause menu |
| F3 | Performance overlay |
| F4 | Tuning panel |

Command-key combinations are never used for gameplay.

### Art direction ("gorgeous" means these things)

- **Palette:**
  - Warm late-afternoon light.
  - Muted mid-tone world: sage and olive greens, warm sand, slate rock.
  - Player-built pieces in a warm crafted-wood amber that is clearly brighter than the ground.
  - The dummy in a single saturated hue (hot coral or magenta) used nowhere else, with a rim light.
  - A sky gradient from soft gold at the horizon to a clear blue above. Fog tinted to the horizon color, so the backdrop melts into the sky.
- **Form:**
  - Chunky, faceted, slightly beveled shapes.
  - Pieces read as crafted panels: framing, bevels and plank facets modeled as geometry, not textures.
  - Silhouettes first, small detail omitted (TF2 and Valorant readability principles).
- **Light:** one sun with soft shadows, bright ambient fill so interiors never go dark, and no dark outlines.
- **Motion:** effects are short, punchy and cheap. Debris chunks tumble and fade, sparks pop, and the elimination burst reads from across the arena.
- **Quality bar** (checked by the gallery scenario):
  - no z-fighting, no visible seams, no clipping into walls;
  - no unlit or flat-grey surfaces;
  - clean anti-aliased edges;
  - sky and fog blend smoothly;
  - the dummy stays distinguishable in a greyscale version of each gallery shot.

### Performance and latency contract

- Measured with the release build on this MacBook Air, **on battery with Low Power Mode on**, the window visible and the Battery preset.
- Frame log over a **5-minute perf scenario:**
  - a steady 60 fps (mean frame interval 16.4–17.0 ms);
  - after the first 10 s, **no frame over 25 ms**, and at least 99% of frames under 18 ms.
- **Launch to controllable in under 5 s** from a warm start, meaning a second launch after a fresh build.
- **Input-to-frame latency:** the software-measured time from input arriving to the frame that shows its effect is reported with its method and limits. The target is **≤ 33 ms (2 frames at 60 Hz)**, excluding display scanout.
- A plugged-in run with Low Power Mode on is allowed as a development proxy. The battery run is the gate.

### Repository and workflow

- Public GitHub repository `jparker2006/pieced`, default branch `main`, pushed after every working step.
- Evidence folders stay out of git. A curated Milestone 1 report with summary numbers and selected gallery screenshots is committed.
- Original assets only: all meshes, sounds and palettes are generated by project code. The UI font must be original or permissively licensed with attribution. Chunky's font may be reused with its preserved source declaration.
- Chunky is not modified.

## Testing Decisions

**What makes a good test here:** it drives the game through its public seams, meaning `PlayerIntent` in and observable game state out, and asserts **behavior players would notice**:

- where the player ends up;
- whether a piece exists and how much HP it has;
- how much damage landed;
- how many ticks a kill took.

It never asserts private functions, system ordering or component internals. It's deterministic: fixed 60 Hz steps, seeded randomness, and no wall-clock dependence.

**Seams (as few as possible):**

1. **Headless simulation seam (primary).** A headless Bevy app runs the real gameplay plugins (intent, movement, build grid, combat, dummy, arena collision) with no renderer, stepped exactly one fixed tick per update, with scripted `PlayerIntent` sequences. Nearly all automated tests live here.
2. **Native scenario seam.** The real app with the real renderer, driven by the same scripted intents through the scenario runner. It produces frame logs, screenshots and summaries for the performance, latency, launch and art gates. It's used for evidence, not unit assertions.
3. **Human feel test.** Jake plays for ten minutes on the trackpad and gives a verdict. Nothing automated substitutes for it.

**Modules covered by headless tests:**

- **Movement:**
  - acceleration and stop times within tolerance;
  - sprint and crouch speeds;
  - slide boost and decay;
  - jump apex never clearing a wall;
  - jump buffer and coyote time;
  - walking up a ramp to the next level;
  - no tunneling through walls at sprint or slide speed;
  - staying inside arena bounds.
- **Build grid:**
  - targeting for each piece kind and facing;
  - overlap rejection;
  - height limit;
  - the rebuild lock;
  - turbo placement cadence while moving;
  - a scripted 1x1 box completing within about 1 s of intents;
  - piece damage, crack stages and destruction removing collision.
- **Combat:**
  - rifle first-shot accuracy and bloom growth and recovery;
  - pump pellet pattern and falloff;
  - headshot multiplier;
  - shield absorbing before health;
  - reload rules (magazine, shell-by-shell, interrupt);
  - weapon switch time;
  - **the TTK contract** (standing dummy at 15 m dies in 1.0–2.0 s of continuous rifle fire, one close pump body shot never kills from full, a wall soaks at least 1 s of rifle fire);
  - structure damage.
- **Dummy:** strafing stays in bounds; elimination; respawn after the delay at a valid spot.
- **Telemetry:** the frame-log summary math (percentiles, counts over threshold) on synthetic inputs.

**Prior art:** Chunky's input tests wrote Bevy input resources directly and stepped the app. Its native scenario runner wrote evidence folders with raw frame intervals, screenshots and JSON summaries, and its TESTING.md kept evidence honest by attributing it to exact builds. Pieced follows the same discipline, with `PlayerIntent` replacing raw input writes.

**Gates for Milestone 1** (IDs used by the goal brief):

| ID | Gate |
|---|---|
| G1 | Launch to controllable in under 5 s |
| G2 | Performance on battery with Low Power Mode on |
| G3 | Latency reported, target ≤ 33 ms |
| G4 | Building works |
| G5 | Weapons work |
| G6 | TTK contract |
| G7 | Automated tests pass |
| G8 | Art review passes |
| G9 | Jake's verdict |

## Out of Scope

**Deferred:**
- **Milestone 2:** bots of any kind beyond the scripted dummy, bot building, rounds and scoring.
- **Later:** the drop, the mini battle royale, the storm, and a dummy that shoots back.

**Excluded:**
- Multiplayer or networking of any kind.
- A browser build, phones or touch, Windows or Linux.
- Voxel blocks, Minecraft textures or any third-party game assets.
- Loot, healing items, material gathering or limits.
- Editing pieces, the cone/roof piece.
- Killcams, killstreaks, perks, loadouts.
- Mantling, grapples and other movement tech beyond sprint, crouch, slide and jump.
- Cosmetics, unlocks, progression, saves or match history beyond the combat readout.
- Key rebinding.
- Cloud AI of any kind, including Jev.
- Controller support.
- Third person.
- Changes to the Chunky repository.

## Further Notes

**Known risks:**

1. **Trackpad input while a key is held.** macOS behavior is unverified; it's the first thing tested on the real machine.
2. **Holding a physical press to spray on a trackpad** may be uncomfortable. If so, the rifle can switch to tap-fire and aim friction can be tuned.
3. **First-person building is unproven.** Epic's first-person mode shipped without building.
4. **The fanless Air may throttle** over long sessions.
5. **Only about 12 GB of free disk.** A Bevy build fits, but only if build profiles and targets are kept lean.

**Milestone 2 preview (not in this spec):**
- A local utility-AI sparring bot writing the same `PlayerIntent`, using the build grid as its navigation graph.
- Human-like reaction and aim limits, difficulty tiers, a wall-when-shot reflex and push-on-reload.
- First-to-5 rounds.

See `docs/research/bot-ai.md`.

**Decision record:** first person, not third person. Fortnite building pieces, not voxels. A slow, Fortnite-like TTK. Public repo. Local-only brain for bots. All decided by Jake in the grilling session.
