//! The results screen (D65, D84): when a run is over, the world dims behind
//! the pause menu's veil and a gold-framed cartoon card (T12's look) shows
//! the wave reached, the score, eliminations, accuracy, headshots and run
//! time, the best run beside them, a bursting "NEW BEST!" when this run beat
//! it, and the seed in small print. **Go again** (or the start key) starts a
//! new run in place; **Quit to menu** goes back to the main menu (chunk 5).
//! The cursor is free while it shows (`input::cursor_lock`), so the trackpad
//! clicks the buttons.
//!
//! M4 chunk 5 (D110): after a death the card waits for the death cam (it
//! comes up once the knights' victory hop is over); the card slides up, its
//! numbers count up from 0 with ticks ([`count_up`], [`COUNT_SECONDS`]), and
//! "NEW BEST!" bursts only after the count. A run started above wave 1
//! (D115) says so under its wave and never shows "NEW BEST!".

use super::{
    BACKDROP, BLUE, BLUE_HOVER, GOLD_FRAME, GOLD_HOVER, RIVET, ResultsButton, RunUi, SLATE,
    SLATE_HOVER, label, set_text, title, write_accuracy, write_run_time, write_thousands,
};
use crate::{
    hud::{
        UiArt,
        style::{ACCENT, INK, PANEL, RIM, caps, dim, ink, text},
    },
    shared::AppState,
    waves::{RestartRun, RunPhase, RunSummary},
};
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use std::fmt::Write;

/// The card's width (px).
pub const CARD_WIDTH: f32 = 720.0;
/// The "NEW BEST!" burst's size (px) and its star's points.
pub const BURST_SIZE: f32 = 150.0;
const BURST_POINTS: usize = 14;
/// The burst image's resolution (px).
const BURST_PIXELS: u32 = 256;
/// Seconds the numbers take to count up, and the "NEW BEST!" burst's
/// delay after the card appears (just after the count).
pub const COUNT_SECONDS: f32 = 1.1;
pub const BURST_DELAY: f32 = COUNT_SECONDS + 0.12;
/// Seconds the card takes to slide up into place.
pub const CARD_SLIDE_SECONDS: f32 = 0.25;
/// A count tick sounds at most this often (s).
const TICK_EVERY: f32 = 0.07;

pub(super) fn build(app: &mut App) {
    app.init_resource::<ResultsClock>()
        .add_systems(Startup, spawn_results)
        .add_systems(
            Update,
            (results_buttons, update_results, results_button_looks).chain(),
        );
}

#[derive(Resource, Debug)]
struct ResultsClock {
    /// Seconds the card has been up (real time).
    shown: Option<f32>,
    /// Seconds since the last count tick sounded.
    tick: f32,
    scratch: String,
}

impl Default for ResultsClock {
    fn default() -> Self {
        Self {
            shown: None,
            tick: 0.0,
            scratch: String::with_capacity(64),
        }
    }
}

/// How far the count-up is `age` seconds after the card appears (0..=1):
/// fast at first, easing into the final numbers.
pub fn count_up(age: f32) -> f32 {
    let x = (age / COUNT_SECONDS).clamp(0.0, 1.0);
    1.0 - (1.0 - x).powi(3)
}

/// `value` counted up to fraction `k` (whole numbers, exact at 1).
pub fn counted(value: u32, k: f32) -> u32 {
    if k >= 1.0 {
        value
    } else {
        (value as f32 * k.max(0.0)).floor() as u32
    }
}

/// Whether the results card is up: the run is over and, after a death, the
/// death cam has held through the knights' victory hop.
pub fn results_ready(run: Option<&crate::waves::Run>, now: u64) -> bool {
    run.is_some_and(|r| r.is_over() && !r.hopping(now))
}

