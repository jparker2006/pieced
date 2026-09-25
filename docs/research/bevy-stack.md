# Research: Bevy technology stack for Pieced

Checked 2026-09-24 against Context7 (Bevy and Avian docs), the crates.io API and sparse index, docs.rs, bevy.org and project READMEs. Anything that couldn't be confirmed is marked UNVERIFIED.

## Bevy version

- **Latest stable: 0.19.1** (2026-08-13). **0.20.0-rc.1** (2026-09-15) is only a release candidate.
- Pin **`=0.19.1`**. It needs Rust ≥ 1.95; the local toolchain is 1.98.1. Every physics and controller crate below requires Bevy `^0.19`.
- Relevant 0.19 features:
  - `MeshRayCast` for ray-vs-triangle picking (in the default `3d` feature).
  - A diagnostics overlay.
  - A settings framework (`SettingsPlugin`).
  - Contact shadows, which should stay off.
- There is **no built-in physics or character controller**.

## Physics and collision

| Crate | Version | Notes |
|---|---|---|
| **avian3d** | **0.7.0** (Bevy ^0.19) | `SpatialQuery` covers rays, shape casts and intersections. The built-in **`MoveAndSlide`** param provides `move_and_slide`, `cast_move`, `depenetrate` and `project_velocity`. Optional `enhanced-determinism`. |
| bevy_rapier3d | 0.36.0 (Bevy ^0.19) | Pins a pre-release `rapier3d =0.35.0-glamx0.2`. |
| bevy_tnua + bevy_tnua_avian3d | 0.32.0 + 0.12.1 | Floating dynamic rigid body. |
| bevy_ahoy | 0.2.0 | Kinematic FPS controller on Avian 0.7 and bevy_enhanced_input 0.26: stairs, ramps, crouch, mantle, coyote time. No slide. Useful to read, not to depend on. |

**Decision:** use avian3d for collision and spatial queries, plus our **own kinematic controller** built on `MoveAndSlide` in `FixedUpdate` at 60 Hz. No dynamic rigid bodies for characters.
- Build pieces are cuboid colliders; ramps are convex hulls.
- Hitscan uses `SpatialQuery::cast_ray`, with head and body colliders on separate collision layers.
- Why: we own the integration code, so it's deterministic. It's cheap for dozens of colliders. It's plain ECS that AI coding handles well, and it can be reused by bots and in headless runs.

## Low-latency rendering on macOS

- **Present modes:**
  - `Fifo` is vsync.
  - `AutoNoVsync` tries Immediate, then Mailbox, then Fifo.
  - Mailbox is listed only for DX11/12, NVIDIA Vulkan and Wayland Vulkan, and requesting it directly can panic.
  - **On Metal the practical choices are Fifo or Immediate.**
- **`Window.desired_maximum_frame_latency`** defaults to 2 (range 1–3). **Set it to 1.** UNVERIFIED: exactly how wgpu's Metal backend maps it.
- **Pipelined rendering** adds about a frame of latency. **Disable it**, as Chunky did.
- **bevy_framepace 0.22.0** sleeps before input is gathered, which reduces motion-to-photon latency. It's an optional A/B experiment.
- **Start with:** Fifo, frame latency 1, no pipelining. A/B test AutoNoVsync with a 60 fps cap and framepace.
- **Measurement:**
  - `FrameTimeDiagnosticsPlugin` and `RenderDiagnosticsPlugin` for timings.
  - A per-frame CSV logged in `Last`.
  - Input-to-present approximation: timestamp in `First` when input arrives, then detect a GPU readback change. Chunky's method gives an upper bound that excludes scanout.

## Cursor and trackpad

- Put `CursorOptions { grab_mode: Locked, visible: false }` on the window entity. macOS does not support `Confined`.
- `AccumulatedMouseMotion.delta` is the sum of that frame's raw motion. **Do not scale it by dt.** Apply look in `Update`.
- Keep Chunky's patterns:
  - pause on focus loss;
  - reset `ButtonInput` when the mode changes;
  - ignore the delta on the frame the cursor is recaptured.
