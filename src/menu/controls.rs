//! Settings → **Controls** (D93): every [`Action`] with its bound key, in
//! the Settings card's style.
//!
//! Click an action's key chip, then press a key or click to bind it; Esc
//! cancels. A key another action uses swaps the two. Cmd and Cmd
//! combinations, Esc and the F3/F4 dev keys are refused. "Reset to
//! defaults" restores D43's layout. Trackpad look is fixed.
//!
//! This page only asks: it sets [`BindingCapture::waiting`], and the input
//! adapter (`src/input.rs`, the only device reader) reads the next key or
//! click and applies it. Bindings live in `Tuning`, so they autosave with
//! the other settings.

use super::{
    MenuPage, MenuState,
    pause::{GOLD_HOVER, card, cartoon_button},
};
use crate::{
    hud::style::{ACCENT, INK, RIM, TEXT, TROUGH, caps, dim, ink, text},
    input::{Action, BindingCapture, CaptureOutcome},
    tuning::Tuning,
};
use bevy::{prelude::*, text::LetterSpacing};
use std::fmt::Write;

pub(super) fn build(app: &mut App) {
    app.add_systems(
        Update,
        (controls_clicks, drop_stale_capture, refresh_controls, chip_looks).chain(),
    );
}

/// The Controls card (a page on the pause menu's layer).
#[derive(Component, Debug)]
pub(crate) struct ControlsCard;

/// The Controls page's buttons.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlsButton {
    /// An action's key chip: click, then press the new key.
    Bind(Action),
    /// Back to D43's layout.
    Reset,
    /// Back to Settings.
    Back,
}

/// The label inside an action's key chip.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyChip(pub Action);

/// The line under the list: what to do, or how the last binding went.
#[derive(Component, Debug)]
pub struct ControlsStatus;

/// The actions in the left column (movement and shooting); the rest go right.
const LEFT: usize = 9;

pub(super) fn spawn_card(root: &mut ChildSpawnerCommands) {
    root.spawn((
        ControlsCard,
        Name::new("Controls"),
        card(900.0),
    ))
    .with_children(|c| {
        c.spawn(Node {
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            margin: UiRect::bottom(px(6)),
            ..default()
        })
        .with_children(|header| {
            header.spawn((
                Text::new("CONTROLS"),
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
                    flex_direction: FlexDirection::Row,
                    column_gap: px(12),
                    ..default()
                })
                .with_children(|buttons| {
                    buttons
                        .spawn(Node {
                            width: px(260),
                            ..default()
                        })
                        .with_child((
                            ControlsButton::Reset,
                            cartoon_button("RESET TO DEFAULTS", false, 46.0, 20.0, None),
                        ));
                    buttons
                        .spawn(Node {
                            width: px(130),
                            ..default()
                        })
                        .with_child((
                            ControlsButton::Back,
                            cartoon_button("BACK", false, 46.0, 20.0, None),
                        ));
                });
        });
        c.spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(40),
            ..default()
        })
        .with_children(|cols| {
            for (title, actions) in [
                ("MOVE AND FIGHT", &Action::ALL[..LEFT]),
                ("BUILD AND RUN", &Action::ALL[LEFT..]),
            ] {
                cols.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    flex_basis: px(0),
                    flex_grow: 1.0,
                    row_gap: px(4),
                    ..default()
                })
                .with_children(|col| {
                    col.spawn((
                        Node {
                            margin: UiRect::new(px(0), px(0), px(10), px(4)),
                            ..default()
                        },
                        children![caps(title, 14.0, ACCENT)],
                    ));
                    for action in actions {
                        spawn_row(col, *action);
                    }
                });
            }
        });
        c.spawn((
            Node {
                margin: UiRect::top(px(12)),
                ..default()
            },
            children![(ControlsStatus, text("", 13.0, dim(0.7)))],
        ));
    });
}

fn spawn_row(col: &mut ChildSpawnerCommands, action: Action) {
    col.spawn(Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::SpaceBetween,
        height: px(36),
        column_gap: px(14),
        ..default()
    })
    .with_children(|row| {
        row.spawn(text(action.label(), 16.0, dim(0.95)));
        row.spawn((
            ControlsButton::Bind(action),
            Button,
            Node {
                width: px(150),
                height: px(30),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(9)),
                ..default()
            },
            BackgroundColor(TROUGH),
            BorderColor::all(RIM),
            ink(1.5),
            children![(
                KeyChip(action),
                Text::new(action.default_binding().name()),
                TextFont::from_font_size(16.0),
                TextColor(TEXT),
            )],
        ));
    });
}

