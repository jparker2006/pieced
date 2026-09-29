//! Render pipeline setup (client only): the 3D world renders into an offscreen
//! target at a controllable resolution, shown full-screen under a crisp native
//! resolution UI. Owns the main camera, its interpolated follow and FOV/zoom.
//!
//! **Full-screen passes per frame** (M4 performance follow-up; pinned by
//! `tests/render_passes.rs` through [`full_screen_passes`]):
//!
//! | pass | size | where |
//! |---|---|---|
//! | FXAA (Battery) | render (≤ 1.4 MP) | the viewmodel camera, the last 3D camera |
//! | the 3D image into [`WorldTarget`] | render | the viewmodel camera's output; the world camera skips its own copy when both share one main texture ([`route_world_output`]) |
//! | HUD over the 3D image, into the window | window (Retina) | the UI camera's [`UiComposite`] schedule |
//!
//! The HUD itself draws into the UI camera's native-resolution texture (its
//! first draw clears it; no separate clear, no depth). Nothing else runs
//! full-screen at the window's resolution.

mod composite;

pub use composite::{
    COMPOSITE_SHADER_PATH, UiComposite, UiCompositeSystems, ensure_ui_composite_schedule,
};

use crate::{
    combat::GunTuning,
    shared::{ActiveTool, Ads, AppState, EyeHeight, LookAngles, Player, PreviousFeet, WeaponKind},
    tuning::Tuning,
    viewmodel::ViewmodelCamera,
};
use bevy::{
    anti_alias::{contrast_adaptive_sharpening::ContrastAdaptiveSharpening, fxaa::Fxaa},
    camera::{
        CameraMainTextureUsages, CameraOutputMode, ClearColorConfig, Hdr, RenderTarget,
        visibility::RenderLayers,
    },
    core_pipeline::tonemapping::Tonemapping,
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        camera::CameraRenderGraph,
        render_resource::{BlendState, Extent3d, TextureFormat, TextureUsages},
    },
    window::{PresentMode, PrimaryWindow},
};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum QualityPreset {
    /// Default: tuned to hold 60 fps on battery with Low Power Mode on.
    #[default]
    Battery,
    /// A bigger pixel budget and denser grass when plugged in (see
    /// [`crate::look::preset_look`]).
    PluggedIn,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GraphicsTuning {
    pub preset: QualityPreset,
    /// 3D render resolution as a fraction of the window's logical size.
    pub render_scale: f32,
    /// Upper bound on 3D render pixels, in megapixels (0 = no cap). Keeps "More
    /// Space" display scaling (1710×1073 logical) from costing extra GPU time.
    /// The quality preset's pixel budget multiplies it.
    pub max_megapixels: f32,
    pub fullscreen: bool,
    /// true = vsync (Fifo); false = no vsync with `frame_cap`.
    pub vsync: bool,
    /// Frame cap used when vsync is off (0 = uncapped).
    pub frame_cap: u32,
}

impl Default for GraphicsTuning {
    fn default() -> Self {
        Self {
            preset: QualityPreset::Battery,
            render_scale: 1.0,
            max_megapixels: 1.4,
            fullscreen: true,
            vsync: true,
            frame_cap: 60,
        }
    }
}

/// How frames are paced, as the game was built (`--knobs pipelined=off`,
/// `latency=N`): recorded in the session log next to the input latency.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderPacing {
    /// Bevy's pipelined rendering: the render world of frame N runs on its
    /// own thread while the main world simulates N+1 (the default since the
    /// M4 performance follow-up).
    pub pipelined: bool,
    /// The window's `desired_maximum_frame_latency`.
    pub frame_latency: u32,
}

impl RenderPacing {
    /// The pacing without knobs.
    pub const DEFAULT: Self = Self {
        pipelined: true,
        frame_latency: 1,
    };

    /// From the `pipelined` and `latency` knobs (unset = the default).
    pub fn from_knobs(knobs: Option<&crate::perf_knobs::PerfKnobs>) -> Self {
        Self {
            pipelined: knobs
                .and_then(|k| k.pipelined)
                .unwrap_or(Self::DEFAULT.pipelined),
            frame_latency: knobs
                .and_then(|k| k.latency)
                .unwrap_or(Self::DEFAULT.frame_latency),
        }
    }

    pub fn label(&self) -> String {
        format!(
            "pipelined {}, frame latency {}",
            if self.pipelined { "on" } else { "off" },
            self.frame_latency
        )
    }
}

