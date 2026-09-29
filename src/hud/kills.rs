//! Kill feedback on the HUD (docs/M4-SPEC.md → Chunk 1, D105), beside the
//! "X" kill marker (drawn with the hitmarker in `systems.rs`):
//!
//! - **Score popups** (Waves only; Practice has no score): a white "+100" at
//!   the knight's chest as he goes down, "+50 HEADSHOT" in gold and "+150
//!   VOID" in violet stacked under it, each rising and fading over
//!   [`KillFeelTuning::popup_seconds`](crate::fx::kills::KillFeelTuning)
//!   (0.8 s). They read the wave director's
//!   [`ScoreAwarded`](crate::waves::ScoreAwarded), so they show exactly what
//!   scored, on the kill's frame. (The wave-clear bonus pops on the HUD score:
//!   `waves::ui::hud`.)
//! - **Multi-kill callouts**: "DOUBLE!", "TRIPLE!", "QUAD!" and "RAMPAGE!"
//!   stamp in beside the crosshair for a chain of kills
//!   ([`Callout`](crate::fx::kills::Callout)).
//!
//! Like the damage numbers, every node is spawned once at startup with its
//! glyphs (so the atlases are filled before the first kill) and only has its
//! text rewritten in place, into strings it already owns: nothing is spawned
//! or allocated per kill. Text is the cartoon font with an ink outline (four
//! ink copies under the face). Animations run on `FreezableTime`, so a kill's
//! hitstop holds them for its two frames.

use super::{
    FrameStartTick,
    layout::{INK_LAYERS, INK_OFFSETS},
    project_to_screen,
    style::INK,
    systems::screen_size,
};
use crate::{
    fx::kills::{Callout, KillConfirmed, KillFeedbackSet, KillFeedbackStats},
    menu::MenuState,
    palette,
    render::{CameraFollowSet, CurrentFov, MainCamera},
    shared::FreezableTime,
    tuning::Tuning,
    viewmodel::ViewmodelSet,
    waves::{ScoreAwarded, ScoreKind},
};
use bevy::{prelude::*, text::FontSize, ui::UiSystems, window::PrimaryWindow};
use std::fmt::Write;

/// Score popups in the pool.
pub const POPUP_POOL: usize = 12;
/// A popup's box (px) and the gap between stacked lines (px).
const POPUP_BOX: Vec2 = Vec2::new(360.0, 56.0);
const POPUP_LINE: f32 = 34.0;
/// Where the first line sits from the knight's chest on screen (px): up and
/// to the right, clear of him and of the damage numbers.
const POPUP_OFFSET: Vec2 = Vec2::new(64.0, -72.0);
/// Font sizes: the kill's "+100", and the bonuses under it.
pub const POPUP_SIZE: f32 = 36.0;
pub const BONUS_SIZE: f32 = 28.0;
/// The callout: its size, box, where it sits from the crosshair (px) and its
/// tilt (rad).
pub const CALLOUT_SIZE: f32 = 58.0;
const CALLOUT_BOX: Vec2 = Vec2::new(420.0, 80.0);
const CALLOUT_OFFSET: Vec2 = Vec2::new(185.0, 64.0);
const CALLOUT_TILT: f32 = -0.12;
/// Colors: the bonuses' gold and violet.
pub const HEADSHOT_GOLD: Color = palette::HEADSHOT;
pub const VOID_VIOLET: Color = Color::srgb(0.8, 0.5, 1.0);
/// Every character a popup or callout draws, so the atlases hold them all.
const GLYPHS: &str = "+0123456789 HEADSHOTVOIDUBLRIPQAM!";

/// What a score popup says.
pub fn popup_style(kind: ScoreKind) -> (f32, Color, &'static str) {
    match kind {
        ScoreKind::Kill | ScoreKind::Wave => (POPUP_SIZE, palette::HIT_WHITE, ""),
        ScoreKind::Headshot => (BONUS_SIZE, HEADSHOT_GOLD, " HEADSHOT"),
        ScoreKind::Void => (BONUS_SIZE, VOID_VIOLET, " VOID"),
    }
}

