//! HUD update systems: status (bars, ammo, hotbar, piece HP, readout, perf),
//! the crosshair, hit feedback, and damage-number placement.

use super::{
    FrameStartTick, HitFeedbackStats, MarkerKind, NumberKind, TrailBar, damage_label,
    layout::{DamageNumber, El, NUMBER_BOX, TICK_LEN},
    number_motion, project_to_screen, spread_to_pixels,
    style::{ACCENT, DANGER, TEXT, dim},
};
use crate::{
    building::AimedPiece,
    combat::{CombatStats, Loadout},
    menu::MenuState,
    palette,
    render::{CameraFollowSet, CurrentFov, MainCamera},
    shared::{ActiveTool, Ads, DamageDealt, DamageTarget, Health, PieceKind, Player, WeaponKind},
    telemetry::FrameStats,
    tuning::Tuning,
};
use bevy::{prelude::*, ui::UiSystems, window::PrimaryWindow};

pub(super) fn build(app: &mut App) {
    app.init_resource::<Hitmarker>()
        .add_systems(
            Update,
            (
                toggle_perf_overlay,
                update_status,
                update_crosshair,
                (hit_feedback, draw_hitmarker).chain(),
            ),
        )
        .add_systems(
            PostUpdate,
            place_damage_numbers
                .after(CameraFollowSet)
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
    }
}

fn piece_name(kind: PieceKind) -> &'static str {
    match kind {
        PieceKind::Wall => "WALL",
        PieceKind::Ramp => "RAMP",
        PieceKind::Floor => "FLOOR",
    }
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
);

fn update_status(
    time: Res<Time<Real>>,
    tuning: Res<Tuning>,
    stats: Res<CombatStats>,
    frame: Option<Res<FrameStats>>,
    menu: Option<Res<MenuState>>,
    player: Option<
        Single<(&Health, &ActiveTool, Option<&Loadout>, Option<&AimedPiece>), With<Player>>,
    >,
    mut memory: Local<StatusMemory>,
    mut parts: Query<StatusParts>,
) {
    let Some(player) = player else {
        return;
    };
    let (health, tool, loadout, aimed) = player.into_inner();
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

    for (el, node, text, color, bg, border, vis, transform) in &mut parts {
        match *el {
            El::Crosshair | El::Numbers => {
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
                    set_color(&mut c, if build { ACCENT } else { dim(0.75) });
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
                    let c = if on { ACCENT } else { Color::NONE };
                    if b.top != c {
                        *b = BorderColor::all(c);
                    }
                }
                if let Some(mut b) = bg {
                    set_bg(
                        &mut b,
                        if on {
                            Color::srgba(0.12, 0.11, 0.1, 0.62)
                        } else {
                            Color::srgba(0.03, 0.04, 0.06, 0.42)
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
                if let Some(mut b) = bg {
                    let c = match (i == active_slot, i >= 2) {
                        (true, true) => palette::WOOD_LIGHT,
                        (true, false) => TEXT,
                        (false, _) => dim(0.5),
                    };
                    set_bg(&mut b, c);
                }
            }
            El::SlotName(i) => {
                if let Some(mut c) = color {
                    set_color(&mut c, if i == active_slot { TEXT } else { dim(0.55) });
                }
            }
            El::BuildBadge => {
                if let Some(mut v) = vis {
                    set_visible(&mut v, build);
                }
            }
            El::PieceGroup => {
                if let Some(mut v) = vis {
                    set_visible(&mut v, aimed.is_some_and(|a| a.0.is_some()));
                }
            }
            El::PieceFill => {
                if let (Some(info), Some(mut n)) = (aimed.and_then(|a| a.0), node) {
                    set_width_pct(&mut n, info.hp / info.max_hp.max(1.0));
                    if let Some(mut b) = bg {
                        let c = match info.crack_stage {
                            0 => ACCENT,
                            1 => Color::srgb(1.0, 0.66, 0.3),
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
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    player: Option<Single<(&ActiveTool, &Ads, Option<&Loadout>), With<Player>>>,
    mut parts: Query<(&El, &mut Node, &mut Visibility, Option<&mut UiTransform>)>,
) {
    let (Some(window), Some(player)) = (window, player) else {
        return;
    };
    let (tool, ads, loadout) = player.into_inner();
    let hud = &tuning.hud;
    let height = window.height();
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
}

fn hit_feedback(
    time: Res<Time<Real>>,
    tuning: Res<Tuning>,
    start: Res<FrameStartTick>,
    player: Option<Single<Entity, With<Player>>>,
    mut damage: MessageReader<DamageDealt>,
    mut marker: ResMut<Hitmarker>,
    mut stats: ResMut<HitFeedbackStats>,
    mut numbers: Query<(
        Entity,
        &mut DamageNumber,
        &mut Text,
        &mut TextFont,
        &mut TextColor,
        &mut Visibility,
    )>,
    mut counter: Local<u32>,
) {
    let player = player.map(|p| *p);
    marker.age += time.delta_secs();
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
        let Ok((_, mut number, mut text, mut font, mut color, mut vis)) = numbers.get_mut(pick)
        else {
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
        text.0 = damage_label(hit.amount);
        font.font_size = kind.size().into();
        color.0 = kind.color();
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
    let pop = 1.0 + 0.3 * (1.0 - (marker.age / 0.06).clamp(0.0, 1.0));
    let (size, distance, fill, edge) = match kind {
        MarkerKind::Hit => (
            1.0,
            9.5,
            palette::HIT_WHITE,
            Color::srgba(0.0, 0.0, 0.0, 0.45),
        ),
        MarkerKind::Headshot => (
            1.1,
            10.5,
            palette::HEADSHOT,
            Color::srgba(0.0, 0.0, 0.0, 0.5),
        ),
        MarkerKind::Kill => (1.5, 14.0, palette::HIT_WHITE, Color::srgb(0.95, 0.16, 0.14)),
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
            scale: Vec2::splat(size * pop * scale),
            rotation: Rot2::radians((-dir.x).atan2(dir.y)),
        };
        if *transform != target {
            *transform = target;
        }
        set_bg(&mut bg, fill.with_alpha(alpha));
        let edge = edge.with_alpha(edge.alpha() * alpha);
        if outline.color != edge {
            outline.color = edge;
            outline.width = px(if kind == MarkerKind::Kill { 1.5 } else { 1.0 });
        }
    }
}

// ---------------------------------------------------------------------------
// Damage numbers
// ---------------------------------------------------------------------------

fn place_damage_numbers(
    time: Res<Time<Real>>,
    tuning: Res<Tuning>,
    fov: Res<CurrentFov>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    camera: Option<Single<&Transform, With<MainCamera>>>,
    mut numbers: Query<(
        &mut DamageNumber,
        &mut UiTransform,
        &mut TextColor,
        &mut TextShadow,
        &mut Visibility,
    )>,
) {
    let (Some(window), Some(camera)) = (window, camera) else {
        return;
    };
    let screen = Vec2::new(window.width(), window.height());
    let camera = GlobalTransform::from(**camera);
    let dt = time.delta_secs();
    let life = tuning.hud.damage_number_seconds;
    for (mut number, mut transform, mut color, mut shadow, mut vis) in &mut numbers {
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
                *transform = UiTransform {
                    translation: Val2::px(at.x.round(), at.y.round()),
                    scale: Vec2::splat(motion.scale),
                    rotation: Rot2::IDENTITY,
                };
                color.0 = number.color.with_alpha(motion.alpha);
                shadow.color = Color::srgba(0.0, 0.0, 0.0, 0.8 * motion.alpha);
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
