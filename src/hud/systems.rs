//! HUD update systems: status (bars, ammo, hotbar, piece HP, readout, perf),
//! the crosshair, hit feedback, and damage-number placement.
//!
//! Screen sizes come from the UI camera's viewport (the window in the game, the
//! capture image offscreen). The hitmarker and damage numbers animate on
//! [`FreezableTime`], so a gallery freeze holds them still; they still appear
//! on the frame the hit registers.

use super::{
    FrameStartTick, HitFeedbackStats, MarkerKind, NumberKind, TrailBar,
    art::UiArt,
    damage_label,
    layout::{DamageNumber, El, INK_LAYERS, INK_OFFSETS, NUMBER_BOX, NumberGlyph, TICK_LEN},
    number_motion, project_to_screen, spread_to_pixels,
    style::{ACCENT, DANGER, INK, RIM, SLOT, TEXT, dim},
};
use crate::{
    building::{AimedPiece, EditMode},
    combat::{CombatStats, Loadout},
    fx::kills::{KillConfirmed, KillFeedbackSet, KillFeedbackStats},
    menu::MenuState,
    palette,
    render::{CameraFollowSet, CurrentFov, MainCamera},
    shared::{
        ActiveTool, Ads, DamageDealt, DamageTarget, FreezableTime, Health, PieceKind, Player,
        WeaponKind,
    },
    telemetry::FrameStats,
    tuning::Tuning,
    viewmodel::{CrystalGlow, ViewmodelSet},
};
use bevy::{prelude::*, text::FontSize, ui::UiSystems, window::PrimaryWindow};

pub(super) fn build(app: &mut App) {
    app.init_resource::<Hitmarker>()
        .add_systems(
            Update,
            (
                toggle_perf_overlay,
                update_status,
                update_crosshair,
                (hit_feedback, draw_hitmarker)
                    .chain()
                    .after(KillFeedbackSet),
            ),
        )
        .add_systems(
            PostUpdate,
            // Projected with the camera as it renders: after the eye follow,
            // the shake and the gallery's framing (all before ViewmodelSet).
            place_damage_numbers
                .after(CameraFollowSet)
                .after(ViewmodelSet)
                .before(UiSystems::Prepare),
        );
}

// ---------------------------------------------------------------------------
// Small change-aware setters (so unchanged UI never re-lays out)
// ---------------------------------------------------------------------------

fn set_text(text: &mut Mut<Text>, value: &str) {
    if text.0 != value {
        text.0 = value.to_string();
    }
}

fn set_color(color: &mut Mut<TextColor>, value: Color) {
    if color.0 != value {
        color.0 = value;
    }
}

fn set_bg(bg: &mut Mut<BackgroundColor>, value: Color) {
    if bg.0 != value {
        bg.0 = value;
    }
}

fn set_width_pct(node: &mut Mut<Node>, fraction: f32) {
    let v = percent((fraction.clamp(0.0, 1.0) * 1000.0).round() / 10.0);
    if node.width != v {
        node.width = v;
    }
}

fn set_visible(vis: &mut Mut<Visibility>, show: bool) {
    let v = if show {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if **vis != v {
        **vis = v;
    }
}

fn slot_of(tool: ActiveTool) -> u8 {
    match tool {
        ActiveTool::Weapon(WeaponKind::Rifle) => 0,
        ActiveTool::Weapon(WeaponKind::Pump) => 1,
        ActiveTool::Build(PieceKind::Wall) => 2,
        ActiveTool::Build(PieceKind::Ramp) => 3,
        ActiveTool::Build(PieceKind::Floor) => 4,
        ActiveTool::Build(PieceKind::Cone) => 5,
    }
}

fn piece_name(kind: PieceKind) -> &'static str {
    match kind {
        PieceKind::Wall => "WALL",
        PieceKind::Ramp => "RAMP",
        PieceKind::Floor => "FLOOR",
        PieceKind::Cone => "CONE",
    }
}