/// The "NEW BEST!" starburst: a gold star with a lighter heart and an ink
/// rim, as straight RGBA (made once at startup).
pub fn burst_pixels(size: u32) -> Vec<u8> {
    let gold = ACCENT.to_srgba();
    let deep = GOLD_FRAME.to_srgba();
    let ink = INK.to_srgba();
    let half = size as f32 / 2.0;
    let outer = 0.98;
    let inner = 0.74;
    let rim = 0.075;
    let aa = 1.5 / half;
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let p = Vec2::new(x as f32 + 0.5 - half, y as f32 + 0.5 - half) / half;
            let r = p.length();
            // The star's edge at this angle: straight lines between tips and
            // valleys.
            let seg = std::f32::consts::TAU / (2 * BURST_POINTS) as f32;
            let a = p.y.atan2(p.x).rem_euclid(std::f32::consts::TAU);
            let k = (a / seg).floor();
            let f = a / seg - k;
            let (r0, r1) = if (k as usize).is_multiple_of(2) {
                (outer, inner)
            } else {
                (inner, outer)
            };
            // Radius of the straight edge between the two corners at `f`.
            let c0 = Vec2::from_angle(k * seg) * r0;
            let c1 = Vec2::from_angle((k + 1.0) * seg) * r1;
            let dir = Vec2::from_angle(a);
            let e = c1 - c0;
            let denom = dir.perp_dot(e);
            let edge = if denom.abs() > 1e-6 {
                c0.perp_dot(e) / denom
            } else {
                r0 + (r1 - r0) * f
            };
            let cover = ((edge - r) / aa + 0.5).clamp(0.0, 1.0);
            let body = ((edge - rim - r) / aa + 0.5).clamp(0.0, 1.0);
            let heart = (1.0 - r / edge.max(1e-3)).clamp(0.0, 1.0);
            let fill = Srgba::new(
                deep.red + (gold.red - deep.red) * (0.35 + heart),
                deep.green + (gold.green - deep.green) * (0.35 + heart),
                deep.blue + (gold.blue - deep.blue) * (0.35 + heart),
                1.0,
            );
            let c = Srgba::new(
                ink.red + (fill.red - ink.red) * body,
                ink.green + (fill.green - ink.green) * body,
                ink.blue + (fill.blue - ink.blue) * body,
                cover,
            );
            data.extend(
                [c.red, c.green, c.blue, c.alpha]
                    .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8),
            );
        }
    }
    data
}

fn burst_image(size: u32) -> Image {
    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        burst_pixels(size),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// A small brass rivet in a button's corner.
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

/// A cartoon button like the pause menu's: brass frame, rivets, a shine, a
/// crystal on the left and the label.
fn button(label_text: &str, action: ResultsButton, crystal: Handle<Image>) -> impl Bundle {
    let height = 64.0;
    let primary = action == ResultsButton::GoAgain;
    (
        action,
        Button,
        Node {
            height: px(height),
            flex_grow: 1.0,
            flex_basis: px(0),
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
            Spawn((
                ImageNode::new(crystal),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(18),
                    width: px(height * 0.55),
                    height: px(height * 0.55),
                    ..default()
                },
            )),
            Spawn(title(label_text, 28.0, Color::WHITE)),
        )),
    )
}

/// One stat row: the label on the left, the value on the right.
fn stat_row(parent: &mut ChildSpawnerCommands, name: &str, value: RunUi, size: f32) {
    parent.spawn((
        Node {
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            height: px(size * 1.45),
            ..default()
        },
        children![text(name, 18.0, dim(0.85)), (value, label("0", size))],
    ));
}

/// A rounded inner panel with a caption, `grow` shares of the row wide.
fn panel(caption: &str, color: Color, grow: f32) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            flex_basis: px(0),
            flex_grow: grow,
            padding: UiRect::new(px(18), px(18), px(10), px(12)),
            row_gap: px(2),
            border: UiRect::all(px(2.5)),
            border_radius: BorderRadius::all(px(14)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.02, 0.035, 0.06, 0.55)),
        BorderColor::all(RIM),
        ink(2.0),
        children![(
            Node {
                margin: UiRect::bottom(px(4)),
                ..default()
            },
            children![caps(caption, 14.0, color)],
        )],
    )
}

