//! The pause menu and its Settings page, in native Bevy UI (trackpad clicks and
//! drags work because the cursor is free while paused).
//!
//! Milestone 2 look (target T12): the world dimmed behind a dark translucent
//! veil, the PIECED logo (`art/blender/assets/logo.py`) over Resume, Settings
//! and Quit as rounded cartoon buttons (gold frame, rivets, a crystal, ink
//! outline), and a Settings card in the same style.

use super::{MenuPage, MenuState, Setting, SettingKind};
use crate::{
    hud::{
        UiArt,
        style::{ACCENT, INK, PANEL, RIM, SHIELD_FILL, TEXT, TROUGH, caps, dim, ink, text},
    },
    shared::AppState,
    tuning::Tuning,
};
use bevy::{prelude::*, text::LetterSpacing, ui::RelativeCursorPosition};

pub(super) fn build(app: &mut App) {
    app.add_systems(Startup, spawn_menu).add_systems(
        Update,
        (
            menu_visibility,
            menu_buttons,
            setting_clicks,
            slider_drag,
            button_looks,
            refresh_widgets,
        )
            .chain(),
    );
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum MenuAction {
    Resume,
    Settings,
    Quit,
    Back,
}

#[derive(Component)]
struct MenuRoot;

#[derive(Component)]
struct MainCard;

#[derive(Component)]
struct SettingsCard;

/// A clickable part of a setting row.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum Widget {
    SliderTrack(Setting),
    Toggle(Setting),
    Choice(Setting, u8),
}

/// Parts redrawn from the current value.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    SliderFill(Setting),
    SliderKnob(Setting),
    Value(Setting),
    ToggleKnob(Setting),
}

const AIM: &[Setting] = &[
    Setting::Sensitivity,
    Setting::AdsMultiplier,
    Setting::BuildMultiplier,
    Setting::Fov,
    Setting::Acceleration,
    Setting::AimFriction,
];
const FEEDBACK: &[Setting] = &[Setting::Bloom, Setting::DamageNumbers, Setting::CameraShake];
const AUDIO: &[Setting] = &[Setting::Volume, Setting::Mute];
const DISPLAY: &[Setting] = &[Setting::Quality, Setting::WindowMode];

/// The veil over the paused world: dark and a little violet, like T12's
/// softened background.
const BACKDROP: Color = Color::srgba(0.035, 0.03, 0.09, 0.58);
/// Button fills: the primary (Resume) in crystal blue, the rest in slate.
const BLUE: Color = Color::srgb(0.17, 0.43, 0.77);
const BLUE_HOVER: Color = Color::srgb(0.23, 0.53, 0.88);
const SLATE: Color = Color::srgb(0.16, 0.21, 0.28);
const SLATE_HOVER: Color = Color::srgb(0.22, 0.29, 0.38);
/// The brass frame round buttons and the Settings card.
const GOLD_FRAME: Color = Color::srgb(0.9, 0.66, 0.22);
const GOLD_HOVER: Color = Color::srgb(1.0, 0.83, 0.36);
const RIVET: Color = Color::srgb(0.62, 0.65, 0.74);

/// Menu buttons are this wide; the logo above them is a little wider.
const BUTTON_WIDTH: f32 = 380.0;
const LOGO_WIDTH: f32 = 600.0;

fn card(width: f32) -> impl Bundle {
    (
        Node {
            width: px(width),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(28)),
            row_gap: px(10),
            border: UiRect::all(px(4)),
            border_radius: BorderRadius::all(px(22)),
            ..default()
        },
        BackgroundColor(PANEL.with_alpha(0.97)),
        BorderColor::all(GOLD_FRAME),
        ink(3.0),
        BoxShadow::new(
            Color::srgba(0.0, 0.0, 0.0, 0.45),
            px(0),
            px(10),
            px(0),
            px(40),
        ),
    )
}

