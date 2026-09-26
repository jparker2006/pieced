# Research: look stack for the cartoon restyle

Checked 2026-09-25. Sources: the local sources of the pinned crates, Context7 (`/websites/rs_bevy`), the crates.io, PyPI and GitHub APIs, the installed Blender 5.2.2 app bundle, and the Blender manual source. Nothing was built, compiled or rendered. "Unverified" means I couldn't confirm it for bevy 0.19.1 or Blender 5.2 specifically.

`$REG` = `/Users/jakeparker/Documents/Codex/2026-09-09/i-m-thinking-about-building-minecraft/work/cargo/registry/src/index.crates.io-1949cf8c6b5b557f` is the Cargo registry that Pieced's lockfile resolves from.

## Decisions this supports

- **Toon shading:** write our own `Material` that keeps the default vertex shader and uses a fragment shader with a 2–3 band N·L ramp, lit by the first directional light.
  - A custom fragment shader skips the PBR light loops.
  - `StandardMaterial { unlit: true }` is just as cheap for flat-colour props.
- **Outlines:** `bevy_mod_outline 0.13.0` supports bevy 0.19 and uses vertex extrusion. A hand-rolled inverted hull is also easy. Avoid screen-space JFA crates on the Air.
- **Sky:** use a pre-baked (or startup-generated) galaxy cubemap on `Skybox`. Rotate it with `Skybox.rotation` each frame. Set `brightness: 1000.0` because the default is 0, which renders black.
- **glTF:** 0.19 renamed `SceneRoot` to **`WorldAssetRoot`** and `SceneInstanceReady` to **`WorldInstanceReady`**. On macOS, pipeline compiles are **synchronous**, so warm up every model and material variant behind the loading screen.
- **Bloom:** adding it forces `Hdr`, which changes how our two cameras (world + viewmodel) composite into one target. Prefer fake glow meshes. If we do use Bloom, make both cameras `Hdr` with the same `Msaa`, and test.
- **Blender MCP:** it sends an anonymous usage ping by default. Register it with `-e DISABLE_TELEMETRY=true`. All asset-service toggles default to off.

## 1. Custom materials (bevy 0.19.1)

**Trait shape.** `bevy_pbr::Material: Asset + AsBindGroup + Clone + Sized`, and every method has a default [1]:

- `vertex_shader()` and `fragment_shader() -> ShaderRef`. `ShaderRef::Default` means the stock mesh shaders.
- `alpha_mode(&self) -> AlphaMode` (default `Opaque`), `opaque_render_method`, `depth_bias(&self) -> f32` and `reads_view_transmission_texture`.
- `enable_prepass()` and `enable_shadows()` (both default `true`), plus the `prepass_*` and `deferred_*` shader hooks.
- `specialize(pipeline: &MaterialPipeline, descriptor: &mut RenderPipelineDescriptor, layout: &MeshVertexBufferLayoutRef, key: MaterialPipelineKey<Self>) -> Result<(), SpecializedMeshPipelineError>`.

Register a material with `MaterialPlugin::<M>::default()` and attach it with `MeshMaterial3d(handle)`. WGSL bindings use `@group(#{MATERIAL_BIND_GROUP}) @binding(n)`. Per-material pipeline data goes through `#[bind_group_data(Key)]`, which `specialize` reads as `key.bind_group_data`.

**Front-face culling.** In `specialize`, set `descriptor.primitive.cull_mode = Some(Face::Front);`. `StandardMaterial` does exactly this from its `cull_mode: Option<Face>` field, and flips it when the view has `INVERT_CULLING` [2].

That gives a **zero-shader inverted hull**: `StandardMaterial { base_color: INK, unlit: true, cull_mode: Some(Face::Front), .. }` on a CPU-inflated copy of the mesh. The outline width is then in world units.

**Alpha modes.** `AlphaMode` is returned per material instance. The shader_material example stores it in a field and returns it [3].

**Lighting cost compared with StandardMaterial and ExtendedMaterial:**