/// The offscreen image the world (and the viewmodel) render into.
#[derive(Resource, Debug, Clone)]
pub struct WorldTarget {
    pub image: Handle<Image>,
    pub size: UVec2,
}

/// The first-person world camera.
#[derive(Component, Debug)]
pub struct MainCamera;

/// Render layer for first-person gun models (drawn by the viewmodel camera only).
pub const VIEWMODEL_LAYER: usize = 1;
/// Render layer used by the UI camera so it never draws world meshes.
pub const UI_LAYER: usize = 31;
/// Camera order of the main world camera; the viewmodel camera uses a higher order
/// on the same target.
pub const MAIN_CAMERA_ORDER: isize = -2;
pub const VIEWMODEL_CAMERA_ORDER: isize = -1;
/// The far layer's own camera (`farres=half`) draws before the world camera.
pub const FAR_CAMERA_ORDER: isize = -3;
/// The UI camera draws last, onto the window.
pub const UI_CAMERA_ORDER: isize = 10;

pub const NEAR_PLANE: f32 = 0.05;
pub const FAR_PLANE: f32 = 1500.0;

/// Current vertical FOV (radians) after ADS zoom smoothing. Other cameras (viewmodel)
/// may read it.
#[derive(Resource, Debug, Clone, Copy)]
pub struct CurrentFov(pub f32);

impl Default for CurrentFov {
    fn default() -> Self {
        Self(70f32.to_radians())
    }
}

/// The main camera is placed at the player's eye in this set (PostUpdate, before
/// transform propagation). Effects that offset the camera (shake) run after it.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CameraFollowSet;

pub struct RenderSetupPlugin;

impl Plugin for RenderSetupPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CurrentFov>()
            .add_systems(Startup, setup_render_target)
            .add_systems(Update, (resize_world_target, update_fov))
            .configure_sets(
                PostUpdate,
                CameraFollowSet.before(TransformSystems::Propagate),
            )
            .add_systems(PostUpdate, follow_player_eye.in_set(CameraFollowSet))
            .add_systems(OnEnter(AppState::Playing), snap_camera)
            .add_systems(Update, sync_present_mode)
            // After every system that turns the viewmodel camera on or off.
            .add_systems(Last, route_world_output);
        composite::build(app);
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .init_resource::<FrameLimiter>()
                .add_systems(ExtractSchedule, extract_frame_limit)
                .add_systems(Render, limit_frame_rate.after(RenderSystems::PostCleanup));
        }
    }

    fn finish(&self, app: &mut App) {
        composite::finish(app);
    }
}

// ---------------------------------------------------------------------------
// Present mode and frame cap
// ---------------------------------------------------------------------------

/// The present mode the graphics settings ask for: vsync is `Fifo`; no vsync is
/// `AutoNoVsync` (Immediate on Metal) paced by the frame limiter.
pub fn present_mode_for(graphics: &GraphicsTuning) -> PresentMode {
    if graphics.vsync {
        PresentMode::Fifo
    } else {
        PresentMode::AutoNoVsync
    }
}

fn sync_present_mode(
    tuning: Res<Tuning>,
    window: Option<Single<&mut Window, With<PrimaryWindow>>>,
) {
    let Some(mut window) = window else {
        return;
    };
    let wanted = present_mode_for(&tuning.graphics);
    if window.present_mode != wanted {
        window.present_mode = wanted;
    }
}

/// Target frame interval for a frame cap (`None` when uncapped).
pub fn frame_interval(cap: u32) -> Option<Duration> {
    (cap > 0).then(|| Duration::from_secs_f64(1.0 / cap as f64))
}

/// The limiter sleeps until this much before the deadline, then spins, because
/// `thread::sleep` on macOS can overshoot by a fraction of a millisecond.
pub const LIMITER_SPIN: Duration = Duration::from_micros(1000);

/// Frame-cap arithmetic: a fixed cadence of frame deadlines `interval` apart.
#[derive(Debug, Clone, PartialEq)]
pub struct FramePacer {
    interval: Duration,
    deadline: Option<Instant>,
}

