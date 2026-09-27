# Research: how real Fortnite building works (plus bloom and controls)

Collected 2026-09-26 for Milestone 2 decisions D32 (ramp rush), D33 (Fortnite building, fully), D34 (guns) and D35 (controls). "Current" means Battle Royale in **Chapter 7 Season 4** (v42.00 shipped 2026-08-20; v42.20 on 2026-09-17) [36][48].

**Markers:**
- **[n]** is a source that was opened and read.
- **[n\*]** was seen only through a search-engine summary. Treat it as unverified.
- *Synthesis* is our own inference, not a sourced fact.
- Dates in brackets, e.g. **(2019)**, flag data from before 2024.

**Access limits:** Reddit, Medium, YouTube transcripts and fortnite.com patch-note pages all refused automated access. Patch notes were read from the Fortnite Wiki's transcriptions, via the wiki's own API.

**The biggest gap:** Epic has never published how the build preview picks a tile and height, and no data-mine write-up was found. Section 2 separates what is documented from how players describe it and from our model.

**Scale:** Fortnite's tile is 5.12 m × 3.84 m [1]. Ours is 4 m × 3 m, the **same 4:3 shape at 0.78× scale**, so ramp angles and edit-tile proportions carry over exactly. Where it helps, Fortnite distances are also given ×0.78, marked "(ours)".

---

## 1. Grid and pieces

| | Fortnite | Pieced today |
|---|---|---|
| Tile | 5.12 × 5.12 m [1][2] | 4 × 4 m |
| Level (wall height) | 3.84 m [1][2] | 3 m |
| Wall | 5.12 × 3.84 m; 0.16 m thick (wiki) [1] or 24 cm (UEFN guideline) [2] | 4 × 3 × 0.2 m |
| Floor | 5.12 × 5.12 m; 0.32 m (wiki) [1] or 24 cm (UEFN) [2] | 0.2 m |
| Stairs / ramp | Fills the whole cell, rising 3.84 over 5.12 m: **36.87°** [1] | 3 over 4 m: **36.87°** (identical) |
| Cone / roof ("pyramid") | 5.12 × 5.12 × **1.92 m** (half a level) [1] | none (would be 1.5 m) |
| Player height | 192 cm [3] (0.50 of a level) | 1.8 m (0.60 of a level) |

- **What each cell can hold:**
  - Pieces fill 1×1×1 cells, and walls sit on cell edges [1].
  - A floor, a ramp and a cone can share one cell. A box, for example, is four walls plus "a floor and cone on top" [50\*].
  - Pieced already keys floor and ramp slots separately (`src/building/grid.rs`), so they can share a cell.
- **Cost and support:**
  - Every piece costs 10 materials, and edits are free [1]. The Battle Royale cap is 500 of each material [1][7].
  - Pieces must connect to the ground or other builds. When the connection is lost, they collapse cell by cell [1].
- **Preview colours** [1]:
  - blue: can place;
  - red: can't;
  - yellow: placed but not yet building because something overlaps it. Players can walk through yellow pieces, and the piece starts building once the obstruction clears.
- **Placement leniency:**
  - v3.00 (2018): pieces place straight through trees, rocks and cars [13][12].
  - v5.10 (2018): a piece placed on top of a player turns a different colour while the player overlaps it, and the player is "much more likely … to be moved outside" of it [5].
  - v5.20 (2018): ramps can't be built through other ramps [17].
  - v4.50 (2018): a wall that lacks support "will now try to build at a closer location" [14].
  - v7.20 (2019): a wall less than 30% above ground gets a free wall on top of it [15].

### Piece HP and build-up

Pieces spawn weak and gain HP over time. The most detailed public table is from **Season 9 (June 2019)** [4]. Epic hasn't published a newer one.

| Material | Wall start → max | Build time | Floor, ramp or cone start → max | Build time |
|---|---|---|---|---|
| Wood | 90 → 150 | 4 s | 84 → 140 | 3.5 s |
| Brick | 99 → 300 | 11.5 s | 93 → 280 | 12 s |
| Metal | 110 → 500 | 24.5 s | 101 → 460 | 22.5 s |

- **Patch history:** v5.10 (July 2018) changed the numbers [5]:
  - wood wall start 100 → 80, and max 200 → 150;
  - brick start 90 → 80;
  - metal max 400 → 500, and metal build time 20 → 25 s.

  GameRevolution reports metal as 500 → 450 [6]. The wiki transcription and the 2019 table both say 500.
- **Conflict:** the wiki's Materials page lists max HP as 150 / 400 / 600 without a source [7]. Treat the 2019 table as the best available.
- **Material trade-off:** wood builds fastest and is weakest, and metal is the reverse [1][7]. A full wood wall takes "5-6 Assault Rifle shots" [7].
- **Compared with Pieced:** walls spawn at full 200 HP, and floors and ramps at 170 (`src/building.rs`).

---

## 2. Build preview and targeting

### 2.1 What is documented

- **Facing:**
  - Pieces snap to the grid, and walls sit on cell boundaries [1].
  - Fortnite has a **Rotate Building** bind (R by default) that turns the previewed piece [26][43]. Without it, stairs rise away from the player (every guide assumes this [30][31\*]).
