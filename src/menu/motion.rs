//! Menu motion (M4 chunk 5, D110): the cartoon buttons slide and scale in
//! whenever a menu page opens (all of them within [`ENTER_SECONDS`], a few
//! hundredths apart), and pop when the pointer comes over them.
//!
//! Every button already has a [`UiTransform`]-less layout; this adds one
//! [`MenuMotion`] and a `UiTransform` to each cartoon button at spawn (an
//! observer), then writes one transform per button per frame only while it
//! moves. Nothing is allocated; nothing new is drawn (UI transforms are free
//! for the UI pass).

use super::{MenuState, pause::Cartoon};
use bevy::prelude::*;

/// Seconds the whole slide-in takes, the last button's stagger included.
pub const ENTER_SECONDS: f32 = 0.2;
/// Seconds one button takes, and the stagger between buttons.
pub const BUTTON_SECONDS: f32 = 0.14;
pub const STAGGER: f32 = 0.02;
/// How far a button slides in from (px, from the left).
pub const SLIDE_FROM: f32 = -70.0;
/// A hover's pop: its length (s), its peak and the scale it rests at while
/// hovered.
pub const HOVER_SECONDS: f32 = 0.16;
pub const HOVER_PEAK: f32 = 1.08;
pub const HOVER_REST: f32 = 1.035;

pub(super) fn build(app: &mut App) {
    app.init_resource::<MenuClock>()
        .add_observer(add_motion)
        .add_systems(Update, animate_buttons);
}

/// Real seconds since the menu page last changed (the slide-in's clock).
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct MenuClock {
    pub age: f32,
    seen: Option<(bool, super::MenuPage, bool)>,
}

impl Default for MenuClock {
    fn default() -> Self {
        Self {
            age: f32::MAX,
            seen: None,
        }
    }
}

/// On every cartoon button: its place in its page (the stagger) and its
/// hover pop's clock.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct MenuMotion {
    pub index: u8,
    /// Seconds since the pointer came over it (None when it isn't over it).
    pub hover: Option<f32>,
}

/// The slide-in `t` seconds after the page opened, for the button at
/// `index`: its x offset (px) and scale. Settled exactly at rest by
/// [`ENTER_SECONDS`].
pub fn enter_pose(t: f32, index: u8) -> (f32, f32) {
    let local = t - STAGGER * f32::from(index.min(3));
    let x = (local / BUTTON_SECONDS).clamp(0.0, 1.0);
    if x >= 1.0 {
        return (0.0, 1.0);
    }
    // Ease out with a small overshoot.
    let c = 1.9;
    let y = x - 1.0;
    let e = 1.0 + (c + 1.0) * y * y * y + c * y * y;
    (SLIDE_FROM * (1.0 - e), 0.82 + 0.18 * e)
}

/// The hover pop `t` seconds after the pointer came over a button: up to
/// [`HOVER_PEAK`], settling at [`HOVER_REST`].
pub fn hover_scale(t: Option<f32>) -> f32 {
    let Some(t) = t else { return 1.0 };
    let x = (t / HOVER_SECONDS).clamp(0.0, 1.0);
    if x >= 1.0 {
        return HOVER_REST;
    }
    let bump = (x * std::f32::consts::PI).sin();
    1.0 + (HOVER_REST - 1.0) * x + (HOVER_PEAK - 1.0) * bump
}

fn add_motion(add: On<Add, Cartoon>, mut commands: Commands) {
    commands.entity(add.entity).insert((
        MenuMotion {
            index: 0,
            hover: None,
        },
        UiTransform::default(),
    ));
}

#[allow(clippy::type_complexity)]
fn animate_buttons(
    time: Res<Time<Real>>,
    menu: Res<MenuState>,
    mut clock: ResMut<MenuClock>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    mut buttons: Query<(
        Entity,
        &Interaction,
        &InheritedVisibility,
        &mut MenuMotion,
        &mut UiTransform,
    )>,
) {
    let dt = time.delta_secs();
    let now = (menu.menu_visible(), menu.page, menu.title);
    if clock.seen != Some(now) {
        clock.seen = Some(now);
        clock.age = 0.0;
    } else if clock.age < 1.0e6 {
        clock.age += dt;
    }
    let entering = clock.age <= ENTER_SECONDS + dt;
    for (entity, interaction, visible, mut motion, mut tf) in &mut buttons {
        let hovered = matches!(interaction, Interaction::Hovered | Interaction::Pressed);
        motion.hover = match (hovered, motion.hover) {
            (true, None) => Some(0.0),
            (true, Some(t)) => Some(t + dt),
            (false, _) => None,
        };
        if !visible.get() && !entering {
            continue;
        }
        // Its place among the buttons of its column: the wrapper's index.
        let index = parents
            .get(entity)
            .ok()
            .and_then(|p| parents.get(p.parent()).ok().map(|g| (p.parent(), g.parent())))
            .and_then(|(wrapper, column)| {
                children
                    .get(column)
                    .ok()
                    .and_then(|c| c.iter().position(|e| e == wrapper))
            })
            .unwrap_or(0)
            .min(u8::MAX as usize) as u8;
        motion.index = index;
        let (x, s) = enter_pose(clock.age, index);
        let want = UiTransform {
            translation: Val2::px(x, 0.0),
            scale: Vec2::splat(s * hover_scale(motion.hover)),
            ..UiTransform::IDENTITY
        };
        if *tf != want {
            *tf = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_slide_in_within_0_2_s_and_settle_exactly() {
        for index in 0..4 {
            let (x0, s0) = enter_pose(0.0, index);
            assert!(x0 <= SLIDE_FROM * 0.99 && s0 < 0.85, "starts out: {x0} {s0}");
            assert_eq!(enter_pose(ENTER_SECONDS, index), (0.0, 1.0));
        }
        let (mid, _) = enter_pose(0.07, 0);
        assert!(mid > SLIDE_FROM && mid < 0.0);
    }

    #[test]
    fn a_hover_pops_then_rests_a_little_big() {
        assert_eq!(hover_scale(None), 1.0);
        let peak = (1..16)
            .map(|i| hover_scale(Some(i as f32 * 0.01)))
            .fold(0.0, f32::max);
        assert!(peak > HOVER_REST + 0.03, "{peak}");
        assert_eq!(hover_scale(Some(1.0)), HOVER_REST);
    }
}