/// Writes a popup's text ("+50 HEADSHOT") into `out`, reusing its storage.
pub fn write_popup(out: &mut String, kind: ScoreKind, points: u32) {
    out.clear();
    let _ = write!(out, "+{points}{}", popup_style(kind).2);
}

/// A callout's fill colour: hotter the longer the chain.
pub fn callout_color(callout: Callout) -> Color {
    match callout {
        Callout::Double => palette::HEADSHOT,
        Callout::Triple => Color::srgb(1.0, 0.6, 0.22),
        Callout::Quad => Color::srgb(1.0, 0.33, 0.3),
        Callout::Rampage => Color::srgb(0.82, 0.45, 1.0),
    }
}

/// A popup's or callout's motion at `age` of `life` seconds: (rise 0..1,
/// alpha, scale). It stamps in large, settles, and fades over its last 35%.
pub fn stamp_motion(age: f32, life: f32, stamp: f32) -> (f32, f32, f32) {
    let t = (age / life.max(0.05)).clamp(0.0, 1.0);
    let rise = 1.0 - (1.0 - t) * (1.0 - t);
    let alpha = if t < 0.65 {
        1.0
    } else {
        1.0 - (t - 0.65) / 0.35
    };
    let settle = (age / 0.09).clamp(0.0, 1.0);
    let scale = 1.0 + stamp * (1.0 - settle) * (1.0 - settle);
    (rise, alpha.clamp(0.0, 1.0), scale)
}

/// A pooled score popup.
#[derive(Component, Debug, Clone, Default)]
pub struct ScorePopup {
    pub active: bool,
    pub age: f32,
    /// The world point it floats from (the knight's chest).
    pub point: Vec3,
    /// Its line in a stack (0 = the kill's "+100").
    pub line: u8,
    pub kind: Option<ScoreKind>,
    pub color: Color,
    /// Shown for the first time this frame.
    pub fresh: bool,
}

/// The multi-kill callout (one at a time; a longer chain replaces it).
#[derive(Component, Debug, Clone, Default)]
pub struct KillCallout {
    pub callout: Option<Callout>,
    pub age: f32,
}

/// One text layer of a popup or callout: `0..INK_LAYERS` ink, then the face.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct KillGlyph(pub u8);

/// The root the popups and callout hang from.
#[derive(Component, Debug, Clone, Copy)]
struct KillUiRoot;

pub(super) fn build(app: &mut App) {
    app.add_message::<KillConfirmed>()
        .add_message::<ScoreAwarded>()
        .add_systems(Startup, spawn_kill_ui)
        .add_systems(
            Update,
            (start_popups, start_callouts).after(KillFeedbackSet),
        )
        .add_systems(
            PostUpdate,
            place_kill_ui
                .after(CameraFollowSet)
                .after(ViewmodelSet)
                .before(UiSystems::Prepare),
        );
}

/// An inked text: ink copies under a face, `w` px of outline.
fn spawn_inked(parent: &mut ChildSpawnerCommands, size: f32, face: Color) {
    let w = (size * 0.085).max(1.5);
    for layer in 0..=INK_LAYERS {
        let is_face = layer == INK_LAYERS;
        let (at, shadow) = if is_face {
            (Vec2::ZERO, Vec2::new(0.4, 1.3) * w)
        } else {
            let (at, shadow) = INK_OFFSETS[layer as usize];
            (at * w, shadow * w)
        };
        let mut text = String::with_capacity(24);
        text.push_str(GLYPHS);
        parent.spawn((
            KillGlyph(layer),
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                ..default()
            },
            Text::new(text),
            TextFont::from_font_size(size),
            TextColor(if is_face { face } else { INK }),
            TextLayout::justify(Justify::Center),
            TextShadow {
                offset: shadow,
                color: INK,
            },
            UiTransform::from_translation(Val2::px(at.x, at.y)),
        ));
    }
}