- **Custom fragment shader:** if it doesn't call `apply_pbr_lighting`, it skips the per-pixel clustered point/spot loop, the directional loop with shadow sampling, and environment maps. Those loops live in `pbr_functions.wgsl` (the cluster loop is around line 458 and the directional loop around line 597) [4].
- **`StandardMaterial { unlit: true }`:** skips lighting through a runtime flag branch (`STANDARD_MATERIAL_FLAGS_UNLIT_BIT` in `pbr.wgsl` line 81 and `pbr_fragment.wgsl` line 245) [4]. For flat-coloured objects it costs about the same as a custom unlit material.
- **`ExtendedMaterial<StandardMaterial, E>`:** `MaterialExtension` has the same hooks, including `specialize` [5]. The usual pattern builds the full PBR input and lighting and then modifies it, so it costs at least as much as `StandardMaterial`.
- **Costs that no material choice removes:**
  - Per-camera CPU cluster assignment: `ClusterConfig` defaults to `FixedZ` with 4096 clusters and 24 slices, and `ClusterConfig::None` turns it off [6]. Pieced has no `PointLight` or `SpotLight` today (checked by grepping `src/`), so this is already small.
  - Shadow-map draws, unless the material returns `enable_shadows() -> false` or the entity has `NotShadowCaster`.
  - The mesh-view bind group, which stays bound.
- **Tonemapping gotcha:** on non-HDR cameras (Pieced today), bevy sets the `TONEMAP_IN_SHADER` shader def and StandardMaterial tonemaps inside its fragment shader [7]. A custom fragment shader has to do the same, or every camera must use `Tonemapping::None`. Otherwise toon objects won't match PBR objects.

**Cheap toon lighting recipe:**

- Keep `vertex_shader()` as the default.
- In the fragment shader, import `bevy_pbr::forward_io::VertexOutput` and `bevy_pbr::mesh_view_bindings::lights`.
- Compute `dot(N, lights.directional_lights[0].direction_to_light)` and quantise it into bands.
- For optional shadows, use `bevy_pbr::shadows::fetch_directional_shadow` [8].
- Vertex colours arrive as `in.color` under `#ifdef VERTEX_COLORS`. That def is set automatically when the mesh has `ATTRIBUTE_COLOR` [9].

**Custom per-vertex attribute,** for example a smoothed outline normal. This follows the `custom_vertex_attribute` example [10][11]:

```rust
const ATTRIBUTE_OUTLINE_NORMAL: MeshVertexAttribute =
    MeshVertexAttribute::new("OutlineNormal", 2_718_281_828, VertexFormat::Float32x3); // use a large random u64 id
mesh.insert_attribute(ATTRIBUTE_OUTLINE_NORMAL, normals /* Vec<[f32; 3]> */); // or .with_inserted_attribute(..)

// In Material::specialize:
let vertex_layout = layout.0.get_layout(&[
    Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
    Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
    ATTRIBUTE_OUTLINE_NORMAL.at_shader_location(2),
])?;
descriptor.vertex.buffers = vec![vertex_layout];
descriptor.primitive.cull_mode = Some(Face::Front);
```

On the WGSL side, the `Vertex` struct is `@builtin(instance_index) instance_index: u32, @location(0) position: vec3<f32>, ...`. Transform with `bevy_pbr::mesh_functions::{get_world_from_local, mesh_position_local_to_clip}` [11].

A custom vertex shader loses skinning and morph targets. That's fine for rigid parts animated through node transforms.

**Smoothed normals:** `Mesh::compute_smooth_normals()` only averages across *shared, indexed* vertices. It does **not** weld split vertices at hard edges, such as cube corners [12]. An inverted hull needs normals averaged by position, so either write our own position-hash average or use bevy_mod_outline's `generate_outline_normals`.

The mesh must still be in the main world. The default `RenderAssetUsages` is `MAIN_WORLD | RENDER_WORLD`, which is also the glTF loader's default [13][14]. Otherwise the `compute_*` helpers panic [12].

