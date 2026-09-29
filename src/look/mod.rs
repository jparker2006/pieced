//! Milestone 2 "Spellbound" rendering foundations (client only): the toon
//! material and its shared lighting, ink outlines, the far-layer material and
//! halo billboards, blob shadows, the color pipeline, quality settings and
//! pipeline warm-up behind the Boot gate. See docs/M2-SPEC.md → Rendering.
//!
//! # Public API
//!
//! Everything below is re-exported from `crate::look`. [`LookPlugin`] (in
//! `ClientPlugins`) registers all of it; nothing needs setting up per slice.
//!
//! ## Surfaces (M2 Amendment B, D47: the shading upgrade)
//!
//! - [`ToonMaterial`]: the material for everything near (world, pieces, props,
//!   characters, viewmodel). Per fragment:
//!   - **Albedo** = `base_color` × the mesh's `COLOR_0` rgb (if any) × an
//!     optional detail texture (UV_0 × `detail_scale`).
//!   - **Three tones** against the key light: shadow (violet, never black),
//!     mid and lit, with soft smoothstep edges; the shadow and mid tones also
//!     get the teal galaxy fill and a sky/ground hemisphere fill.
//!   - **Baked AO**: `COLOR_0`'s *alpha* is ambient occlusion baked by the
//!     Blender pipeline (`art/blender/lib/ao.py`: 1 open, 0 enclosed); it
//!     darkens toward a violet tint. On `AlphaMode::Blend`/`Add` materials the
//!     vertex alpha stays opacity (ghosts, the edit grid) unless
//!     `.with_vertex_alpha(VertexAlpha::Occlusion)` (a see-through Blender part
//!     such as the gun's glass: keeps `base_color`'s alpha, darkens in
//!     crevices). `.with_occlusion(k)` scales the AO (0 ignores it, e.g. the
//!     knight's eyes, which sit deep in the visor and bake to about 0.15).
//!   - **Cartoon highlight**: a crisp blob where N·H^shininess > 0.5 plus a
//!     broad sheen, tinted toward the surface's hue for metals, lit side only.
//!     Its light blends from the key toward one fixed to the camera
//!     (`highlight_view_bias`, [`VIEW_HIGHLIGHT`]), so gleams sit on top-front
//!     edges from any view, as painted.
//!     Its [`Surface`] (`specular`, `shininess`, `sheen`, `tint`) comes from
//!     the **palette's surface tags** (`art/surfaces.json`: brass, gold_rings,
//!     star_gold = brass; gun_iron, knight_steel, nail_head = steel;
//!     crystal_blue, crystal_violet, shield_cyan = crystal; glass_cyan = glass;
//!     glove_white, eye_white, knight_purple = satin) whenever the albedo
//!     (before detail) is a tagged palette colour; otherwise from the
//!     material's own `surface` (matte by default). Tag new palette colours
//!     there (a test catches collisions); no mesh or material change needed,
//!     and the shared vertex-coloured material still batches.
//!   - **Rim** (stronger than M1), then **emissive** × strength, then the
//!     **colour grade** ([`grade`], `pieced::grade::grade` in WGSL; skipped on
//!     `Add`).
//!
//!   `AlphaMode::Opaque`, `Blend` (ghosts) or `Add`. Constructors:
//!   `ToonMaterial::new(color)`, `::vertex_colored()`, then
//!   `.with_emissive(color, strength)`, `.with_alpha(mode)`, `.with_rim(k)`,
//!   `.with_surface(Surface::named("crystal").unwrap())`,
//!   `.with_specular(strength, shininess)`, `.with_occlusion(k)`,
//!   `.with_vertex_alpha(v)`, `.double_sided()`,
//!   `.with_detail(image, uv_scale, strength)`. Leave its `lighting` field alone.
//! - [`ToonLighting`] (resource): key direction/colour (lit tone), mid tint,
//!   shadow tint, the two tone thresholds and edge softness, in-tone gradient,
//!   fill direction/colour, sky and ground fills, rim colour/strength/power,
//!   highlight colour/softness, AO strength/tint and the surface table. Edit it
//!   and every `ToonMaterial` (and the ground's material) follows within a
//!   frame. [`light_direction`] builds a direction from azimuth/elevation.
//!   [`toon_shade`] / [`toon_shade_sample`] predict a surface's on-screen
//!   colour on the CPU. [`ToonLight`] is the packed copy materials carry; its
//!   M1 fields keep their meaning (the ground reads them).
//! - **Colour grade** ([`grade`]): vibrance, contrast and a slight split tone
//!   (cool shadows, warm highlights) at the end of the toon and far shaders,
//!   instead of a full-screen pass; cameras keep `Tonemapping::None`. Any other
//!   lit world shader should `#import pieced::grade::grade` and apply it to its
//!   final colour before `tone_mapping` so it matches (the ground should).
//! - [`FarMaterial`]: unlit, hazed toward [`FarHaze`] (resource: color, start,
//!   density) with distance. For the station, ships, far islands, the planet.
//!   `FarMaterial::new(color).with_haze(0..1).with_emissive(c, k)`. Never
//!   outlined. Opaque far models darken by their baked AO too
//!   ([`FAR_AO_STRENGTH`]) and are graded before the haze.
//! - [`GroundMaterial`]: the island's grass, toon-lit like [`ToonMaterial`]
//!   (vertex colours multiply `base_color`), with the faint glowing build grid
//!   drawn in world space by its shader (`ground.wgsl`). Never outlined.
//!
//! ## Outlines
//!
//! - Put [`Outline`] (`Outline::default()`, `Outline::ink(color)`,
//!   `.with_width(px)`) on a mesh entity, or on a mesh-less root (a glTF model)
//!   to outline every mesh below it. `color: None` derives a dark desaturated
//!   ink from the surface ([`ink_color`]). Mark meshes that must never be inked
//!   with [`NoOutline`].
//! - Meshes must carry smooth outline normals: build them with
//!   [`with_outline_normals`]`(mesh)` before `meshes.add(..)` (glTF meshes can
//!   have them generated the same way, or exported as `_OUTLINE_NORMAL`).
//! - Width: [`DEFAULT_WIDTH_PX`] (3 px since D47) at [`REFERENCE_HEIGHT_PX`],
//!   constant on screen up to [`TAPER_START`] m, tapering to [`TAPER_KEEP`] ×
//!   by [`FADE_START`] (so small mid-distance things don't bloat), fading out
//!   between [`FADE_START`] and [`FADE_END`] m ([`outline_width_px`]).
//! - Backend: [`OutlineBackend`] (`Hull` by default, `Mod` = bevy_mod_outline,
//!   `Off`), from the quality preset or `--knobs outline=mod|hull|off`.
//!
//! ## Glow and shadows
//!
//! - [`Halo`]`::new(color, size_m, intensity)` on any entity (with a
//!   `Transform`) draws an additive camera-facing glow there. Change or remove
//!   the component to update it. All halos batch into one draw.
//! - [`BlobShadow`]`::new(radius)` on any entity keeps a soft decal on the
//!   ground (world and pieces) below it; it shrinks and fades as the entity
//!   rises and hides with it.
//!
//! ## Blender models
//!
//! - Models from `models::spawn_model` are dressed automatically when
//!   `ModelSpawned` fires: every mesh gets the shared vertex-coloured
//!   [`ToonMaterial`] and outline normals. Put [`ModelLook`] on the root for
//!   `Viewmodel` (toon plus the viewmodel render layer) or `Far` (the far
//!   material), and [`Outline`] on the root to ink it. Override single parts
//!   (crystals, eyes) after [`ModelDressed`]; the shared handles are in
//!   [`ModelMaterials`].
//!
//! ## Settings and warm-up
//!
//! - [`LookSettings`] (resource, read-only): the preset ([`preset_look`])
//!   merged with perf knobs, per frame: outline backend, MSAA for both cameras,
//!   FXAA, and the far / halos / blobs / sky-rotation switches. Battery: MSAA
//!   off + FXAA (MSAA 4× was the battery spike source); Plugged in: MSAA 4×.
//!   Offscreen tests can set knobs with the `PIECED_KNOBS` env var.
//! - Both 3D cameras use `Tonemapping::None` without debanding, so palette
//!   colors land on screen as authored.
//! - [`warmup::Warmup`] (system param): during Boot, `warmup.add(mesh,
//!   material)` or `warmup.add_with(mesh, material, extra_components)` for
//!   every material/mesh-layout combination your slice will draw later. Boot
//!   waits (`BootGate` key [`warmup::WARMUP_GATE`]) until each has been drawn
//!   3 frames behind the loading overlay.

