# M4 target board: art-review notes

Captures come from `tests/m4_board.rs` (offscreen, headless, the whole client
with the UI composited). Each round's JPEGs sit in `roundN/`. The targets
(`docs/design/concepts/M4-V1` … `V8`) stay local and are never committed.
Scores are the art slice's own predictions (1–5 against the target), not
Jake's.

## Round 0: baseline (`main` at `d092b78`)

| View | Predicted | Biggest gaps |
|---|---|---|
| V1 spawn mid-wave | 3 | Knights thin and dull grey-violet, like stick figures at 8 m. Grey-violet drop ships with no gold. Faceted trees. Lime lawn |
| V2 rifle ADS | 2.5 | The ADS pose shows the rifle's wooden back as a brown tube (viewmodel pose). Knight far and small |
| V3 pump blast | 2 | A kill at 3 m whites out the frame, so no knight is seen flying back |
| V4 headshot kill | 2.5 | The poof covers the knight; the helmet and armor don't read on the kill frame |
| V5 knight wind-up | 2.5 | The wind-up doesn't read: **the wand model pointed backwards in the glove** (a transform bug), so a raised wand hung its crystal under the fist |
| V6 box-up fight | 3.5 | Reads well. The bricks are a little flat, and no orbs burst on the walls in the frame |
| V7 wave banner | 2.5 | No banner yet (chunk 5, HUD). Ships small and grey |
| V8 main menu | 2 | A wide orbit shot with a row of buttons; the target has stacked buttons, a hero knight on bricks and the castle framed |

## Round 1: first art pass

What changed:
- **Knight** (`art/blender/assets/knight.py`), in the same hitboxes (S5 fit green):
  - bright silver steel (`knight_steel` `#828296` → `#A9AEC4`);
  - gold rivets round the face plate, a nose ridge down the grille, and catchlights in the pupils;
  - layered pauldrons (a cop over a lame) with rolled gold rims and a gold rivet;
  - gold trims on the vambraces and greaves, with rivets on the knee cops;
  - a velvet hat band edged in gold;
  - chunkier sleeves, gauntlets, fingers and thighs;
  - a fuller coat, and a robe belled out to the capsule's edge with deep pleats;
  - a dark velvet lining (`knight_purple_shadow`) inside the robe, coat and cape;
  - a bigger medallion and buckle.

  He is 10,988 tris with every eye state (about 10.7k shown).
- **Wand fix** (`src/arena/visuals/wand.rs`): the crystal now points forward and down at rest, and up toward the target when raised, where the orb leaves. A test pins it.
- **Trees**: round cloud puffs at 17 × 10 facets instead of faceted gems (≤ 3k tris).
- **Drop ship**: a gilded airship (brass hull, bronze belly, purple wings, blue glowing windows), as in V1 and V7.
- **Lawn**: a slightly deeper tone (`GRASS_TONE`), tighter dark patches (17 m tiles) and darker blade strokes.
- **V8 hero shot**: the logo and stacked buttons at the left. A knight stands on a brick plinth with his wand raised and blazing, and the camera drifts gently, framed low with the castle on the right and the galaxy over the buttons.