## 2. Outlines

| Crate | Bevy | Technique | Notes |
|---|---|---|---|
| **bevy_mod_outline 0.13.0** (2026-07-09) | `^0.19.0` (README table: 0.13.x ↔ 0.19.x) | Vertex extrusion (`OutlinePlugin::EXTRUDE_VERTEX` → `OutlineMode::ExtrudeFlat`, or `ExtrudeReal`), or jump flood (`OutlineMode::FloodFlat`, `log2(width px)` passes over the outline's bounding box) | MIT/Apache, about 113k downloads. See below. |
| bevy_mesh_outline 0.4.1 | `^0.19.0` | Screen-space JFA distance field (compute) | Requires `OutlineCamera` and `DepthPrepass`. Heavier for a fanless Air [16]. |
| bevy_shader_mtoon 0.4.0 | `^0.19.0` | MToon (VRM) toon shader | Not evaluated. |
| bevy-toon 0.2.0, bevy_wind_waker_shader 0.6.0 | `^0.18` only | — | Not usable on 0.19. |

More on bevy_mod_outline 0.13.0 [15]:

- **Rendering:** outlines draw in their own passes after the main 3D pass, with a separate depth buffer (stencil, opaque and transparent passes), so other geometry doesn't clip them.
- **Components:** `OutlineVolume { visible, width /* logical px */, colour }`, `OutlineStencil`, `OutlineMode`, `InheritOutline` / `PropagateOutline`, `OutlineMsaa`, and the `GlobalOutlineMode` resource.
- **Outline normals:** extrusion needs smooth outline normals. Use `OutlineMeshExt::generate_outline_normals` or `AutoGenerateOutlineNormalsPlugin`. The attribute is `ATTRIBUTE_OUTLINE_NORMAL` (id 1585570526).
- **New in 0.13:** frustum culling, and anti-aliasing of flood outlines under MSAA.
- **Cost:** each outlined entity adds a stencil draw and a volume draw, plus full-size outline depth textures (and MSAA textures when MSAA is on). Not measured on the M4 (unverified).

**Hand-rolled inverted hull.** Straightforward:

- Add a child entity with the same `Mesh3d` handle and a `MeshMaterial3d<OutlineMaterial>`.
- In the vertex shader, push each vertex along the outline normal. Offset in clip space for a constant pixel width, or in model space for a world-unit width.
- In `specialize`, set `cull_mode = Some(Face::Front)`.
- Return `enable_shadows() -> false` and `enable_prepass() -> false`, and add `NotShadowCaster`.
- `RenderLayers` is **not** inherited by children. Either insert it on each child, or add `HierarchyPropagatePlugin::<RenderLayers>` and put `Propagate(RenderLayers::layer(n))` on the root [17]. Viewmodel glTF children need the same treatment.
- Only outlined meshes pay for the extra draw. To limit outlines to near objects, toggle their `Visibility` by distance, or use `VisibilityRange`.

## 3. Sky

- **Skybox fields in 0.19.1:** `Skybox { image: Option<Handle<Image>>, brightness: f32, rotation: Quat }` [18][C7].
  - It is defined in `bevy_light` and re-exported as `bevy::core_pipeline::Skybox`.
  - Defaults are `image: None` (nothing drawn), `brightness: 0.0` (**black**) and `rotation: IDENTITY`.
  - The docs say it doesn't light the scene.
- **Brightness:** the value is multiplied by camera exposure [19]. The default `Exposure` is EV100 9.7, which gives `exp2(-9.7) / 1.2 ≈ 0.001`. So `brightness: 1000.0` shows the texture roughly as authored, which is the value the example uses.
- **Rotation:** extraction uploads `Transform::from_rotation(rotation.inverse())` every frame [19]. Setting `skybox.rotation = Quat::from_rotation_y(t * speed)` is essentially free.
- **Draw cost:** one fullscreen triangle at the end of the main opaque pass, with depth test `GreaterEqual` and no depth write [19]. Only sky pixels run the shader, and each does one cubemap sample.
- **Procedural cubemap at runtime:**

```rust
let mut img = Image::new(
    Extent3d { width: n, height: n, depth_or_array_layers: 6 },
    TextureDimension::D2,
    bytes, // 6 faces back-to-back, n*n*4 bytes each
    TextureFormat::Rgba8UnormSrgb,
    RenderAssetUsages::RENDER_WORLD,
);
img.texture_view_descriptor = Some(TextureViewDescriptor {
    dimension: Some(TextureViewDimension::Cube),
    ..default()
});
```

  - `Image::new` and `Image::new_fill` take `(Extent3d, TextureDimension, data or pixel, TextureFormat, RenderAssetUsages)` [20].
  - For a vertically stacked PNG, call `image.reinterpret_stacked_2d_as_array(6)` and set the same view descriptor, as the skybox example does [21].
  - Face order is assumed to be the standard GPU cube layer order (+X, −X, +Y, −Y, +Z, −Z). Bevy's docs don't state it, so it's unverified: check it with a debug-coloured cube.
  - `skybox.wgsl` negates z when sampling [19].
- **Cheapest slowly rotating galaxy:** a baked cubemap at 512–1024 px per face, rotated by `Skybox.rotation`.
  - That's about 6 MB of VRAM at 512 px in RGBA8, or 24 MB at 1024 px.
  - Use a KTX2 with mips if the stars shimmer.
  - Generating it on the CPU at startup works too, but costs startup time (unmeasured).
  - A sky-dome mesh costs the same but brings draw-order and far-plane concerns.

## 4. glTF

- **Spawning:** `commands.spawn((WorldAssetRoot(asset_server.load(GltfAssetLabel::Scene(0).from_asset("models/x.glb"))), Transform::from_xyz(..)))`. `SceneRoot` is now `WorldAssetRoot`, and the ready event is `bevy::world_serialization::WorldInstanceReady` [22].
- **Finding named nodes after spawn:**
  - Every glTF node entity gets a `Name` (the node's name, or `GltfNode{i}` if it has none) [23].
  - Mesh primitives are *child* entities named `"{mesh}.{material}"`, with `GltfMeshName` and `GltfMaterialName` components.
  - Extras land in `GltfExtras` (node and primitive), `GltfMeshExtras`, `GltfMaterialExtras` and `GltfSceneExtras`, each as a JSON string.
  - Pattern: add an observer on `On<WorldInstanceReady>`, walk `children.iter_descendants(ev.entity)`, match `Name == "Helmet"` or `"GauntletL"`, cache those entities in a component, then drive their `Transform`s procedurally. The `edit_material_on_gltf` example shows the observer and descendant walk [22].
- **Vertex colours:** glTF `COLOR_0` loads as `Mesh::ATTRIBUTE_COLOR` [24]. StandardMaterial multiplies `base_color` by it automatically [9].
- **Custom vertex attributes:** supported in 0.19.1 [25].
  - Register with `DefaultPlugins.set(GltfPlugin::default().add_custom_vertex_attribute(name, ATTR))`.
  - The gltf crate strips **one** leading underscore, so a glTF attribute `_OUTLINE_NORMAL` is registered as `"OUTLINE_NORMAL"`. (The example registers `"_BARYCENTRIC"` for a glTF `__BARYCENTRIC`.)
  - Blender uppercases custom attribute names and exports only those starting with `_`, and only with `export_attributes=True` [30].
  - The underscore mapping comes from reading the source (unverified), so check it with a test load.
- **Axes:**
  - `GltfConvertCoordinates` defaults to no conversion. glTF forward is +Z and Bevy forward is −Z [26].
  - So a model facing Blender −Y (front view) ends up facing Bevy +Z, which is backwards.
  - Fix it by rotating 180° on spawn, or by setting `GltfPlugin { convert_coordinates: GltfConvertCoordinates { rotate_scene_entity: true, .. }, .. }`.
- **Swapping in the toon material,** either:
  - on `WorldInstanceReady`, replace `MeshMaterial3d<StandardMaterial>` (as in the `edit_material_on_gltf` example), or
  - register a `GltfExtensionHandler` whose `on_spawn_mesh_and_material` inserts ours. In 0.19 the glTF-to-StandardMaterial conversion is itself an extension handler inside bevy_pbr [27]. The `gltf_extension_mesh_2d` example shows how to register one.
- **Launch-time notes for about 20 small .glb files:**
  - Loads run async and in parallel. Gate gameplay on `asset_server.is_loaded_with_dependencies(h)` [14].
  - Use per-file settings: `load_with_settings(path, |s: &mut GltfLoaderSettings| { s.load_cameras = false; s.load_lights = false; s.load_animations = false; })` [13].
  - **On macOS, bevy always compiles render pipelines synchronously.** `synchronous_pipeline_compilation` "has no effect on macOS" [28].
    - Every new combination of material type, vertex layout and shader def compiles on its first draw, which causes a hitch.
    - Warm up by drawing every model/material variant once behind the loading screen.
    - Keep vertex layouts uniform, for example give every mesh `COLOR_0`.
  - Vertex colours instead of textures keep the .glb files tiny and avoid image decoding.

## 5. Bloom and post-processing

- **Bloom exists in 0.19.1** as `bevy::post_process::bloom::Bloom`, marked `#[require(Hdr)]` [29].
  - Presets are `NATURAL` (the default), `OLD_SCHOOL`, `ANAMORPHIC` and `SCREEN_BLUR`.
  - The mip chain's largest mip is `max_mip_dimension` tall (default 512), with `ilog2(512) − 1 = 8` mips.
  - Passes: one downsample from full res to mip 0, 7 more downsamples, 7 upsamples, and a final upsample blended into the full-res main texture. That's 16 small passes, and only the first read and the final composite touch full res.
  - `Hdr` also turns the main texture (and its 4× MSAA texture) into `Rgba16Float`, roughly double the bandwidth of `Rgba8`.
  - Milliseconds on the M4 are unverified.
- **Anti-aliasing options in 0.19.1** (`bevy::anti_alias`, part of the default `3d` feature, with `smaa_luts` on) [31]:
  - `fxaa::Fxaa { enabled, edge_threshold, edge_threshold_min }` is one full-res pass.
  - `smaa::Smaa { preset: Low | Medium | High | Ultra }` is three full-res passes: edge detection, blend weights and neighbourhood blending.
  - `taa::TemporalAntiAliasing` requires `Msaa::Off`.
  - The `anti_aliasing` example sets `Msaa::Off` when it switches to FXAA or SMAA.
- **MSAA with several cameras on one target (Pieced's world and viewmodel cameras share an image):**
  - Cameras share intermediate textures only when (target, usage, format — HDR or not, Msaa) all match [32].
  - With `MsaaWriteback::Auto`, the second camera blits the resolved image back into the 4× texture every frame at full res. It does this because it isn't the first camera on the target and it clears with `None` [33].
  - Upscaling uses alpha blending for every camera after the first. "First" is counted per (target, hdr) [34]. So a non-HDR viewmodel over an HDR world would be treated as first and replace the world instead of blending over it.
  - **If we add Bloom, make both cameras `Hdr` and give them the same `Msaa`, then test** (derived from reading the source).
  - Post-process AA on the world camera runs before the viewmodel draws, so viewmodel edges need their own AA.

## 6. Blender 5.2 LTS, headless

Installed: Blender **5.2.2** at `/Applications/Blender.app`, with `blender` on PATH through the Homebrew cask wrapper. Its glTF add-on is 5.2.40.

Command-line flags [35]:

```sh
blender -b --factory-startup --python-exit-code 1 -P tools/blender/export.py -- --out assets/models
# or with a .blend: blender -b scene.blend --python-exit-code 1 -P script.py -- ...
```

- `-b` runs in background mode.
- `-P` runs a Python script.
- `--python-exit-code` sets a non-zero exit code if the script raises.
- Everything after `--` is ignored by Blender and ends up in `sys.argv`.

Export call. The option names come from the installed add-on [30]:

```python
bpy.ops.export_scene.gltf(
    filepath=out, export_format='GLB', use_selection=True,
    export_apply=True,            # default False; applying modifiers blocks shape keys
    export_yup=True,              # default True
    export_extras=True,           # custom properties -> glTF extras (default False)
    export_vertex_color='ACTIVE', # default 'MATERIAL' = only if the material uses it; also 'NAME', 'NONE'
    export_all_vertex_colors=False,
    export_attributes=True,       # custom attributes starting with "_" (default False)
    export_materials='EXPORT',    # 'VIEWPORT' exports the viewport display colour as base colour
    export_cameras=False, export_lights=False, export_animations=False,
)
```

The exporter notes that colour attributes "are already linear", so no colour-space conversion happens [30].

**EEVEE previews from `blender -b`:**

- The engine ids in 5.2 are `'BLENDER_EEVEE'` and `'BLENDER_WORKBENCH'` (from the bundled UI scripts).
- The current manual's EEVEE limitations page lists headless rendering as unsupported only on "headless Windows systems" [36].
- So `blender -b ... -E BLENDER_EEVEE -f 1` should work on macOS (Metal) while a user is logged in. **Unverified**: no renders were run.
- Workbench is the cheaper preview engine.
- Either engine loads the GPU, so schedule renders for when the Mac is idle.

## 7. Fonts

| Font | License | Designer | Official file |
|---|---|---|---|
| Lilita One | **SIL OFL 1.1**, Reserved Font Name "Lilita" | Juan Montoreano | `google/fonts/ofl/lilitaone/LilitaOne-Regular.ttf` (28 KB) and `OFL.txt` [37] |
| Luckiest Guy | **Apache 2.0** | Astigmatic (Brian J. Bonislawsky) | `google/fonts/apache/luckiestguy/LuckiestGuy-Regular.ttf` (73 KB) and `LICENSE.txt` [38] |

Ship each licence text next to its font. Because of the Reserved Font Name, don't ship a *modified* Lilita (for example a renamed or subset build) under that name. Using the unmodified TTF avoids the question.

## 8. Blender MCP (github.com/ahujasid/blender-mcp)

- **What it is:** the URL now redirects to **`ahujasid/mcp-for-blender`**. It's MIT-licensed, has about 29k stars and was pushed 2026-09-25 [39].
  - The PyPI package was renamed from `blender-mcp` to **`mcp-for-blender` 2.1.0**. `uvx blender-mcp` still works.
  - It has two parts. `addon.py` is a Blender add-on that runs a socket server on `localhost:9876` and executes commands, including arbitrary Python, with Auto-Start on by default. The stdio MCP server (Python ≥ 3.10) talks to that add-on [40][41].
- **Install (README Quickstart):**
  1. `brew install uv` (uv is already at `/opt/homebrew/bin/uv`).
  2. `claude mcp add blender -e DISABLE_TELEMETRY=true -e BLENDER_MCP_SAFE_MODE=1 -- uvx mcp-for-blender`. The `-e` flag syntax is confirmed by `claude mcp add --help`.
  3. `uvx mcp-for-blender install-addon`, or install `addon.py` by hand.
  4. In Blender, enable **Interface: MCP for Blender**, open the N panel and press **Start MCP Server**.
  5. Run only one MCP client at a time.
- **Download size:**
  - The repo is about 1 MB; the wheel is 149 KB and bundles the 257 KB `addon.py`.
  - The runtime dependencies are about 28 packages, roughly 4 MB of wheels. I estimated that from `uv.lock` for cp312 on macOS arm64.
  - uv may also fetch a managed Python (unverified). The system `python3` is 3.14.
- **External services:** Poly Haven, Sketchfab, Poly Pizza, Hyper3D Rodin (through its own API or fal.ai), Hunyuan3D (Tencent) and Tripo.
  - Each is a per-scene checkbox, and **all default to `False`** in `addon.py` [41].
  - Paid "Premium" generation goes through mcp-for-blender.com.
- **Telemetry:**
  - Content (prompts, code, screenshots) is opt-in and off by default.
  - **A minimal anonymous usage record** (install ID, tool name, success, duration, versions, OS) is sent to a Supabase endpoint **by default**.
  - Turn it off with `DISABLE_TELEMETRY=true` (`BLENDER_MCP_DISABLE_TELEMETRY` and `MCP_DISABLE_TELEMETRY` also work) [40][42].
- **Safe mode:** `BLENDER_MCP_SAFE_MODE=1` screens scripts and blocks file, network and process access [40].
- **Blender 5.x:**
  - The add-on declares a minimum of Blender 3.0.
  - Issues reported on 5.1 (#243) and 5.2 LTS on Windows (#299) are closed. Maintainers treat 5.x as working.
  - Not tested here.

## Sources

- [C7] Context7 `/websites/rs_bevy`, Skybox struct and example (docs.rs/bevy/latest/bevy/core_pipeline/struct.Skybox.html)
- [1] `$REG/bevy_pbr-0.19.1/src/material.rs` lines 146–285
- [2] `$REG/bevy_pbr-0.19.1/src/pbr_material.rs` (`cull_mode` line 664; specialize lines 1540–1552)
- [3] `$REG/bevy-0.19.1/examples/shader/shader_material.rs`
- [4] `$REG/bevy_pbr-0.19.1/src/render/pbr_functions.wgsl`, `pbr.wgsl`, `pbr_fragment.wgsl`
- [5] `$REG/bevy_pbr-0.19.1/src/extended_material.rs`
- [6] `$REG/bevy_light-0.19.1/src/cluster/mod.rs` (`ClusterConfig` and its Default)
- [7] `$REG/bevy_pbr-0.19.1/src/render/mesh.rs` lines 477–481
- [8] `$REG/bevy_pbr-0.19.1/src/render/mesh_view_types.wgsl`, `mesh_view_bindings.wgsl`, `shadows.wgsl`
- [9] `$REG/bevy_pbr-0.19.1/src/render/pbr_fragment.wgsl` lines 54–101; `forward_io.wgsl`
- [10] `$REG/bevy-0.19.1/examples/shader_advanced/custom_vertex_attribute.rs`
- [11] https://raw.githubusercontent.com/bevyengine/bevy/v0.19.1/assets/shaders/custom_vertex_attribute.wgsl
- [12] `$REG/bevy_mesh-0.19.1/src/mesh.rs` (`insert_attribute` line 372, `compute_smooth_normals` line 1380)
- [13] `$REG/bevy_gltf-0.19.1/src/loader/mod.rs` (`GltfLoaderSettings` lines 185–236)
- [14] `$REG/bevy_asset-0.19.1/src/render_asset.rs` line 40; `$REG/bevy_asset-0.19.1/src/server/mod.rs`
- [15] https://crates.io/crates/bevy_mod_outline (0.13.0 README, CHANGELOG, `src/lib.rs`), https://github.com/komadori/bevy_mod_outline
- [16] https://crates.io/crates/bevy_mesh_outline (0.4.1 README), https://github.com/gylleus/bevy_mesh_outline; crates.io sparse index for bevy-toon, bevy_wind_waker_shader and bevy_shader_mtoon
- [17] `$REG/bevy_app-0.19.1/src/propagate.rs`
- [18] `$REG/bevy_light-0.19.1/src/probe.rs` lines 221–256; `$REG/bevy_core_pipeline-0.19.1/src/lib.rs` line 23
- [19] `$REG/bevy_core_pipeline-0.19.1/src/skybox/mod.rs`, `skybox.wgsl`; `$REG/bevy_camera-0.19.1/src/camera.rs` lines 234–283
- [20] `$REG/bevy_image-0.19.1/src/image.rs` (`new` line 1102, `new_fill` line 1185, `reinterpret_stacked_2d_as_array` line 1395)
- [21] `$REG/bevy-0.19.1/examples/3d/skybox.rs`
- [22] `$REG/bevy-0.19.1/examples/gltf/update_gltf_scene.rs`, `edit_material_on_gltf.rs`; `$REG/bevy_world_serialization-0.19.1/src/{components.rs,world_asset_spawner.rs}`
- [23] `$REG/bevy_gltf-0.19.1/src/loader/gltf_ext/{scene.rs,mesh.rs}`, `loader/mod.rs` lines 1541–1567 and 1690–1733, `assets.rs`
- [24] `$REG/bevy_gltf-0.19.1/src/vertex_attributes.rs` lines 284–322
- [25] `$REG/bevy_gltf-0.19.1/src/lib.rs` lines 232–266; `$REG/bevy-0.19.1/examples/gltf/custom_gltf_vertex_attribute.rs`; `$REG/gltf-json-1.4.1/src/mesh.rs` line 298
- [26] `$REG/bevy_gltf-0.19.1/src/convert_coordinates.rs`
- [27] `$REG/bevy_pbr-0.19.1/src/gltf.rs`; `$REG/bevy-0.19.1/examples/gltf/gltf_extension_mesh_2d.rs`
- [28] `$REG/bevy_render-0.19.1/src/render_resource/pipeline_cache.rs` lines 205–214 and 805–835; `bevy_render-0.19.1/src/lib.rs` lines 133–135
- [29] `$REG/bevy_post_process-0.19.1/src/bloom/{settings.rs,mod.rs}` (mip math in `prepare_bloom_textures`)
- [30] `/Applications/Blender.app/Contents/Resources/5.2/scripts/addons_core/io_scene_gltf2/__init__.py` and `blender/exp/primitive_extract.py`
- [31] `$REG/bevy_anti_alias-0.19.1/src/{fxaa,smaa,taa}/mod.rs`, `Cargo.toml`; `$REG/bevy-0.19.1/Cargo.toml` (`3d_api` includes `smaa_luts`); `$REG/bevy-0.19.1/examples/3d/anti_aliasing.rs`
- [32] `$REG/bevy_render-0.19.1/src/view/mod.rs` lines 1212–1330 (`MainTextureKey`)
- [33] `$REG/bevy_post_process-0.19.1/src/msaa_writeback.rs` lines 107–123; `$REG/bevy_camera-0.19.1/src/clear_color.rs`
- [34] `$REG/bevy_render-0.19.1/src/camera.rs` lines 740–762; `$REG/bevy_core_pipeline-0.19.1/src/upscaling/mod.rs` lines 60–82
- [35] https://projects.blender.org/blender/blender-manual/raw/branch/main/manual/advanced/command_line/arguments.rst
- [36] https://projects.blender.org/blender/blender-manual/raw/branch/main/manual/render/eevee/limitations/limitations.rst ("Headless Rendering")
- [37] https://github.com/google/fonts/tree/main/ofl/lilitaone (`METADATA.pb`: `license: "OFL"`)
- [38] https://github.com/google/fonts/tree/main/apache/luckiestguy (`METADATA.pb`: `license: "APACHE2"`)
- [39] GitHub API `repos/ahujasid/blender-mcp` (redirects to `ahujasid/mcp-for-blender`); https://pypi.org/project/mcp-for-blender/
- [40] https://github.com/ahujasid/mcp-for-blender/blob/main/README.md (Quickstart, Safe mode, Telemetry Control)
- [41] https://github.com/ahujasid/mcp-for-blender/blob/main/addon.py (`bl_info`, scene `BoolProperty` defaults, socket host and port)
- [42] https://github.com/ahujasid/mcp-for-blender/blob/main/src/blender_mcp/telemetry.py; `pyproject.toml`; `uv.lock`; issues #243 and #299
