//! The main menu (D92, M3 chunk 5): the game opens here after Boot.
//!
//! - The PIECED logo top left over the live island and sky, the cartoon
//!   buttons stacked under it in the pause menu's style (T12): **Waves**
//!   with the best run under it, **Practice**, **Settings** (the Settings
//!   card and its Controls page, over the island) and **Quit**.
//! - The hero shot (M4-V8): a knight on a brick plinth in the arena, his
//!   wand raised and glowing, framed low from in front with the castle
//!   filling the right of the view and the galaxy over the buttons. The
//!   camera drifts a little ([`HeroCamera`]) so the island stays alive. The
//!   knight and plinth are Blender models shown only on the title (no
//!   collider: the arena's pieces and fights never see them). No player
//!   input (the adapter only drives the player in `Playing`), the HUD and
//!   the gun hidden, the cursor free.
//! - Waves and Practice start that mode in place (`waves::modes`); the time
//!   from the click to the first controllable frame prints as
//!   `PIECED_PLAY_MS <ms> <mode>` and goes into `session.json`.
//!
//! Everything is spawned once at startup and only has its visibility and
//! text rewritten; the orbit writes one transform per frame.

use super::{
    MenuPage, MenuState,
    pause::{LOGO_WIDTH, cartoon_button},
};
use crate::{
    arena::visuals::wand::{
        CRYSTAL_GLOW, GLOW_STEPS, WAND_MODEL, WandAssets, raise_rotation, wand_in_arm,
    },
    hud::{
        UiArt,
        style::{PANEL, RIM, TEXT, caps, ink},
    },
    knight::{KNIGHT_MODEL, KnightAssets},
    look::{Halo, ModelDressed, Outline},
    models::{MODEL_FORWARD_FIX, ModelLibrary, ModelParts, spawn_model},
    render::{CameraFollowSet, MainCamera},
    scenario::ScenarioRun,
    session::{PlayRecord, SessionLog},
    shared::{AppState, GameMode},
    waves::{PersonalBest, modes::StartMode, ui::write_thousands},
};
use bevy::{
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use std::{fmt::Write, time::Instant};

pub(super) fn build(app: &mut App) {
    app.init_resource::<HeroCamera>()
        .init_resource::<PlayClock>()
        .init_resource::<LastPlay>()
        .add_message::<ModelDressed>()
        .add_systems(Startup, spawn_title)
        .add_systems(OnEnter(AppState::Menu), reset_orbit)
        .add_systems(
            Update,
            (
                title_visibility,
                title_buttons,
                refresh_best,
                spawn_hero,
                dress_hero,
                hero_visibility,
            ),
        )
        .add_systems(
            PostUpdate,
            orbit_camera
                .in_set(CameraFollowSet)
                .run_if(in_state(AppState::Menu)),
        )
        .add_systems(Last, report_play);
}

/// The main menu's buttons.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainMenuButton {
    Waves,
    Practice,
    Settings,
    Quit,
}

/// The title screen's root.
#[derive(Component, Debug)]
pub struct TitleRoot;

/// "BEST  WAVE 7 · 12,450" under the Waves button.
#[derive(Component, Debug)]
pub struct BestLine;

/// Where the hero knight's plinth stands: in the arena's south, off the
/// build grid's edges, the castle (azimuth 30° from the spawn) behind it.
pub const HERO_PLINTH: Vec3 = Vec3::new(-3.0, 0.0, 15.5);
/// The plinth is the brick wall model squashed into a block: its width,
/// height and depth scales.
const PLINTH_SCALE: Vec3 = Vec3::new(0.62, 0.45, 3.6);
/// The knight's feet, on the plinth's top.
pub const HERO_FEET: Vec3 = Vec3::new(HERO_PLINTH.x, 3.0 * PLINTH_SCALE.y, HERO_PLINTH.z);
/// The camera's bearing to the knight (degrees clockwise from north), its
/// distance and height; the view's bearing and pitch.
const HERO_BEARING: f32 = 17.0;
const HERO_DISTANCE: f32 = 5.0;
const HERO_EYE_Y: f32 = 1.9;
const VIEW_BEARING: f32 = 9.0;
const VIEW_PITCH: f32 = 11.0;

/// Unit vector along the ground at `deg` clockwise from north (-Z).
fn bearing(deg: f32) -> Vec3 {
    let a = deg.to_radians();
    Vec3::new(a.sin(), 0.0, -a.cos())
}

/// The main menu's camera: a low hero shot of the knight on his plinth with
/// the castle behind, drifting gently (a few decimetres and a degree or two
/// over about 20 s) so the island stays alive.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct HeroCamera {
    /// Seconds on the title so far (real time; resets each visit).
    pub t: f32,
}

