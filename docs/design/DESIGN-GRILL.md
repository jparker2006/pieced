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

## Handoff: where to pick up after compaction

- **Next:** grill round 4, then a new image round, then the spec and `/goal` brief.
  - Round 4 should settle: D12 (building material); outline technique (proposed: inverted-hull outlines on near objects, none on the far backdrop); enemy details (a cartoon B2 knight with Looney-wizard flavor); gun family details (A1/A2 build, both guns); exact far-view composition; and the done-state specifics (gallery views, performance gate, a "feel" check with Jake).
- **Images:** generate them with the combined direction (C-style close-up, Looney wizards, a cartoon B2 knight, the A-style gun, B1/C1 spells, D1 sky, a grassy floating island).
  - Use Codex CLI: `codex exec --skip-git-repo-check --ephemeral -m gpt-5.5 -s workspace-write -C docs/design/concepts "<prompt>" < /dev/null`.
  - Stdin **must** be `/dev/null`, or the job hangs.
  - Run **one at a time**. Parallel runs can copy each other's outputs.
  - Generated images stay local and are git-ignored.
- **Tools:** Blender 5.2.2 is being installed with Homebrew (`/Applications/Blender.app`), approved by Jake. Free disk is about 23 GB.
- **Milestone 1:**
  - **Parked.** G1 and G6 PASS; the rest are pending (see `docs/evidence/M1-report.md`).
  - **G2 blocker:** battery performance. The UI-MSAA-off and 1.4 MP cap fixes (`0420120`) haven't been measured on screen. The viewmodel camera's MSAA writeback is the top suspect for the rest.
  - **Measurement conditions:** the Mac must be idle and plugged in (or on battery for G2), with no heavy background load.
- **Environment gotchas:**
  - `~/Documents` is iCloud-synced and over quota. The build cache now lives in `~/Library/Caches/pieced-target` (`scripts/env.sh`).
  - Scenario windows get covered whenever Jake uses the Mac. Never run full-screen tests while he's using it.
  - Jake's background services (a glox Python service, a VM) add CPU load.