mod blob;
mod far;
pub mod grade;
mod ground;
mod halo;
mod model_look;
mod outline;
mod settings;
pub mod surfaces;
mod toon;
pub mod warmup;

pub use blob::{
    BlobAssets, BlobDecal, BlobDecalLink, BlobGround, BlobMaterial, BlobShadow, blob_footprint,
};
pub use far::{FAR_AO_STRENGTH, FarHaze, FarHazeBlock, FarMaterial};
pub use grade::grade;
pub use ground::{GroundMaterial, grid_line_distance, grid_line_intensity};
pub use halo::{
    HALO_MAX_INTENSITY, Halo, HaloAssets, HaloLink, HaloMaterial, HaloSprite, halo_image,
    pack_halo, unpack_halo,
};
pub use model_look::{ModelDressed, ModelLook, ModelMaterials};
pub use outline::{
    ATTRIBUTE_OUTLINE_NORMAL, DEFAULT_WIDTH_PX, FADE_END, FADE_START, InheritedOutline,
    InkMaterial, NoOutline, Outline, OutlineHull, OutlineHullLink, REFERENCE_HEIGHT_PX, TAPER_KEEP,
    TAPER_START, ink_color, outline_width_px, smooth_outline_normals, with_outline_normals,
};
pub use settings::{
    LookSettings, ModOutlineAvailable, OutlineBackend, PresetLook, look_fxaa, msaa_for,
    preset_look, resolve_look,
};
pub use surfaces::{Surface, SurfaceTable};
pub use toon::{
    ToonLight, ToonLighting, ToonMaterial, ToonSample, VIEW_HIGHLIGHT, VertexAlpha,
    highlight_light, light_direction, toon_shade, toon_shade_linear, toon_shade_sample,
};