- **Trackpad while a key is held: UNVERIFIED on macOS.** Test it on the M4 Air by holding W and swiping, and plan an external-mouse fallback.

## Cheap, good-looking low-poly rendering

- **Flat shading:** `.with_duplicated_vertices().with_computed_flat_normals()`, or author per-face normals directly. Use `StandardMaterial` with a flat base color, high roughness and zero metallic. Share material handles and merge static meshes.
- **Lighting:** one `DirectionalLight` with shadows and a single cascade covering the arena, plus `AmbientLight` rather than an environment map. No shadow-casting point lights.
- **`DistanceFog`** is cheap.
- **Anti-aliasing:** MSAA 4× gives crisp polygon edges, and is usually cheap on tile-based Apple GPUs (UNVERIFIED on M4). Deferred rendering requires MSAA off, so stay forward.
- **Keep off on a fanless laptop:** contact shadows, SSR, SSAO, TAA, motion-vector prepasses, volumetrics, area lights and Solari. Bloom and HDR only if measured affordable.
- Keep the scale-factor override at 1. Rendering at native Retina density is about 4× the pixels.

## Testing

- **Headless app:** `MinimalPlugins` plus the gameplay and Avian plugins, with no render plugin, then call `app.update()` in a loop. UNVERIFIED: whether Avian's `collider-from-mesh` needs `AssetPlugin`/`Assets<Mesh>` headless. Prefer primitive colliders.
- **Deterministic stepping:** `Time::<Fixed>::from_hz(60.0)` plus `TimeUpdateStrategy::ManualDuration(1/60 s)`, so each update is exactly one tick.
- **Scripted input:** write `ButtonInput<KeyCode>` or `AccumulatedMouseMotion` directly (Chunky prior art). Better: write the game's own intent struct.
- **Frame log:** a CSV row per frame in `Last`. The summary gives p50/p95/p99, the maximum, and counts of frames over 20 ms and over 25 ms.

## Crates (verified against Bevy ^0.19 on crates.io unless noted)

| Crate | Version | Use |
|---|---|---|
| bevy | =0.19.1 | engine (add `wav` feature) |
| avian3d | =0.7.0 | collision and queries |
| bevy_egui | =0.42.0 | dev tuning panel |
| bevy_framepace | =0.22.0 | optional latency experiment |
| pathfinding | 4.16.0 | Milestone 2 bot search (Bevy-independent) |
| bevy_landmass | =0.12.0 | navmesh alternative (not planned) |

Not compatible: `iyes_perf_ui 0.5.0` (Bevy 0.16) and `bevy_tnua_rapier3d` with rapier 0.36.

## Sources

- https://bevy.org/news/bevy-0-19/
- https://github.com/bevyengine/bevy/releases
- https://crates.io/crates/bevy
- https://docs.rs/bevy/0.19.1/bevy/window/struct.Window.html
- https://docs.rs/bevy/latest/bevy/window/enum.PresentMode.html
- https://docs.rs/bevy/latest/bevy/window/struct.CursorOptions.html
- https://docs.rs/bevy/latest/bevy/input/mouse/struct.AccumulatedMouseMotion.html
- https://docs.rs/bevy/latest/bevy/render/pipelined_rendering/struct.PipelinedRenderingPlugin.html
- https://docs.rs/bevy/latest/bevy/time/enum.TimeUpdateStrategy.html
- https://docs.rs/bevy/latest/bevy/pbr/prelude/struct.DistanceFog.html
- https://docs.rs/avian3d/latest/avian3d/character_controller/move_and_slide/struct.MoveAndSlide.html
- https://github.com/avianphysics/avian
- https://github.com/janhohenheim/bevy_ahoy
- https://github.com/aevyrie/bevy_framepace
- https://github.com/bevyengine/bevy/issues/6174