fn spawn_results(
    mut commands: Commands,
    art: Option<Res<UiArt>>,
    images: Option<ResMut<Assets<Image>>>,
) {
    let art = art.map(|a| a.clone()).unwrap_or_default();
    let burst = images
        .map(|mut images| images.add(burst_image(BURST_PIXELS)))
        .unwrap_or_default();
    commands
        .spawn((
            Name::new("Results"),
            RunUi::Results,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(BACKDROP),
            GlobalZIndex(90),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            root.spawn((
                Name::new("Results card"),
                RunUi::ResultsCard,
                UiTransform::default(),
                Node {
                    width: px(CARD_WIDTH),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Stretch,
                    padding: UiRect::new(px(34), px(34), px(22), px(26)),
                    row_gap: px(14),
                    border: UiRect::all(px(5)),
                    border_radius: BorderRadius::all(px(26)),
                    ..default()
                },
                BackgroundColor(PANEL.with_alpha(0.97)),
                BorderColor::all(GOLD_FRAME),
                ink(3.5),
                BoxShadow::new(
                    Color::srgba(0.0, 0.0, 0.0, 0.45),
                    px(0),
                    px(12),
                    px(0),
                    px(40),
                ),
            ))
            .with_children(|card| {
                // The header: "RUN OVER" over "WAVE n".
                card.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|h| {
                    h.spawn(caps("RUN OVER", 18.0, dim(0.8)));
                    h.spawn((RunUi::ResultsWave, title("WAVE 1", 68.0, ACCENT)));
                    // D115: a run started above wave 1 is marked, unranked.
                    h.spawn((
                        RunUi::StartedAt,
                        caps("STARTED AT WAVE 10 \u{b7} NOT RANKED", 15.0, dim(0.8)),
                        Visibility::Hidden,
                    ));
                });
                // This run beside the best run.
                card.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(16),
                    ..default()
                })
                .with_children(|cols| {
                    cols.spawn(panel("THIS RUN", ACCENT, 1.4))
                        .with_children(|p| {
                            stat_row(p, "Score", RunUi::ResultsScore, 30.0);
                            stat_row(p, "Eliminations", RunUi::ResultsEliminations, 22.0);
                            stat_row(p, "Accuracy", RunUi::ResultsAccuracy, 22.0);
                            stat_row(p, "Headshots", RunUi::ResultsHeadshots, 22.0);
                            stat_row(p, "Run time", RunUi::ResultsTime, 22.0);
                        });
                    cols.spawn(panel("BEST RUN", dim(0.85), 1.0))
                        .with_children(|p| {
                            stat_row(p, "Wave", RunUi::BestWave, 22.0);
                            stat_row(p, "Score", RunUi::BestScore, 22.0);
                            stat_row(p, "Eliminations", RunUi::BestEliminations, 22.0);
                            stat_row(p, "Run time", RunUi::BestTime, 22.0);
                        });
                });
                // The buttons.
                card.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(16),
                    margin: UiRect::top(px(4)),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(button(
                        "GO AGAIN",
                        ResultsButton::GoAgain,
                        art.crystal_blue.clone(),
                    ));
                    row.spawn(button(
                        "QUIT TO MENU",
                        ResultsButton::Quit,
                        art.crystal_blue.clone(),
                    ));
                });
                // The key hint and the seed.
                card.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((RunUi::ResultsHint, text("Enter  go again", 14.0, dim(0.7))));
                    row.spawn((RunUi::Seed, text("Seed 0", 14.0, dim(0.55))));
                });
                // "NEW BEST!": a gold starburst over the card's corner.
                card.spawn((
                    RunUi::NewBest,
                    Node {
                        position_type: PositionType::Absolute,
                        right: px(-BURST_SIZE * 0.3),
                        top: px(-BURST_SIZE * 0.36),
                        width: px(BURST_SIZE),
                        height: px(BURST_SIZE),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    ImageNode::new(burst),
                    UiTransform::default(),
                    Visibility::Hidden,
                    children![(
                        Node {
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        children![
                            title("NEW", 24.0, Color::WHITE),
                            title("BEST!", 30.0, Color::WHITE),
                        ],
                    )],
                ));
            });
        });
}

/// The burst's pop and wobble at `age` seconds (scale, rotation in radians).
pub fn burst_motion(age: f32) -> (f32, f32) {
    let x = (age / 0.45).clamp(0.0, 1.0);
    // Bursts out past full size, then settles and breathes.
    let pop = 1.0 - (1.0 - x).powi(3) + 0.5 * (x * std::f32::consts::PI).sin() * (1.0 - x);
    let breathe = 1.0 + 0.04 * (age * 5.0).sin();
    let wobble = -0.21 + 0.06 * (age * 3.1).sin();
    (pop * breathe, wobble)
}