impl FramePacer {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            deadline: None,
        }
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// When the frame finishing at `now` should end. Deadlines keep a fixed
    /// cadence, so a slightly late frame is followed by a shorter wait. A frame
    /// that overruns its slot by more than half an interval restarts the cadence
    /// at `now` rather than rushing several frames to catch up.
    pub fn next_deadline(&mut self, now: Instant) -> Instant {
        let deadline = match self.deadline {
            None => now,
            Some(previous) => {
                let next = previous + self.interval;
                if now > next + self.interval / 2 {
                    now
                } else {
                    next
                }
            }
        };
        self.deadline = Some(deadline);
        deadline
    }
}

/// How to wait from `now` until `deadline`: (sleep, then spin).
pub fn wait_split(now: Instant, deadline: Instant, spin: Duration) -> (Duration, Duration) {
    let total = deadline.saturating_duration_since(now);
    let sleep = total.saturating_sub(spin);
    (sleep, total - sleep)
}

fn wait_until(deadline: Instant) {
    let (sleep, _) = wait_split(Instant::now(), deadline, LIMITER_SPIN);
    if !sleep.is_zero() {
        std::thread::sleep(sleep);
    }
    while Instant::now() < deadline {
        std::hint::spin_loop();
    }
}

/// Render-world frame limiter, active only with vsync off and a nonzero cap.
#[derive(Resource, Debug, Default)]
struct FrameLimiter(Option<FramePacer>);

fn extract_frame_limit(mut limiter: ResMut<FrameLimiter>, tuning: Extract<Res<Tuning>>) {
    let graphics = &tuning.graphics;
    let interval = if graphics.vsync {
        None
    } else {
        frame_interval(graphics.frame_cap)
    };
    if limiter.0.as_ref().map(FramePacer::interval) != interval {
        limiter.0 = interval.map(FramePacer::new);
    }
}

/// Runs at the very end of the frame, after the frame was submitted and
/// presented, so the wait happens before the next frame gathers input.
fn limit_frame_rate(mut limiter: ResMut<FrameLimiter>) {
    if let Some(pacer) = limiter.0.as_mut() {
        let deadline = pacer.next_deadline(Instant::now());
        wait_until(deadline);
    }
}

/// 3D render resolution: the window's logical size × `scale`, then shrunk (keeping
/// the aspect ratio) so it never exceeds `max_megapixels` (0 = no cap).
pub fn target_size(window: &Window, scale: f32, max_megapixels: f32) -> UVec2 {
    render_size(
        Vec2::new(window.width(), window.height()),
        scale,
        max_megapixels,
    )
}

/// The megapixel cap after the quality preset's pixel budget.
pub fn pixel_cap(graphics: &GraphicsTuning) -> f32 {
    graphics.max_megapixels * crate::look::preset_look(graphics.preset).pixel_budget
}

pub fn render_size(logical: Vec2, scale: f32, max_megapixels: f32) -> UVec2 {
    let mut size = logical * scale.clamp(0.25, 2.0);
    let pixels = size.x * size.y;
    let cap = max_megapixels * 1.0e6;
    if cap > 0.0 && pixels > cap {
        size *= (cap / pixels).sqrt();
    }
    UVec2::new(
        (size.x.round() as u32).max(64),
        (size.y.round() as u32).max(64),
    )
}

fn setup_render_target(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    tuning: Res<Tuning>,
) {
    let size = window
        .map(|w| {
            target_size(
                &w,
                tuning.graphics.render_scale,
                pixel_cap(&tuning.graphics),
            )
        })
        .unwrap_or(UVec2::new(1470, 956));
    let mut image = Image::new_target_texture(
        size.x,
        size.y,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    );
    // COPY_SRC lets the GPU timing's end-of-camera marks wait on it.
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let image = images.add(image);
    commands.insert_resource(WorldTarget {
        image: image.clone(),
        size,
    });
    commands.spawn((
        Name::new("Main camera"),
        MainCamera,
        Camera3d::default(),
        RenderTarget::Image(image.into()),
        Camera {
            order: MAIN_CAMERA_ORDER,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.62, 0.78, 0.95)),
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            fov: tuning.look.fov_deg.to_radians(),
            near: NEAR_PLANE,
            far: FAR_PLANE,
            ..default()
        }),
        // Both 3D cameras share one MSAA setting from the quality preset;
        // `look` also sets `Tonemapping::None` on them (palette colors as authored).
        crate::look::msaa_for(crate::look::preset_look(tuning.graphics.preset).msaa_samples),
        Transform::from_xyz(0.0, 1.6, 0.0),
    ));
    commands.spawn((
        Name::new("UI camera"),
        Camera2d,
        // Not `Core2d`: the HUD over a transparent clear, then one full-screen
        // composite of HUD and 3D image into the window (`composite`).
        CameraRenderGraph::new(UiComposite),
        IsDefaultUiCamera,
        RenderLayers::layer(UI_LAYER),
        // The UI draws at native Retina resolution (up to ~7 MP); Bevy's UI shader
        // anti-aliases its own edges, so 4x MSAA here would only cost bandwidth.
        Msaa::Off,
        Camera {
            order: UI_CAMERA_ORDER,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            // The composite writes the window; Bevy's own copy never runs.
            output_mode: CameraOutputMode::Skip,
            ..default()
        },
        // COPY_DST lets the GPU timing's last mark wait for the composite's
        // read of this texture (`crate::gpu_timing`).
        CameraMainTextureUsages::default().with(TextureUsages::COPY_DST),
    ));
}