/// The HUD's size in logical pixels: the UI camera's viewport (the window, or
/// the image it draws into offscreen), else the primary window.
pub(super) fn screen_size(
    ui: &Query<&Camera, With<IsDefaultUiCamera>>,
    window: &Query<&Window, With<PrimaryWindow>>,
) -> Option<Vec2> {
    ui.iter()
        .find_map(Camera::logical_viewport_size)
        .or_else(|| {
            window
                .iter()
                .next()
                .map(|w| Vec2::new(w.width(), w.height()))
        })
        .filter(|s| s.x > 1.0 && s.y > 1.0)
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

fn toggle_perf_overlay(keys: Res<ButtonInput<KeyCode>>, mut tuning: ResMut<Tuning>) {
    if keys.just_pressed(KeyCode::F3) {
        tuning.hud.perf_overlay = !tuning.hud.perf_overlay;
    }
}

#[derive(Default)]
struct StatusMemory {
    shield: Option<TrailBar>,
    health: Option<TrailBar>,
    perf_timer: f32,
    perf: Option<(String, String, String, Color)>,
}

type StatusParts<'a> = (
    &'a El,
    Option<&'a mut Node>,
    Option<&'a mut Text>,
    Option<&'a mut TextColor>,
    Option<&'a mut BackgroundColor>,
    Option<&'a mut BorderColor>,
    Option<&'a mut Visibility>,
    Option<&'a mut UiTransform>,
    Option<&'a mut ImageNode>,
);

/// The ammo crystal's tint and scale for a glow (0.25 empty .. 1 full, above 1
/// while a fresh crystal charges): it dims as the magazine empties and swells
/// in the reload flash, like the crystal on the gun.
pub fn ammo_crystal_look(glow: f32) -> (f32, f32) {
    let brightness = 0.4 + 0.6 * glow.clamp(0.0, 1.0);
    let scale = 1.0 + 0.5 * (glow - 1.0).clamp(0.0, 0.6);
    (
        (brightness * 100.0).round() / 100.0,
        (scale * 100.0).round() / 100.0,
    )
}

