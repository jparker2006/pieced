//! The wave-start banner (M4 chunk 5, D110, target M4-V7): a big cartoon
//! "WAVE 7" in brass-and-crystal lettering on a golden sunburst, with a
//! ribbon under it reading "17 KNIGHTS INCOMING" (the wave's real size),
//! sweeps in over the island, holds for [`BANNER_SECONDS`], then shrinks
//! up into the HUD's wave counter, which pops as it lands.
//!
//! It starts on [`WaveStarted`], the message the adaptive score sends with
//! the round sting (`audio::music`), so the banner and the sting land on the
//! same frame. It stays in the top third of the screen: it never covers the
//! crosshair ([`covers_crosshair`] pins that, the spec allows 0.3 s).
//!
//! **Cost.** Every node is spawned once at startup; a banner only rewrites
//! two strings it already owns and one `UiTransform` per frame while it
//! shows. The lettering is layered text (a dark brass extrusion, four ink
//! copies with their shadows for the outline, a gold face and a lighter top
//! half through a clip), the sunburst one conic plus one radial gradient
//! (the UI's gradient pipeline, which the death vignette also uses). It is
//! drawn behind the loading screen during Boot with every digit, so its
//! pipelines and glyphs are ready before the first wave. UI pass only; about
//! 15 draws while it shows, none otherwise.

use super::{GOLD_FRAME, RunUi, set_text};
use crate::{
    audio::music::WaveStarted,
    hud::{
        UiArt, squash_pop,
        style::{INK, PANEL, TEXT, ink},
    },
    shared::AppState,
    tuning::Tuning,
};
use bevy::prelude::*;
use std::fmt::Write;

/// Seconds the banner is up over the island (sweep and hold), then the
/// seconds it takes to shrink into the HUD's wave counter.
pub const BANNER_SECONDS: f32 = 1.5;
pub const SHRINK_SECONDS: f32 = 0.4;
/// The sweep in (s).
pub const SWEEP_SECONDS: f32 = 0.35;
/// The whole banner, sweep to landing (s).
pub const BANNER_TOTAL: f32 = BANNER_SECONDS + SHRINK_SECONDS;
/// The banner's top, as a percentage of the screen height.
pub const BANNER_TOP_PCT: f32 = 11.0;
/// The block's size at rest (px): the sunburst's ellipse, which is its
/// biggest part. The letters and the ribbon sit inside it.
pub const BANNER_SIZE: Vec2 = Vec2::new(900.0, 330.0);
/// The HUD's wave counter's centre, px below the top of the screen (the
/// strip's top plus half its frame).
pub const COUNTER_Y: f32 = super::hud::STRIP_TOP + 32.0;
/// The lettering's font size (px).
pub const TITLE_SIZE: f32 = 132.0;
/// The ribbon's font size (px).
const LINE_SIZE: f32 = 34.0;
/// The outline's width (px) and the extrusion's depth.
const OUTLINE: f32 = 5.0;
const EXTRUDE: f32 = 8.0;
/// Rays in the sunburst.
const RAYS: usize = 18;
/// What the banner shows during Boot (every digit, so the glyphs are cached).
const WARM_TITLE: &str = "WAVE 0123456789";
const WARM_LINE: &str = "0123456789 KNIGHTS INCOMING";

/// Brass: the face's deep gold, its lit top half, the extrusion.
pub const BRASS: Color = Color::srgb(0.98, 0.66, 0.14);
pub const BRASS_LIGHT: Color = Color::srgb(1.0, 0.9, 0.46);
pub const BRASS_DEEP: Color = Color::srgb(0.55, 0.3, 0.07);

/// Where the ink copies sit round the face, in outline widths: (the copy's
/// offset, its shadow's offset from the copy); eight directions in all.
const INK_OFFSETS: [(Vec2, Vec2); 4] = [
    (Vec2::new(-0.72, -0.72), Vec2::new(1.44, 0.0)),
    (Vec2::new(-0.72, 0.72), Vec2::new(1.44, 0.0)),
    (Vec2::new(-1.0, 0.0), Vec2::new(2.0, 0.0)),
    (Vec2::new(0.0, -1.0), Vec2::new(0.0, 2.0)),
];