/// The parts of a 3D camera that decide whether it shares Bevy's main
/// texture with another (same target, MSAA, HDR and texture usages).
type CameraRoute<'a> = (
    &'a mut Camera,
    &'a RenderTarget,
    &'a Msaa,
    Has<Hdr>,
    &'a CameraMainTextureUsages,
);

/// Whether the world and viewmodel cameras draw into one shared main
/// texture, with the viewmodel camera, which draws last, active.
fn shares_main_texture(
    world: (&RenderTarget, &Msaa, bool, &CameraMainTextureUsages),
    viewmodel: (bool, &RenderTarget, &Msaa, bool, &CameraMainTextureUsages),
) -> bool {
    let same_target = match (world.0, viewmodel.1) {
        (RenderTarget::Image(a), RenderTarget::Image(b)) => a.handle == b.handle,
        _ => false,
    };
    viewmodel.0
        && same_target
        && world.1 == viewmodel.2
        && world.2 == viewmodel.3
        && world.3.0 == viewmodel.4.0
}

/// The viewmodel camera's output when it writes the whole shared image: a
/// straight copy over a cleared target (nothing to blend with or load).
pub const VIEWMODEL_SOLE_OUTPUT: CameraOutputMode = CameraOutputMode::Write {
    blend_state: Some(BlendState::REPLACE),
    clear_color: ClearColorConfig::Custom(Color::BLACK),
};

/// One copy of the 3D image into [`WorldTarget`] per frame. Both 3D cameras
/// normally share one main texture, and the viewmodel camera (drawing last,
/// with FXAA) copies all of it: the world camera's own copy would be
/// overwritten unseen, so it skips it. When they don't share (the split MSAA
/// knob) or the viewmodel camera is off (the death beat, `viewmodel=off`),
/// each camera writes as Bevy does by default.
pub fn route_world_output(
    mut world: Query<CameraRoute, (With<MainCamera>, Without<ViewmodelCamera>)>,
    mut viewmodel: Query<CameraRoute, (With<ViewmodelCamera>, Without<MainCamera>)>,
) {
    let (Ok(world), Ok(viewmodel)) = (world.single_mut(), viewmodel.single_mut()) else {
        // Without a viewmodel camera the world camera writes (the default).
        return;
    };
    let (mut world_camera, world_target, world_msaa, world_hdr, world_usages) = world;
    let (mut vm_camera, vm_target, vm_msaa, vm_hdr, vm_usages) = viewmodel;
    let shared = shares_main_texture(
        (world_target, world_msaa, world_hdr, world_usages),
        (vm_camera.is_active, vm_target, vm_msaa, vm_hdr, vm_usages),
    );
    // Written only on a change: `Camera` changes are not free.
    if shared != matches!(world_camera.output_mode, CameraOutputMode::Skip) {
        world_camera.output_mode = if shared {
            CameraOutputMode::Skip
        } else {
            CameraOutputMode::default()
        };
        vm_camera.output_mode = if shared {
            VIEWMODEL_SOLE_OUTPUT
        } else {
            CameraOutputMode::default()
        };
    }
}

/// Where a full-screen pass runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassSize {
    /// The 3D render size (≤ [`GraphicsTuning::max_megapixels`]).
    Render,
    /// The window's native (Retina) size.
    Window,
}

/// A full-screen pass the cameras run each frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FullScreenPass {
    pub name: &'static str,
    pub size: PassSize,
}