- **Epic ties pitch to piece choice.** Chapter 7's **Simple Build** (v39.00, 2025-11-29) uses two keys: one builds walls, the other builds stairs, floors or roofs depending on where you look [21][22][23]:
  - look at the upper half of the screen: stairs;
  - look at the lower half: a floor;
  - keep looking down: a roof on top of that floor.
- **Looking down means building below.** v9.20 (2019) fixed players "accidentally plac[ing] a floor piece above when attempting to build below them" [16].
- **You can build in every movement state:** sprinting, sliding, jumping, falling and mantling [40]. Sliding also allows shooting and building (v19.00) [41].
- **Reach:** Epic doesn't publish it. One secondary source claims "three tiles away … up from the previous two-tile limit" since Chapter 3 [49\*], but its page now 404s, so this is **unverified**. Three tiles would be 15.4 m, or 12 m at our scale.

### 2.2 How players describe it

- **Aim picks the tile.**
  - To double-ramp, you "place two ramps next to each other going in a right-to-left motion", sweeping the crosshair sideways [31\*].
  - The tile follows the crosshair left and right, including diagonal and side tiles, while the piece keeps its snapped facing.
- **Ramp rush:** "as you're going up the ramp, look slightly towards the ground and place a floor connected to the bottom of it, then look towards the middle of the ramp and place a wall connected to the front of it" [31\*]. Also: "look down on the ramp where you are placing the wall and floor", and "jump on top of the ramps … so that you don't get stuck under your ramp" [30].
- **Tunnels:** floor below you, then floor above you, then walls to the sides. Turbo-building the side walls "will also [place] a wall in front of you", because the preview sweeps across the front edge as you turn [32\*].
- **Floor-ramp tunnel:** once you're "past the halfway point of the floor", the next piece goes behind you [32\*]. Where you stand inside the tile matters.

### 2.3 Our model of Fortnite's targeting (*Synthesis*)

This is consistent with everything above. Its thresholds are ours, computed for **Pieced's first-person camera**: eye 1.62 m above the feet, player at the centre of a 4 m tile. Fortnite's over-the-shoulder camera makes its rays land further out.

| Question | Behaviour (model) | Pieced numbers |
|---|---|---|
| Which facing? | Yaw snapped to the nearest of 4. The ramp rises away from you and the wall sits on the edge you face. | unchanged |
| Ramp looking **down** | Your own tile, at the level you stand on, so the ramp lifts you (90s, retakes) | Pitch ≥ 39° down from the tile centre; ≥ 22° from the back edge |
| Ramp looking **straight** or slightly down | The tile ahead, at the level of the ground there | Pitch 15–39° down lands in the next tile; near level falls back to the next tile |
| Ramp looking **up** | The tile ahead one level up; looking steeply up, your own tile one level up (a cover ramp or roof) | Pitch 13–35° up: ahead, one level up; above 35°: overhead |
| Own tile or tile ahead? | Decided by where the ray lands, so your position in the tile matters. There's no fixed distance threshold. | Own-tile boundary moves from 22° to 78° down as you walk from the back of the tile to the front |
| Sideways | The tile follows the ray's horizontal landing point, which gives diagonals and double ramps | Needs 2D landing, not "own + one ahead" |
| Reach | Only as far as the ray lands, capped somewhere around 1–3 tiles (unverified) | Recommend a cap of 1 tile around yours, tunable to 2 |
| Mid-air | The same rules, measured from your current height. Looking down while falling builds below you. | Snap window as today (0.3 m) |
| Climbing a ramp | "Ahead" means the next tile at the **ramp's top level**. The slot just above the ramp you're on is never chosen while you face up-slope. | See Recommendations T3 |

### 2.4 Where Pieced differs today

Source: the module doc of `src/building/targeting.rs`.

| Rule | Pieced now | Fortnite (sources and model) |
|---|---|---|
| Tiles reachable | Own cell, plus one along the facing | Where the aim lands, including side and diagonal tiles (double ramps, side floors) |
| Wall when you're near an edge | Skips to the next grid line if you're closer than a body width | Pieces may overlap the player, who is pushed out or can pass through [1][5]. Boxes always form around you. |
| Wall while climbing | Your cell's front edge, at the ramp's top level. That's exactly where you walk from this ramp onto the next one, so it **blocks the rush**. | Guides place it at the high end of the ramp ahead, or under your own ramp's top [30][31\*] |
| Ramp while running on flat ground | A moderate downward look targets **your own cell**: 39° or more down at the tile centre, 22° or more at the back of the tile. You then run off it, so it ends up behind you. | Fortnite's camera sits behind the player, so the same look lands further ahead. Only a steep look builds under you. (*Synthesis*) |
| Level across the front edge | `floor(feet.y / 3)` at the foot of each new ramp can come out one level low if the feet sit a hair under a level. The next ramp then lands one level low, leaving a stray ramp under the path. (*Hypothesis from reading the code, untested.*) | Placements always come out at the right height |
| Upward look while climbing | Looking up more than about 22–29° targets the next tile **two** levels up | Matches the "cover ramp" players build above themselves [30\*], so this is fine |

**Likely causes of Jake's "ramp started placing behind me" (D32):** the "ramp while running on flat ground" and "level across the front edge" rows (*Synthesis*). The "wall while climbing" row would also stop any ramp + wall rush. Headless tests should confirm them before any fix (see "Tests to add").