fn update_status(
    time: Res<Time<Real>>,
    tuning: Res<Tuning>,
    stats: Res<CombatStats>,
    frame: Option<Res<FrameStats>>,
    menu: Option<Res<MenuState>>,
    glow: Option<Res<CrystalGlow>>,
    art: Option<Res<UiArt>>,
    player: Option<
        Single<
            (
                &Health,
                &ActiveTool,
                Option<&Loadout>,
                Option<&AimedPiece>,
                Option<&EditMode>,
            ),
            With<Player>,
        >,
    >,
    mut memory: Local<StatusMemory>,
    mut parts: Query<StatusParts>,
    callouts: Query<&super::kills::KillCallout>,
) {
    let Some(player) = player else {
        return;
    };
    let (health, tool, loadout, aimed, edit) = player.into_inner();
    // A multi-kill callout owns the space by the crosshair: the aimed piece's
    // name and bar step aside while it shows (M4 art, V4).
    let calling = callouts.iter().any(|c| c.callout.is_some());
    let editing = edit.is_some_and(EditMode::is_editing);
    let hud = &tuning.hud;
    let dt = time.delta_secs();

    // Bars and their trailing chunks.
    let shield = if health.max_shield > 0.0 {
        health.shield / health.max_shield
    } else {
        0.0
    };
    let hp = if health.max_hp > 0.0 {
        health.hp / health.max_hp
    } else {
        0.0
    };
    let shield_trail = memory.shield.get_or_insert(TrailBar::new(shield));
    shield_trail.update(shield, dt, hud.bar_trail_hold, hud.bar_trail_rate);
    let shield_trail = shield_trail.shown;
    let health_trail = memory.health.get_or_insert(TrailBar::new(hp));
    health_trail.update(hp, dt, hud.bar_trail_hold, hud.bar_trail_rate);
    let health_trail = health_trail.shown;

    // Ammo.
    let build = tool.is_build();
    let (label, ammo, max, ammo_color, reload) = match (*tool, loadout) {
        (ActiveTool::Build(kind), _) => (
            "BUILD",
            piece_name(kind).to_string(),
            String::new(),
            TEXT,
            None,
        ),
        (ActiveTool::Weapon(kind), Some(loadout)) => {
            let gun = loadout.gun(kind);
            let gt = tuning.combat.gun(kind);
            let reload = gun.reload_progress(gt).map(|p| {
                if gt.reload_per_shell {
                    ((gun.ammo as f32 + p) / gt.magazine.max(1) as f32).min(1.0)
                } else {
                    p
                }
            });
            let color = if gun.ammo == 0 {
                DANGER
            } else if gun.ammo * 4 <= gt.magazine {
                palette::HEADSHOT
            } else {
                TEXT
            };
            let label = match kind {
                WeaponKind::Rifle => "SCAR",
                WeaponKind::Pump => "PUMP",
            };
            (
                label,
                gun.ammo.to_string(),
                format!("/ {}", gt.magazine),
                color,
                reload,
            )
        }
        (ActiveTool::Weapon(_), None) => ("", String::new(), String::new(), TEXT, None),
    };
    let active_slot = slot_of(*tool);
    let crystal = match *tool {
        ActiveTool::Weapon(kind) => {
            let image = art.as_ref().map(|a| match kind {
                WeaponKind::Rifle => a.crystal_blue.clone(),
                WeaponKind::Pump => a.crystal_violet.clone(),
            });
            let look = ammo_crystal_look(glow.as_ref().map_or(1.0, |g| g.of(kind)));
            Some((image, look))
        }
        ActiveTool::Build(_) => None,
    };

    // Readout.
    let readout = [
        stats
            .last_ttk
            .map(|t| format!("{t:.2}s"))
            .unwrap_or_else(|| "-".into()),
        if stats.shots == 0 {
            "-".into()
        } else {
            format!("{:.0}%", stats.accuracy() * 100.0)
        },
        if stats.hits == 0 {
            "-".into()
        } else {
            format!("{:.0}%", stats.headshot_rate() * 100.0)
        },
        stats.eliminations.to_string(),
    ];

    // Performance overlay text, refreshed a few times a second so it's readable.
    memory.perf_timer -= dt;
    if hud.perf_overlay && (memory.perf_timer <= 0.0 || memory.perf.is_none()) {
        memory.perf_timer = 0.25;
        if let Some(frame) = frame.as_ref() {
            let avg = if frame.fps > 0.0 {
                1000.0 / frame.fps
            } else {
                0.0
            };
            let worst_color = if frame.worst_ms < 18.0 {
                palette::HEALTH
            } else if frame.worst_ms < 25.0 {
                palette::HEADSHOT
            } else {
                DANGER
            };
            memory.perf = Some((
                format!("{:.0} FPS", frame.fps),
                format!("{avg:.1} ms"),
                format!("worst {:.1} ms", frame.worst_ms),
                worst_color,
            ));
        }
    }
    let perf = memory.perf.clone();
    let menu_up = menu.is_some_and(|m| m.menu_visible());

    for (el, node, text, color, bg, border, vis, transform, image) in &mut parts {
        match *el {
            El::Root | El::Crosshair | El::Numbers => {
                // The pause menu stands alone over the world (T12).
                if let Some(mut v) = vis {
                    set_visible(&mut v, !menu_up);
                }
            }
            El::ShieldFill => {
                if let Some(mut n) = node {
                    set_width_pct(&mut n, shield);
                }
            }
            El::ShieldTrail => {
                if let Some(mut n) = node {
                    set_width_pct(&mut n, shield_trail);
                }
            }
            El::ShieldValue => {
                if let Some(mut t) = text {
                    set_text(&mut t, &format!("{}", health.shield.ceil() as i32));
                }
            }
            El::HealthFill => {
                if let Some(mut n) = node {
                    set_width_pct(&mut n, hp);
                }
            }
            El::HealthTrail => {
                if let Some(mut n) = node {
                    set_width_pct(&mut n, health_trail);
                }
            }
            El::HealthValue => {
                if let Some(mut t) = text {
                    set_text(&mut t, &format!("{}", health.hp.ceil() as i32));
                }
                if let Some(mut c) = color {
                    set_color(&mut c, if hp <= 0.25 { DANGER } else { TEXT });
                }
            }
            El::WeaponLabel => {
                if let Some(mut t) = text {
                    set_text(&mut t, label);
                }
                if let Some(mut c) = color {
                    set_color(&mut c, if build { ACCENT } else { dim(0.9) });
                }
            }
            El::AmmoCrystal => {
                if let Some(mut v) = vis {
                    set_visible(&mut v, crystal.is_some());
                }
                if let Some((wanted, (brightness, scale))) = &crystal {
                    if let (Some(mut img), Some(wanted)) = (image, wanted.as_ref()) {
                        if img.image != *wanted {
                            img.image = wanted.clone();
                        }
                        let tint = Color::srgb(*brightness, *brightness, *brightness);
                        if img.color != tint {
                            img.color = tint;
                        }
                    }
                    if let Some(mut t) = transform {
                        let s = Vec2::splat(*scale);
                        if t.scale != s {
                            t.scale = s;
                        }
                    }
                }
            }
            El::AmmoValue => {
                if let Some(mut t) = text {
                    set_text(&mut t, &ammo);
                }
                if let Some(mut c) = color {
                    set_color(&mut c, ammo_color);
                }
            }
            El::AmmoMax => {
                if let Some(mut t) = text {
                    set_text(&mut t, &max);
                }
            }
            El::ReloadRow => {
                if let Some(mut v) = vis {
                    set_visible(&mut v, reload.is_some());
                }
            }
            El::ReloadFill => {
                if let Some(mut n) = node {
                    set_width_pct(&mut n, reload.unwrap_or(0.0));
                }
            }
            El::Slot(i) => {
                let on = i == active_slot;
                if let Some(mut b) = border {
                    let c = if on { ACCENT } else { RIM };
                    if b.top != c {
                        *b = BorderColor::all(c);
                    }
                }
                if let Some(mut b) = bg {
                    set_bg(
                        &mut b,
                        if on {
                            Color::srgba(0.16, 0.27, 0.26, 0.92)
                        } else {
                            SLOT
                        },
                    );
                }
                if let Some(mut t) = transform {
                    let y = if on { -5.0 } else { 0.0 };
                    let v = Val2::px(0.0, y);
                    if t.translation != v {
                        t.translation = v;
                    }
                }
            }
            El::SlotIcon(i) => {
                if let Some(mut img) = image {
                    let c = if i == active_slot {
                        Color::WHITE
                    } else {
                        Color::srgba(0.8, 0.8, 0.84, 0.92)
                    };
                    if img.color != c {
                        img.color = c;
                    }
                }
            }
            El::BuildBadge => {
                if let Some(mut v) = vis {
                    set_visible(&mut v, build || editing);
                }
            }
            El::BuildBadgeText => {
                if let Some(mut t) = text {
                    set_text(&mut t, if editing { "EDIT MODE" } else { "BUILD MODE" });
                }
            }
            El::PieceGroup => {
                if let Some(mut v) = vis {
                    set_visible(&mut v, !calling && aimed.is_some_and(|a| a.0.is_some()));
                }
            }
            El::PieceFill => {
                if let (Some(info), Some(mut n)) = (aimed.and_then(|a| a.0), node) {
                    set_width_pct(&mut n, info.hp / info.max_hp.max(1.0));
                    if let Some(mut b) = bg {
                        let c = match info.crack_stage {
                            0 => ACCENT,
                            1 => Color::srgb(1.0, 0.6, 0.25),
                            _ => DANGER,
                        };
                        set_bg(&mut b, c);
                    }
                }
            }
            El::PieceName => {
                if let (Some(info), Some(mut t)) = (aimed.and_then(|a| a.0), text) {
                    set_text(&mut t, piece_name(info.kind));
                }
            }
            El::PieceValue => {
                if let (Some(info), Some(mut t)) = (aimed.and_then(|a| a.0), text) {
                    set_text(
                        &mut t,
                        &format!("{} / {}", info.hp.ceil() as i32, info.max_hp.round() as i32),
                    );
                }
            }
            El::Readout => {
                if let Some(mut v) = vis {
                    set_visible(&mut v, hud.combat_readout);
                }
            }
            El::ReadoutValue(i) => {
                if let Some(mut t) = text {
                    set_text(&mut t, &readout[i as usize]);
                }
            }
            El::Perf => {
                if let Some(mut v) = vis {
                    set_visible(&mut v, hud.perf_overlay);
                }
            }
            El::PerfFps | El::PerfMs | El::PerfWorst => {
                if let (Some(perf), Some(mut t)) = (perf.as_ref(), text) {
                    let value = match *el {
                        El::PerfFps => &perf.0,
                        El::PerfMs => &perf.1,
                        _ => &perf.2,
                    };
                    set_text(&mut t, value);
                    if *el == El::PerfWorst
                        && let Some(mut c) = color
                    {
                        set_color(&mut c, perf.3);
                    }
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Crosshair
// ---------------------------------------------------------------------------

fn update_crosshair(
    tuning: Res<Tuning>,
    fov: Res<CurrentFov>,
    ui: Query<&Camera, With<IsDefaultUiCamera>>,
    window: Query<&Window, With<PrimaryWindow>>,
    player: Option<Single<(&ActiveTool, &Ads, Option<&Loadout>), With<Player>>>,
    mut parts: Query<(&El, &mut Node, &mut Visibility, Option<&mut UiTransform>)>,
) {
    let (Some(screen), Some(player)) = (screen_size(&ui, &window), player) else {
        return;
    };
    let (tool, ads, loadout) = player.into_inner();
    let hud = &tuning.hud;
    let height = screen.y;
    let scale = hud.crosshair_scale.clamp(0.5, 3.0);
    let px_of = |deg: f32| spread_to_pixels(deg, fov.0, height);

    let (mode, gap, ring) = match *tool {
        ActiveTool::Build(_) => (2u8, 0.0, 0.0),
        ActiveTool::Weapon(WeaponKind::Rifle) => {
            let rifle = &tuning.combat.rifle;
            let spread = match (hud.show_bloom, loadout) {
                (true, Some(l)) => l.rifle.spread_deg(rifle, ads.0),
                _ => {
                    rifle.base_spread_deg
                        * if ads.0 {
                            rifle.ads_spread_multiplier
                        } else {
                            1.0
                        }
                }
            };
            (0, hud.crosshair_min_gap * scale + px_of(spread), 0.0)
        }
        ActiveTool::Weapon(WeaponKind::Pump) => {
            let pump = &tuning.combat.pump;
            let radius = pump.pellet_spread_deg
                * if ads.0 {
                    pump.ads_spread_multiplier
                } else {
                    1.0
                };
            (1, 0.0, px_of(radius).max(6.0))
        }
    };

    for (el, mut node, mut vis, transform) in &mut parts {
        match *el {
            El::Tick(i) => {
                set_visible(&mut vis, mode == 0);
                if let Some(mut t) = transform {
                    let dir = match i {
                        0 => Vec2::new(0.0, -1.0),
                        1 => Vec2::new(1.0, 0.0),
                        2 => Vec2::new(0.0, 1.0),
                        _ => Vec2::new(-1.0, 0.0),
                    };
                    let offset = dir * (gap + TICK_LEN * scale * 0.5);
                    // Whole pixels keep the ticks crisp.
                    let v = Val2::px(offset.x.round(), offset.y.round());
                    if t.translation != v {
                        t.translation = v;
                    }
                    let s = Vec2::splat(scale);
                    if t.scale != s {
                        t.scale = s;
                    }
                }
            }
            El::Dot => {
                set_visible(&mut vis, true);
            }
            El::PumpRing => {
                set_visible(&mut vis, mode == 1);
                if mode == 1 {
                    let d = (ring * 2.0).round();
                    if node.width != px(d) {
                        node.width = px(d);
                        node.height = px(d);
                        node.left = px(-d / 2.0);
                        node.top = px(-d / 2.0);
                    }
                }
            }
            El::BuildReticle => {
                set_visible(&mut vis, mode == 2);
                if let Some(mut t) = transform {
                    let s = Vec2::splat(scale);
                    if t.scale != s {
                        t.scale = s;
                    }
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Hit feedback
// ---------------------------------------------------------------------------

/// The current hitmarker (one at a time; stronger kinds take over).
#[derive(Resource, Debug, Default)]
struct Hitmarker {
    kind: Option<MarkerKind>,
    age: f32,
    /// A kill's big X is gold: the kill was a headshot (M4).
    gold: bool,
}

/// The layers of one pooled number's glyphs, for restyling on a hit.
type GlyphParts<'a> = (
    &'a NumberGlyph,
    &'a mut Text,
    &'a mut TextFont,
    &'a mut TextColor,
    &'a mut TextShadow,
    &'a mut UiTransform,
);

/// Restyles a pooled number's glyph layers for `kind` and `label`: the face in
/// the kind's color with an ink drop, the ink copies round it.
fn style_glyphs(
    children: &Children,
    glyphs: &mut Query<GlyphParts, Without<DamageNumber>>,
    kind: NumberKind,
    label: &str,
) {
    let w = kind.outline();
    for child in children.iter() {
        let Ok((layer, mut text, mut font, mut color, mut shadow, mut transform)) =
            glyphs.get_mut(child)
        else {
            continue;
        };
        if text.0 != label {
            text.0 = label.to_string();
        }
        let size = FontSize::Px(kind.size());
        if !matches!(font.font_size, FontSize::Px(s) if s == kind.size()) {
            font.font_size = size;
        }
        let (at, shade, fill) = if layer.0 >= INK_LAYERS {
            (
                Vec2::ZERO,
                TextShadow {
                    offset: Vec2::new(0.4, 1.3) * w,
                    color: kind.ink(),
                },
                kind.color(),
            )
        } else {
            let (at, shadow) = INK_OFFSETS[layer.0 as usize];
            (
                at * w,
                TextShadow {
                    offset: shadow * w,
                    color: kind.ink(),
                },
                kind.ink(),
            )
        };
        color.0 = fill;
        *shadow = shade;
        transform.translation = Val2::px(at.x, at.y);
    }
}

fn hit_feedback(
    time: FreezableTime,
    tuning: Res<Tuning>,
    start: Res<FrameStartTick>,
    player: Option<Single<Entity, With<Player>>>,
    mut damage: MessageReader<DamageDealt>,
    mut marker: ResMut<Hitmarker>,
    mut stats: ResMut<HitFeedbackStats>,
    mut numbers: Query<(Entity, &mut DamageNumber, &mut Visibility, &Children)>,
    mut glyphs: Query<GlyphParts, Without<DamageNumber>>,
    mut counter: Local<u32>,
    mut kills: MessageReader<KillConfirmed>,
    mut kill_stats: Option<ResMut<KillFeedbackStats>>,
) {
    let player = player.map(|p| *p);
    marker.age += time.delta_secs();
    // Every confirmed kill (a void knock-off too) shows the big X (M4).
    for kill in kills.read() {
        marker.kind = Some(MarkerKind::Kill);
        marker.age = 0.0;
        marker.gold = kill.headshot;
        if let Some(stats) = kill_stats.as_mut() {
            stats.markers_same_frame += u32::from(kill.tick > start.0);
        }
    }
    for hit in damage.read() {
        if player.is_none() || hit.source != player || hit.amount <= 0.0 {
            continue;
        }
        let same_frame = hit.tick > start.0;
        if hit.target_kind == DamageTarget::Character {
            let kind = MarkerKind::of(hit.killed, hit.headshot);
            let live = marker.kind.is_some_and(|k| {
                let life = if k == MarkerKind::Kill {
                    tuning.hud.kill_marker_seconds
                } else {
                    tuning.hud.hitmarker_seconds
                };
                marker.age < life
            });
            if !live || marker.kind.is_none_or(|k| kind >= k) {
                if kind == MarkerKind::Kill && !(live && marker.kind == Some(MarkerKind::Kill)) {
                    marker.gold = hit.headshot;
                }
                marker.kind = Some(kind);
            }
            marker.age = 0.0;
            stats.hits += 1;
            stats.markers_same_frame += same_frame as u32;
        }
        if !tuning.hud.damage_numbers {
            continue;
        }
        // Take a free number, or recycle the oldest.
        let Some(pick) = numbers
            .iter()
            .max_by(|a, b| {
                (!a.1.active)
                    .cmp(&!b.1.active)
                    .then(a.1.age.total_cmp(&b.1.age))
            })
            .map(|entry| entry.0)
        else {
            continue;
        };
        let Ok((_, mut number, mut vis, children)) = numbers.get_mut(pick) else {
            continue;
        };
        let kind = NumberKind::of(hit.target_kind, hit.headshot, hit.to_shield);
        *counter = counter.wrapping_add(1);
        // Successive numbers fan out left and right so a spray stays readable.
        const FAN: [f32; 6] = [-28.0, 26.0, -12.0, 40.0, -40.0, 12.0];
        let jitter = FAN[(*counter as usize) % FAN.len()];
        *number = DamageNumber {
            active: true,
            age: 0.0,
            point: hit.point,
            jitter,
            color: kind.color(),
        };
        style_glyphs(children, &mut glyphs, kind, &damage_label(hit.amount));
        *vis = Visibility::Inherited;
        if hit.target_kind == DamageTarget::Character {
            stats.numbers_same_frame += same_frame as u32;
        }
    }
}

fn draw_hitmarker(
    tuning: Res<Tuning>,
    marker: Res<Hitmarker>,
    mut bars: Query<(
        &El,
        &mut BackgroundColor,
        &mut Outline,
        &mut UiTransform,
        &mut Visibility,
    )>,
) {
    let (kind, life) = match marker.kind {
        Some(MarkerKind::Kill) => (MarkerKind::Kill, tuning.hud.kill_marker_seconds),
        Some(k) => (k, tuning.hud.hitmarker_seconds),
        None => (MarkerKind::Hit, 0.0),
    };
    let t = if life > 0.0 { marker.age / life } else { 1.0 };
    let show = marker.kind.is_some() && t < 1.0;
    let alpha = if t < 0.5 { 1.0 } else { (1.0 - t) * 2.0 }.clamp(0.0, 1.0);
    let settle = (marker.age / 0.06).clamp(0.0, 1.0);
    // The kill's X stamps in bigger and settles (M4).
    let pop = if kind == MarkerKind::Kill {
        1.0 + 0.55 * (1.0 - settle) * (1.0 - settle)
    } else {
        1.0 + 0.3 * (1.0 - settle)
    };
    // (Bar thickness, bar length) scales, distance from the centre (px),
    // fill, ink. The kill's is one big bold X, its arms nearly meeting.
    let (size, distance, fill, edge) = match kind {
        MarkerKind::Hit => (Vec2::ONE, 9.5, palette::HIT_WHITE, INK.with_alpha(0.85)),
        MarkerKind::Headshot => (
            Vec2::splat(1.1),
            10.5,
            palette::HEADSHOT,
            INK.with_alpha(0.9),
        ),
        MarkerKind::Kill => (
            Vec2::new(1.6, 2.6),
            14.5,
            if marker.gold {
                palette::HEADSHOT
            } else {
                palette::HIT_WHITE
            },
            INK,
        ),
    };
    let scale = tuning.hud.crosshair_scale.clamp(0.5, 3.0);
    for (el, mut bg, mut outline, mut transform, mut vis) in &mut bars {
        let El::MarkerBar(i) = *el else {
            continue;
        };
        set_visible(&mut vis, show);
        if !show {
            continue;
        }
        let dir = match i {
            0 => Vec2::new(1.0, -1.0),
            1 => Vec2::new(1.0, 1.0),
            2 => Vec2::new(-1.0, 1.0),
            _ => Vec2::new(-1.0, -1.0),
        }
        .normalize();
        let offset = dir * distance * pop * scale;
        let target = UiTransform {
            translation: Val2::px(offset.x, offset.y),
            scale: size * pop * scale,
            rotation: Rot2::radians((-dir.x).atan2(dir.y)),
        };
        if *transform != target {
            *transform = target;
        }
        set_bg(&mut bg, fill.with_alpha(alpha));
        let edge = edge.with_alpha(edge.alpha() * alpha);
        let width = px(if kind == MarkerKind::Kill { 1.4 } else { 1.5 });
        if outline.color != edge || outline.width != width {
            outline.color = edge;
            outline.width = width;
        }
    }
}

// ---------------------------------------------------------------------------
// Damage numbers
// ---------------------------------------------------------------------------

fn place_damage_numbers(
    time: FreezableTime,
    tuning: Res<Tuning>,
    fov: Res<CurrentFov>,
    ui: Query<&Camera, With<IsDefaultUiCamera>>,
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Option<Single<&Transform, With<MainCamera>>>,
    mut numbers: Query<(
        &mut DamageNumber,
        &mut UiTransform,
        &mut Visibility,
        &Children,
    )>,
    mut glyphs: Query<(&NumberGlyph, &mut TextColor, &mut TextShadow), Without<DamageNumber>>,
) {
    let (Some(screen), Some(camera)) = (screen_size(&ui, &window), camera) else {
        return;
    };
    let camera = GlobalTransform::from(**camera);
    let dt = time.delta_secs();
    let life = tuning.hud.damage_number_seconds;
    for (mut number, mut transform, mut vis, children) in &mut numbers {
        if !number.active {
            continue;
        }
        // Age starts counting the frame after the hit, so the first frame shows it fresh.
        let motion = number_motion(number.age, life, tuning.hud.damage_number_rise);
        number.age += dt;
        let expired = number.age > life + dt;
        let pos = project_to_screen(&camera, fov.0, screen, number.point);
        match (expired, pos) {
            (false, Some(pos)) => {
                // Numbers drift outward as they rise.
                let drift = number.jitter
                    * (1.0 + 0.6 * motion.rise / tuning.hud.damage_number_rise.max(1.0));
                // Up and to the right of the hit, clear of the crosshair.
                let at = pos - NUMBER_BOX * 0.5 + Vec2::new(18.0 + drift, -34.0 - motion.rise);
                let target = UiTransform {
                    translation: Val2::px(at.x.round(), at.y.round()),
                    scale: motion.scale * motion.squash,
                    rotation: Rot2::IDENTITY,
                };
                if *transform != target {
                    *transform = target;
                }
                let alpha = motion.alpha;
                for child in children.iter() {
                    if let Ok((layer, mut color, mut shadow)) = glyphs.get_mut(child) {
                        let fill = if layer.0 >= INK_LAYERS {
                            number.color
                        } else {
                            INK
                        };
                        set_color(&mut color, fill.with_alpha(alpha));
                        let shade = INK.with_alpha(alpha);
                        if shadow.color != shade {
                            shadow.color = shade;
                        }
                    }
                }
                set_visible(&mut vis, true);
            }
            _ => {
                if expired {
                    number.active = false;
                }
                set_visible(&mut vis, false);
            }
        }
    }
}