pub(super) fn build(app: &mut App) {
    app.init_resource::<WaveBanner>()
        .add_message::<WaveStarted>()
        .add_systems(Startup, spawn_banner)
        .add_systems(
            PostUpdate,
            drive_banner.before(bevy::ui::UiSystems::Layout),
        );
}

/// The banner's state: which wave, its real-time age, and the knights it
/// announces. Public so tests (and the session log) can read it.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct WaveBanner {
    /// The wave being announced and the seconds since its [`WaveStarted`].
    pub showing: Option<(u32, f32)>,
    pub knights: u32,
    /// Seconds since the banner landed in the counter (the counter's pop).
    pub landed: Option<f32>,
    /// Banners started so far.
    pub shown: u32,
}

impl WaveBanner {
    /// Whether the banner is on screen.
    pub fn visible(&self) -> bool {
        self.showing.is_some()
    }
}

/// Where the banner is `t` seconds after its wave started: its offset from
/// its resting place (px), its scale, its turn (radians), and the ribbon's
/// own scale. `None` once it has landed in the counter. `screen_h` is the
/// screen's height (logical px): the counter's offset depends on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BannerPose {
    pub offset: Vec2,
    pub scale: f32,
    pub turn: f32,
    pub line: f32,
}

fn ease_out_back(x: f32) -> f32 {
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    let x = x.clamp(0.0, 1.0) - 1.0;
    1.0 + c3 * x * x * x + c1 * x * x
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// The banner's resting centre (px from the top-left) on a `screen` sized
/// screen.
pub fn banner_centre(screen: Vec2) -> Vec2 {
    Vec2::new(
        screen.x / 2.0,
        screen.y * BANNER_TOP_PCT / 100.0 + BANNER_SIZE.y / 2.0,
    )
}

/// The banner's motion (see [`BannerPose`]).
pub fn banner_pose(t: f32, screen_h: f32) -> Option<BannerPose> {
    if !(0.0..BANNER_TOTAL).contains(&t) {
        return None;
    }
    // Sweeps in from the upper left, turning, and slams to a stop.
    let sweep = ease_out_back(t / SWEEP_SECONDS);
    let from = Vec2::new(-520.0, -200.0);
    let mut offset = from * (1.0 - sweep);
    let mut scale = 0.45 + 0.55 * sweep;
    let mut turn = -0.16 * (1.0 - sweep);
    // Then breathes while it holds.
    if t > SWEEP_SECONDS {
        scale *= 1.0 + 0.018 * ((t - SWEEP_SECONDS) * 9.0).sin();
    }
    // The ribbon stamps in just after the letters.
    let line_t = ((t - 0.18) / 0.26).clamp(0.0, 1.0);
    let mut line = if line_t <= 0.0 {
        0.0
    } else {
        ease_out_back(line_t)
    };
    // Then it shrinks up into the HUD's wave counter.
    if t >= BANNER_SECONDS {
        let k = smooth((t - BANNER_SECONDS) / SHRINK_SECONDS);
        let centre_y = screen_h * BANNER_TOP_PCT / 100.0 + BANNER_SIZE.y / 2.0;
        offset = offset.lerp(Vec2::new(0.0, COUNTER_Y - centre_y), k);
        scale *= 1.0 - 0.9 * k;
        turn = 0.0;
        line *= 1.0 - smooth(k * 2.5);
    }
    Some(BannerPose {
        offset,
        scale,
        turn,
        line,
    })
}

/// Whether the banner covers the crosshair (the screen's centre) `t`
/// seconds in, on a `screen` sized screen (logical px). The spec allows
/// 0.3 s; it never does.
pub fn covers_crosshair(t: f32, screen: Vec2) -> bool {
    let Some(pose) = banner_pose(t, screen.y) else {
        return false;
    };
    // Its bounding box, turned or not (a turn only grows the box a little:
    // take the circumscribed square's growth to be safe).
    let grow = 1.0 + pose.turn.abs();
    let half = BANNER_SIZE / 2.0 * pose.scale * grow;
    let centre = banner_centre(screen) + pose.offset;
    let crosshair = screen / 2.0;
    (crosshair - centre).abs().cmple(half).all()
}

/// Writes the banner's title, "WAVE 7".
pub fn write_title(out: &mut String, wave: u32) {
    let _ = write!(out, "WAVE {wave}");
}

/// Writes the ribbon, "17 KNIGHTS INCOMING" ("1 KNIGHT INCOMING").
pub fn write_line(out: &mut String, knights: u32) {
    let _ = write!(
        out,
        "{knights} {} INCOMING",
        if knights == 1 { "KNIGHT" } else { "KNIGHTS" }
    );
}

/// A lettering layer: the title text in `color` at the face's place plus
/// `offset` (absolute, so every layer lays out exactly over the face).
fn layer(color: Color, offset: Vec2, shadow: Option<Vec2>) -> impl Bundle {
    (
        RunUi::WaveBannerTitle,
        Text::new(WARM_TITLE),
        TextFont::from_font_size(TITLE_SIZE),
        TextColor(color),
        TextLayout::no_wrap(),
        TextShadow {
            offset: shadow.unwrap_or(Vec2::ZERO),
            color: if shadow.is_some() { INK } else { Color::NONE },
        },
        Node {
            position_type: PositionType::Absolute,
            left: px(offset.x),
            top: px(offset.y),
            ..default()
        },
    )
}

/// The sunburst behind the letters: gold rays and a warm glow in an
/// ellipse.
fn sunburst() -> BackgroundGradient {
    let mut stops = Vec::with_capacity(RAYS * 4);
    let seg = std::f32::consts::TAU / (RAYS * 2) as f32;
    let ray = Color::srgba(1.0, 0.84, 0.36, 0.42);
    for i in 0..RAYS {
        let a = (2 * i) as f32 * seg;
        stops.push(AngularColorStop::new(ray, a));
        stops.push(AngularColorStop::new(ray, a + seg));
        stops.push(AngularColorStop::new(Color::NONE, a + seg));
        stops.push(AngularColorStop::new(Color::NONE, a + 2.0 * seg));
    }
    let glow = RadialGradient::new(
        UiPosition::CENTER,
        RadialGradientShape::FarthestSide,
        vec![
            ColorStop::percent(Color::srgba(1.0, 0.86, 0.45, 0.62), 0.0),
            ColorStop::percent(Color::srgba(1.0, 0.72, 0.25, 0.28), 45.0),
            ColorStop::percent(Color::srgba(1.0, 0.7, 0.2, 0.0), 100.0),
        ],
    );
    BackgroundGradient(vec![
        ConicGradient::new(UiPosition::CENTER, stops).into(),
        glow.into(),
    ])
}

fn spawn_banner(mut commands: Commands, art: Option<Res<UiArt>>) {
    let art = art.map(|a| a.clone()).unwrap_or_default();
    commands
        .spawn((
            Name::new("Wave banner"),
            RunUi::WaveBanner,
            Node {
                position_type: PositionType::Absolute,
                top: percent(BANNER_TOP_PCT),
                width: percent(100),
                height: px(BANNER_SIZE.y),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            // Drawn behind the loading screen during Boot (see the module
            // docs); hidden from then on until a wave starts.
            Visibility::Inherited,
            GlobalZIndex(21),
        ))
        .with_children(|root| {
            root.spawn((
                RunUi::WaveBannerPivot,
                Node {
                    width: px(BANNER_SIZE.x),
                    height: px(BANNER_SIZE.y),
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    row_gap: px(2),
                    ..default()
                },
                UiTransform::default(),
            ))
            .with_children(|pivot| {
                // The sunburst, an ellipse filling the block.
                pivot.spawn((
                    RunUi::WaveBannerBurst,
                    Node {
                        position_type: PositionType::Absolute,
                        width: percent(100),
                        height: percent(100),
                        border_radius: BorderRadius::all(percent(50)),
                        ..default()
                    },
                    sunburst(),
                ));
                // The letters, flanked by crystals.
                pivot
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: px(10),
                        ..default()
                    })
                    .with_children(|row| {
                        let gem = |size: f32, turn: f32| {
                            (
                                ImageNode::new(art.crystal_blue.clone()),
                                Node {
                                    width: px(size),
                                    height: px(size),
                                    ..default()
                                },
                                UiTransform::from_rotation(Rot2::radians(turn)),
                            )
                        };
                        row.spawn(gem(74.0, 0.35));
                        row.spawn(Node::default()).with_children(|letters| {
                            // The extrusion, the outline (its copies' shadows
                            // fill the other four directions), then the face in
                            // flow so it sizes the box, and its lit top half.
                            letters.spawn(layer(
                                INK,
                                Vec2::new(0.0, EXTRUDE + OUTLINE * 0.8),
                                None,
                            ));
                            letters.spawn(layer(BRASS_DEEP, Vec2::new(0.0, EXTRUDE), None));
                            for (at, shadow) in INK_OFFSETS {
                                letters.spawn(layer(INK, at * OUTLINE, Some(shadow * OUTLINE)));
                            }
                            letters.spawn((
                                RunUi::WaveBannerFace,
                                Text::new(WARM_TITLE),
                                TextFont::from_font_size(TITLE_SIZE),
                                TextColor(BRASS),
                                TextLayout::no_wrap(),
                            ));
                            letters
                                .spawn(Node {
                                    position_type: PositionType::Absolute,
                                    left: px(0),
                                    top: px(0),
                                    right: px(0),
                                    height: percent(50),
                                    overflow: Overflow::clip(),
                                    ..default()
                                })
                                .with_child(layer(BRASS_LIGHT, Vec2::ZERO, None));
                        });
                        row.spawn(gem(74.0, -0.35));
                    });
                // The ribbon: "17 KNIGHTS INCOMING" between two small gems.
                pivot
                    .spawn((
                        RunUi::WaveBannerLine,
                        Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            column_gap: px(12),
                            padding: UiRect::new(px(22), px(22), px(4), px(8)),
                            margin: UiRect::top(px(-8)),
                            border: UiRect::all(px(4)),
                            border_radius: BorderRadius::all(px(18)),
                            ..default()
                        },
                        BackgroundColor(PANEL.with_alpha(0.94)),
                        BorderColor::all(GOLD_FRAME),
                        ink(3.0),
                        UiTransform::default(),
                    ))
                    .with_children(|ribbon| {
                        let small = || {
                            (
                                ImageNode::new(art.crystal_blue.clone()),
                                Node {
                                    width: px(26),
                                    height: px(26),
                                    ..default()
                                },
                            )
                        };
                        ribbon.spawn(small());
                        ribbon.spawn((
                            RunUi::WaveBannerKnights,
                            Text::new(WARM_LINE),
                            TextFont::from_font_size(LINE_SIZE),
                            TextColor(TEXT),
                            TextShadow {
                                offset: Vec2::new(0.0, 3.0),
                                color: INK,
                            },
                            TextLayout::no_wrap(),
                        ));
                        ribbon.spawn(small());
                    });
            });
        });
}