/// A small riveted brass dot for a button corner.
fn rivet(left: bool, top: bool) -> impl Bundle {
    let at = px(6);
    (
        Node {
            position_type: PositionType::Absolute,
            left: if left { at } else { Val::Auto },
            right: if left { Val::Auto } else { at },
            top: if top { at } else { Val::Auto },
            bottom: if top { Val::Auto } else { at },
            width: px(8),
            height: px(8),
            border: UiRect::all(px(1.5)),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(RIVET),
        BorderColor::all(INK),
    )
}

/// A rounded cartoon button: a brass frame with rivets, a shine along the top,
/// an ink outline, a crystal on the left (when given) and the label.
fn menu_button(
    label: &str,
    action: MenuAction,
    height: f32,
    size: f32,
    crystal: Option<Handle<Image>>,
) -> impl Bundle {
    let primary = action == MenuAction::Resume;
    let icon = crystal.map(|image| {
        (
            ImageNode::new(image),
            Node {
                position_type: PositionType::Absolute,
                left: px(18),
                width: px(height * 0.58),
                height: px(height * 0.58),
                ..default()
            },
        )
    });
    (
        action,
        Button,
        Node {
            height: px(height),
            width: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(px(4)),
            border_radius: BorderRadius::all(px(height * 0.27)),
            ..default()
        },
        BackgroundColor(if primary { BLUE } else { SLATE }),
        BorderColor::all(GOLD_FRAME),
        ink(3.0),
        Children::spawn((
            Spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(12),
                    right: px(12),
                    top: px(3),
                    height: percent(32),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.13)),
            )),
            Spawn(rivet(true, true)),
            Spawn(rivet(false, true)),
            Spawn(rivet(true, false)),
            Spawn(rivet(false, false)),
            SpawnIter(icon.into_iter()),
            Spawn((
                Text::new(label),
                TextFont::from_font_size(size),
                TextColor(TEXT),
                TextShadow {
                    offset: Vec2::new(0.0, 2.5),
                    color: INK,
                },
                LetterSpacing::Px(1.0),
            )),
        )),
    )
}

fn spawn_menu(mut commands: Commands, art: Option<Res<UiArt>>) {
    let art = art.map(|a| a.clone()).unwrap_or_default();
    let logo_height = LOGO_WIDTH * art.logo_size.y as f32 / art.logo_size.x.max(1) as f32;
    commands
        .spawn((
            MenuRoot,
            Name::new("Pause menu"),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(BACKDROP),
            GlobalZIndex(100),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            root.spawn((
                MainCard,
                Node {
                    width: px(LOGO_WIDTH),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(16),
                    ..default()
                },
            ))
            .with_children(|c| {
                c.spawn((
                    Name::new("Logo"),
                    ImageNode::new(art.logo.clone()),
                    Node {
                        width: px(LOGO_WIDTH),
                        height: px(logo_height),
                        margin: UiRect::bottom(px(4)),
                        ..default()
                    },
                ));
                for (label, action) in [
                    ("RESUME", MenuAction::Resume),
                    ("SETTINGS", MenuAction::Settings),
                    ("QUIT", MenuAction::Quit),
                ] {
                    c.spawn(Node {
                        width: px(BUTTON_WIDTH),
                        ..default()
                    })
                    .with_child(menu_button(
                        label,
                        action,
                        68.0,
                        30.0,
                        Some(art.crystal_blue.clone()),
                    ));
                }
                c.spawn((
                    Node {
                        margin: UiRect::top(px(6)),
                        ..default()
                    },
                    children![text("Esc resume   F3 stats   F4 tuning", 13.0, dim(0.6))],
                ));
            });
            root.spawn((SettingsCard, card(900.0))).with_children(|c| {
                c.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    margin: UiRect::bottom(px(6)),
                    ..default()
                })
                .with_children(|header| {
                    header.spawn((
                        Text::new("SETTINGS"),
                        TextFont::from_font_size(32.0),
                        TextColor(TEXT),
                        TextShadow {
                            offset: Vec2::new(0.0, 2.5),
                            color: INK,
                        },
                        LetterSpacing::Px(2.5),
                    ));
                    header
                        .spawn(Node {
                            width: px(130),
                            ..default()
                        })
                        .with_child(menu_button("BACK", MenuAction::Back, 46.0, 20.0, None));
                });
                c.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(40),
                    ..default()
                })
                .with_children(|cols| {
                    let column = |cols: &mut ChildSpawnerCommands,
                                  groups: &[(&str, &[Setting])]| {
                        cols.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            flex_basis: px(0),
                            flex_grow: 1.0,
                            row_gap: px(4),
                            ..default()
                        })
                        .with_children(|col| {
                            for (title, settings) in groups {
                                col.spawn((
                                    Node {
                                        margin: UiRect::new(px(0), px(0), px(10), px(4)),
                                        ..default()
                                    },
                                    children![caps(*title, 14.0, ACCENT)],
                                ));
                                for setting in *settings {
                                    spawn_row(col, *setting);
                                }
                            }
                        });
                    };
                    column(cols, &[("AIM", AIM)]);
                    column(
                        cols,
                        &[
                            ("FEEDBACK", FEEDBACK),
                            ("AUDIO", AUDIO),
                            ("DISPLAY", DISPLAY),
                        ],
                    );
                });
                c.spawn((
                    Node {
                        margin: UiRect::top(px(12)),
                        ..default()
                    },
                    children![text(
                        "Changes apply immediately and are saved automatically.",
                        13.0,
                        dim(0.6)
                    )],
                ));
            });
        });
}