/// Where the hero camera is `t` seconds in.
pub fn hero_eye(t: f32) -> Transform {
    let base = HERO_FEET.with_y(0.0) - bearing(HERO_BEARING) * HERO_DISTANCE;
    let side = bearing(HERO_BEARING + 90.0);
    let sway = (t * std::f32::consts::TAU / 21.0).sin();
    let bob = (t * std::f32::consts::TAU / 13.0).sin();
    let eye = base.with_y(HERO_EYE_Y) + side * (0.35 * sway) + Vec3::Y * (0.08 * bob);
    let yaw = (VIEW_BEARING + 1.2 * sway).to_radians();
    let pitch = VIEW_PITCH.to_radians();
    let ahead = Vec3::new(
        yaw.sin() * pitch.cos(),
        pitch.sin(),
        -yaw.cos() * pitch.cos(),
    );
    Transform::from_translation(eye).looking_to(ahead, Vec3::Y)
}

/// The title's hero: the knight on his plinth (the root is shown only on the
/// main menu).
#[derive(Component, Debug)]
pub struct MenuHero;

/// The hero's knight model root, and his wand's.
#[derive(Component, Debug)]
struct HeroKnight;
#[derive(Component, Debug)]
struct HeroWand;

/// Spawns the hero once the model library is ready.
fn spawn_hero(
    mut commands: Commands,
    library: Option<Res<ModelLibrary>>,
    spawned: Query<(), With<MenuHero>>,
) {
    let Some(library) = library else {
        return;
    };
    if !spawned.is_empty() || library.get(KNIGHT_MODEL).is_none() {
        return;
    }
    let root = commands
        .spawn((
            MenuHero,
            Name::new("Menu hero"),
            Transform::default(),
            Visibility::Hidden,
        ))
        .id();
    let to_camera = -bearing(HERO_BEARING);
    let plinth = Transform::from_translation(HERO_PLINTH)
        .looking_to(to_camera, Vec3::Y)
        .with_scale(PLINTH_SCALE);
    if let Some(block) = spawn_model(&mut commands, &library, "wall_brick", plinth) {
        commands
            .entity(block)
            .insert((Outline::default(), ChildOf(root)));
    }
    // He turns a little toward the castle, his wand arm to the camera.
    let facing = Quat::from_rotation_y(-0.45) * to_camera;
    let knight = Transform::from_translation(HERO_FEET).looking_to(facing, Vec3::Y);
    if let Some(model) = spawn_model(&mut commands, &library, KNIGHT_MODEL, knight) {
        commands
            .entity(model)
            .insert((HeroKnight, Outline::default(), ChildOf(root)));
    }
}

/// Dresses the hero once his models are in: the knight's warm-rim material,
/// only the open eyes, the wand arm raised with the wand in his glove and its
/// crystal blazing.
#[allow(clippy::too_many_arguments)]
fn dress_hero(
    mut commands: Commands,
    mut dressed: MessageReader<ModelDressed>,
    parts: ModelParts,
    knights: Query<(), With<HeroKnight>>,
    wands: Query<(), With<HeroWand>>,
    children: Query<&Children>,
    meshes: Query<(), With<Mesh3d>>,
    mut transforms: Query<&mut Transform>,
    library: Option<Res<ModelLibrary>>,
    knight_assets: Option<Res<KnightAssets>>,
    wand_assets: Option<Res<WandAssets>>,
) {
    for event in dressed.read() {
        let is_knight = knights.contains(event.root);
        let is_wand = wands.contains(event.root);
        if !is_knight && !is_wand {
            continue;
        }
        if let Some(assets) = &knight_assets {
            for e in children.iter_descendants(event.root) {
                if meshes.contains(e) {
                    commands
                        .entity(e)
                        .insert(MeshMaterial3d(assets.material.clone()));
                }
            }
        }
        if is_wand {
            let crystal = parts.find(event.root, "Crystal").and_then(|node| {
                std::iter::once(node)
                    .chain(children.iter_descendants(node))
                    .find(|e| meshes.contains(*e))
            });
            if let (Some(crystal), Some(assets)) = (crystal, &wand_assets) {
                commands
                    .entity(crystal)
                    .insert(MeshMaterial3d(assets.glow[GLOW_STEPS - 1].clone()));
            }
            if let Some(tip) = parts.find(event.root, "Tip") {
                commands
                    .entity(tip)
                    .insert(Halo::new(CRYSTAL_GLOW, 0.75, 2.4));
            }
            continue;
        }
        for prefix in ["EyeWide", "EyeBlink", "EyeX"] {
            for side in ["L", "R"] {
                if let Some(eye) = parts.find(event.root, &format!("{prefix}{side}")) {
                    commands.entity(eye).insert(Visibility::Hidden);
                }
            }
        }
        let fix = MODEL_FORWARD_FIX;
        if let Some(arm) = parts.find(event.root, "PivotArmR") {
            if let Ok(mut t) = transforms.get_mut(arm) {
                // Raised high, wand to the sky.
                t.rotation = fix.inverse()
                    * raise_rotation(1.0)
                    * Quat::from_rotation_x(0.95)
                    * Quat::from_rotation_z(-0.2)
                    * fix
                    * t.rotation;
            }
            if let Some(library) = &library
                && let Some(wand) = spawn_model(&mut commands, library, WAND_MODEL, wand_in_arm())
            {
                commands
                    .entity(wand)
                    .insert((HeroWand, Outline::default(), ChildOf(arm)));
            }
        }
        if let Some(arm) = parts.find(event.root, "PivotArmL")
            && let Ok(mut t) = transforms.get_mut(arm)
        {
            // The other fist on his hip.
            t.rotation = fix.inverse() * Quat::from_rotation_z(0.5) * fix * t.rotation;
        }
        if let Some(head) = parts.find(event.root, "PivotHead")
            && let Ok(mut t) = transforms.get_mut(head)
        {
            // Chin up, looking out over the island.
            t.rotation = fix.inverse() * Quat::from_rotation_x(0.12) * fix * t.rotation;
        }
    }
}