---

## 3. Ramp rushing

**Inputs:**
- run forward (sprint-by-default is on for everyone [42]);
- select stairs;
- **hold place** so turbo repeats every 0.05 s [8];
- look roughly level or slightly down.

**Why it's endless:**
- Every turbo tick re-targets and places into any empty, valid slot [8][12].
- While you stand on ramp N, the target is the next tile at ramp N's top level. Once you step onto ramp N+1, the target moves to N+2 within one tick.
- At Fortnite's 7.39 m/s sprint [40], one 5.12 m tile takes about 0.7 s, which is about 14 turbo chances. Pieced's 7.5 m/s crosses 4 m in 0.53 s, about 10 chances. In tiles per second, Pieced's sprint is about 30% faster than Fortnite's: 1.9 tiles/s against 1.4 (*Synthesis*).

| Variant | Sequence | Why |
|---|---|---|
| Ramp only | Hold ramp and run | Fastest, no protection |
| Ramp + wall | Place the wall first, then the ramp behind it, each level | The wall at the ramp's front edge blocks shots from ahead [30][31\*] |
| Ramp + floor + wall | Ramp, then look down for a floor under it, then look at the middle of the ramp for a wall at its front. Costs 3 pieces per level. | The floor holds the rush up if the ramp is shot out [31\*] |
| Double ramp (+ floors + walls) | Sweep two ramps side by side from right to left, then two floors left to right, then two walls right to left | Wider protection, for pushing three or more enemies [31\*] |

---

## 4. Turbo build, 90s, and the other techniques

### Turbo timeline

| Version | Change |
|---|---|
| v3.00 (Feb 2018) | Turbo introduced. "Now you can 'paint' building pieces into the world quickly" [12][13] |
| v4.30 (2018) | First piece 0.15 s after holding, then **0.05 s** between pieces. The 0.15 s gave time to switch piece or rotate [8]. |
| v7.40 (2019) | Initial timer 0.15 → **0.05 s** [9] |
| v10.20 (2019) | Tried 0.15 s between pieces; reverted in v10.20.1 [10][11] |
| v10.20.1 (Aug 2019) | After a piece is destroyed, a **0.15 s** lock before that spot can be rebuilt. Ties between players go to a random roll. [1][11] |
| v7.20, v41.10 | Fixes so turbo keeps going while holding extra build keys, and never silently fails to place [15][48] |

**The rule:** turbo only ever places **the piece the preview currently shows**, and only if that slot is empty and valid. As you move or turn, the preview sweeps and turbo fills each new empty slot. Pieced already matches the cadence (0.05 s = 3 ticks, and 0.15 s lock). Turbo can be switched off [26].

### 90s

"Quickly building a ramp, two walls, and a floor… called a 90 because the player turns 90 degrees" [33]. A common description: "a ramp with a wall behind it then rotate 90 degrees and place another wall … jump on top to maintain timing" [30].

**What the system must allow** (*Synthesis*, from the steps above):
1. **Turn, then build in your own column.** At the top of a ramp, after a 90° turn, a ramp goes into your own tile one level up, facing the new way. You jump so it slides under you.
2. **Pieces may overlap the builder.** The new ramp lifts the builder (the yellow and push-out rules [1][5]), and walls may be placed right against you.
3. **The facing re-snaps the instant you turn,** and walls land on your own tile's edges behind or beside you.
4. **Turbo keeps painting while you turn and jump.**
5. **Targeting works mid-air** from your current height.

### Waterfall, tunnels and boxes

| Technique | What it is | What the system needs |
|---|---|---|
| Waterfall | While falling, build walls or a ramp or floor below you, then jump off and repeat, "3 walls or less" per drop [34\*] | Looking down while airborne targets slots below your level. Pieced has no fall damage, so this is low priority. |
| Tunnel | Floor below, floor above, walls to the sides. A full tunnel adds a front wall to edit. [32\*][30] | Looking down gives your own tile at your level; looking up gives your own tile one level up; side walls sit on your own edges |
| 1×1 box | Four walls around you, a floor and cone on top, then a cone or ramp below [50\*][30] | Walls always on your own tile's edges wherever you stand, allowed to overlap you, plus fast 90° turns |

---

## 5. Editing

### Grids

- Epic v4.50 (2018): selected tiles show "opaque rather than clear" [14].
- Tiles are numbered **1 2 3 / 4 5 6 / 7 8 9**, top-left to bottom-right.

| Piece | Grid | Selection | Tile size (ours) |
|---|---|---|---|
| Wall | 3×3 [1][27][28] | Click tiles to remove them | 1.33 m wide × 1.0 m tall |
| Floor | 2×2 [27][28] | Click tiles to remove them | 2 × 2 m |
| Stairs | 2×2 path [27][28][1] | **Drag** a path: hold, drag across the tiles, release [29] | 2 × 2 m |
| Cone | 2×2 corners [1][27] | Each tile lowers or raises a corner; any configuration is allowed [1] | 2 × 2 m |

- Walls and floors allow only a fixed set of shapes. The cone accepts any configuration [1].

### Enter, confirm, reset