| View | Predicted | Gaps left |
|---|---|---|
| V1 | 3.5 | Knights better but still small and busy at 8 m. The lawn is still brighter and limier than the target. No rune circles under the beams |
| V2 | 2.5 | The ADS pose still looks down a brown tube (viewmodel pose, chunk 2's slice). An orb burst near the player tints the frame |
| V3 | 2.5 | The burst hides the knight; capture later so the take reads |
| V4 | 3 | An orb impact near the player washes the frame orange; the poof still hides the dented helmet on the capture frame |
| V5 | 3 | The wand now glows at the tip, but the pose isn't the target's wide hero stance (chunk 4's authored wind-up clip) |
| V6 | 3.5 | As before |
| V7 | 2.5 | Needs the chunk 5 banner. Capture the ships later, when they're bigger |
| V8 | 3.5 | The composition matches. The knight could be a little bigger, and the plinth top reads as stretched bricks |

Budgets: full-wave headless count 632k tris (knights 172k), 1,372 meshes in
150 batches (`tests/budget.rs`, now pinned at 665k / 1,500 / 160). The offscreen
full-wave GPU world pass is 4.15/5.57 ms (mean/p95) against 4.20/5.42 ms before
(`tests/wave_cost_offscreen.rs`), unchanged within noise.

The independent review of round 1 (the orchestrator's `pieced-art-reviewer`)
scored V1 3, V2 2, V3 2, V4 1.5, V5 2, V6 3, V7 3 and V8 3. My predictions
ran about half a point high; the reviewer's scores are the ones that count.

## Round 2: the chibi knight, a saucer, readable kills

What changed:
- **Knight: chibi, three values** (`knight.py`):
  - The helmet is as tall as the head sphere allows: a 0.41 m bucket, up from 0.3 m, still inside the sphere plus S5's 5 cm.
  - One tall dark visor band with bigger eyes, and **no grille bars** (they read as a skull's teeth at range).
  - The coat is closed over the chest (purple body, gold seam), the robe hem is at mid-shin and the boots are bigger.
  - The small rivets and trims are gone (value noise in greyscale).
  - The steel takes a new `polished` surface in `art/surfaces.json`: a hard, broad specular band.
  - The hat rides 8 cm higher on the taller helmet. It's cosmetic, so it's not in the fit.
  - 9,610 tris with every eye state.
- **Drop ship → gilded saucer** (`dropship.py`): a stepped brass disc with a bright rim, a purple underside, a ring of 12 blue portholes and a crystal canopy. Rune circles under the beams are brighter (`CIRCLE_INTENSITY` 2.0 → 2.6, halo 0.6 → 0.9).
- **Halo wash**: big halos (≥ 0.6 m) fade from 0.42 to 0.8 size/distance, so an orb's fire is gone by about 2 m from the eye. The guns' small glows are exempt. The board test now keeps bystanders from casting during the shooting views.
- **Kills**:
  - Pump pellet bursts have small cores and long violet and gold rays (V3's hollow starburst, the knight visible inside it).
  - The elimination poof is lower and smaller (it billows round his legs).
  - The aimed piece's "WALL 200/200" bar hides while a multi-kill callout shows (`src/hud/systems.rs`).
- **Wind-up** (V5): a wider tip flare, and three swirl sparks (halos, dark when idle) circling the tip through the wind-up.
- **Ground and grade**:
  - thinner, subtler grid lines (strength 0.3 → 0.22, glow 0.13 → 0.08, core 16 → 13 mm);
  - 40% more flower clusters, slightly bigger;
  - a warmer highlight split-tone in the grade.
- **V8**: a knee-high brick plinth with a plank cap and a crisp top edge, the camera a little lower and closer. The dropped hats from the board's kills are cleared first.

| View | Predicted | Gaps left |
|---|---|---|
| V1 | 3.5 | The saucers and beams read like the target. Knights are chunkier but still small at 7–9 m; no helmet sheen at that size |
| V2 | 2 | Not touched: the ADS pose is the viewmodel's (guns.py and the chunk 2 ADS pose own it) |
| V3 | 3.5 | A hollow violet starburst on a flailing knight. He's knocked back (gameplay knockback), so he reads a little small |
| V4 | 2.5 | The poof is smaller, but the headshot flash plus poof still hide the helmet pop on the kill frame |
| V5 | 3.5 | A bigger head with a clear visor band, and the glowing wand in his fist. The pose is not the target's hero stance (chunk 4's clip) |
| V6 | 3.5 | Clean, readable box-up; no orb impacts on the walls in the frame |
| V7 | 3 (world) | The saucer with its beam and circle; still needs chunk 5's banner |
| V8 | 3.5 | The composition matches. The dusk foreground isn't darker yet |

Budgets for round 2: full-wave headless count of 624k tris (knights 150k), 1,509
meshes in 153 batches (the mesh pin is now 1,560; the three swirl halos per
knight share the halo batch). Offscreen full-wave GPU: the world pass is 4.06/5.19 ms
(mean/p95), against 4.20/5.42 at baseline; total 8.31/10.16 ms.
