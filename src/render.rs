//! Render pipeline setup (client only): the 3D world renders into an offscreen
//! target at a controllable resolution, shown full-screen under a crisp native
//! resolution UI. Owns the main camera, its interpolated follow and FOV/zoom.

use crate::{
    combat::GunTuning,
    shared::{ActiveTool, Ads, AppState, EyeHeight, LookAngles, Player, PreviousFeet, WeaponKind},
    tuning::Tuning,
};
use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        render_resource::{Extent3d, TextureFormat},
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
    /// Extra shadow resolution and effects when plugged in.
    PluggedIn,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GraphicsTuning {
    pub preset: QualityPreset,
    /// 3D render resolution as a fraction of the window's logical size.
    pub render_scale: f32,
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
            fullscreen: true,
            vsync: true,
            frame_cap: 60,
        }
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

/// The full-screen UI image that displays [`WorldTarget`].
#[derive(Component, Debug)]
pub struct WorldView;

/// Render layer for first-person gun models (drawn by the viewmodel camera only).
pub const VIEWMODEL_LAYER: usize = 1;
/// Render layer used by the UI camera so it never draws world meshes.
pub const UI_LAYER: usize = 31;
/// Camera order of the main world camera; the viewmodel camera uses a higher order
/// on the same target.
pub const MAIN_CAMERA_ORDER: isize = -2;
pub const VIEWMODEL_CAMERA_ORDER: isize = -1;

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
            .add_systems(Update, sync_present_mode);
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .init_resource::<FrameLimiter>()
                .add_systems(ExtractSchedule, extract_frame_limit)
                .add_systems(Render, limit_frame_rate.after(RenderSystems::PostCleanup));
        }
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

pub fn target_size(window: &Window, scale: f32) -> UVec2 {
    let scale = scale.clamp(0.25, 2.0);
    UVec2::new(
        ((window.width() * scale).round() as u32).max(64),
        ((window.height() * scale).round() as u32).max(64),
    )
}

fn setup_render_target(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    tuning: Res<Tuning>,
) {
    let size = window
        .map(|w| target_size(&w, tuning.graphics.render_scale))
        .unwrap_or(UVec2::new(1470, 956));
    let image = images.add(Image::new_target_texture(
        size.x,
        size.y,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    ));
    commands.insert_resource(WorldTarget {
        image: image.clone(),
        size,
    });
    commands.spawn((
        Name::new("Main camera"),
        MainCamera,
        Camera3d::default(),
        RenderTarget::Image(image.clone().into()),
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
        Msaa::Sample4,
        Transform::from_xyz(0.0, 1.6, 0.0),
    ));
    commands.spawn((
        Name::new("UI camera"),
        Camera2d,
        IsDefaultUiCamera,
        RenderLayers::layer(UI_LAYER),
        Camera {
            order: 10,
            ..default()
        },
    ));
    commands.spawn((
        Name::new("World view"),
        WorldView,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        ImageNode::new(image),
        GlobalZIndex(-1000),
    ));
}

fn resize_world_target(
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    tuning: Res<Tuning>,
    target: Option<ResMut<WorldTarget>>,
    mut images: ResMut<Assets<Image>>,
) {
    let (Some(window), Some(mut target)) = (window, target) else {
        return;
    };
    let size = target_size(&window, tuning.graphics.render_scale);
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