/// Shows the hero only on the main menu.
fn hero_visibility(
    state: Res<State<AppState>>,
    mut heroes: Query<&mut Visibility, With<MenuHero>>,
) {
    let want = if *state.get() == AppState::Menu {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut v in &mut heroes {
        v.set_if_neq(want);
    }
}

/// The Play click being timed: when, and which mode.
#[derive(Resource, Debug, Default)]
struct PlayClock(Option<(Instant, GameMode)>);

/// The last `PIECED_PLAY_MS`: milliseconds from the Play click to the first
/// controllable frame, and the mode.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct LastPlay(pub Option<(f64, GameMode)>);

/// The buttons, in the row's order.
const BUTTONS: [(&str, MainMenuButton); 4] = [
    ("WAVES", MainMenuButton::Waves),
    ("PRACTICE", MainMenuButton::Practice),
    ("SETTINGS", MainMenuButton::Settings),
    ("QUIT", MainMenuButton::Quit),
];
const BUTTON_WIDTH: f32 = 380.0;

fn spawn_title(mut commands: Commands, art: Option<Res<UiArt>>) {
    let art = art.map(|a| a.clone()).unwrap_or_default();
    let logo_width = LOGO_WIDTH * 0.8;
    let logo_height = logo_width * art.logo_size.y as f32 / art.logo_size.x.max(1) as f32;
    commands
        .spawn((
            TitleRoot,
            Name::new("Main menu"),
            Node {
                position_type: PositionType::Absolute,
                left: percent(4),
                top: percent(5),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                row_gap: px(22),
                ..default()
            },
            GlobalZIndex(90),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            root.spawn((
                Name::new("Logo"),
                ImageNode::new(art.logo.clone()),
                Node {
                    width: px(logo_width),
                    height: px(logo_height),
                    ..default()
                },
            ));
            root.spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                row_gap: px(14),
                ..default()
            })
            .with_children(|row| {
                for (label, button) in BUTTONS {
                    row.spawn(Node {
                        width: px(BUTTON_WIDTH),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::FlexEnd,
                        row_gap: px(6),
                        ..default()
                    })
                    .with_children(|column| {
                        column.spawn((
                            button,
                            cartoon_button(
                                label,
                                button == MainMenuButton::Waves,
                                72.0,
                                26.0,
                                Some(art.crystal_blue.clone()),
                            ),
                        ));
                        if button == MainMenuButton::Waves {
                            // The best run, on a small dark pill so it reads
                            // over any sky.
                            column.spawn((
                                Node {
                                    padding: UiRect::axes(px(12), px(4)),
                                    border: UiRect::all(px(2)),
                                    border_radius: BorderRadius::MAX,
                                    ..default()
                                },
                                BackgroundColor(PANEL),
                                BorderColor::all(RIM),
                                ink(2.0),
                                children![(BestLine, caps("", 13.0, TEXT))],
                            ));
                        }
                    });
                }
            });
        });
}

fn reset_orbit(mut orbit: ResMut<HeroCamera>) {
    orbit.t = 0.0;
}