| Step | How it works |
|---|---|
| Enter | Press **Building Edit** while aiming at your own piece. PC default is **G** [43][44]. Controllers have an "Edit Hold Time" setting [26]. |
| Select | Select Building Edit (**LMB** by default [44]): click tiles, or hold and drag across them [29] |
| Confirm | Press edit again. Or confirm on release: "Edit Confirm On Release", v10.30 in 2019 [18], replaced in v24.30 by **Auto Confirm Edits** with Off, Edit, Reset or Both [19][19\*]. Editing guides call confirm-on-release the biggest single setting for edit speed [52]. |
| Reset | **Reset Building Edit** (**RMB** in edit mode by default [43][44]) restores the full piece. Since Chapter 6 Season 4 (2025), a **Reset Building** bind resets an edited piece you're aiming at *without entering edit mode* [24][25]. |
| Simple Edit | v33.00: one press edits "based on the part of the building you're looking at", with a smaller set of edits [20] |
| Pre-edit | Edit a piece before placing it. It can be disabled. [26] |
| Who can edit | Only you and your teammates [1] |

**Timing:**
- Epic publishes no edit or reset delay.
- The edit simply applies when confirmed. v7.20 made editing stop interrupting gunfire [15].
- A top player managed about **60 edits in 7 s**, roughly 0.12 s per edit-and-reset cycle [47\*]. Input speed is the only real limit.

### Common edits

| Edit | Tiles | Ours (opening) |
|---|---|---|
| Window | Wall tile 5 (or 4 or 6) [27][28] | 1.33 × 1.0 m, spanning 1.0–2.0 m height (eye 1.62 m) |
| Door | Tiles 5 + 8, two stacked [28][29\*] | 1.33 × 2.0 m (player is 1.8 m tall, 0.7 m wide) |
| Half wall | Remove the top row, 1–3 ("mid wall"), or the top two rows, 1–6 ("low wall") [27] | Walls 2.0 m or 1.0 m high |
| Arch | Tiles 5, 7, 8, 9 [28] | |
| Triangle | An L of 3 tiles in one corner cuts the wall diagonally [28] | |
| Pillar | Remove 6, leaving one vertical column [27] | |
| Floor hole or corner | 1 of 4 [27][28] | 2 × 2 m hole |
| Half floor | 2 adjacent [27][28] | |
| Half ramp | Drag 2 tiles along one side [27] | A 2 m-wide stair |
| L or U stairs | Drag 3 tiles (L) or 4 tiles (U, a 180° turn) [27][28] | |

**Edited pieces:**
- Removed tiles have no collision and let bullets and players through (implied by doors and windows [27]).
- Whether an edit keeps the piece's current HP is **not documented**.

---

## 6. Cone (roof, "pyramid")

- **Shape:** a pyramid 5.12 × 5.12 × 1.92 m. It sits on a cell's floor position, so it caps a box at the top of its walls [1][50\*].
- **Edits:** it has a 2×2 corner grid [27]:
  - 1 tile: "tall triangle", an angled peak;
  - 2 adjacent: "roof-ramp", a directional slope;
  - 2 diagonal: "for shelter" [28];
  - 3 tiles: "tent" [27].
- **When players use it:**
  - capping a 1×1 box [30][50\*];
  - blocking an opponent's movement, and "piece control" (taking the cell above or next to an enemy) [50\*];
  - cone peeks, flipped with an edit and then reset [24];
  - trapping, e.g. editing a cone open, throwing explosives in, and resetting it [53\*].

---

## 7. Weapons (short: are Pieced's guns close?)

| | Fortnite now (Ch7 S4) | Pieced |
|---|---|---|
| **Rifle (SCAR, legendary)** | 36 damage, 5.5 shots/s, **25-round** magazine (since v40.00), 2.25 s reload, **1.5× headshot** (1.75× → 1.6× → 1.55× → 1.5× during 2026) [35] | 28 damage, 6 shots/s, 30 rounds, 2.0 s, 1.5× |
| Rifle falloff | Starts at 50 m, 80% at 75 m, 66% at 95 m [35] | Full to 25 m, 70% at 50 m |
| Rifle time to kill (200 HP, body) | 6 hits in about 0.91 s | 8 hits in about 1.17 s (by design, 1–2 s) |
| **Rifle bloom** | Hitscan with first-shot accuracy, which dates from 2018 [39]. You get it standing or crouched and not running, after pausing fire [35][40][38]. Crouching resets it faster and tightens spread [35]. Jumping widens hip spread (v13.00, v23.00) [35]. ADS tightens spread "significantly" [35]. v20.00 made the first-shot state easier to reach [51]. | First-shot accurate, +0.3° per shot up to 1.8°, recovering after 0.3 s at 8°/s. ADS multiplies spread by 0.4. No crouch or move modifiers. |
| Example spread modifiers | Burst AR, v5.40 (2018): ADS −40%, crouch −20%, jumping or falling +10%, sprinting +30% [37] | none |
| **Pump** | 10 pellets in a **fixed** pattern (v5.0, 2018), **star-shaped** since v42.00. ADS tightens it. [36] | 10 pellets, fixed 4.5° pattern, ADS ×0.8 |
| Pump damage | 87–119 body (legendary 119), **2× headshot** (was 1.85×), capped at 165–205, 0.7 shots/s, "minimum pellet-hit count of 3" [36] | 100 max body, 1.5× headshot per pellet, 0.9 s between shots |
| Pump falloff | Starts at 7 m, 78% at 10 m, 49% at 15 m, no damage past 31 m [36] | Starts at 8 m, 30% by 15 m |
| Structure damage | Rifle 36 per hit; pump 45–55 per shot (cut 50% in v4.50) [35][36][14] | Rifle 28 per hit; pump up to 100 per shot |

