//! The run HUD (D65): wave, knights left and score along the top, in M2's
//! cartoon frames (ink line, slate rim, dark panel, the cartoon font); the
//! break's banner (D79) and a new wave's title in the middle; the potion's
//! cyan "+25" under the crosshair.
//!
//! The top strip sits in the top centre, which the M1/M2 layout leaves empty
//! (the readout is top right, the performance overlay top left), so no
//! existing anchor moves.

use super::{
    BannerKind, GOLD_FRAME, POTION_CYAN, RunUi, break_banner, label, set_text, title,
    write_banner_title, write_thousands,
};
use crate::{
    hud::{
        squash_pop,
        style::{ACCENT, INK, PANEL, RIM, TROUGH, caps, dim, ink},
    },
    shared::{GameCue, Player},
    tuning::Tuning,
    waves::{RunPhase, RunSummary},
};
use bevy::prelude::*;
use std::fmt::Write;

/// The strip's distance from the top of the screen (px).
pub const STRIP_TOP: f32 = 14.0;
/// The banner's top, as a percentage of the screen height.
pub const BANNER_TOP: f32 = 21.0;
/// How long a new wave's title stays up (s).
pub const WAVE_TITLE_SECONDS: f32 = 1.6;
/// How long the score's pop lasts (s).
pub const SCORE_POP_SECONDS: f32 = 0.28;
/// The "+25": its life (s), rise (px) and gap below the crosshair (px).
pub const POTION_NUMBER_SECONDS: f32 = 1.0;
pub const POTION_NUMBER_RISE: f32 = 36.0;
pub const POTION_NUMBER_GAP: f32 = 64.0;

pub(super) fn build(app: &mut App) {
    app.init_resource::<HudClock>()
        .add_systems(Startup, spawn_run_hud)
        .add_systems(Update, (update_run_hud, potion_number).chain());
}

/// The HUD's animation state (real-time clocks) and a scratch string.
#[derive(Resource, Debug)]
struct HudClock {
    score: Option<u32>,
    score_pop: f32,
    /// (wave, fighting) last frame.
    wave_seen: Option<(u32, bool)>,
    wave_title: f32,
    count: Option<u32>,
    count_pop: f32,
    banner_pop: f32,
    banner_kind: Option<BannerKind>,
    potion: Option<f32>,
    scratch: String,
}

impl Default for HudClock {
    fn default() -> Self {
        Self {
            score: None,
            score_pop: f32::MAX,
            wave_seen: None,
            wave_title: f32::MAX,
            count: None,
            count_pop: f32::MAX,
            banner_pop: f32::MAX,
            banner_kind: None,
            potion: None,
            scratch: String::with_capacity(64),
        }
    }
}

/// A frame of the top strip: a caption over a value.
fn stat_frame(
    parent: &mut ChildSpawnerCommands,
    caption: &str,
    value: RunUi,
    rim: Color,
    value_size: f32,
    min_width: f32,
    frame: Option<RunUi>,
) {
    let mut e = parent.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            min_width: px(min_width),
            padding: UiRect::new(px(16), px(16), px(4), px(6)),
            border: UiRect::all(px(3)),
            border_radius: BorderRadius::all(px(14)),
            ..default()
        },
        BackgroundColor(PANEL),
        BorderColor::all(rim),
        ink(3.0),
        UiTransform::default(),
    ));
    e.with_children(|f| {
        f.spawn(caps(caption, 12.0, dim(0.8)));
        f.spawn((value, label("0", value_size)));
    });
    if let Some(frame) = frame {
        e.insert(frame);
    }
}