fn spawn_row(col: &mut ChildSpawnerCommands, setting: Setting) {
    col.spawn(Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::SpaceBetween,
        height: px(38),
        column_gap: px(14),
        ..default()
    })
    .with_children(|row| {
        row.spawn(text(setting.label(), 16.0, dim(0.95)));
        row.spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(12),
            ..default()
        })
        .with_children(|control| match setting.kind() {
            SettingKind::Slider { .. } => {
                control
                    .spawn((
                        Widget::SliderTrack(setting),
                        Button,
                        RelativeCursorPosition::default(),
                        Node {
                            width: px(170),
                            height: px(26),
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ))
                    .with_children(|hit| {
                        hit.spawn((
                            Node {
                                width: percent(100),
                                height: px(10),
                                margin: UiRect::top(px(8)),
                                border: UiRect::all(px(1.5)),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(TROUGH),
                            BorderColor::all(RIM),
                            ink(1.5),
                        ))
                        .with_children(|track| {
                            track.spawn((
                                Part::SliderFill(setting),
                                Node {
                                    width: percent(50),
                                    height: percent(100),
                                    border_radius: BorderRadius::MAX,
                                    ..default()
                                },
                                BackgroundColor(SHIELD_FILL),
                            ));
                        });
                        hit.spawn((
                            Part::SliderKnob(setting),
                            Node {
                                position_type: PositionType::Absolute,
                                left: percent(50),
                                top: px(3),
                                width: px(20),
                                height: px(20),
                                margin: UiRect::left(px(-10)),
                                border: UiRect::all(px(2.5)),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(TEXT),
                            BorderColor::all(INK),
                        ));
                    });
                control
                    .spawn(Node {
                        width: px(56),
                        justify_content: JustifyContent::FlexEnd,
                        ..default()
                    })
                    .with_children(|v| {
                        v.spawn((Part::Value(setting), text("", 16.0, TEXT)));
                    });
            }
            SettingKind::Toggle => {
                control
                    .spawn((
                        Widget::Toggle(setting),
                        Button,
                        Node {
                            width: px(50),
                            height: px(28),
                            border: UiRect::all(px(2)),
                            border_radius: BorderRadius::MAX,
                            padding: UiRect::all(px(2)),
                            ..default()
                        },
                        BackgroundColor(TROUGH),
                        BorderColor::all(RIM),
                        ink(1.5),
                    ))
                    .with_children(|t| {
                        t.spawn((
                            Part::ToggleKnob(setting),
                            Node {
                                width: px(20),
                                height: px(20),
                                border: UiRect::all(px(2)),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(TEXT),
                            BorderColor::all(INK),
                        ));
                    });
                control
                    .spawn(Node {
                        width: px(56),
                        justify_content: JustifyContent::FlexEnd,
                        ..default()
                    })
                    .with_children(|v| {
                        v.spawn((Part::Value(setting), text("", 16.0, dim(0.8))));
                    });
            }
            SettingKind::Choice(options) => {
                control
                    .spawn((
                        Node {
                            flex_direction: FlexDirection::Row,
                            padding: UiRect::all(px(3)),
                            column_gap: px(3),
                            border: UiRect::all(px(1.5)),
                            border_radius: BorderRadius::all(px(12)),
                            ..default()
                        },
                        BackgroundColor(TROUGH),
                        BorderColor::all(RIM),
                        ink(1.5),
                    ))
                    .with_children(|seg| {
                        for (i, option) in options.iter().enumerate() {
                            seg.spawn((
                                Widget::Choice(setting, i as u8),
                                Button,
                                Node {
                                    padding: UiRect::axes(px(12), px(4)),
                                    border_radius: BorderRadius::all(px(9)),
                                    ..default()
                                },
                                BackgroundColor(Color::NONE),
                                children![(
                                    Text::new(*option),
                                    TextFont::from_font_size(15.0),
                                    TextColor(TEXT),
                                )],
                            ));
                        }
                    });
            }
        });
    });
}

fn menu_visibility(
    menu: Res<MenuState>,
    mut root: Query<&mut Visibility, With<MenuRoot>>,
    mut main: Query<&mut Node, (With<MainCard>, Without<SettingsCard>)>,
    mut settings: Query<&mut Node, With<SettingsCard>>,
) {
    if !menu.is_changed() {
        return;
    }
    for mut v in &mut root {
        v.set_if_neq(if menu.menu_visible() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    // Hidden pages take no layout space.
    let display = |on: bool| if on { Display::Flex } else { Display::None };
    for mut node in &mut main {
        let d = display(menu.page == MenuPage::Main);
        if node.display != d {
            node.display = d;
        }
    }
    for mut node in &mut settings {
        let d = display(menu.page == MenuPage::Settings);
        if node.display != d {
            node.display = d;
        }
    }
}

fn menu_buttons(
    buttons: Query<(&Interaction, &MenuAction, &InheritedVisibility), Changed<Interaction>>,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
    mut menu: ResMut<MenuState>,
    mut exit: MessageWriter<AppExit>,
) {
    for (interaction, action, visible) in &buttons {
        if *interaction != Interaction::Pressed || !visible.get() {
            continue;
        }
        match action {
            MenuAction::Resume => {
                if *state.get() == AppState::Paused {
                    next.set(AppState::Playing);
                } else {
                    menu.menu_open = false;
                }
            }
            MenuAction::Settings => menu.page = MenuPage::Settings,
            MenuAction::Back => menu.page = MenuPage::Main,
            MenuAction::Quit => {
                exit.write(AppExit::Success);
            }
        }
    }
}

fn setting_clicks(
    widgets: Query<(&Interaction, &Widget, &InheritedVisibility), Changed<Interaction>>,
    mut tuning: ResMut<Tuning>,
) {
    for (interaction, widget, visible) in &widgets {
        if *interaction != Interaction::Pressed || !visible.get() {
            continue;
        }
        match *widget {
            Widget::Toggle(setting) => {
                let on = setting.get(&tuning) >= 0.5;
                setting.set(&mut tuning, if on { 0.0 } else { 1.0 });
            }
            Widget::Choice(setting, i) => {
                if setting.get(&tuning).round() as u8 != i {
                    setting.set(&mut tuning, i as f32);
                }
            }
            Widget::SliderTrack(_) => {}
        }
    }
}

/// Click or drag anywhere on a slider's track to set it.
fn slider_drag(
    sliders: Query<(
        &Interaction,
        &RelativeCursorPosition,
        &Widget,
        &InheritedVisibility,
    )>,
    mut tuning: ResMut<Tuning>,
) {
    for (interaction, cursor, widget, visible) in &sliders {
        let Widget::SliderTrack(setting) = *widget else {
            continue;
        };
        if *interaction != Interaction::Pressed || !visible.get() {
            continue;
        }
        let Some(pos) = cursor.normalized else {
            continue;
        };
        // `normalized` runs from -0.5 (left edge) to 0.5 (right edge).
        let fraction = (pos.x + 0.5).clamp(0.0, 1.0);
        let before = setting.get(&tuning);
        let mut probe = tuning.clone();
        setting.set_fraction(&mut probe, fraction);
        if (setting.get(&probe) - before).abs() > 1e-6 {
            setting.set_fraction(&mut tuning, fraction);
        }
    }
}

fn button_looks(
    tuning: Res<Tuning>,
    mut buttons: Query<
        (
            &Interaction,
            &mut BackgroundColor,
            Option<&mut BorderColor>,
            Option<&MenuAction>,
            Option<&Widget>,
        ),
        With<Button>,
    >,
) {
    for (interaction, mut bg, border, action, widget) in &mut buttons {
        let hovered = matches!(interaction, Interaction::Hovered | Interaction::Pressed);
        let pressed = *interaction == Interaction::Pressed;
        let (fill, edge) = match (action, widget) {
            (Some(action), _) => {
                let (rest, hover) = if *action == MenuAction::Resume {
                    (BLUE, BLUE_HOVER)
                } else {
                    (SLATE, SLATE_HOVER)
                };
                let fill = match (hovered, pressed) {
                    (_, true) => rest.darker(0.06),
                    (true, false) => hover,
                    _ => rest,
                };
                (fill, if hovered { GOLD_HOVER } else { GOLD_FRAME })
            }
            (None, Some(Widget::Choice(setting, i))) => {
                let selected = setting.get(&tuning).round() as u8 == *i;
                (
                    if selected {
                        ACCENT
                    } else if hovered {
                        Color::srgba(1.0, 1.0, 1.0, 0.12)
                    } else {
                        Color::NONE
                    },
                    Color::NONE,
                )
            }
            (None, Some(Widget::Toggle(setting))) => {
                let on = setting.get(&tuning) >= 0.5;
                (
                    match (on, hovered) {
                        (true, _) => SHIELD_FILL,
                        (false, true) => Color::srgba(0.16, 0.22, 0.3, 1.0),
                        (false, false) => TROUGH,
                    },
                    if hovered { GOLD_HOVER } else { RIM },
                )
            }
            _ => continue,
        };
        bg.set_if_neq(BackgroundColor(fill));
        if let Some(mut border) = border {
            border.set_if_neq(BorderColor::all(edge));
        }
    }
}

fn refresh_widgets(
    tuning: Res<Tuning>,
    menu: Res<MenuState>,
    mut parts: Query<(&Part, &mut Node, Option<&mut Text>)>,
    mut choice_text: Query<(&ChildOf, &mut TextColor), Without<Part>>,
    choices: Query<&Widget>,
) {
    if !tuning.is_changed() && !menu.is_changed() {
        return;
    }
    for (part, mut node, text) in &mut parts {
        match *part {
            Part::SliderFill(s) => {
                let w = percent(s.fraction(&tuning) * 100.0);
                if node.width != w {
                    node.width = w;
                }
            }
            Part::SliderKnob(s) => {
                let l = percent(s.fraction(&tuning) * 100.0);
                if node.left != l {
                    node.left = l;
                }
            }
            Part::Value(s) => {
                if let Some(mut text) = text {
                    let value = s.display(&tuning);
                    if text.0 != value {
                        text.0 = value;
                    }
                }
            }
            Part::ToggleKnob(s) => {
                let on = s.get(&tuning) >= 0.5;
                let margin = UiRect::left(px(if on { 20 } else { 0 }));
                if node.margin != margin {
                    node.margin = margin;
                }
            }
        }
    }
    // Selected choice labels switch to dark ink on the gold pill.
    for (parent, mut color) in &mut choice_text {
        if let Ok(Widget::Choice(setting, i)) = choices.get(parent.parent()) {
            let selected = setting.get(&tuning).round() as u8 == *i;
            color.set_if_neq(TextColor(if selected { INK } else { TEXT }));
        }
    }
}
