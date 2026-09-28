//! The main menu (D92, M3 chunk 5): the game opens here after Boot.
//!
//! - The PIECED logo over the live island and sky, the camera slowly
//!   orbiting the arena. No player input (the adapter only drives the player
//!   in `Playing`), the HUD and the gun hidden, the cursor free.
//! - A row of cartoon buttons in the pause menu's style (T12): **Waves**
//!   with the best run under it, **Practice**, **Settings** (the Settings
//!   card and its Controls page, over the island) and **Quit**.
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
    hud::{
        UiArt,
        style::{PANEL, RIM, TEXT, caps, ink},
    },
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
    app.init_resource::<OrbitCamera>()
        .init_resource::<PlayClock>()
        .init_resource::<LastPlay>()
        .add_systems(Startup, spawn_title)
        .add_systems(OnEnter(AppState::Menu), reset_orbit)
        .add_systems(Update, (title_visibility, title_buttons, refresh_best))
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

/// The main menu's camera: a slow circle round the arena, looking at its
/// middle, starting behind the player's spawn (the castle ahead).
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct OrbitCamera {
    pub center: Vec3,
    /// Horizontal distance from `center` (m): a little outside the arena.
    pub radius: f32,
    /// Height above `center` (m).
    pub height: f32,
    /// Radians per second (a lap takes about two minutes).
    pub speed: f32,
    /// Angle at t = 0 (0 = the +Z side, behind the spawn).
    pub start: f32,
    /// Seconds orbited so far (real time; resets each visit).
    pub t: f32,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            center: Vec3::new(0.0, 1.5, 0.0),
            radius: 34.0,
            height: 10.0,
            speed: 0.05,
            start: 0.0,
            t: 0.0,
        }
    }
}

/// Where the orbit puts the camera `t` seconds in.
pub fn orbit_eye(orbit: &OrbitCamera, t: f32) -> Transform {
    let angle = orbit.start + orbit.speed * t;
    let eye = orbit.center
        + Vec3::new(
            angle.sin() * orbit.radius,
            orbit.height,
            angle.cos() * orbit.radius,
        );
    Transform::from_translation(eye).looking_at(orbit.center, Vec3::Y)
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
const BUTTON_WIDTH: f32 = 230.0;

fn spawn_title(mut commands: Commands, art: Option<Res<UiArt>>) {
    let art = art.map(|a| a.clone()).unwrap_or_default();
    let logo_width = LOGO_WIDTH * 1.1;
    let logo_height = logo_width * art.logo_size.y as f32 / art.logo_size.x.max(1) as f32;
    commands
        .spawn((
            TitleRoot,
            Name::new("Main menu"),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect::new(px(0), px(0), percent(5), percent(6)),
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
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::FlexStart,
                column_gap: px(18),
                ..default()
            })
            .with_children(|row| {
                for (label, button) in BUTTONS {
                    row.spawn(Node {
                        width: px(BUTTON_WIDTH),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: px(8),
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

fn reset_orbit(mut orbit: ResMut<OrbitCamera>) {
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
    mut orbit: ResMut<OrbitCamera>,
    camera: Option<Single<&mut Transform, With<MainCamera>>>,
) {
    orbit.t += time.delta_secs();
    if let Some(mut camera) = camera {
        **camera = orbit_eye(&orbit, orbit.t);
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
    fn the_orbit_circles_the_arena_looking_at_its_middle() {
        let orbit = OrbitCamera::default();
        let start = orbit_eye(&orbit, 0.0);
        assert!(start.translation.z > 30.0, "starts behind the spawn");
        let later = orbit_eye(&orbit, 10.0);
        for eye in [start, later] {
            let flat = (eye.translation - orbit.center).with_y(0.0).length();
            assert!((flat - orbit.radius).abs() < 1e-3);
            let ahead = eye.forward().as_vec3();
            let to_center = (orbit.center - eye.translation).normalize();
            assert!(ahead.dot(to_center) > 0.999);
        }
        assert_ne!(start.translation, later.translation);
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
