//! The pause blur (M4 chunk 5, D110): when the game pauses, **one**
//! downsampled blur of the last frame goes behind the pause menu, and the 3D
//! world stops rendering until play resumes (which also saves battery).
//!
//! - The world renders into [`WorldTarget`], which keeps its last frame when
//!   no camera draws into it. On pausing, a small 2D camera ([`BlurCamera`],
//!   order [`BLUR_CAMERA_ORDER`], after the 3D cameras and before the UI)
//!   draws one full-screen [`BlurMaterial`] node (a 7×7 Gaussian,
//!   `assets/shaders/pause_blur.wgsl`) from it into a quarter-size image
//!   for [`BLUR_FRAMES`] frames, then switches off.
//! - Every 3D camera (the world, the gun, the far layer) is switched off
//!   while the pause menu's main page shows; they come back on resume, and
//!   on the Settings pages so the graphics settings preview live
//!   (docs/M4-SPEC.md → Settings).
//! - The pause menu's backdrop shows the blurred image stretched over the
//!   screen under its veil.
//!
//! **Cost.** Per pause: one 7×7 pass at a sixteenth of the render's pixels
//! (≈ 0.1 ms, once). While paused: no 3D passes at all, just the UI pass
//! and the composite, with one more screen-size UI image (≈ 0.3 ms). The
//! blur's pipeline is warmed behind the loading screen (the camera draws it
//! throughout Boot). Nothing is allocated per pause.

use super::{MenuPage, MenuState};
use crate::{
    render::{MainCamera, WorldTarget},
    shared::AppState,
};
use bevy::{
    asset::io::embedded::EmbeddedAssetRegistry,
    camera::RenderTarget,
    prelude::*,
    render::{
        RenderApp,
        render_resource::{AsBindGroup, TextureFormat},
    },
    shader::ShaderRef,
};
use std::path::{Path, PathBuf};

/// The blur camera draws after the 3D cameras and before the UI.
pub const BLUR_CAMERA_ORDER: isize = 0;
/// Frames the blur camera draws after a pause starts (the last 3D frame is
/// already in the world image; the second is a safety margin).
pub const BLUR_FRAMES: u32 = 2;
/// The blurred image's size as a fraction of the render size.
pub const BLUR_SCALE: u32 = 4;
/// The veil over the blurred world (the pause menu's page) and over the live
/// world behind Settings (lighter, so changes read).
pub const VEIL_BLURRED: f32 = 0.38;
pub const VEIL_LIVE: f32 = 0.2;
const SHADER: &str = "embedded://pieced/shaders/pause_blur.wgsl";

/// The blur's material: the world image and the tap spacing.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct BlurMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub world: Handle<Image>,
    /// xy: a source texel in uv; z: the taps' spacing in texels.
    #[uniform(2)]
    pub params: Vec4,
}

impl UiMaterial for BlurMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

/// The blur's 2D camera.
#[derive(Component, Debug)]
pub struct BlurCamera;

/// The full-screen blurred image behind the pause menu.
#[derive(Component, Debug)]
pub struct BlurBackdrop;

/// The blur's state (public for tests and the session log).
#[derive(Resource, Debug, Default, Clone)]
pub struct PauseBlur {
    pub image: Option<Handle<Image>>,
    /// Frames the blur camera still has to draw.
    pub frames_left: u32,
    /// The world is frozen: its 3D cameras are off.
    pub frozen: bool,
    /// Blurs made so far.
    pub blurs: u32,
    /// The 3D cameras this switched off (switched back on at resume).
    parked: Vec<Entity>,
}

pub(super) fn build(app: &mut App) {
    if let Some(registry) = app.world().get_resource::<EmbeddedAssetRegistry>() {
        registry.insert_asset(
            PathBuf::new(),
            Path::new("pieced/shaders/pause_blur.wgsl"),
            include_bytes!("../../assets/shaders/pause_blur.wgsl").as_slice(),
        );
    }
    if app.get_sub_app(RenderApp).is_some() {
        app.add_plugins(UiMaterialPlugin::<BlurMaterial>::default());
    } else {
        app.init_asset::<BlurMaterial>();
    }
    app.insert_resource(PauseBlur {
        parked: Vec::with_capacity(4),
        ..default()
    })
    .add_systems(Update, (setup_blur, drive_blur).chain());
}

/// Whether the pause menu wants the frozen, blurred world: paused, on its
/// main page (Settings keeps the world live to preview changes).
pub fn wants_blur(state: AppState, menu: &MenuState) -> bool {
    state == AppState::Paused && menu.menu_visible() && menu.page == MenuPage::Main && !menu.title
}