**Verdict:**
- The guns are **close** in structure: hitscan, first-shot accuracy with bloom, a fixed pump pattern, and one pump body shot never kills.
- The real differences:
  - Fortnite's pump headshot is 2× and ours is 1.5×;
  - Fortnite's pump is weak against builds and ours cracks boxes;
  - Fortnite adds crouch and movement spread modifiers.
- Jake is happy with the guns (D34), so none of this needs changing.

---

## 8. Controls

### Fortnite PC defaults

| Action | Default |
|---|---|
| Move | W A S D [43][44] |
| Jump | Space |
| Sprint | **Left Shift** [43][44] |
| Crouch | **Left Ctrl**. Hold while running to **slide** (since v19.00). [44][41] |
| Fire / place | LMB |
| ADS ("Target") | **RMB, held**. A **Toggle Targeting** option exists. [44][26] |
| Reload / rotate piece | R [43] |
| Weapon slots | 1–5 [44] |
| Wall / floor / stairs / roof | **F1 / F2 / F3 / F4** [43][44b][44c]. One site lists Z / X / C / V instead [44]. |
| Edit | **G**; select tiles with LMB; reset with RMB [43][44] |
| Change material | RMB in build mode [43] |

**Sprint history:**
- "**Sprint By Default**" was added in v5.10 (the sprint key then walks), made the default for new players in v6.30, and turned on for all players in v12.10 [5][42].
- The **Toggle Sprint** and **Sprint By Default** settings still exist [26].
- Chapter 3 added a faster **tactical sprint** on Shift that stows the weapon and uses stamina [40]. ADS isn't possible while sprinting [40].
- Speeds: run 5.48 m/s, sprint 7.39 m/s, crouch-run 4.18 m/s [40]. Pieced's are 5.5, 7.5 and 2.8.

### What players remap

The defaults are widely called unusable: F1–F4 "force your index finger to do too much work" [44c].

- **With side mouse buttons:** wall on Mouse 5, stairs on Mouse 4, floor on X, cone on Left Shift, edit on F, reset on scroll wheel [45].
- **Without side buttons:** "wall on Q, stairs on C, floor on X, and cone on Left Shift", with edit on F, E or G [45].
- **Other layouts:**
  - Q / Mouse 4 / E / Mouse 5 for wall, floor, stairs and roof, edit on F, crouch-while-building on Left Shift, sprint-by-default on [46];
  - cone on V [45].
- **Laptops:** "avoid binds that require precise far reaches" [45\*].
- No Fortnite source addresses trackpads.

---

## Recommendations for Pieced

All of these are mapped to the 4 m / 3 m grid. Numbers are starting points for the tuning panel. The *Synthesis* caveat from section 2 applies to every targeting rule.

### Targeting

- **T1. Keep the four-way facing.** Add about 5° of hysteresis at the 45° boundaries so the preview doesn't flicker. Rotate stays out of scope.
- **T2. Pick floor, ramp and cone tiles by 2D ray landing.**
  1. Intersect the eye ray with the build plane: your base level's ground when looking down, the next level's plane when looking up.
  2. Target the tile under the landing point, within one tile of yours in any direction, diagonals included.
  3. If the ray doesn't land within reach, fall back to the tile ahead at the ahead level.
  4. Clamp the own-tile choice so any look of 55° or more down always means your own tile, and any look of 10° or less down never does.
  5. **First-person forward bias:** while you're moving forward faster than run speed, push the landing point 1.5 m further along your facing (about 0.2 s of sprint), so a casual downward look while rushing lands ahead, not under you. This makes up for Fortnite's camera sitting behind the player (*Synthesis*).

  This keeps today's behaviour at the tile centre when standing still (own tile at 39° or more down; tile ahead at 15–39°; overhead at 35° or more up) and adds side tiles and double ramps.
- **T3. Ramp continuation.** When your feet are on a ramp that rises along your facing:
  - base the level on **that ramp's slot**, not on `feet.y` (this removes the float-edge risk);
  - "ahead" means the next tile at the ramp's level + 1;
  - never target your own column one level up (the cell above your ramp).

  Once you've turned 90° or more away from the ramp's rise, your own column one level up becomes legal again, which makes 90s work.
- **T4. Walls.**
  - Always place walls on **your own tile's edge** in the facing direction. Drop the "skip to the next line when closer than a body width" rule.
  - While climbing straight, put the wall **at the high end of the ramp ahead**: the far edge of the next tile, one level up. Looking down puts it under your own ramp's top instead. Never use the edge between your ramp and the next one.
  - Looking up past the top of the level still builds one higher.
- **T5. Overlap instead of rejecting.**
  - Allow walls and ramps that overlap the builder. Push the builder toward the centre of the tile their feet are in, or up onto a new ramp's surface.
  - Allow overlaps with other characters too: they phase through the piece until clear, which is Fortnite's yellow state [1][5].
  - Keep `BlocksCharacter` only as a fallback when no push-out is possible.