fn spawn_kill_ui(mut commands: Commands) {
    commands
        .spawn((
            Name::new("Kill feedback"),
            KillUiRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            GlobalZIndex(11),
        ))
        .with_children(|root| {
            for k in 0..POPUP_POOL {
                // Each size is in the pool from the start (atlases filled).
                let size = if k % 2 == 0 { POPUP_SIZE } else { BONUS_SIZE };
                root.spawn((
                    ScorePopup::default(),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        top: px(0),
                        width: px(POPUP_BOX.x),
                        height: px(POPUP_BOX.y),
                        ..default()
                    },
                    UiTransform::default(),
                    Visibility::Hidden,
                ))
                .with_children(|p| spawn_inked(p, size, palette::HIT_WHITE));
            }
            root.spawn((
                KillCallout::default(),
                Node {
                    position_type: PositionType::Absolute,
                    left: percent(50),
                    top: percent(50),
                    width: px(CALLOUT_BOX.x),
                    height: px(CALLOUT_BOX.y),
                    margin: UiRect::new(
                        px(-CALLOUT_BOX.x / 2.0),
                        px(0),
                        px(-CALLOUT_BOX.y / 2.0),
                        px(0),
                    ),
                    ..default()
                },
                UiTransform::default(),
                Visibility::Hidden,
            ))
            .with_children(|c| spawn_inked(c, CALLOUT_SIZE, palette::HEADSHOT));
        });
}

/// Rewrites a popup's or callout's glyph layers: `text`, the face `color`,
/// the font `size`.
fn restyle(
    children: &Children,
    glyphs: &mut Query<(&KillGlyph, &mut Text, &mut TextFont, &mut TextColor)>,
    write: impl Fn(&mut String),
    size: f32,
    color: Color,
) {
    for child in children.iter() {
        let Ok((layer, mut text, mut font, mut fill)) = glyphs.get_mut(child) else {
            continue;
        };
        write(&mut text.0);
        if !matches!(font.font_size, FontSize::Px(s) if s == size) {
            font.font_size = FontSize::Px(size);
        }
        fill.0 = if layer.0 >= INK_LAYERS { color } else { INK };
    }
}

/// Takes a popup for each score award (Waves), stacking a kill's bonuses
/// under its "+100".
#[allow(clippy::type_complexity)]
fn start_popups(
    start: Option<Res<FrameStartTick>>,
    mut awards: MessageReader<ScoreAwarded>,
    mut popups: Query<(Entity, &mut ScorePopup, &Children)>,
    mut glyphs: Query<(&KillGlyph, &mut Text, &mut TextFont, &mut TextColor)>,
    mut stats: Option<ResMut<KillFeedbackStats>>,
) {
    let frame_start = start.map_or(0, |s| s.0);
    for award in awards.read() {
        if award.kind == ScoreKind::Wave {
            continue;
        }
        // The line: after any popups already up for this knight this tick.
        let line = popups
            .iter()
            .filter(|(_, p, _)| p.active && p.fresh && p.point == award.at + Vec3::Y * 1.3)
            .count() as u8;
        // A free popup, else the oldest.
        let Some(pick) = popups
            .iter()
            .max_by(|a, b| {
                (!a.1.active)
                    .cmp(&!b.1.active)
                    .then(a.1.age.total_cmp(&b.1.age))
            })
            .map(|p| p.0)
        else {
            continue;
        };
        let Ok((_, mut popup, children)) = popups.get_mut(pick) else {
            continue;
        };
        let (size, color, _) = popup_style(award.kind);
        *popup = ScorePopup {
            active: true,
            age: 0.0,
            point: award.at + Vec3::Y * 1.3,
            line,
            kind: Some(award.kind),
            color,
            fresh: true,
        };
        let (kind, points) = (award.kind, award.points);
        restyle(
            children,
            &mut glyphs,
            |s| write_popup(s, kind, points),
            size,
            color,
        );
        if let Some(stats) = stats.as_mut() {
            stats.popups_same_frame += u32::from(award.tick > frame_start);
        }
    }
}