#[allow(clippy::type_complexity)]
fn update_results(
    time: Res<Time<Real>>,
    tuning: Res<crate::tuning::Tuning>,
    summary: Option<Res<RunSummary>>,
    run: Option<Res<crate::waves::Run>>,
    tick: Option<Res<crate::shared::SimTick>>,
    mut queue: Option<ResMut<crate::audio::PlayQueue>>,
    mut clock: ResMut<ResultsClock>,
    mut texts: Query<(&RunUi, &mut Text)>,
    mut shown: Query<(&RunUi, &mut Visibility, Option<&mut UiTransform>), Without<Text>>,
    mut labels: Query<(&RunUi, &mut Visibility), With<Text>>,
) {
    let clock = &mut *clock;
    let now = tick.map_or(0, |t| t.0);
    let over = results_ready(run.as_deref(), now)
        && summary
            .as_ref()
            .is_some_and(|s| matches!(s.phase, RunPhase::Over { .. }));
    let dt = time.delta_secs();
    let before = clock.shown;
    clock.shown = if over {
        Some(clock.shown.map_or(0.0, |t| t + dt))
    } else {
        None
    };
    let age = clock.shown.unwrap_or(0.0);
    let k = count_up(age);
    let new_best = over && summary.as_ref().is_some_and(|s| s.new_best) && age >= BURST_DELAY;
    let unranked = over && summary.as_ref().is_some_and(|s| s.start_wave > 1);
    for (part, mut v, tf) in &mut shown {
        let on = match part {
            RunUi::Results => over,
            RunUi::NewBest => new_best,
            RunUi::ResultsCard => {
                if let Some(mut tf) = tf {
                    let x = (age / CARD_SLIDE_SECONDS).clamp(0.0, 1.0);
                    let rise = 1.0 - (1.0 - x).powi(3);
                    let want = UiTransform {
                        translation: Val2::px(0.0, 90.0 * (1.0 - rise)),
                        scale: Vec2::splat(0.92 + 0.08 * rise),
                        ..UiTransform::IDENTITY
                    };
                    if *tf != want {
                        *tf = want;
                    }
                }
                continue;
            }
            _ => continue,
        };
        v.set_if_neq(if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if *part == RunUi::NewBest
            && let Some(mut tf) = tf
            && on
        {
            let (scale, turn) = burst_motion(age - BURST_DELAY);
            tf.scale = Vec2::splat(scale);
            tf.rotation = Rot2::radians(turn);
        }
    }
    for (part, mut v) in &mut labels {
        if *part == RunUi::StartedAt {
            v.set_if_neq(if unranked {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    }
    let (Some(summary), true) = (summary, over) else {
        return;
    };
    let counting = before.is_none_or(|b| count_up(b) < 1.0);
    if !summary.is_changed() && !counting {
        return;
    }
    // A tick while the numbers roll (capped to one every TICK_EVERY).
    clock.tick += dt;
    if k < 1.0 && clock.tick >= TICK_EVERY && summary.results.score > 0 {
        clock.tick = 0.0;
        if let Some(queue) = queue.as_mut() {
            queue.push(crate::audio::Sfx::HitTick, None, time.elapsed_secs_f64());
        }
    }
    let r = &summary.results;
    let best = summary.best.as_ref();
    let scratch = &mut clock.scratch;
    for (part, mut t) in &mut texts {
        match part {
            RunUi::ResultsWave => set_text(&mut t, scratch, |s| {
                let _ = write!(s, "WAVE {}", r.wave);
            }),
            RunUi::StartedAt => set_text(&mut t, scratch, |s| {
                let _ = write!(
                    s,
                    "STARTED AT WAVE {} \u{b7} NOT RANKED",
                    summary.start_wave
                );
            }),
            RunUi::ResultsScore => {
                set_text(&mut t, scratch, |s| write_thousands(s, counted(r.score, k)))
            }
            RunUi::ResultsEliminations => set_text(&mut t, scratch, |s| {
                let _ = write!(s, "{}", counted(r.eliminations, k));
            }),
            RunUi::ResultsAccuracy => {
                set_text(&mut t, scratch, |s| write_accuracy(s, r.accuracy * k))
            }
            RunUi::ResultsHeadshots => set_text(&mut t, scratch, |s| {
                let _ = write!(s, "{}", counted(r.headshots, k));
            }),
            RunUi::ResultsTime => {
                set_text(&mut t, scratch, |s| write_run_time(s, r.run_seconds * k))
            }
            RunUi::BestWave => set_text(&mut t, scratch, |s| match best {
                Some(b) => {
                    let _ = write!(s, "{}", b.wave);
                }
                None => s.push('\u{2014}'),
            }),
            RunUi::BestScore => set_text(&mut t, scratch, |s| match best {
                Some(b) => write_thousands(s, b.score),
                None => s.push('\u{2014}'),
            }),
            RunUi::BestEliminations => set_text(&mut t, scratch, |s| match best {
                Some(b) => {
                    let _ = write!(s, "{}", b.eliminations);
                }
                None => s.push('\u{2014}'),
            }),
            RunUi::BestTime => set_text(&mut t, scratch, |s| match best {
                Some(b) => write_run_time(s, b.run_seconds),
                None => s.push('\u{2014}'),
            }),
            RunUi::Seed => set_text(&mut t, scratch, |s| {
                let _ = write!(s, "Seed {}", summary.seed);
            }),
            RunUi::ResultsHint => set_text(&mut t, scratch, |s| {
                let key = tuning.bindings.name(crate::input::Action::Start);
                let _ = write!(s, "{key}  go again");
            }),
            _ => {}
        }
    }
}

/// Go again and Quit, clicked while the card is up (and the game isn't
/// paused over it).
fn results_buttons(
    buttons: Query<(&Interaction, &ResultsButton), Changed<Interaction>>,
    summary: Option<Res<RunSummary>>,
    state: Res<State<AppState>>,
    mut restart: MessageWriter<RestartRun>,
    mut next: ResMut<NextState<AppState>>,
) {
    let up = *state.get() == AppState::Playing
        && summary.is_some_and(|s| matches!(s.phase, RunPhase::Over { .. }));
    if !up {
        return;
    }
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match button {
            ResultsButton::GoAgain => {
                restart.write(RestartRun);
            }
            ResultsButton::Quit => {
                next.set(AppState::Menu);
            }
        }
    }
}

fn results_button_looks(
    mut buttons: Query<
        (
            &Interaction,
            &ResultsButton,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        Changed<Interaction>,
    >,
) {
    for (interaction, button, mut bg, mut border) in &mut buttons {
        let hovered = matches!(interaction, Interaction::Hovered | Interaction::Pressed);
        let (rest, hover) = if *button == ResultsButton::GoAgain {
            (BLUE, BLUE_HOVER)
        } else {
            (SLATE, SLATE_HOVER)
        };
        let fill = match (hovered, *interaction == Interaction::Pressed) {
            (_, true) => rest.darker(0.06),
            (true, false) => hover,
            _ => rest,
        };
        bg.set_if_neq(BackgroundColor(fill));
        border.set_if_neq(BorderColor::all(if hovered {
            GOLD_HOVER
        } else {
            GOLD_FRAME
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_burst_is_a_gold_star_with_an_ink_rim_and_clear_corners() {
        let size = 128;
        let px = burst_pixels(size);
        let at = |x: u32, y: u32| {
            let i = ((y * size + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        assert_eq!(at(0, 0)[3], 0, "corners are clear");
        let centre = at(64, 64);
        assert_eq!(centre[3], 255);
        assert!(
            centre[0] > 200 && centre[1] > 150 && centre[2] < 120,
            "{centre:?}"
        );
        // Walking out from the centre along a tip: gold, then ink, then clear.
        let mut saw_ink = false;
        for x in 64..128 {
            let p = at(x, 64);
            if p[3] > 200 && p[0] < 40 {
                saw_ink = true;
            }
        }
        assert!(saw_ink, "an ink rim");
    }

    #[test]
    fn the_numbers_count_up_then_land_exactly() {
        assert_eq!(counted(650, count_up(0.0)), 0);
        let mid = counted(650, count_up(COUNT_SECONDS * 0.3));
        assert!((100..650).contains(&mid), "{mid}");
        assert_eq!(counted(650, count_up(COUNT_SECONDS)), 650);
        assert_eq!(counted(650, count_up(30.0)), 650);
        let mut last = 0;
        for i in 0..=110 {
            let v = counted(9_999, count_up(i as f32 * 0.01));
            assert!(v >= last, "never counts down");
            last = v;
        }
        const { assert!(BURST_DELAY > COUNT_SECONDS, "NEW BEST! after the count") };
    }

    #[test]
    fn the_burst_pops_in_then_breathes() {
        assert_eq!(burst_motion(0.0).0, 0.0);
        let peak = (1..45)
            .map(|i| burst_motion(i as f32 * 0.01).0)
            .fold(0.0, f32::max);
        assert!(peak > 1.1, "overshoots: {peak}");
        let late = burst_motion(3.0).0;
        assert!((0.95..1.05).contains(&late), "{late}");
    }
}