- **T6. Reach.**
  - `reach_tiles = 1` around your tile by default, tunable to 2. Vertically, from your base level −1 (looking down from a ledge or while falling) to +1.
  - Fortnite's reach is unverified, and a long first-person reach makes placement fiddly [game-feel].

### Turbo and ramp rush

- **R1. Keep the timings.** 0.05 s turbo and the 0.15 s rebuild lock match Fortnite exactly [8][9][11].
  - Switching piece keys mid-hold must place the new piece on the next tick without a new click (Fortnite's 0.15 s first delay existed only to allow switching, and is now 0.05 s [8][9]).
  - An invalid preview places nothing.
- **R2. No slowdown.** Auto-sprint stays active in build mode. Fortnite allows building in every movement state [40].
- **R3. Ramp-rush acceptance (headless).** Sprint up an endless rush with the pitch at −25°, −10°, 0° and +10°:
  - every ramp lands at (x+k, L+k);
  - no piece lands behind or above the player;
  - no slowdown occurs.
  - Repeat for ramp + wall (the wall at the high end of each next ramp) and for ramp + floor + wall.

### 90s, boxes and tunnels

- **R4. Make 90s possible.** With T3–T5 in place, add a scenario of wall, turn 90°, ramp in your own column one level up, jump, repeated four levels in at most about 3 s.
- **R5. Make boxes form around you.** A 1×1 box, four walls plus a floor or cone on top, must complete from anywhere inside the tile. T4 guarantees the walls. The existing ≤ 1 s box test should also run from the tile's corners.
- **R6. Tunnels:** a floor at your level when looking down, and one level up when looking steeply up, from anywhere in the tile.

### Build-up HP (optional)

- **R7.** Fortnite pieces grow, e.g. a wood wall from 60% to 100% in 4 s [4]. Pieced spawns at full HP to honour "a wall soaks at least 1 s of rifle fire".
- If Jake wants the feel, start at 85% (170 HP, about 1.0 s of fire) and grow to 100% over 3 s.
- Otherwise keep the current behaviour.

### Editing (new)

- **R8. Grids and shapes.**
  - Wall: 3×3 tiles of 1.33 × 1.0 m, limited to a whitelist of shapes: window, door, mid wall, low wall, arch, triangle, pillar, plus mirrored variants.
  - Floor: 2×2 tiles of 2 × 2 m.
  - Ramp: 2×2 drag path, giving half ramp and L and U stairs.
  - Cone: 2×2 corners, any configuration.
  - An invalid selection shows red and won't confirm.
- **R9. Flow and keys.**
  - **G** opens edit on your piece under the crosshair (within build reach), on key-down with no hold time.
  - A **trackpad press** toggles a tile; press and drag paints tiles; **release confirms**, like Fortnite's Auto Confirm "Edit" [19], and this is on by default. G again also confirms.
  - **R** in edit mode resets the piece and exits.
  - **R** while aiming at an edited piece outside edit mode is a quick reset, like Fortnite's "Reset Building" [24].
  - Esc, 1 or 2 cancel.
- **R10. Timing and behaviour.**
  - Open, confirm and reset all apply on the same tick; don't add artificial delays (pros manage about 0.12 s per cycle [47\*]).
  - An edited piece keeps its **HP fraction**. Removed tiles lose collision and let bullets through. A reset restores the full shape at the same HP.
  - Optional toggle: **Simple Edit**, where G alone applies the obvious edit for the spot you look at (lower middle → door, centre → window, upper row → low wall). It's trackpad-friendly and is Epic's own accessibility feature [20].

### Cone (new)

- **R11. The piece:** a 4 × 4 × 1.5 m pyramid in its own `Cone` slot, able to share a cell with a floor and a ramp.
  - It targets like a floor: looking steeply up puts it above you (capping a box); looking level or down puts it in the tile ahead.
  - HP matches the floor (170). It uses the 2×2 corner edits from R8.
  - Bind it to **V**.

### Weapons (no change needed; optional polish)

- **R12. Optional polish only.**
  - A crouch spread bonus (−20%) and a jump spread penalty (+10%), from Fortnite's own modifier pattern [37].
  - Raise the pump headshot to 2× if headshots feel weak.
  - Everything else is already Fortnite-shaped.

### Proposed key layout (WASD plus trackpad)

| Input | Action |
|---|---|
| W A S D | Move, with **auto-sprint always on** (Fortnite's Sprint By Default [42]). ADS, crouch and reloading drop you to run speed. |
| **Left Shift (hold)** | **ADS**. A settings toggle switches it to press-to-toggle, like Fortnite's Toggle Targeting [26]. No effect in build or edit mode. |
| Space | Jump |
| C (hold) | Crouch; slide when pressed while moving at sprint speed (unchanged) |
| Trackpad move | Look and aim only |
| Trackpad press | Fire or place. Hold for full-auto or turbo. In edit mode: select or drag tiles, and release to confirm. |
| 1 / 2 | Rifle / pump (also leaves build and edit mode) |
| **Q / E / F / V** | Wall / ramp / floor / **cone** (V is freed when ADS moves to Shift; a common Fortnite cone bind [45]) |
| **G** | Edit (Fortnite default [43]) |
| R | Reload. Also: reset while in edit mode, and quick-reset an aimed edited piece in build mode. |
| Esc / F3 / F4 | Pause / performance overlay / tuning (unchanged) |

- **Two-finger click:** can stay as an alternative ADS, off by default, so the trackpad is only for looking, aiming and clicking (D35).
- **Left Ctrl is avoided:** on macOS, Ctrl + trackpad click is the system's right-click gesture (*untested* in Pieced; noted as a risk only).
- **Test on the Air early:** a Force Touch press-and-drag for tile painting, while holding keys.

### Tests to add

Headless, through `PlayerIntent`:
- the ramp-rush grid from R3;
- a 90s tower;
- boxes started from each corner of the tile;
- double ramps via sideways aim;
- wall + ramp rush never self-blocks;
- edit shapes: collision removed, HP kept, reset restores;
- quick reset;
- cone placement and edits.

---

## Sources

1. Fortnite Wiki, "Building" (wikitext via the MediaWiki API): dimensions, grids, colours, turbo and edit history. https://fortnite.fandom.com/wiki/Building
2. Epic, UEFN "Architectural Modeling Guidelines". https://dev.epicgames.com/documentation/en-us/uefn/architectural-modeling-guidelines-in-unreal-editor-for-fortnite
3. Epic, "Fortnite-Ready Assets Best Practices" (player 192 cm, grid 512). https://dev.epicgames.com/documentation/fortnite/fortniteready-assets-best-practices-in-fortnite
4. Kr4m, "Best Material for Building, Season 9" (June 2019). https://kr4m.com/fortnite-material/
5. Fortnite Wiki, "Update v5.10" (patch notes, July 2018). https://fortnite.fandom.com/wiki/Update_v5.10
6. GameRevolution, "Fortnite Building Nerf" (v5.10). https://www.gamerevolution.com/guides/410337-fortnite-building-nerf-new-material-health-pickaxe-damage-other-changes
7. Fortnite Wiki, "Materials (Battle Royale)". https://fortnite.fandom.com/wiki/Materials_(Battle_Royale)
8. Fortnite Wiki, "Update v4.30". https://fortnite.fandom.com/wiki/Update_v4.30
9. Fortnite Wiki, "Update v7.40". https://fortnite.fandom.com/wiki/Update_v7.40
10. Fortnite Wiki, "Update v10.20". https://fortnite.fandom.com/wiki/Update_v10.20
11. FortniteNews, "Fortnite reverts controversial turbo build changes" (Aug 2019). https://fortnitenews.com/fortnite-reverts-controversial-turbo-build-changes/
12. PC Gamer, "Fortnite update 3.0.0 adds extensive building improvements" (Feb 2018; quotes Epic's blog). https://www.pcgamer.com/fortnite-update-300-adds-extensive-building-improvements/
13. Fortnite Wiki, "Update v3.00". https://fortnite.fandom.com/wiki/Update_v3.00
14. Fortnite Wiki, "Update v4.50". https://fortnite.fandom.com/wiki/Update_v4.50
15. Fortnite Wiki, "Update v7.20". https://fortnite.fandom.com/wiki/Update_v7.20
16. Fortnite Wiki, "Update v9.20". https://fortnite.fandom.com/wiki/Update_v9.20
17. Fortnite Wiki, "Update v5.20". https://fortnite.fandom.com/wiki/Update_v5.20
18. Fortnite Wiki, "Update v10.30". https://fortnite.fandom.com/wiki/Update_v10.30
19. Fortnite Wiki, "Update v24.30" (Auto Confirm Edits). https://fortnite.fandom.com/wiki/Update_v24.30 ; the Off / Edit / Reset / Both option list\*: https://x.com/iFireMonkey/status/1653354480031739904
20. Fortnite Wiki, "Update v33.00" (Simple Edit). https://fortnite.fandom.com/wiki/Update_v33.00
21. Fortnite Wiki, "Update v39.00" (Simple Build). https://fortnite.fandom.com/wiki/Update_v39.00
22. Game Rant, "How to Toggle Simple Building". https://gamerant.com/fortnite-how-simple-build-edit-toggle-off-on/
23. VGC, "Epic details Fortnite Chapter 7's gameplay changes" (Nov 2025). https://www.videogameschronicle.com/news/epic-details-fortnite-chapter-7s-gameplay-changes-including-simple-build-and-self-revive/
24. Beebom, "Reset Building setting" (Aug 2025). https://beebom.com/fortnite-players-discover-secret-setting-that-makes-edits-ridiculously-quick/
25. Epic Developer Community forum, "Keybind 'Reset Building' causes an Editing Bug" (Oct 2025). https://forums.unrealengine.com/t/keybind-reset-building-causes-an-editing-bug/2666388
26. Fortnite Wiki, "Settings" (weirdgloop mirror, same text). https://fortnite.weirdgloop.org/w/Settings
27. MMO Auctions, "Fortnite Editing Guide" (Feb 2020). https://mmoauctions.com/news/fortnite-editing-guide-know-your-walls-before-you-crash-into-one
28. gaming-tools.com, "Every Fortnite building edit". http://gaming-tools.com/fortnite/edit-buildings/
29. Get Hyped Sports, "Complete Fortnite Editing Guide" (Dec 2018). https://gethypedsports.com/fortnite-editing-guide/ ; door as tiles 5 + 8\* (via search summary)
30. Dignitas, "A Beginner's Guide to Building in Fortnite" (June 2024). https://dignitas.gg/articles/a-beginner-s-guide-to-building-in-fortnite
31. \*GamersRDY, "The Basics of Ramp Rushing" and "Double Ramp-Floor-Wall Ramp Rush" (2021; site timed out, read via search summary). https://gamersrdy.com/fortnite/2021/07/06/the-basics-of-ramp-rushing-in-fortnite/ , https://gamersrdy.com/fortnite/2021/11/12/double-ramp-floor-wall-ramp-rush/
32. \*GamersRDY tunnel guides (via search summary). https://gamersrdy.com/fortnite/2021/10/01/how-to-build-the-standard-tunnel-in-fortnite/ , https://gamersrdy.com/fortnite/2021/07/06/how-to-do-the-floor-ramp-tunnel-in-fortnite/
33. Fortnite Wiki, "Community Terminology" (90s). https://fortnite.fandom.com/wiki/Community_Terminology
34. \*Waterfall technique (search summary of a TikTok and AllKeyShop guide). https://www.allkeyshop.com/blog/building-guide-for-fortnite-mastering-techniques-and-strategies-for-success-gaming-news-r/
35. Fortnite Wiki (weirdgloop), "Assault Rifle / High Tier" (v42.10 stats and history). https://fortnite.weirdgloop.org/w/Assault_Rifle/High_Tier
36. Fortnite Wiki (weirdgloop), "Pump Shotgun" (v42.10 stats and history). https://fortnite.weirdgloop.org/w/Pump_Shotgun
37. Fortnite Wiki, "Update v5.40" (Burst AR spread modifiers). https://fortnite.fandom.com/wiki/Update_v5.40
38. ArenaFPS, "Fortnite Bloom" (Jan 2018) and "First Shot Accuracy" (Apr 2018). https://arenafps.com/fortnite-battle-royale-weapon-bloom-mechanics/ , https://arenafps.com/fortnite-first-shot-accuracy-new-shooting-mechanics/
39. Fortnite Wiki, "Shooting Test 1" (2018 origin of first-shot accuracy). https://fortnite.fandom.com/wiki/Shooting_Test_1
40. Fortnite Wiki (weirdgloop), "Movement" (speeds, and what's allowed in each state). https://fortnite.weirdgloop.org/w/Movement
41. Fortnite Wiki, "Update v19.00" (sliding; build while sliding). https://fortnite.fandom.com/wiki/Update_v19.00
42. Fortnite Wiki, "Update v6.30" and "Update v12.10" (Sprint by Default). https://fortnite.fandom.com/wiki/Update_v6.30 , https://fortnite.fandom.com/wiki/Update_v12.10
43. Fortnite Wiki, "Controls" (PC defaults). https://fortnite.fandom.com/wiki/Controls
44. OnlineGameCommands, "Default PC Fortnite Keybinds" (Jan 2026). https://www.onlinegamecommands.com/default-pc-fortnite-keyboard-keybinds/ ; [44b] keymap.io (Sept 2026) https://keymap.io/controls/fortnite/ ; [44c] keyboardcontrols.com https://keyboardcontrols.com/shortcuts/games/fortnite/
45. setup.gg, "Best Fortnite Keybinds 2026" (Sept 2026). https://www.setup.gg/game/fortnite/best-keybinds/ ; \*laptop advice via search summary of the same page
46. Charlie INTEL, "Best keybinds and settings" (Dec 2024). https://www.charlieintel.com/fortnite/the-best-keybinds-and-settings-for-fortnite-on-mouse-keyboard-84934/
47. \*Sportskeeda, "Fortnite: World's fastest editors" (the 60 edits in 7 s figure, via search summary). https://sportskeeda.com/fortnite/fortnite-world-s-fastest-editors
48. Fortnite Wiki (weirdgloop), "Update v42.00", "v42.10", "v41.10" (current season, turbo fixes). https://fortnite.weirdgloop.org/w/Update_v42.00
49. \*Z League, "Master Fortnite Build Mechanics: Chapter 3 Guide" (three-tile reach claim; page now 404). https://www.zleague.gg/theportal/fortnite-building-guide/
50. \*Sportskeeda, "How to build and box fight in Fortnite" (box and cone use, via search summary). https://www.sportskeeda.com/esports/how-build-box-fight-fortnite
51. Fortnite Wiki, "Update v20.00" (first-shot accuracy smoothing). https://fortnite.fandom.com/wiki/Update_v20.00
52. Gamertag Mythras, "Fortnite Editing Guide" (May 2026, updated Aug 2026). https://gamertagmythras.com/blog/fortnite/fortnite-editing-guide
53. \*Fortnite Wiki (weirdgloop), "Dynamite" strategy note (seen only as a wiki search snippet). https://fortnite.weirdgloop.org/w/Dynamite

Pieced references: `src/building/targeting.rs` (targeting rules), `src/building.rs` (turbo and tuning), `src/combat.rs` (bloom numbers), `src/input.rs` (current keys), `docs/research/game-feel.md` ("[game-feel]").