/// The Controls page is up (over the main menu or the pause menu).
fn page_up(menu: &MenuState) -> bool {
    menu.menu_visible() && menu.page == MenuPage::Controls
}

fn controls_clicks(
    buttons: Query<(&Interaction, &ControlsButton), Changed<Interaction>>,
    mut menu: ResMut<MenuState>,
    mut capture: ResMut<BindingCapture>,
    mut tuning: ResMut<Tuning>,
) {
    if !page_up(&menu) {
        return;
    }
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match *button {
            ControlsButton::Bind(action) => {
                capture.waiting = Some(action);
                capture.last = None;
            }
            ControlsButton::Reset => {
                if tuning.bindings != crate::input::Bindings::default() {
                    tuning.bindings.reset();
                }
                capture.waiting = None;
                capture.last = None;
            }
            ControlsButton::Back => {
                capture.waiting = None;
                menu.back();
            }
        }
    }
}

/// A capture ends when the page closes (Back, Esc out of the menu, play).
fn drop_stale_capture(menu: Res<MenuState>, mut capture: ResMut<BindingCapture>) {
    if capture.waiting.is_some() && !page_up(&menu) {
        capture.waiting = None;
    }
}

/// Rewrites the key chips and the status line when a binding, the capture
/// or the page changes (never per frame).
fn refresh_controls(
    tuning: Res<Tuning>,
    capture: Res<BindingCapture>,
    menu: Res<MenuState>,
    mut chips: Query<(&KeyChip, &mut Text, &mut TextColor), Without<ControlsStatus>>,
    mut status: Query<&mut Text, With<ControlsStatus>>,
) {
    if !tuning.is_changed() && !capture.is_changed() && !menu.is_changed() {
        return;
    }
    for (chip, mut label, mut color) in &mut chips {
        let waiting = capture.waiting == Some(chip.0);
        let name = if waiting {
            "Press a key"
        } else {
            tuning.bindings.name(chip.0)
        };
        if label.0 != name {
            label.0.clear();
            label.0.push_str(name);
        }
        color.set_if_neq(TextColor(if waiting { INK } else { TEXT }));
    }
    let mut line = String::new();
    write_status(&mut line, &capture, &tuning.bindings);
    for mut text in &mut status {
        if text.0 != line {
            text.0.clone_from(&line);
        }
    }
}

/// The status line for the current capture.
pub fn write_status(out: &mut String, capture: &BindingCapture, b: &crate::input::Bindings) {
    if let Some(action) = capture.waiting {
        let _ = write!(
            out,
            "Press a key or click for {}. Esc cancels.",
            action.label()
        );
        return;
    }
    let _ = match capture.last {
        Some(CaptureOutcome::Bound {
            action,
            binding,
            swapped: Some(other),
        }) => write!(
            out,
            "{} is now {}. Swapped: {} is now {}.",
            action.label(),
            binding.name(),
            other.label(),
            b.name(other)
        ),
        Some(CaptureOutcome::Bound {
            action, binding, ..
        }) => write!(out, "{} is now {}.", action.label(), binding.name()),
        Some(CaptureOutcome::Cancelled(action)) => {
            write!(out, "{} unchanged.", action.label())
        }
        Some(CaptureOutcome::Refused(action, why)) => {
            write!(out, "{}. {} unchanged.", why.message(), action.label())
        }
        None => write!(
            out,
            "Click an action, then press a key or click. Trackpad look is fixed."
        ),
    };
}

/// The chips light up while waiting and on hover (like the Settings toggles).
fn chip_looks(
    capture: Res<BindingCapture>,
    mut chips: Query<(
        &Interaction,
        &ControlsButton,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    for (interaction, button, mut bg, mut border) in &mut chips {
        let ControlsButton::Bind(action) = *button else {
            continue;
        };
        let hovered = matches!(interaction, Interaction::Hovered | Interaction::Pressed);
        let waiting = capture.waiting == Some(action);
        let fill = if waiting {
            ACCENT
        } else if hovered {
            Color::srgba(0.16, 0.22, 0.3, 1.0)
        } else {
            TROUGH
        };
        bg.set_if_neq(BackgroundColor(fill));
        border.set_if_neq(BorderColor::all(if waiting || hovered {
            GOLD_HOVER
        } else {
            RIM
        }));
    }
}