/// Every full-screen pass the active cameras run per frame, in camera
/// order, read from the components the render graph acts on:
/// post-processing (tonemapping, FXAA, sharpening), each 3D camera's copy
/// into its output, full-screen UI images, and the UI camera's composite
/// (or a stock `Core2d` camera's clear and copy). `tests/render_passes.rs`
/// pins it, so a new full-screen pass can't slip in unnoticed.
pub fn full_screen_passes(world: &mut World) -> Vec<FullScreenPass> {
    let mut cameras: Vec<(isize, Entity)> = world
        .query::<(Entity, &Camera)>()
        .iter(world)
        .filter(|(_, c)| c.is_active)
        .map(|(e, c)| (c.order, e))
        .collect();
    cameras.sort();
    let pass = |name, size| FullScreenPass { name, size };
    let mut out = Vec::new();
    for (_, entity) in cameras {
        let e = world.entity(entity);
        let Some(camera) = e.get::<Camera>() else {
            continue;
        };
        let size = if matches!(e.get::<RenderTarget>(), Some(RenderTarget::Image(_))) {
            PassSize::Render
        } else {
            PassSize::Window
        };
        let writes = !matches!(camera.output_mode, CameraOutputMode::Skip);
        if e.contains::<Camera3d>() {
            if e.get::<Tonemapping>().is_some_and(|t| *t != Tonemapping::None) {
                out.push(pass("tonemapping", size));
            }
            if e.get::<Fxaa>().is_some_and(|f| f.enabled) {
                out.push(pass("fxaa", size));
            }
            if e
                .get::<ContrastAdaptiveSharpening>()
                .is_some_and(|c| c.enabled)
            {
                out.push(pass("sharpening", size));
            }
            if writes {
                out.push(pass("copy_to_output", size));
            }
        } else if e
            .get::<CameraRenderGraph>()
            .is_some_and(|g| g.0 == bevy::ecs::schedule::ScheduleLabel::intern(&UiComposite))
        {
            out.push(pass("ui_composite", PassSize::Window));
        } else {
            out.push(pass("clear_2d", size));
            if writes {
                out.push(pass("copy_to_output", size));
            }
        }
    }
    // A UI image stretched over the whole screen is a full-screen pass at
    // the window's resolution (how the 3D image used to be shown).
    let full = |v: Val| matches!(v, Val::Percent(p) if p >= 99.9) || v == Val::Vw(100.0);
    let screen_images = world
        .query_filtered::<&Node, With<ImageNode>>()
        .iter(world)
        .filter(|n| full(n.width) && full(n.height))
        .count();
    for _ in 0..screen_images {
        out.push(pass("ui_fullscreen_image", PassSize::Window));
    }
    out
}

fn resize_world_target(
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    tuning: Res<Tuning>,
    dynres: Option<Res<crate::perf::DynamicResolution>>,
    target: Option<ResMut<WorldTarget>>,
    mut images: ResMut<Assets<Image>>,
) {
    let (Some(window), Some(mut target)) = (window, target) else {
        return;
    };
    // `dynres=on` (M4, D100) scales the Battery preset between 0.8 and 1.0.
    let dynamic = dynres.map_or(1.0, |d| d.scale);
    let size = target_size(
        &window,
        tuning.graphics.render_scale * dynamic,
        pixel_cap(&tuning.graphics),
    );
    if size == target.size {
        return;
    }
    if let Some(mut image) = images.get_mut(&target.image) {
        image.resize(Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        });
        target.size = size;
    }
}

/// ADS zoom target for the current tool.
fn zoom_for(tool: &ActiveTool, ads: bool, rifle: &GunTuning, pump: &GunTuning) -> f32 {
    match (ads, tool) {
        (true, ActiveTool::Weapon(WeaponKind::Rifle)) => rifle.ads_zoom,
        (true, ActiveTool::Weapon(WeaponKind::Pump)) => pump.ads_zoom,
        _ => 1.0,
    }
}

fn update_fov(
    time: Res<Time>,
    tuning: Res<Tuning>,
    player: Option<Single<(&ActiveTool, &Ads), With<Player>>>,
    mut fov: ResMut<CurrentFov>,
    mut projection: Option<Single<&mut Projection, With<MainCamera>>>,
) {
    let zoom = player
        .map(|p| zoom_for(p.0, p.1.0, &tuning.combat.rifle, &tuning.combat.pump))
        .unwrap_or(1.0);
    let target = tuning.look.fov_deg.clamp(50.0, 100.0).to_radians() * zoom;
    // About 0.1 s to settle.
    let blend = 1.0 - (-time.delta_secs() * 30.0).exp();
    fov.0 += (target - fov.0) * blend;
    if let Some(projection) = projection.as_mut()
        && let Projection::Perspective(p) = projection.as_mut()
        && (p.fov - fov.0).abs() > 1e-5
    {
        p.fov = fov.0;
    }
}

