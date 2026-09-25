//! The pause menu and its Settings page, in native Bevy UI (trackpad clicks and
//! drags work because the cursor is free while paused).

use super::{MenuPage, MenuState, Setting, SettingKind};
use crate::{
    hud::style::{ACCENT, INK, TEXT, caps, dim, text},
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

const CARD_BG: Color = Color::srgba(0.07, 0.075, 0.1, 0.98);
const BUTTON_BG: Color = Color::srgba(1.0, 1.0, 1.0, 0.055);
const BUTTON_HOVER: Color = Color::srgba(1.0, 1.0, 1.0, 0.11);
const BUTTON_BORDER: Color = Color::srgba(1.0, 1.0, 1.0, 0.08);
const TRACK_BG: Color = Color::srgba(1.0, 1.0, 1.0, 0.13);

fn card(width: f32) -> impl Bundle {
    (
        Node {
            width: px(width),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(28)),
            row_gap: px(10),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(16)),
            ..default()
        },
        BackgroundColor(CARD_BG),
        BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.07)),
        BoxShadow::new(
            Color::srgba(0.0, 0.0, 0.0, 0.45),
            px(0),
            px(10),
            px(0),
            px(40),
        ),
    )
}

fn menu_button(label: &str, action: MenuAction, primary: bool) -> impl Bundle {
    (
        action,
        Button,
        Node {
            height: px(48),
            width: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(10)),
            ..default()
        },
        BackgroundColor(if primary { ACCENT } else { BUTTON_BG }),
        BorderColor::all(if primary { ACCENT } else { BUTTON_BORDER }),
        children![(
            Text::new(label),
            TextFont::from_font_size(18.0),
            TextColor(if primary { INK } else { TEXT }),
            LetterSpacing::Px(1.5),
        )],
    )
}

