# Assets

Every file under `assets/` and where it comes from. Everything here is original to
Pieced (MIT, like the code) and regenerable from the repo, except files marked
with a third-party license. `tests/assets.rs` fails if a file is missing from
this table, a row names a file that doesn't exist, or a row doesn't point at an
existing source script or license file in its second column.

## Models (`assets/models/`)

Built headless by Blender 5.2 from Python scripts in `art/blender/`:

```sh
scripts/build-art.sh              # rebuild every model (+ previews in art/previews/, git-ignored)
scripts/build-art.sh rock_a       # rebuild one
scripts/build-art.sh --check      # fail unless the committed files match a fresh build
```

Each model is a `.glb` (palette colours as glTF `COLOR_0`, one white `Toon`
material, named nodes) plus a sidecar `.json` with per-part bounds, triangle
counts and attach points in Bevy model space (metres, +Y up, -Z forward; see
`art/blender/lib/export.py`). `src/models.rs` embeds and loads them. Colours come
from `art/palette.json`, sampled from the target images by
`art/tools/sample_palette.py`.

| File | Made by | Notes |
|---|---|---|
| `models/manifest.json` | `art/blender/build.py` | Every model the game loads (`[{name, file}]`) |
| `models/axis_probe.glb` | `art/blender/assets/probe.py` | Orientation test fixture: `Forward` empty 1 m in front |
| `models/axis_probe.json` | `art/blender/assets/probe.py` | Sidecar |
| `models/gloves.glb` | `art/blender/assets/gloves.py` | White four-finger cartoon gloves (`GloveR`, `GloveL`) with dark sleeves; each pivots on its grip frame |
| `models/gloves.json` | `art/blender/assets/gloves.py` | Sidecar |
| `models/pump.glb` | `art/blender/assets/guns.py` | Pump viewmodel: bell muzzle, sliding `PumpGrip`, violet `Crystal` in spinning gold `Rings`, `Shard` shell |
| `models/pump.json` | `art/blender/assets/guns.py` | Sidecar: `MuzzleTip`, `CrystalSocket`, `GripR`, `GripL` (on `PumpGrip`), `Sight`, `SightFront` |
| `models/rifle.glb` | `art/blender/assets/guns.py` | Rifle viewmodel: brass, dark wood, glass `Chamber` with a blue `Crystal`, energy-cell `Mag` |
| `models/rifle.json` | `art/blender/assets/guns.py` | Sidecar: `MuzzleTip`, `CrystalSocket`, `GripR`, `GripL`, `Sight`, `SightFront` |
| `models/rock_a.glb` | `art/blender/assets/props.py` | Arena rock, 1.25 m, crouch cover (D29) |
| `models/rock_a.json` | `art/blender/assets/props.py` | Sidecar |
| `models/rock_b.glb` | `art/blender/assets/props.py` | Arena rock with a buddy rock, 1.05 m (D29) |
| `models/rock_b.json` | `art/blender/assets/props.py` | Sidecar |
| `models/stump_a.glb` | `art/blender/assets/props.py` | Arena stump, 0.56 m, jumpable (D29) |
| `models/stump_a.json` | `art/blender/assets/props.py` | Sidecar |
| `models/tree_a.glb` | `art/blender/assets/props.py` | Puffy island-margin tree, about 5.7 m |
| `models/tree_a.json` | `art/blender/assets/props.py` | Sidecar |

## Shaders (`assets/shaders/`)

Hand-written WGSL, embedded into the binary by the Rust module that uses it.

| File | Made by | Notes |
|---|---|---|
| `shaders/sky.wgsl` | hand-written, used by `src/arena/visuals/sky.rs` | Milestone 1 sky dome |
| `shaders/target_rim.wgsl` | hand-written, used by `src/arena/visuals/target.rs` | Milestone 1 dummy rim light |

## Fonts (`assets/fonts/`)

The only third-party files: two openly licensed cartoon display fonts from
`github.com/google/fonts`, downloaded with Jake's OK (D23). The HUD keeps one of them.

| File | Made by | Notes |
|---|---|---|
| `fonts/LuckiestGuy-Regular.ttf` | `assets/fonts/LuckiestGuy-LICENSE.txt` | Luckiest Guy by Astigmatic, Apache 2.0 (`apache/luckiestguy`) |
| `fonts/LuckiestGuy-LICENSE.txt` | `assets/fonts/LuckiestGuy-LICENSE.txt` | Its license |
| `fonts/LilitaOne-Regular.ttf` | `assets/fonts/LilitaOne-OFL.txt` | Lilita One by Juan Montoreano, SIL OFL 1.1, Reserved Font Name "Lilita" (`ofl/lilitaone`) |
| `fonts/LilitaOne-OFL.txt` | `assets/fonts/LilitaOne-OFL.txt` | Its license |

## This index

| File | Made by | Notes |
|---|---|---|
| `ASSETS.md` | hand-written, checked by `tests/assets.rs` | This file |

## Adding a model

1. In a family module under `art/blender/assets/` (e.g. `props.py`), write
   `build_<name>(root)`: make named parts with `scene.make_part`, colour faces by
   palette name with `palette.tag` / `palette.paint_object` (names from
   `art/palette.json`), add attach points with `scene.make_attach`, model
   facing Blender -Y with the pivot on the ground. Register it in the module's
   `ASSETS` list as `Asset("<name>", "<kind>", build_<name>)`; the kind sets
   the triangle budget (`art/blender/lib/registry.py`).
2. Run `scripts/build-art.sh <name>` and review `art/previews/<name>.png`
   (3/4 and front views); iterate until it reads like the targets.
3. Add `"<name>"` to `EMBEDDED_MODELS` in `src/models.rs` and rows for
   `models/<name>.glb` and `models/<name>.json` to the Models table, then run `cargo test`.
   Spawn it with `models::spawn_model`.

## Adding a file

Add a row in the matching table: the path relative to `assets/` in backticks,
then the script that makes it (or, for a third-party file, its license file,
e.g. `assets/fonts/OFL.txt`) in backticks. The only third-party files allowed in
Milestone 2 are the HUD fonts and their licenses (docs/M2-SPEC.md, Asset pipeline).