/// Places the camera at the player's eye, interpolated between fixed ticks.
fn follow_player_eye(
    fixed: Res<Time<Fixed>>,
    player: Option<Single<(&Transform, &PreviousFeet, &EyeHeight, &LookAngles), With<Player>>>,
    mut camera: Option<Single<&mut Transform, (With<MainCamera>, Without<Player>)>>,
    state: Res<State<AppState>>,
) {
    let (Some(player), Some(camera)) = (player, camera.as_mut()) else {
        return;
    };
    if *state.get() == AppState::Menu {
        // The main menu orbits the arena instead (`menu::main_menu`, also in
        // `CameraFollowSet`).
        return;
    }
    let (transform, previous, eye, look) = player.into_inner();
    let alpha = if *state.get() == AppState::Playing {
        fixed.overstep_fraction().clamp(0.0, 1.0)
    } else {
        1.0
    };
    let feet = previous.0.lerp(transform.translation, alpha);
    camera.translation = feet + Vec3::Y * eye.0;
    camera.rotation = look.rotation();
}

fn snap_camera(
    player: Option<Single<(&Transform, &EyeHeight, &LookAngles), With<Player>>>,
    mut camera: Option<Single<&mut Transform, (With<MainCamera>, Without<Player>)>>,
) {
    if let (Some(player), Some(camera)) = (player, camera.as_mut()) {
        let (transform, eye, look) = player.into_inner();
        camera.translation = transform.translation + Vec3::Y * eye.0;
        camera.rotation = look.rotation();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_size_caps_megapixels_and_keeps_aspect() {
        // "More Space" scaling on the 15" Air: 1710×1073 logical.
        let capped = render_size(Vec2::new(1710.0, 1073.0), 1.0, 1.4);
        let pixels = capped.x as f32 * capped.y as f32;
        assert!(pixels <= 1.401e6 && pixels > 1.38e6, "{capped}");
        let aspect = capped.x as f32 / capped.y as f32;
        assert!((aspect - 1710.0 / 1073.0).abs() < 0.01);
        // Under the cap, and with no cap, the logical size is kept.
        assert_eq!(
            render_size(Vec2::new(1280.0, 800.0), 1.0, 1.4),
            UVec2::new(1280, 800)
        );
        assert_eq!(
            render_size(Vec2::new(1710.0, 1073.0), 1.0, 0.0),
            UVec2::new(1710, 1073)
        );
        assert_eq!(
            render_size(Vec2::new(1280.0, 800.0), 0.5, 1.4),
            UVec2::new(640, 400)
        );
    }

    #[test]
    fn pipelined_rendering_is_the_default_and_a_knob_turns_it_off() {
        use crate::perf_knobs::PerfKnobs;
        let default = RenderPacing::from_knobs(None);
        assert_eq!(default, RenderPacing::DEFAULT);
        assert!(default.pipelined);
        assert_eq!(default.frame_latency, 1);
        assert_eq!(default.label(), "pipelined on, frame latency 1");
        let off = RenderPacing::from_knobs(Some(&PerfKnobs::parse("pipelined=off")));
        assert!(!off.pipelined);
        let other = RenderPacing::from_knobs(Some(&PerfKnobs::parse("latency=2,fxaa=off")));
        assert_eq!(
            other,
            RenderPacing {
                pipelined: true,
                frame_latency: 2
            }
        );
    }

    #[test]
    fn plugged_in_preset_raises_the_pixel_budget() {
        let mut graphics = GraphicsTuning::default();
        assert_eq!(pixel_cap(&graphics), graphics.max_megapixels);
        graphics.preset = QualityPreset::PluggedIn;
        assert!(pixel_cap(&graphics) > graphics.max_megapixels);
        // The default window on the 15" Air renders at the reference height the
        // outline width is tuned for.
        let battery = render_size(Vec2::new(1710.0, 1107.0), 1.0, 1.4);
        assert!((battery.y as f32 - crate::look::REFERENCE_HEIGHT_PX).abs() < 8.0);
    }
}
