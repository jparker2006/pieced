# Backlog

Features Jake wants later. They're not in the current milestone's scope; each becomes part of a future spec.

| Added | Item | Notes |
|---|---|---|
| 2026-09-26 | **Key rebinding** | Jake: "I should be able to change my own keybinds." Excluded from M1 and M2. Add a Controls page to Settings: rebind every `PlayerIntent` action to a key or trackpad click, reset to defaults, detect conflicts, and save the bindings with the other settings. The input adapter (`src/input.rs`) is the only place that reads devices, so rebinding lives there. |
| 2026-09-24 | **Bots** (Milestone 3) | A local utility-AI sparring bot driving `PlayerIntent`; see `docs/research/bot-ai.md`. |