/// Makes the blur image, camera and nodes once the world image exists.
fn setup_blur(
    mut commands: Commands,
    target: Option<Res<WorldTarget>>,
    images: Option<ResMut<Assets<Image>>>,
    materials: Option<ResMut<Assets<BlurMaterial>>>,
    mut blur: ResMut<PauseBlur>,
) {
    if blur.image.is_some() {
        return;
    }
    let (Some(target), Some(mut images), Some(mut materials)) = (target, images, materials) else {
        return;
    };
    let size = (target.size / BLUR_SCALE).max(UVec2::splat(16));
    let image = images.add(Image::new_target_texture(
        size.x,
        size.y,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    ));
    let texel = Vec2::ONE / target.size.as_vec2().max(Vec2::ONE);
    let material = materials.add(BlurMaterial {
        world: target.image.clone(),
        params: Vec4::new(texel.x, texel.y, 3.5, 0.0),
    });
    let camera = commands
        .spawn((
            Name::new("Pause blur camera"),
            BlurCamera,
            Camera2d,
            RenderTarget::Image(image.clone().into()),
            Camera {
                order: BLUR_CAMERA_ORDER,
                clear_color: ClearColorConfig::Custom(Color::BLACK),
                // Drawn through Boot, behind the loading screen, so its
                // pipeline is ready for the first pause.
                is_active: true,
                ..default()
            },
            Msaa::Off,
        ))
        .id();
    commands.spawn((
        Name::new("Pause blur pass"),
        MaterialNode(material),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        UiTargetCamera(camera),
    ));
    // Anchored to all four edges (not 100% wide): only while paused.
    commands.spawn((
        Name::new("Pause blur backdrop"),
        BlurBackdrop,
        ImageNode::new(image.clone()),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            ..default()
        },
        GlobalZIndex(99),
        Visibility::Hidden,
    ));
    blur.image = Some(image);
}

/// Pausing: blur the last frame once, then switch the 3D cameras off.
/// Resuming (or opening Settings): switch them back on.
#[allow(clippy::type_complexity)]
fn drive_blur(
    state: Res<State<AppState>>,
    menu: Res<MenuState>,
    mut blur: ResMut<PauseBlur>,
    mut blur_camera: Query<&mut Camera, With<BlurCamera>>,
    mut cameras: Query<(Entity, &mut Camera), (With<Camera3d>, Without<BlurCamera>)>,
    main: Query<(), With<MainCamera>>,
    mut backdrop: Query<&mut Visibility, With<BlurBackdrop>>,
) {
    let booting = *state.get() == AppState::Boot;
    let want = wants_blur(*state.get(), &menu);
    let blur = &mut *blur;
    if want && !blur.frozen {
        // The world image holds the last frame: blur it, then freeze.
        blur.frozen = true;
        blur.frames_left = BLUR_FRAMES;
        blur.blurs += 1;
    } else if !want && blur.frozen {
        blur.frozen = false;
        blur.frames_left = 0;
        for (entity, mut camera) in &mut cameras {
            if blur.parked.contains(&entity) && !camera.is_active {
                camera.is_active = true;
            }
        }
        blur.parked.clear();
    }
    if blur.frozen {
        // The first frame still draws the 3D image the blur reads (the
        // world stands still, so it is the frame the player paused on).
        let first = blur.frames_left == BLUR_FRAMES;
        if !first && main.iter().next().is_some() {
            for (entity, mut camera) in &mut cameras {
                if camera.is_active {
                    camera.is_active = false;
                    if !blur.parked.contains(&entity) {
                        blur.parked.push(entity);
                    }
                }
            }
        }
    }
    let drawing = booting || blur.frames_left > 0;
    blur.frames_left = blur.frames_left.saturating_sub(1);
    for mut camera in &mut blur_camera {
        if camera.is_active != drawing {
            camera.is_active = drawing;
        }
    }
    let show = blur.frozen && want;
    for mut v in &mut backdrop {
        v.set_if_neq(if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_pause_menus_main_page_blurs() {
        let mut menu = MenuState {
            menu_open: true,
            ..default()
        };
        assert!(wants_blur(AppState::Paused, &menu));
        assert!(!wants_blur(AppState::Playing, &menu));
        menu.page = MenuPage::Settings;
        assert!(!wants_blur(AppState::Paused, &menu), "settings preview live");
        menu.page = MenuPage::Main;
        menu.panel_open = true;
        assert!(!wants_blur(AppState::Paused, &menu), "the tuning panel");
        menu.panel_open = false;
        menu.title = true;
        assert!(!wants_blur(AppState::Menu, &menu), "the main menu is live");
    }
}