fn title_visibility(menu: Res<MenuState>, mut root: Query<&mut Visibility, With<TitleRoot>>) {
    if !menu.is_changed() {
        return;
    }
    for mut v in &mut root {
        v.set_if_neq(if menu.title_visible() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

fn title_buttons(
    buttons: Query<(&Interaction, &MainMenuButton), Changed<Interaction>>,
    state: Res<State<AppState>>,
    mut menu: ResMut<MenuState>,
    mut start: MessageWriter<StartMode>,
    mut clock: ResMut<PlayClock>,
    mut exit: MessageWriter<AppExit>,
) {
    if *state.get() != AppState::Menu || !menu.title_visible() {
        return;
    }
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let mode = match button {
            MainMenuButton::Waves => GameMode::Waves,
            MainMenuButton::Practice => GameMode::Practice,
            MainMenuButton::Settings => {
                menu.page = MenuPage::Settings;
                continue;
            }
            MainMenuButton::Quit => {
                exit.write(AppExit::Success);
                continue;
            }
        };
        start.write(StartMode(mode));
        clock.0 = Some((Instant::now(), mode));
        return;
    }
}

/// Writes the best run under the Waves button when it changes.
fn refresh_best(best: Res<PersonalBest>, mut lines: Query<&mut Text, With<BestLine>>) {
    if !best.is_changed() {
        return;
    }
    let mut line = String::new();
    write_best(&mut line, &best);
    for mut text in &mut lines {
        if text.0 != line {
            text.0.clone_from(&line);
        }
    }
}

/// "BEST  WAVE 7 · 12,450", or "NO RUNS YET".
pub fn write_best(out: &mut String, best: &PersonalBest) {
    match &best.0 {
        Some(b) => {
            let _ = write!(out, "BEST  WAVE {} \u{b7} ", b.wave);
            write_thousands(out, b.score);
        }
        None => out.push_str("NO RUNS YET"),
    }
}

fn orbit_camera(
    time: Res<Time<Real>>,
    mut orbit: ResMut<HeroCamera>,
    camera: Option<Single<&mut Transform, With<MainCamera>>>,
) {
    orbit.t += time.delta_secs();
    if let Some(mut camera) = camera {
        **camera = hero_eye(orbit.t);
    }
}

/// Prints `PIECED_PLAY_MS` once play is controllable after a Play click
/// (the cursor captured, as for the launch time), and records it.
fn report_play(
    mut clock: ResMut<PlayClock>,
    mut last: ResMut<LastPlay>,
    state: Res<State<AppState>>,
    cursor: Option<Single<&CursorOptions, With<PrimaryWindow>>>,
    scenario: Option<Res<ScenarioRun>>,
    session: Option<Res<SessionLog>>,
) {
    let Some((since, mode)) = clock.0 else {
        return;
    };
    if *state.get() != AppState::Playing {
        return;
    }
    let live = scenario.is_some() || cursor.is_none_or(|c| c.grab_mode == CursorGrabMode::Locked);
    if !live {
        return;
    }
    clock.0 = None;
    let ms = since.elapsed().as_secs_f64() * 1000.0;
    let label = match mode {
        GameMode::Waves => "waves",
        GameMode::Practice => "practice",
    };
    println!("PIECED_PLAY_MS {ms:.1} {label}");
    last.0 = Some((ms, mode));
    if let Some(session) = session {
        session.record_play(PlayRecord { ms, mode: label });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hero_shot_frames_the_knight_with_the_castle_behind_and_drifts() {
        let start = hero_eye(0.0);
        let later = hero_eye(5.0);
        for eye in [start, later] {
            // Inside the arena, low over the grass.
            let p = eye.translation;
            assert!(p.x.abs() < 24.0 && p.z.abs() < 24.0 && (1.5..3.0).contains(&p.y));
            let ahead = eye.forward().as_vec3();
            let right = eye.right().as_vec3();
            // The knight's chest a little right of the middle, ahead.
            let to_knight = (HERO_FEET + Vec3::Y * 1.0 - p).normalize();
            assert!(ahead.dot(to_knight) > 0.9, "the knight is in view");
            assert!(right.dot(to_knight) > 0.0, "right of centre");
            // The castle (azimuth 30°) in the right half.
            assert!(right.dot(bearing(30.0)) > 0.1, "the castle on the right");
        }
        // It drifts, but only a little.
        assert_ne!(start.translation, later.translation);
        assert!(start.translation.distance(later.translation) < 1.0);
    }

    #[test]
    fn the_best_line_reads_like_the_results() {
        let mut s = String::new();
        write_best(&mut s, &PersonalBest(None));
        assert_eq!(s, "NO RUNS YET");
        s.clear();
        write_best(
            &mut s,
            &PersonalBest(Some(crate::waves::BestRun {
                wave: 7,
                score: 12_450,
                seed: 1,
                eliminations: 30,
                run_seconds: 300.0,
            })),
        );
        assert_eq!(s, "BEST  WAVE 7 \u{b7} 12,450");
    }
}