/// Stamps the multi-kill callout in for a chain of two or more kills.
fn start_callouts(
    mut kills: MessageReader<KillConfirmed>,
    mut callouts: Query<(&mut KillCallout, &Children)>,
    mut glyphs: Query<(&KillGlyph, &mut Text, &mut TextFont, &mut TextColor)>,
) {
    let Some(best) = kills
        .read()
        .filter_map(|k| Callout::for_chain(k.chain))
        .max()
    else {
        return;
    };
    for (mut callout, children) in &mut callouts {
        callout.callout = Some(best);
        callout.age = 0.0;
        restyle(
            children,
            &mut glyphs,
            |s| {
                s.clear();
                s.push_str(best.text());
            },
            CALLOUT_SIZE,
            callout_color(best),
        );
    }
}

/// Floats, fades and places the popups (projected from the world, like the
/// damage numbers) and the callout.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn place_kill_ui(
    time: FreezableTime,
    tuning: Res<Tuning>,
    fov: Res<CurrentFov>,
    menu: Option<Res<MenuState>>,
    ui: Query<&Camera, With<IsDefaultUiCamera>>,
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Option<Single<&Transform, With<MainCamera>>>,
    mut root: Query<&mut Visibility, (With<KillUiRoot>, Without<ScorePopup>, Without<KillCallout>)>,
    mut popups: Query<
        (
            &mut ScorePopup,
            &mut UiTransform,
            &mut Visibility,
            &Children,
        ),
        Without<KillCallout>,
    >,
    mut callouts: Query<
        (
            &mut KillCallout,
            &mut UiTransform,
            &mut Visibility,
            &Children,
        ),
        Without<ScorePopup>,
    >,
    mut glyphs: Query<(&KillGlyph, &mut TextColor, &mut TextShadow)>,
) {
    let dt = time.delta_secs();
    let feel = &tuning.kills;
    let menu_up = menu.is_some_and(|m| m.menu_visible());
    for mut v in &mut root {
        v.set_if_neq(if menu_up {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
    let screen = screen_size(&ui, &window);
    let camera = camera.map(|c| GlobalTransform::from(**c));
    let mut fade = |children: &Children, color: Color, alpha: f32| {
        for child in children.iter() {
            if let Ok((layer, mut fill, mut shadow)) = glyphs.get_mut(child) {
                let want = if layer.0 >= INK_LAYERS { color } else { INK }.with_alpha(alpha);
                if fill.0 != want {
                    fill.0 = want;
                }
                let ink = INK.with_alpha(alpha);
                if shadow.color != ink {
                    shadow.color = ink;
                }
            }
        }
    };
    for (mut popup, mut tf, mut vis, children) in &mut popups {
        if !popup.active {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        let (rise, alpha, scale) = stamp_motion(popup.age, feel.popup_seconds, 0.45);
        popup.fresh = false;
        popup.age += dt;
        if popup.age > feel.popup_seconds + dt {
            popup.active = false;
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        let pos = screen
            .zip(camera)
            .and_then(|(s, c)| project_to_screen(&c, fov.0, s, popup.point));
        let Some(pos) = pos else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        let at = pos + POPUP_OFFSET - Vec2::new(0.0, POPUP_BOX.y * 0.5)
            + Vec2::new(
                0.0,
                POPUP_LINE * f32::from(popup.line) - feel.popup_rise * rise,
            );
        let target = UiTransform {
            translation: Val2::px(at.x.round(), at.y.round()),
            scale: Vec2::splat(scale),
            rotation: Rot2::IDENTITY,
        };
        if *tf != target {
            *tf = target;
        }
        fade(children, popup.color, alpha);
        vis.set_if_neq(Visibility::Inherited);
    }
    for (mut callout, mut tf, mut vis, children) in &mut callouts {
        let Some(which) = callout.callout else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        let (rise, alpha, scale) = stamp_motion(callout.age, feel.callout_seconds, 0.8);
        callout.age += dt;
        if callout.age > feel.callout_seconds + dt {
            callout.callout = None;
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        let squash = super::squash_pop(callout.age);
        let at = CALLOUT_OFFSET - Vec2::new(0.0, 14.0 * rise);
        let target = UiTransform {
            translation: Val2::px(at.x, at.y),
            scale: squash * scale,
            rotation: Rot2::radians(CALLOUT_TILT),
        };
        if *tf != target {
            *tf = target;
        }
        fade(children, callout_color(which), alpha);
        vis.set_if_neq(Visibility::Inherited);
    }
}
