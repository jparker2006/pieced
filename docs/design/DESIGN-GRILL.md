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

## Handoff: where to pick up after compaction

- **Next:** Jake reviews the 12 target images (T01–T12). Regenerate any he rejects. Then write the design spec and the `/goal` brief.
- **Images:** the target board uses R4-M1 as a style reference (`codex exec ... -i R4-M1-brick-walls-wood-floors.png -- "<prompt>"`; the `--` is required because `-i` takes several values).
  - Use Codex CLI: `codex exec --skip-git-repo-check --ephemeral -m gpt-5.5 -s workspace-write -C docs/design/concepts "<prompt>" < /dev/null`.
  - Stdin **must** be `/dev/null`, or the job hangs.
  - Run **one at a time**. Parallel runs can copy each other's outputs.
  - Generated images stay local and are git-ignored.
- **Tools:** Blender 5.2.2 LTS is installed (`/Applications/Blender.app`, CLI `blender`). Free disk is about 21 GB.
- **Milestone 1:**
  - **Parked.** G1 and G6 PASS; the rest are pending (see `docs/evidence/M1-report.md`).
  - **G2 blocker:** battery performance. The UI-MSAA-off and 1.4 MP cap fixes (`0420120`) haven't been measured on screen. The viewmodel camera's MSAA writeback is the top suspect for the rest.
  - **Measurement conditions:** the Mac must be idle and plugged in (or on battery for G2), with no heavy background load.
- **Environment gotchas:**
  - `~/Documents` is iCloud-synced and over quota. The build cache now lives in `~/Library/Caches/pieced-target` (`scripts/env.sh`).
  - Scenario windows get covered whenever Jake uses the Mac. Never run full-screen tests while he's using it.
  - Jake's background services (a glox Python service, a VM) add CPU load.
