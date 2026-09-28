# Backlog

Features Jake wants later. They're not in the current milestone's scope; each becomes part of a future spec.

| Added | Item | Notes |
|---|---|---|
| 2026-09-26 | **Key rebinding** | Jake: "I should be able to change my own keybinds." Excluded from M1 and M2. Add a Controls page to Settings: rebind every `PlayerIntent` action to a key or trackpad click, reset to defaults, detect conflicts, and save the bindings with the other settings. The input adapter (`src/input.rs`) is the only place that reads devices, so rebinding lives there. |
| 2026-09-24 | **Bots** (Milestone 3) | Now the Waves mode (D54–D70 in `docs/design/DESIGN-GRILL.md`). Knights are driven through `PlayerIntent`; see `docs/research/bot-ai.md`. |
| 2026-09-27 | **More knight types** | Brute (smashes walls), mage (lobs spells over walls), builder knight (ramps up to you), unlocking at waves 3, 5 and 8, per D60. After the grunt wave is fun. |
| 2026-09-27 | **Economy** | Limited materials and ammo, knight drops, a Zombies-style points shop and gun upgrades (D62). |
| 2026-09-27 | **More content** | New maps and guns to keep endless runs fresh (D58); a 1v1 build-fight mode; cosmetics such as robe colours and gun skins. |
| 2026-09-27 | **Rename the game and name the modes** | D66: "Waves" is a placeholder. |
| 2026-09-28 | **Bigger map and more progression** | Jake after play-test 2: "map should be bigger with more progression / stuff to do... but thats out of scope". Pairs with More content (new maps) and the Economy (points shop, upgrades). |
| 2026-09-28 | **After Waves: an AAA look pass, then story and progression** | Jake after play-test 3: "when we finish this pass, when we start making it look like a true AAA game, and then after that, when we start doing the story behind it and actual progression, it's going to be so fun." That's the order of the next milestones: look, then story and progression. |
