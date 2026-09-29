//! The UI camera's render schedule, [`UiComposite`] (M4 performance follow-up).
//!
//! **Before**, the UI camera was a stock `Camera2d`. At the window's Retina
//! size (3420×2214 = 7.6 MP on the 15" Air at "More Space") every frame it
//! ran: a 2D main pass clearing a color and a depth texture it never used, the
//! UI pass drawing the 3D image as a full-screen `ImageNode` under the HUD,
//! then `upscaling` copying the whole texture into the window. Play-test 2
//! measured it at 2.17 ms (UI) plus most of post's 2.35 ms.
//!
//! **Now** the camera keeps `Camera2d` (Bevy UI only renders on 2D or 3D
//! cameras) but runs this schedule instead of `Core2d`:
//!
//! 1. `ui_pass`: the HUD over a transparent clear. Its first draw clears the
//!    color target (Bevy's first-use clear), so there is no separate clear
//!    pass and no depth texture. The texture then holds premultiplied color
//!    (every UI pipeline alpha-blends, which composes as premultiplied
//!    "over" from a transparent start).
//! 2. [`clear_unused_ui`]: only when the UI drew nothing this frame, a
//!    clear-only pass, so the composite never reads a stale HUD.
//! 3. The dev panel (`bevy_egui`, F4) when its plugin is present.
//! 4. [`composite`]: **one** full-screen pass into the window: the 3D image
//!    stretched from its render size (bilinear, like the old image node) under
//!    the HUD. `assets/shaders/composite.wgsl`.
//!
//! The math is the same as drawing the HUD over the 3D image directly (the
//! "over" operator is associative); only the intermediate's 8-bit rounding of
//! partly transparent HUD pixels differs.

use super::WorldTarget;
use bevy::{
    asset::io::embedded::EmbeddedAssetRegistry,
    core_pipeline::FullscreenShader,
    ecs::schedule::{ScheduleBuildSettings, ScheduleLabel},
    prelude::*,
    render::{
        GpuResourceAppExt, Render, RenderApp, RenderStartup, RenderSystems,
        camera::ExtractedCamera,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        render_phase::ViewSortedRenderPhases,
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
            CachedRenderPipelineId, ColorTargetState, ColorWrites, FilterMode, FragmentState,
            PipelineCache, RenderPassDescriptor, RenderPipelineDescriptor, Sampler,
            SamplerBindingType, SamplerDescriptor, ShaderStages, SpecializedRenderPipeline,
            SpecializedRenderPipelines, TextureFormat, TextureSampleType, TextureViewId,
            binding_types::{sampler, texture_2d},
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        texture::GpuImage,
        view::{ExtractedView, ViewTarget},
    },
    shader::Shader,
    ui_render::{TransparentUi, UiCameraView, ui_pass},
};
use std::path::{Path, PathBuf};

/// The UI camera's render schedule (see the module docs).
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct UiComposite;

/// The two stages of [`UiComposite`], in order.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiCompositeSystems {
    /// Everything drawn into the camera's native-resolution texture.
    Ui,
    /// The full-screen composite into the window.
    Composite,
}

/// The embedded path of the composite shader.
pub const COMPOSITE_SHADER_PATH: &str = "embedded://pieced/shaders/composite.wgsl";

/// Creates the [`UiComposite`] schedule in the render app if it doesn't
/// exist yet (idempotent: [`crate::gpu_timing`] adds its marks to it too).
pub fn ensure_ui_composite_schedule(render_app: &mut SubApp) {
    let exists = render_app
        .world()
        .get_resource::<Schedules>()
        .is_some_and(|s| s.contains(UiComposite));
    if exists {
        return;
    }
    let mut schedule = Schedule::new(UiComposite);
    // Like Bevy's camera schedules: render systems encode commands, they
    // never spawn, so no sync points.
    schedule.set_build_settings(ScheduleBuildSettings {
        auto_insert_apply_deferred: false,
        ..Default::default()
    });
    schedule.configure_sets((UiCompositeSystems::Ui, UiCompositeSystems::Composite).chain());
    render_app.add_schedule(schedule);
}

/// Registers the schedule, the pipeline and its systems (from
/// [`super::RenderSetupPlugin`]).
pub(super) fn build(app: &mut App) {
    let has_render = app.get_sub_app(RenderApp).is_some();
    if !has_render {
        return;
    }
    if let Some(registry) = app.world().get_resource::<EmbeddedAssetRegistry>() {
        registry.insert_asset(
            PathBuf::new(),
            Path::new("pieced/shaders/composite.wgsl"),
            include_bytes!("../../assets/shaders/composite.wgsl").as_slice(),
        );
    }
    app.add_plugins(ExtractResourcePlugin::<WorldTarget>::default());
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    ensure_ui_composite_schedule(render_app);
    render_app
        .init_gpu_resource::<SpecializedRenderPipelines<CompositePipeline>>()
        .add_systems(RenderStartup, init_composite_pipeline)
        .add_systems(
            Render,
            prepare_composite_pipelines.in_set(RenderSystems::Prepare),
        )
        .add_systems(
            UiComposite,
            (ui_pass, clear_unused_ui)
                .chain()
                .in_set(UiCompositeSystems::Ui),
        )
        .add_systems(UiComposite, composite.in_set(UiCompositeSystems::Composite));
}

/// Adds the dev panel's pass once every plugin is built (its plugin comes
/// after this one).
pub(super) fn finish(app: &mut App) {
    if !app.is_plugin_added::<bevy_egui::EguiPlugin>() {
        return;
    }
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app.add_systems(
        UiComposite,
        (bevy_egui::render::prepare_egui_pass, bevy_egui::render::egui_pass)
            .chain()
            .after(clear_unused_ui)
            .in_set(UiCompositeSystems::Ui),
    );
}

