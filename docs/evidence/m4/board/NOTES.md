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