fn spawn_menu(mut commands: Commands) {
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
            BackgroundColor(Color::srgba(0.02, 0.025, 0.04, 0.58)),
            GlobalZIndex(100),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            root.spawn((MainCard, card(360.0))).with_children(|c| {
                c.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    margin: UiRect::bottom(px(14)),
                    row_gap: px(4),
                    ..default()
                })
                .with_children(|title| {
                    title.spawn((
                        Text::new("PIECED"),
                        TextFont::from_font_size(34.0),
                        TextColor(ACCENT),
                        LetterSpacing::Px(9.0),
                    ));
                    title.spawn(caps("PAUSED", 12.0, dim(0.55)));
                });
                c.spawn(menu_button("RESUME", MenuAction::Resume, true));
                c.spawn(menu_button("SETTINGS", MenuAction::Settings, false));
                c.spawn(menu_button("QUIT", MenuAction::Quit, false));
                c.spawn((
                    Node {
                        justify_content: JustifyContent::Center,
                        margin: UiRect::top(px(8)),
                        ..default()
                    },
                    children![text("Esc resume   F3 stats   F4 tuning", 11.0, dim(0.45))],
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
                        TextFont::from_font_size(24.0),
                        TextColor(TEXT),
                        LetterSpacing::Px(5.0),
                    ));
                    header
                        .spawn(Node {
                            width: px(120),
                            ..default()
                        })
                        .with_children(|b| {
                            b.spawn(menu_button("BACK", MenuAction::Back, false));
                        });
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
                                    children![caps(*title, 11.0, ACCENT)],
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
                        11.0,
                        dim(0.45)
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
        row.spawn(text(setting.label(), 14.0, dim(0.9)));
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
                            height: px(24),
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ))
                    .with_children(|hit| {
                        hit.spawn((
                            Node {
                                width: percent(100),
                                height: px(6),
                                margin: UiRect::top(px(9)),
                                border_radius: BorderRadius::all(px(3)),
                                ..default()
                            },
                            BackgroundColor(TRACK_BG),
                        ))
                        .with_children(|track| {
                            track.spawn((
                                Part::SliderFill(setting),
                                Node {
                                    width: percent(50),
                                    height: percent(100),
                                    border_radius: BorderRadius::all(px(3)),
                                    ..default()
                                },
                                BackgroundColor(ACCENT),
                            ));
                        });
                        hit.spawn((
                            Part::SliderKnob(setting),
                            Node {
                                position_type: PositionType::Absolute,
                                left: percent(50),
                                top: px(4),
                                width: px(16),
                                height: px(16),
                                margin: UiRect::left(px(-8)),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(TEXT),
                            BoxShadow::new(
                                Color::srgba(0.0, 0.0, 0.0, 0.4),
                                px(0),
                                px(1),
                                px(0),
                                px(3),
                            ),
                        ));
                    });
                control
                    .spawn(Node {
                        width: px(52),
                        justify_content: JustifyContent::FlexEnd,
                        ..default()
                    })
                    .with_children(|v| {
                        v.spawn((Part::Value(setting), text("", 14.0, TEXT)));
                    });
            }
            SettingKind::Toggle => {
                control
                    .spawn((
                        Widget::Toggle(setting),
                        Button,
                        Node {
                            width: px(46),
                            height: px(26),
                            border_radius: BorderRadius::MAX,
                            padding: UiRect::all(px(3)),
                            ..default()
                        },
                        BackgroundColor(TRACK_BG),
                    ))
                    .with_children(|t| {
                        t.spawn((
                            Part::ToggleKnob(setting),
                            Node {
                                width: px(20),
                                height: px(20),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(TEXT),
                        ));
                    });
                control
                    .spawn(Node {
                        width: px(52),
                        justify_content: JustifyContent::FlexEnd,
                        ..default()
                    })
                    .with_children(|v| {
                        v.spawn((Part::Value(setting), text("", 14.0, dim(0.7))));
                    });
            }
            SettingKind::Choice(options) => {
                control
                    .spawn((
                        Node {
                            flex_direction: FlexDirection::Row,
                            padding: UiRect::all(px(3)),
                            column_gap: px(3),
                            border_radius: BorderRadius::all(px(9)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.07)),
                    ))
                    .with_children(|seg| {
                        for (i, option) in options.iter().enumerate() {
                            seg.spawn((
                                Widget::Choice(setting, i as u8),
                                Button,
                                Node {
                                    padding: UiRect::axes(px(12), px(5)),
                                    border_radius: BorderRadius::all(px(7)),
                                    ..default()
                                },
                                BackgroundColor(Color::NONE),
                                children![text(*option, 13.0, TEXT)],
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
        let (fill, edge) = match (action, widget) {
            (Some(MenuAction::Resume), _) => (
                if hovered {
                    Color::srgb(0.98, 0.83, 0.6)
                } else {
                    ACCENT
                },
                ACCENT,
            ),
            (Some(_), _) => (
                if hovered { BUTTON_HOVER } else { BUTTON_BG },
                if hovered {
                    ACCENT.with_alpha(0.55)
                } else {
                    BUTTON_BORDER
                },
            ),
            (None, Some(Widget::Choice(setting, i))) => {
                let selected = setting.get(&tuning).round() as u8 == *i;
                (
                    if selected {
                        ACCENT.with_alpha(0.9)
                    } else if hovered {
                        BUTTON_HOVER
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
                        (true, _) => ACCENT,
                        (false, true) => Color::srgba(1.0, 1.0, 1.0, 0.2),
                        (false, false) => TRACK_BG,
                    },
                    Color::NONE,
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
    // Selected choice labels switch to dark ink on the accent pill.
    for (parent, mut color) in &mut choice_text {
        if let Ok(Widget::Choice(setting, i)) = choices.get(parent.parent()) {
            let selected = setting.get(&tuning).round() as u8 == *i;
            color.set_if_neq(TextColor(if selected { INK } else { TEXT }));
        }
    }
}