use crate::{
    app::BootGate,
    perf_knobs::PerfKnobs,
    render::{CameraFollowSet, VIEWMODEL_LAYER},
    shared::AppState,
};
use bevy::{
    asset::io::embedded::EmbeddedAssetRegistry,
    camera::visibility::RenderLayers,
    mesh::{MeshTag, VertexAttributeValues},
    prelude::*,
};
use std::path::{Path, PathBuf};
use warmup::Warmup;

pub struct LookPlugin;

/// The halo billboard systems (attach, sync, the halos setting), in
/// `PostUpdate`. Systems that adjust halo sprites afterwards (the overdraw
/// cap, `crate::perf`) run after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct HaloSystems;

impl Plugin for LookPlugin {
    fn build(&self, app: &mut App) {
        register_shaders(app);
        load_shader_library(app);
        knobs_from_env(app);
        if mod_outline_requested(app) {
            app.add_plugins(bevy_mod_outline::OutlinePlugin::EXTRUDE_VERTEX)
                .init_resource::<ModOutlineAvailable>();
        }
        app.init_resource::<BootGate>();
        app.world_mut()
            .resource_mut::<BootGate>()
            .hold(warmup::WARMUP_GATE);
        app.add_plugins((
            MaterialPlugin::<ToonMaterial>::default(),
            MaterialPlugin::<InkMaterial>::default(),
            MaterialPlugin::<FarMaterial>::default(),
            MaterialPlugin::<HaloMaterial>::default(),
            MaterialPlugin::<BlobMaterial>::default(),
        ))
        .init_resource::<ToonLighting>()
        .init_resource::<FarHaze>()
        .add_message::<crate::models::ModelSpawned>()
        .add_message::<ModelDressed>()
        .init_resource::<ModelMaterials>()
        .init_resource::<LookSettings>()
        .init_resource::<outline::InkMaterials>()
        .init_resource::<warmup::WarmupState>()
        .add_observer(settings::dress_look_camera)
        .add_observer(outline::remove_outline_hull)
        .add_systems(
            Startup,
            (
                (halo::create_halo_assets, blob::create_blob_assets),
                warm_look_variants,
                warmup::spawn_loading_overlay,
            )
                .chain(),
        )
        .add_systems(OnExit(AppState::Boot), warmup::remove_loading_overlay)
        .add_systems(PreUpdate, settings::update_look_settings)
        .add_systems(
            Update,
            (
                settings::apply_camera_msaa,
                apply_far_setting,
                outline::update_mod_outlines,
                warmup::run_warmup,
                model_look::dress_spawned_models,
            ),
        )
        .add_systems(
            PostUpdate,
            (
                (
                    outline::propagate_outlines,
                    outline::apply_outline_backend,
                    outline::spawn_outline_hulls,
                    outline::sync_outline_hulls,
                )
                    .chain(),
                (
                    halo::attach_halo_sprites,
                    halo::sync_halo_sprites,
                    halo::apply_halo_setting,
                )
                    .chain()
                    .in_set(HaloSystems),
                warmup::place_warmup_items.after(CameraFollowSet),
            )
                .before(TransformSystems::Propagate),
        )
        .add_systems(
            PostUpdate,
            (
                toon::sync_global_block::<ToonLighting, ToonMaterial>,
                toon::sync_global_block::<FarHaze, FarMaterial>,
            ),
        );
        blob::add_blob_systems(app);
        ground::add_ground(app);
    }
}