impl ExtractResource for WorldTarget {
    type Source = WorldTarget;

    fn extract_resource(source: &Self::Source) -> Self {
        source.clone()
    }
}

/// The composite's pipeline layout, sampler and shaders.
#[derive(Resource)]
pub struct CompositePipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    fullscreen_shader: FullscreenShader,
    fragment_shader: Handle<Shader>,
}

fn init_composite_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
    asset_server: Res<AssetServer>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "pieced_composite_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: false }),
            ),
        ),
    );
    // Bilinear, like the default image sampler the old full-screen image
    // node used for the 3D image.
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("pieced_composite_sampler"),
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    commands.insert_resource(CompositePipeline {
        layout,
        sampler,
        fullscreen_shader: fullscreen_shader.clone(),
        fragment_shader: asset_server.load(COMPOSITE_SHADER_PATH),
    });
}

impl SpecializedRenderPipeline for CompositePipeline {
    /// The window's (or output image's) format.
    type Key = TextureFormat;

    fn specialize(&self, format: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("pieced_composite".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: self.fragment_shader.clone(),
                targets: vec![Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        }
    }
}

/// The composite pipeline a UI camera view uses.
#[derive(Component)]
pub struct ViewCompositePipeline(CachedRenderPipelineId, TextureFormat);

fn prepare_composite_pipelines(
    mut commands: Commands,
    mut pipeline_cache: ResMut<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<CompositePipeline>>,
    pipeline: Option<Res<CompositePipeline>>,
    views: Query<(
        Entity,
        &ViewTarget,
        &ExtractedCamera,
        Option<&ViewCompositePipeline>,
    )>,
) {
    let Some(pipeline) = pipeline else { return };
    for (entity, target, camera, current) in &views {
        if camera.schedule != UiComposite.intern() {
            continue;
        }
        let Some(format) = target.out_texture_view_format() else {
            continue;
        };
        if current.is_some_and(|c| c.1 == format) {
            continue;
        }
        let id = pipelines.specialize(&pipeline_cache, &pipeline, format);
        // Compiled while Boot's loading overlay is up (the UI camera exists
        // from the first frame), never mid-play.
        pipeline_cache.block_on_render_pipeline(id);
        commands
            .entity(entity)
            .insert(ViewCompositePipeline(id, format));
    }
}

/// When the UI drew nothing this frame, `ui_pass` never cleared the texture:
/// clear it here so the composite never shows a stale HUD. (Rare: the HUD,
/// menus and the loading overlay always have nodes.)
pub fn clear_unused_ui(
    view: ViewQuery<(&ViewTarget, Option<&UiCameraView>)>,
    ui_views: Query<&ExtractedView>,
    phases: Res<ViewSortedRenderPhases<TransparentUi>>,
    mut ctx: RenderContext,
) {
    let (target, ui_view) = view.into_inner();
    let drew = ui_view
        .and_then(|v| ui_views.get(v.0).ok())
        .and_then(|v| phases.get(&v.retained_view_entity))
        .is_some_and(|phase| !phase.items.is_empty());
    if drew {
        return;
    }
    let attachment = target.get_unsampled_color_attachment();
    let _pass = ctx
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("pieced_ui_clear"),
            color_attachments: &[Some(attachment)],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
}

/// The last bind group, kept while its two texture views stay the same
/// (a resize or the dynamic resolution makes a new one).
#[derive(Default)]
pub struct CompositeBindGroup {
    cached: Option<(TextureViewId, TextureViewId, BindGroup)>,
}

/// The one full-screen pass into the window (see the module docs).
pub fn composite(
    view: ViewQuery<(&ViewTarget, Option<&ViewCompositePipeline>)>,
    world_target: Option<Res<WorldTarget>>,
    images: Res<RenderAssets<GpuImage>>,
    pipeline: Option<Res<CompositePipeline>>,
    pipeline_cache: Res<PipelineCache>,
    mut cache: Local<CompositeBindGroup>,
    mut ctx: RenderContext,
) {
    let (target, view_pipeline) = view.into_inner();
    let ready = (|| {
        let world = images.get(&world_target.as_ref()?.image)?;
        let render_pipeline = pipeline_cache.get_render_pipeline(view_pipeline?.0)?;
        let pipeline = pipeline.as_ref()?;
        let ui = target.main_texture_view();
        let fresh = cache
            .cached
            .as_ref()
            .is_some_and(|(w, u, _)| *w == world.texture_view.id() && *u == ui.id());
        if !fresh {
            let group = ctx.render_device().create_bind_group(
                Some("pieced_composite"),
                &pipeline_cache.get_bind_group_layout(&pipeline.layout),
                &BindGroupEntries::sequential((&world.texture_view, &pipeline.sampler, ui)),
            );
            cache.cached = Some((world.texture_view.id(), ui.id(), group));
        }
        Some(render_pipeline)
    })();
    // Always clear: nothing is read back, and a cleared (not loaded) target
    // costs no bandwidth on a tile-based GPU. It also keeps the drawable from
    // showing garbage before the pipeline is ready.
    let Some(attachment) = target.out_texture_color_attachment(Some(LinearRgba::BLACK)) else {
        return;
    };
    let mut pass = ctx
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("pieced_composite"),
            color_attachments: &[Some(attachment)],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    let (Some(render_pipeline), Some((_, _, bind_group))) = (ready, cache.cached.as_ref()) else {
        return;
    };
    pass.set_pipeline(render_pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
}
