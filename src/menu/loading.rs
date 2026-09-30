//! The loading screen's progress bar and tips (M4 chunk 5, D110): over the
//! Boot overlay (`look::warmup`), a brass-framed bar fills as Boot's holds
//! (models, pieces, the sound bank, pipeline warm-up…) are released, and a
//! tip under it changes every [`TIP_SECONDS`], naming the **bound** keys
//! ("Hold ⟨Shift⟩ to aim down sights").
//!
//! Spawned once at startup, drawn only during Boot, then despawned. The bar
//! moves one node width per frame; the tip rewrites its own string when it
//! changes. Nothing new to warm: plain UI nodes and text.

use crate::{
    app::BootGate,
    hud::style::{ACCENT, INK, PANEL, TEXT, TROUGH, ink},
    input::{Action, Bindings},
    shared::AppState,
    tuning::Tuning,
    waves::ui::set_text,
};
use bevy::prelude::*;
use std::fmt::Write;

/// Seconds each tip stays up.
pub const TIP_SECONDS: f32 = 2.6;
/// How many tips there are.
pub const TIP_COUNT: usize = 8;

pub(super) fn build(app: &mut App) {
    app.init_resource::<LoadingProgress>()
        .add_systems(Startup, spawn_loading)
        .add_systems(Update, drive_loading.run_if(in_state(AppState::Boot)))
        .add_systems(OnExit(AppState::Boot), remove_loading);
}

/// The loading screen's parts.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadingUi {
    Root,
    BarFill,
    Tip,
}

/// How far Boot has got (0..=1, never going back) and which tip shows.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct LoadingProgress {
    pub fraction: f32,
    pub tip: usize,
    /// Real seconds in Boot.
    pub seconds: f32,
    /// The most holds seen at once (the bar's full length).
    most_held: usize,
}

/// Writes tip `index` with the bound keys.
pub fn write_tip(out: &mut String, index: usize, b: &Bindings) {
    let _ = match index % TIP_COUNT {
        0 => write!(out, "Hold {} to aim down sights", b.name(Action::Aim)),
        1 => write!(out, "Pump knights off the edge for +150"),
        2 => write!(
            out,
            "{} {} {} {} build a wall, ramp, floor and cone",
            b.name(Action::Wall),
            b.name(Action::Ramp),
            b.name(Action::Floor),
            b.name(Action::Cone)
        ),
        3 => write!(
            out,
            "Press {} to edit a piece: cut a window or a door",
            b.name(Action::Edit)
        ),
        4 => write!(out, "Headshot kills are worth +50"),
        5 => write!(
            out,
            "Press {} while sprinting to slide",
            b.name(Action::Crouch)
        ),
        6 => write!(out, "Clearing wave n is worth 250 \u{d7} n"),
        _ => write!(
            out,
            "Press {} to skip the break between waves",
            b.name(Action::Start)
        ),
    };
}

/// The bar's fill for `held` holds when at most `most` were held at once,
/// after `seconds`: the released share, never quite full until Boot ends.
pub fn loading_fraction(held: usize, most: usize, seconds: f32) -> f32 {
    let released = if most == 0 {
        0.0
    } else {
        1.0 - held as f32 / most as f32
    };
    // A slow creep so a long hold (the warm-up) still shows life.
    let creep = 0.15 * (1.0 - (-seconds / 3.0).exp());
    (0.05 + 0.8 * released + creep).min(0.97)
}

fn spawn_loading(mut commands: Commands) {
    commands
        .spawn((
            Name::new("Loading progress"),
            LoadingUi::Root,
            Node {
                position_type: PositionType::Absolute,
                bottom: percent(12),
                width: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(14),
                ..default()
            },
            // Over the loading overlay (z 1000).
            GlobalZIndex(1001),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: px(420),
                    height: px(22),
                    padding: UiRect::all(px(3)),
                    border: UiRect::all(px(3)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(TROUGH),
                BorderColor::all(crate::waves::ui::GOLD_FRAME),
                ink(2.5),
            ))
            .with_child((
                LoadingUi::BarFill,
                Node {
                    width: percent(5),
                    height: percent(100),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(ACCENT),
            ));
            root.spawn((
                Node {
                    padding: UiRect::axes(px(16), px(6)),
                    border_radius: BorderRadius::all(px(12)),
                    ..default()
                },
                BackgroundColor(PANEL.with_alpha(0.7)),
            ))
            .with_child((
                LoadingUi::Tip,
                Text::new(String::with_capacity(96)),
                TextFont::from_font_size(22.0),
                TextColor(TEXT),
                TextShadow {
                    offset: Vec2::new(0.0, 2.0),
                    color: INK,
                },
            ));
        });
}

fn drive_loading(
    time: Res<Time<Real>>,
    gate: Option<Res<BootGate>>,
    tuning: Res<Tuning>,
    mut progress: ResMut<LoadingProgress>,
    mut scratch: Local<String>,
    mut fills: Query<(&LoadingUi, &mut Node)>,
    mut tips: Query<(&LoadingUi, &mut Text)>,
) {
    progress.seconds += time.delta_secs();
    let held = gate.map_or(0, |g| g.held().count());
    progress.most_held = progress.most_held.max(held);
    let fraction = loading_fraction(held, progress.most_held, progress.seconds);
    progress.fraction = progress.fraction.max(fraction);
    progress.tip = (progress.seconds / TIP_SECONDS) as usize % TIP_COUNT;
    let width = percent(progress.fraction * 100.0);
    for (part, mut node) in &mut fills {
        if *part == LoadingUi::BarFill && node.width != width {
            node.width = width;
        }
    }
    let tip = progress.tip;
    for (part, mut text) in &mut tips {
        if *part == LoadingUi::Tip {
            set_text(&mut text, &mut scratch, |s| {
                write_tip(s, tip, &tuning.bindings)
            });
        }
    }
}

fn remove_loading(mut commands: Commands, roots: Query<(Entity, &LoadingUi)>) {
    for (entity, part) in &roots {
        if *part == LoadingUi::Root {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tips_name_the_bound_keys() {
        let mut b = Bindings::default();
        let mut s = String::new();
        write_tip(&mut s, 0, &b);
        assert_eq!(s, "Hold Shift to aim down sights");
        b.bind(Action::Aim, crate::input::Binding::Key(KeyCode::KeyZ))
            .unwrap();
        s.clear();
        write_tip(&mut s, 0, &b);
        assert_eq!(s, "Hold Z to aim down sights");
        s.clear();
        write_tip(&mut s, 1, &b);
        assert!(s.contains("+150"));
    }

    #[test]
    fn the_bar_fills_as_holds_release_and_never_claims_done() {
        let start = loading_fraction(4, 4, 0.0);
        let half = loading_fraction(2, 4, 0.5);
        let most = loading_fraction(0, 4, 2.0);
        assert!(start < 0.1 && half > start && most > half);
        assert!(loading_fraction(0, 4, 100.0) < 1.0);
    }
}