fn spawn_run_hud(mut commands: Commands) {
    // The top strip.
    commands
        .spawn((
            Name::new("Run HUD"),
            RunUi::Hud,
            Node {
                position_type: PositionType::Absolute,
                top: px(STRIP_TOP),
                width: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::FlexStart,
                column_gap: px(12),
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(12),
        ))
        .with_children(|strip| {
            stat_frame(strip, "KNIGHTS", RunUi::Knights, RIM, 28.0, 96.0, None);
            stat_frame(strip, "WAVE", RunUi::Wave, GOLD_FRAME, 34.0, 104.0, None);
            let score = Some(RunUi::ScoreFrame);
            stat_frame(strip, "SCORE", RunUi::Score, RIM, 28.0, 124.0, score);
        });

    // The banner: the break's title, countdown and hint, or a wave's title.
    commands
        .spawn((
            Name::new("Run banner"),
            RunUi::Banner,
            Node {
                position_type: PositionType::Absolute,
                top: percent(BANNER_TOP),
                width: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(10),
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(20),
        ))
        .with_children(|b| {
            // The title on a gold-framed ribbon, so it reads over any scene.
            b.spawn((
                Node {
                    padding: UiRect::new(px(30), px(30), px(2), px(8)),
                    border: UiRect::all(px(4)),
                    border_radius: BorderRadius::all(px(20)),
                    ..default()
                },
                BackgroundColor(PANEL.with_alpha(0.86)),
                BorderColor::all(GOLD_FRAME),
                ink(3.0),
                children![(
                    RunUi::BannerTitle,
                    title("WAVE 1", 58.0, ACCENT),
                    UiTransform::default(),
                )],
            ));
            b.spawn((
                RunUi::Countdown,
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(4),
                    ..default()
                },
            ))
            .with_children(|c| {
                c.spawn(caps("NEXT WAVE IN", 16.0, dim(0.9)));
                c.spawn((
                    Node {
                        width: px(96),
                        height: px(96),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(5)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(PANEL.with_alpha(0.95)),
                    BorderColor::all(GOLD_FRAME),
                    ink(3.0),
                    children![(
                        RunUi::CountdownValue,
                        title("10", 52.0, Color::WHITE),
                        UiTransform::default(),
                    )],
                ));
            });
            b.spawn((
                RunUi::Hint,
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                    margin: UiRect::top(px(4)),
                    ..default()
                },
            ))
            .with_children(|h| {
                h.spawn(label("Press", 20.0));
                h.spawn((
                    Node {
                        padding: UiRect::axes(px(10), px(2)),
                        border: UiRect::all(px(2)),
                        border_radius: BorderRadius::all(px(7)),
                        ..default()
                    },
                    BackgroundColor(TROUGH),
                    BorderColor::all(RIM),
                    ink(2.0),
                    children![(RunUi::HintKey, label(crate::input::start_key_name(), 18.0))],
                ));
                h.spawn(label("to start", 20.0));
            });
        });

    // The potion's "+25", centred under the crosshair.
    commands.spawn((
        Name::new("Potion number"),
        Node {
            position_type: PositionType::Absolute,
            top: percent(50),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        },
        GlobalZIndex(15),
        children![(
            RunUi::PotionNumber,
            Text::new("+25"),
            TextFont::from_font_size(34.0),
            TextColor(POTION_CYAN),
            TextShadow {
                offset: Vec2::new(1.5, 2.5),
                color: INK,
            },
            UiTransform::default(),
            Visibility::Hidden,
        )],
    ));
}

/// A pop that overshoots and settles: 1 at `age` ≥ `length`.
pub fn pop_scale(age: f32, length: f32, amount: f32) -> f32 {
    if !(0.0..length).contains(&age) {
        return 1.0;
    }
    let t = age / length;
    1.0 + amount * (1.0 - t).powi(2) * (t * std::f32::consts::PI * 1.5).cos()
}

fn show(visibility: &mut Mut<Visibility>, on: bool) {
    visibility.set_if_neq(if on {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
}

#[allow(clippy::type_complexity)]
fn update_run_hud(
    time: Res<Time<Real>>,
    summary: Option<Res<RunSummary>>,
    mut clock: ResMut<HudClock>,
    mut texts: Query<(&RunUi, &mut Text)>,
    mut shown: Query<(&RunUi, &mut Visibility), Without<Text>>,
    mut scaled: Query<(&RunUi, &mut UiTransform)>,
) {
    let dt = time.delta_secs();
    let clock = &mut *clock;
    let Some(summary) = summary else {
        for (_, mut v) in &mut shown {
            show(&mut v, false);
        }
        return;
    };

    // The score pops when it ticks up.
    if clock.score.is_some_and(|s| summary.score > s) {
        clock.score_pop = 0.0;
    }
    clock.score = Some(summary.score);
    clock.score_pop += dt;

    // A new wave's title: at a run's start and when a break ends.
    let fighting = summary.phase == RunPhase::Fighting;
    let seen = (summary.wave, fighting);
    if fighting && clock.wave_seen != Some(seen) {
        clock.wave_title = 0.0;
    }
    clock.wave_seen = Some(seen);
    clock.wave_title += dt;

    let over = matches!(summary.phase, RunPhase::Over { .. });
    let banner = break_banner(&summary).or_else(|| {
        (fighting && clock.wave_title < WAVE_TITLE_SECONDS)
            .then_some(BannerKind::Wave(summary.wave))
    });
    // The title pops in when it changes (not on every tick of the countdown).
    let key = |b: Option<BannerKind>| {
        b.map(|k| match k {
            BannerKind::Cleared { wave, .. } => (0, wave),
            BannerKind::Wave(wave) => (1, wave),
        })
    };
    if key(banner) != key(clock.banner_kind) {
        clock.banner_pop = 0.0;
    }
    clock.banner_kind = banner;
    clock.banner_pop += dt;
    let count = match banner {
        Some(BannerKind::Cleared { count, .. }) => Some(count),
        _ => None,
    };
    if count.is_some() && count != clock.count {
        clock.count_pop = 0.0;
    }
    clock.count = count;
    clock.count_pop += dt;

    let scratch = &mut clock.scratch;
    for (part, mut text) in &mut texts {
        match part {
            RunUi::Wave => set_text(&mut text, scratch, |s| {
                let _ = write!(s, "{}", summary.wave);
            }),
            RunUi::Knights => set_text(&mut text, scratch, |s| {
                let _ = write!(s, "{}", summary.knights_left);
            }),
            RunUi::Score => set_text(&mut text, scratch, |s| write_thousands(s, summary.score)),
            RunUi::BannerTitle => {
                if let Some(kind) = banner {
                    set_text(&mut text, scratch, |s| write_banner_title(s, kind));
                }
            }
            RunUi::CountdownValue => {
                if let Some(count) = count {
                    set_text(&mut text, scratch, |s| {
                        let _ = write!(s, "{count}");
                    });
                }
            }
            RunUi::HintKey => set_text(&mut text, scratch, |s| {
                s.push_str(crate::input::start_key_name())
            }),
            _ => {}
        }
    }
    for (part, mut v) in &mut shown {
        match part {
            RunUi::Hud => show(&mut v, !over),
            RunUi::Banner => show(&mut v, banner.is_some() && !over),
            RunUi::Countdown | RunUi::Hint => show(&mut v, count.is_some()),
            _ => {}
        }
    }
    for (part, mut tf) in &mut scaled {
        let scale = match part {
            RunUi::ScoreFrame => {
                squash_pop(clock.score_pop * 0.6)
                    * pop_scale(clock.score_pop, SCORE_POP_SECONDS, 0.22)
            }
            RunUi::BannerTitle => Vec2::splat(pop_scale(clock.banner_pop, 0.35, 0.35)),
            RunUi::CountdownValue => Vec2::splat(pop_scale(clock.count_pop, 0.3, 0.25)),
            _ => continue,
        };
        if tf.scale != scale {
            tf.scale = scale;
        }
    }
}

/// The cyan "+25" when the player drinks a potion: pops, rises and fades.
#[allow(clippy::type_complexity)]
fn potion_number(
    time: Res<Time<Real>>,
    tuning: Res<Tuning>,
    player: Option<Single<Entity, With<Player>>>,
    mut cues: MessageReader<GameCue>,
    mut clock: ResMut<HudClock>,
    mut number: Query<(
        &RunUi,
        &mut Text,
        &mut TextColor,
        &mut TextShadow,
        &mut Visibility,
        &mut UiTransform,
    )>,
) {
    let me = player.map(|p| *p);
    let clock = &mut *clock;
    for cue in cues.read() {
        if let GameCue::PotionPicked { who, .. } = *cue
            && Some(who) == me
        {
            clock.potion = Some(0.0);
        }
    }
    let Some(age) = clock.potion else {
        return;
    };
    let amount = tuning.waves.potion_shield.round() as u32;
    let t = (age / POTION_NUMBER_SECONDS).clamp(0.0, 1.0);
    let alpha = if t < 0.6 { 1.0 } else { 1.0 - (t - 0.6) / 0.4 };
    let rise = POTION_NUMBER_RISE * (1.0 - (1.0 - t).powi(2));
    let done = age >= POTION_NUMBER_SECONDS;
    let scratch = &mut clock.scratch;
    for (part, mut text, mut color, mut shadow, mut v, mut tf) in &mut number {
        if *part != RunUi::PotionNumber {
            continue;
        }
        set_text(&mut text, scratch, |s| {
            let _ = write!(s, "+{amount}");
        });
        color.0 = POTION_CYAN.with_alpha(alpha);
        shadow.color = INK.with_alpha(alpha);
        show(&mut v, !done);
        tf.translation = Val2::px(0.0, POTION_NUMBER_GAP - rise);
        tf.scale = squash_pop(age) * pop_scale(age, 0.25, 0.4);
    }
    clock.potion = (!done).then_some(age + time.delta_secs());
}