/// Starts the banner on [`WaveStarted`] (the round sting's frame), moves it,
/// and pops the HUD's wave counter as it lands.
#[allow(clippy::type_complexity)]
fn drive_banner(
    time: Res<Time<Real>>,
    tuning: Res<Tuning>,
    state: Res<State<AppState>>,
    windows: Query<&Window>,
    mut started: MessageReader<WaveStarted>,
    mut banner: ResMut<WaveBanner>,
    mut scratch: Local<String>,
    mut roots: Query<(&RunUi, &mut Visibility)>,
    mut texts: Query<(&RunUi, &mut Text)>,
    mut moved: Query<(&RunUi, &mut UiTransform)>,
) {
    let dt = time.delta_secs();
    let booting = *state.get() == AppState::Boot;
    let mut fresh = false;
    for start in started.read() {
        banner.showing = Some((start.wave, 0.0));
        banner.knights = tuning.waves.wave_size(start.wave);
        banner.landed = None;
        banner.shown += 1;
        fresh = true;
    }
    // The menus leave no banner hanging over them.
    if matches!(state.get(), AppState::Menu) {
        banner.showing = None;
    }
    if let Some((wave, age)) = banner.showing
        && !fresh
    {
        let age = age + dt;
        if age >= BANNER_TOTAL {
            banner.showing = None;
            banner.landed = Some(0.0);
        } else {
            banner.showing = Some((wave, age));
        }
    } else if let Some(landed) = banner.landed.as_mut() {
        *landed += dt;
        if *landed > 1.0 {
            banner.landed = None;
        }
    }
    let visible = banner.visible() || booting;
    for (part, mut v) in &mut roots {
        if *part == RunUi::WaveBanner {
            v.set_if_neq(if visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    }
    if fresh && let Some((wave, _)) = banner.showing {
        for (part, mut text) in &mut texts {
            match part {
                RunUi::WaveBannerTitle | RunUi::WaveBannerFace => set_text(&mut text, &mut scratch, |s| {
                    write_title(s, wave);
                }),
                RunUi::WaveBannerKnights => set_text(&mut text, &mut scratch, |s| {
                    write_line(s, banner.knights);
                }),
                _ => {}
            }
        }
    }
    let screen_h = windows.iter().next().map_or(800.0, Window::height);
    let pose = banner
        .showing
        .and_then(|(_, age)| banner_pose(age, screen_h));
    let pop = banner.landed.map_or(Vec2::ONE, |t| {
        squash_pop(t) * super::hud::pop_scale(t, 0.3, 0.35)
    });
    for (part, mut tf) in &mut moved {
        let want = match (part, pose) {
            (RunUi::WaveBannerPivot, Some(p)) => UiTransform {
                translation: Val2::px(p.offset.x, p.offset.y),
                scale: Vec2::splat(p.scale.max(0.0)),
                rotation: Rot2::radians(p.turn),
            },
            (RunUi::WaveBannerLine, Some(p)) => UiTransform {
                scale: Vec2::splat(p.line.max(0.0)),
                ..UiTransform::IDENTITY
            },
            (RunUi::WaveFrame, _) => UiTransform {
                scale: pop,
                ..UiTransform::IDENTITY
            },
            _ => continue,
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
    fn the_lines_read_like_the_board() {
        let mut s = String::new();
        write_title(&mut s, 7);
        assert_eq!(s, "WAVE 7");
        s.clear();
        write_line(&mut s, 17);
        assert_eq!(s, "17 KNIGHTS INCOMING");
        s.clear();
        write_line(&mut s, 1);
        assert_eq!(s, "1 KNIGHT INCOMING");
    }

    #[test]
    fn it_sweeps_in_holds_then_shrinks_into_the_counter() {
        let h = 900.0;
        let start = banner_pose(0.0, h).unwrap();
        assert!(start.offset.length() > 300.0 && start.scale < 0.6);
        let rest = banner_pose(1.0, h).unwrap();
        assert!(rest.offset.length() < 1.0, "at rest: {rest:?}");
        assert!((rest.scale - 1.0).abs() < 0.03);
        assert!((rest.line - 1.0).abs() < 1e-3);
        let late = banner_pose(BANNER_TOTAL - 1e-3, h).unwrap();
        assert!(late.scale < 0.15, "shrunk: {late:?}");
        let centre = banner_centre(Vec2::new(1400.0, h));
        assert!((centre.y + late.offset.y - COUNTER_Y).abs() < 2.0);
        assert!(banner_pose(BANNER_TOTAL, h).is_none());
    }
}
