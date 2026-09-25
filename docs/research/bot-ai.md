# Research: shooter bot AI (for Milestone 2)

Collected 2026-09-24. Milestone 1 does not build a bot. It builds one piece of infrastructure the bot will need: **a shared player-intent interface**.

## Architecture

- **Recommendation:** a hand-written **utility-scored mode selector**, re-scored every 100–200 ms with a bonus for staying in the current mode so the bot doesn't flip-flop. Inside each mode, run short scripted action sequences. Low-level aim and fire states are small FSMs.
- **Rejected:**
  - **Behavior trees:** their yes/no checks handle trade-offs between HP, ammo, materials and height poorly.
  - **GOAP:** plans go stale within hundreds of ms in a twitch 1v1.
  - **HTN:** worth it later for multi-step build tactics.
- **Killzone 3's "virtual controller" rule:** bots write **the same input struct the player's keys produce**. That keeps them fair and makes headless bot-vs-bot testing free. **Milestone 1 must therefore route all gameplay through a player-intent struct.**

## Tactics on the build grid

- **The build grid is the navigation graph.** A node is a cell, level and surface (ground, floor or ramp).
  - A successor function reads the live piece map on each search.
  - A wall on an edge blocks it; a ramp links level n to n+1; a floor adds walkable nodes.
  - Nothing needs rebuilding: when a piece changes, drop any cached path that crosses it and re-plan.
- **Path cost** = distance + climb + exposure to the enemy + decaying "danger" where the bot was hit.
- **Choosing a position (Crytek pattern):**
  1. Gather candidate cells.
  2. Filter cheaply by reachability and weapon range band.
  3. Score by cover, peek option, height, preferred range and exposure.
  4. Run expensive line-of-sight rays last, best score first.
- **"Virtual cover":** a cell where the bot can place a wall facing the enemy scores nearly as well as real cover.
- **Push or retreat** depends on:
  - HP and shield difference;
  - an observed enemy reload;
  - damage traded over the last ~3 s;
  - the enemy losing a wall;
  - own ammo and materials;
  - height difference.

## Human-like aim and fairness

- **Reaction time:** simple visual 0.19–0.22 s; go/no-go 0.38 s. Use 0.2–0.4 s, longer for faint or distant targets (Game AI Pro 2, ch. 5).
- **Counter-Strike bot:**
  - aim offset re-rolled periodically by skill;
  - view turned by a spring-damper;
  - tiers differ in aim quality and added fire delay.
- **Barriales (Game AI Pro 3, ch. 33):**
  - base 0.5 s before a hit is allowed;
  - under 5 m ×0.5; target crouching ×2; target in cover ×2; target running at the bot ×0.5;
  - misses land where the player can see them.
- **Model (Synthesis):**
  - Aim at a delayed snapshot of the target, extrapolated with a noisy velocity estimate.
  - First shot comes after reaction plus settle time.
  - The error cone tightens while tracking and widens when the bot is hit.
- **Starting tiers (guesses to tune against Jake's logged trackpad accuracy):**

| Tier | Reaction | Tracking lag | Error @10 m | Turn rate | Build rate |
|---|---|---|---|---|---|
| Easy | 450 ms | 250 ms | 60 cm | 180°/s | 1/s |
| Normal | 330 ms | 180 ms | 35 cm | 300°/s | 2/s |
| Hard | 260 ms | 140 ms | 20 cm | 450°/s | 3/s |
| Expert | 210 ms | 110 ms | 12 cm | 600°/s | 4/s |

## Bot building (design; Epic has published nothing)

- **Wall when shot:** hit in the open, wait at least 250 ms, then wall the edge facing the attacker. At most one wall per 0.8 s.
- **Take height:** a wall-then-ramp sequence when the enemy is higher or when pushing.
- **Box up:** when low, or when reloading.
- **Counter-build:** rifle the enemy's walls, then switch to the pump under about 8 m.

## Fun, not just good

- Telegraph intent: audible building and reloading.
- Commit to decisions instead of dithering.
- Expose personality knobs.
- **No cheating:** the bot works from a fading last-known position and never sees through walls.
- Adjust difficulty between rounds, never mid-fight.
- No damage from off-screen without an audio cue.

## Testing

- Run headless at a fixed timestep with seeded randomness, faster than real time.
- Run bot-vs-bot round-robins.
- **Metrics:**
  - line of sight to first shot;
  - hit rate by weapon and range;
  - TTK;
  - pieces placed or lost per minute;
  - time in cover while under fire;
  - mode switches per minute;
  - stuck events;
  - "unfair deaths".
- Show gizmo overlays for candidate cells and their scores.

## Crates (crates.io, 2026-09-24)

| Crate | Version | Verdict |
|---|---|---|
| pathfinding | 4.16.0 | **Use** for A* over the build grid |
| bevy_northstar | 0.7.0 (Bevy ^0.19) | Overkill for a small arena |
| bevior_tree | 0.11.0 (Bevy ^0.19) | BT option, not needed |
| bevy_gearbox | 0.9.0 (Bevy ^0.19) | FSM option |
| big-brain, oxidized_navigation | — | Archived; avoid |

## Milestone 2 sparring bot (preview)

- **Loops:** perception at 30 Hz (100° FOV, line of sight, memory, hearing reloads), decisions at 8 Hz (Engage, TakeCover, WallUp, Push, Reload, Reposition), and movement and aim at 60 Hz.
- **Done when**, over 50 headless fights at Normal:
  - no first shot comes under 250 ms after line of sight;
  - walls go up within 600 ms of being hit in the open;
  - there are zero "unfair deaths".

## Sources

- https://media.gdcvault.com/gdc04/slides/making_of_official.pdf
- http://www.gameaipro.com/GameAIPro2/GameAIPro2_Chapter05_Agent_Reaction_Time_How_Fast_Should_An_AI_React.pdf
- http://www.gameaipro.com/GameAIPro3/GameAIPro3_Chapter33_Using_Your_Combat_AI_Accuracy_to_Balance_Difficulty.pdf
- http://www.gameaipro.com/GameAIPro/GameAIPro_Chapter26_Tactical_Position_Selection.pdf
- http://www.gameaipro.com/GameAIPro/GameAIPro_Chapter29_Hierarchical_AI_for_Multiplayer_Bots_in_Killzone_3.pdf
- http://www.gameaipro.com/GameAIPro/GameAIPro_Chapter09_An_Introduction_to_Utility_Theory.pdf
- https://www.gamedeveloper.com/programming/gdc-2005-proceeding-handling-complexity-in-the-i-halo-2-i-ai
- https://gdcvault.com/play/1013282/Three-States-and-a-Plan
- https://www.engadget.com/2019-09-23-fortnite-epic-games-adding-bots.html
- https://crates.io/crates/pathfinding