/// Offscreen tests and tools have no command line of their own: they can set
/// perf knobs with `PIECED_KNOBS=msaa=4,fxaa=off` (only when no `--knobs`
/// resource was inserted).
fn knobs_from_env(app: &mut App) {
    if app.world().contains_resource::<PerfKnobs>() {
        return;
    }
    if let Ok(raw) = std::env::var("PIECED_KNOBS")
        && !raw.is_empty()
    {
        app.insert_resource(PerfKnobs::parse(&raw));
    }
}

/// `bevy_mod_outline` adds passes (and an MSAA write-back blit on every MSAA
/// camera) as soon as its plugin exists, so it is only added when it is the
/// backend being measured: `--knobs outline=mod`.
fn mod_outline_requested(app: &App) -> bool {
    let knobs = app
        .world()
        .get_resource::<PerfKnobs>()
        .cloned()
        .or_else(|| PerfKnobs::from_args(&std::env::args().collect::<Vec<_>>()));
    knobs.and_then(|k| k.outline) == Some(OutlineBackend::Mod)
}

/// Embeds the look shaders in the binary (see `arena::visuals` for why the
/// path is fixed rather than `embedded_asset!`).
fn register_shaders(app: &mut App) {
    let registry = app.world().resource::<EmbeddedAssetRegistry>();
    for (path, bytes) in [
        (
            "pieced/shaders/toon.wgsl",
            include_bytes!("../../assets/shaders/toon.wgsl").as_slice(),
        ),
        (
            "pieced/shaders/ink.wgsl",
            include_bytes!("../../assets/shaders/ink.wgsl").as_slice(),
        ),
        (
            "pieced/shaders/far.wgsl",
            include_bytes!("../../assets/shaders/far.wgsl").as_slice(),
        ),
        (
            "pieced/shaders/halo.wgsl",
            include_bytes!("../../assets/shaders/halo.wgsl").as_slice(),
        ),
        (
            "pieced/shaders/blob.wgsl",
            include_bytes!("../../assets/shaders/blob.wgsl").as_slice(),
        ),
        (
            GRADE_SHADER,
            include_bytes!("../../assets/shaders/grade.wgsl").as_slice(),
        ),
    ] {
        registry.insert_asset(PathBuf::new(), Path::new(path), bytes);
    }
}

/// The import-only shader library (`#import pieced::grade::grade`).
const GRADE_SHADER: &str = "pieced/shaders/grade.wgsl";

/// Keeps the import-only shaders loaded, so `#import pieced::grade` resolves
/// in any material (the loader only knows import paths of loaded shaders).
#[derive(Resource)]
pub struct LookShaderLibrary {
    pub grade: Handle<Shader>,
}

fn load_shader_library(app: &mut App) {
    // Headless apps (no renderer) have no shader assets and need none.
    if !app.world().contains_resource::<Assets<Shader>>() {
        return;
    }
    let Some(server) = app.world().get_resource::<AssetServer>() else {
        return;
    };
    let grade = server.load(format!("embedded://{GRADE_SHADER}"));
    app.insert_resource(LookShaderLibrary { grade });
}

/// `far=off` hides every far-layer mesh: all of them when the setting changes,
/// and any spawned later while it is off.
fn apply_far_setting(
    settings: Res<LookSettings>,
    mut far: Query<(&mut Visibility, Ref<MeshMaterial3d<FarMaterial>>)>,
) {
    let changed = settings.is_changed();
    if !changed && settings.far {
        return;
    }
    let visibility = if settings.far {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for (mut v, material) in &mut far {
        if changed || material.is_added() {
            v.set_if_neq(visibility);
        }
    }
}

/// A unit cube with the vertex layout our mesh builders produce: position,
/// normal, `COLOR_0`, and (when `outlined`) outline normals.
fn builder_layout_cube(outlined: bool) -> Mesh {
    let mut mesh = Mesh::from(Cuboid::default());
    mesh.remove_attribute(Mesh::ATTRIBUTE_UV_0);
    let count = mesh.count_vertices();
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_COLOR,
        VertexAttributeValues::Float32x4(vec![[1.0; 4]; count]),
    );
    if outlined {
        with_outline_normals(mesh)
    } else {
        mesh
    }
}

/// Warms every material variant this module creates, on both the world and
/// the viewmodel layers: toon (opaque, double-sided, blended ghosts) on both
/// builder vertex layouts, outline hulls, the far layer, halos and blob decals.
fn warm_look_variants(
    mut warmup: Warmup,
    mut meshes: ResMut<Assets<Mesh>>,
    mut toon: ResMut<Assets<ToonMaterial>>,
    mut far: ResMut<Assets<FarMaterial>>,
    halos: Res<HaloAssets>,
    blobs: Res<BlobAssets>,
) {
    let outlined = meshes.add(builder_layout_cube(true));
    let plain = meshes.add(builder_layout_cube(false));
    let opaque = toon.add(ToonMaterial::vertex_colored());
    let sheet = toon.add(ToonMaterial::vertex_colored().double_sided());
    let ghost = toon.add(
        ToonMaterial::new(Color::srgba(0.55, 0.85, 1.0, 0.4))
            .with_emissive(Color::srgb(0.55, 0.85, 1.0), 0.5)
            .with_alpha(AlphaMode::Blend),
    );
    let viewmodel = RenderLayers::layer(VIEWMODEL_LAYER);
    for layers in [RenderLayers::layer(0), viewmodel.clone()] {
        warmup.add_with(
            outlined.clone(),
            opaque.clone(),
            (layers.clone(), Outline::default()),
        );
        warmup.add_with(plain.clone(), opaque.clone(), layers.clone());
        warmup.add_with(plain.clone(), ghost.clone(), layers.clone());
        warmup.add_with(outlined.clone(), ghost.clone(), layers);
    }
    warmup.add(plain.clone(), sheet);
    warmup.add(plain, far.add(FarMaterial::default()));
    warmup.add_with(
        halos.quad.clone(),
        halos.material.clone(),
        MeshTag(pack_halo(Color::WHITE, 1.0)),
    );
    warmup.add_with(
        halos.quad.clone(),
        halos.material.clone(),
        (MeshTag(pack_halo(Color::WHITE, 1.0)), viewmodel),
    );
    warmup.add_with(blobs.quad.clone(), blobs.material.clone(), MeshTag(128));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warmup_cubes_match_the_builder_layouts() {
        let outlined = builder_layout_cube(true);
        assert!(outlined.attribute(ATTRIBUTE_OUTLINE_NORMAL).is_some());
        assert!(outlined.attribute(Mesh::ATTRIBUTE_COLOR).is_some());
        assert!(outlined.attribute(Mesh::ATTRIBUTE_UV_0).is_none());
        let plain = builder_layout_cube(false);
        assert!(plain.attribute(ATTRIBUTE_OUTLINE_NORMAL).is_none());
        assert!(plain.attribute(Mesh::ATTRIBUTE_NORMAL).is_some());
    }

    #[test]
    fn far_knob_hides_far_meshes_including_later_ones() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(resolve_look(
                crate::render::QualityPreset::Battery,
                Some(&PerfKnobs::parse("far=off")),
                false,
            ))
            .add_systems(Update, apply_far_setting);
        let far = || {
            (
                MeshMaterial3d::<FarMaterial>(Handle::default()),
                Visibility::Inherited,
            )
        };
        let early = app.world_mut().spawn(far()).id();
        app.update();
        let late = app.world_mut().spawn(far()).id();
        app.update();
        for e in [early, late] {
            assert_eq!(app.world().get::<Visibility>(e), Some(&Visibility::Hidden));
        }
        app.world_mut().resource_mut::<LookSettings>().far = true;
        app.update();
        for e in [early, late] {
            assert_eq!(
                app.world().get::<Visibility>(e),
                Some(&Visibility::Inherited)
            );
        }
    }

    #[test]
    fn outline_propagates_from_a_model_root_to_its_meshes() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_systems(Update, outline::propagate_outlines);
        let root = app.world_mut().spawn(Outline::ink(Color::BLACK)).id();
        let part = app
            .world_mut()
            .spawn((Mesh3d(Handle::default()), ChildOf(root)))
            .id();
        let own = app
            .world_mut()
            .spawn((
                Mesh3d(Handle::default()),
                Outline::default().with_width(3.0),
                ChildOf(part),
            ))
            .id();
        let never = app
            .world_mut()
            .spawn((Mesh3d(Handle::default()), NoOutline, ChildOf(root)))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<Outline>(part),
            Some(&Outline::ink(Color::BLACK))
        );
        assert!(app.world().get::<InheritedOutline>(part).is_some());
        assert_eq!(app.world().get::<Outline>(own).unwrap().width_px, 3.0);
        assert!(app.world().get::<Outline>(never).is_none());
        // Root changes flow down to inherited outlines only.
        app.world_mut().get_mut::<Outline>(root).unwrap().width_px = 2.0;
        app.update();
        assert_eq!(app.world().get::<Outline>(part).unwrap().width_px, 2.0);
        assert_eq!(app.world().get::<Outline>(own).unwrap().width_px, 3.0);
    }
}
