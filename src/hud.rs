//! Slice F — crosshair, bars, ammo, hotbar, hitmarkers, damage numbers, piece HP,
//! combat readout and the performance overlay.
//!
//! The HUD is native-resolution Bevy UI drawn by the UI camera over the offscreen
//! world image. Hit feedback reads [`DamageDealt`](crate::shared::DamageDealt) in
//! `Update`, right after the fixed step that produced it, so the hitmarker, the
//! damage number and the hit sound all appear on the frame the hit registers.
//! The pure rules (spread → pixels, projection, number styling, bar trails) live
//! here as functions so they're testable without a window.

mod layout;
mod systems;

pub(crate) use layout::style;

use crate::{
    palette,
    shared::{DamageTarget, SimTick},
};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct HudTuning {
    /// Crosshair gap follows the current spread; off shows a fixed crosshair.
    pub show_bloom: bool,
    pub crosshair_scale: f32,
    pub damage_numbers: bool,
    pub perf_overlay: bool,
    pub combat_readout: bool,
    /// Crosshair gap (px) at zero spread.
    pub crosshair_min_gap: f32,
    /// Seconds a hitmarker stays up (kills use `kill_marker_seconds`).
    pub hitmarker_seconds: f32,
    pub kill_marker_seconds: f32,
    pub damage_number_seconds: f32,
    /// How far damage numbers rise over their life (px).
    pub damage_number_rise: f32,
    /// Seconds the lost chunk of a bar holds before draining.
    pub bar_trail_hold: f32,
    /// Bar fraction per second the lost chunk drains.
    pub bar_trail_rate: f32,
}

impl Default for HudTuning {
    fn default() -> Self {
        Self {
            show_bloom: true,
            crosshair_scale: 1.0,
            damage_numbers: true,
            perf_overlay: false,
            combat_readout: true,
            crosshair_min_gap: 4.0,
            hitmarker_seconds: 0.2,
            kill_marker_seconds: 0.45,
            damage_number_seconds: 0.85,
            damage_number_rise: 60.0,
            bar_trail_hold: 0.35,
            bar_trail_rate: 1.2,
        }
    }
}

// ---------------------------------------------------------------------------
// Pure rules
// ---------------------------------------------------------------------------

/// Screen distance (px) from the center of a direction `angle_deg` off the view
/// axis, for a vertical FOV (radians) and a viewport `height_px` tall.
pub fn spread_to_pixels(angle_deg: f32, vertical_fov: f32, height_px: f32) -> f32 {
    let half = (vertical_fov * 0.5).tan();
    if half <= 1e-6 || angle_deg <= 0.0 {
        return 0.0;
    }
    angle_deg.to_radians().min(1.5).tan() / half * height_px * 0.5
}

/// Projects a world point to screen pixels (origin top-left, y down) for a
/// perspective camera at `camera` with vertical FOV `vertical_fov` (radians)
/// filling `screen` (px). `None` behind the camera.
pub fn project_to_screen(
    camera: &GlobalTransform,
    vertical_fov: f32,
    screen: Vec2,
    point: Vec3,
) -> Option<Vec2> {
    let view = camera.affine().inverse().transform_point3(point);
    if view.z > -0.05 {
        return None;
    }
    let half_h = (vertical_fov * 0.5).tan();
    let aspect = screen.x / screen.y.max(1.0);
    let ndc = Vec2::new(
        view.x / (-view.z * half_h * aspect),
        view.y / (-view.z * half_h),
    );
    Some(Vec2::new(
        (ndc.x + 1.0) * 0.5 * screen.x,
        (1.0 - ndc.y) * 0.5 * screen.y,
    ))
}

/// How a damage number is styled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberKind {
    /// Health damage: white.
    Body,
    /// Any headshot: yellow and larger.
    Headshot,
    /// Damage that hit shield: blue.
    Shield,
    /// Damage to a building piece: smaller and muted.
    Structure,
}

impl NumberKind {
    pub fn of(target: DamageTarget, headshot: bool, to_shield: f32) -> Self {
        match target {
            DamageTarget::Piece => Self::Structure,
            DamageTarget::Character if headshot => Self::Headshot,
            DamageTarget::Character if to_shield > 0.0 => Self::Shield,
            DamageTarget::Character => Self::Body,
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Body => palette::HIT_WHITE,
            Self::Headshot => palette::HEADSHOT,
            Self::Shield => palette::SHIELD,
            Self::Structure => Color::srgb(0.87, 0.88, 0.9),
        }
    }

    /// Font size (px).
    pub fn size(self) -> f32 {
        match self {
            Self::Body | Self::Shield => 27.0,
            Self::Headshot => 33.0,
            Self::Structure => 19.0,
        }
    }
}

/// The text of a damage number: whole points, never "0".
pub fn damage_label(amount: f32) -> String {
    format!("{}", amount.round().max(1.0) as i64)
}

/// A damage number's animation at `age` seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumberMotion {
    /// Upward offset (px).
    pub rise: f32,
    pub alpha: f32,
    pub scale: f32,
}

pub fn number_motion(age: f32, lifetime: f32, rise: f32) -> NumberMotion {
    let life = lifetime.max(0.05);
    let t = (age / life).clamp(0.0, 1.0);
    // Ease-out rise, a quick pop in, and a fade over the last 40%.
    let eased = 1.0 - (1.0 - t).powi(2);
    let pop = (age / 0.07).clamp(0.0, 1.0);
    NumberMotion {
        rise: rise * eased,
        alpha: if t < 0.6 { 1.0 } else { 1.0 - (t - 0.6) / 0.4 },
        scale: 1.35 - 0.35 * pop,
    }
}

/// Which hitmarker to show. A kill outranks a headshot, which outranks a hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MarkerKind {
    Hit,
    Headshot,
    Kill,
}

impl MarkerKind {
    pub fn of(killed: bool, headshot: bool) -> Self {
        if killed {
            Self::Kill
        } else if headshot {
            Self::Headshot
        } else {
            Self::Hit
        }
    }
}

/// A health bar's "lost chunk": after damage it holds at the old value, then
/// drains down to the current one. Healing snaps it up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrailBar {
    /// Where the lost chunk currently ends (fraction 0..=1).
    pub shown: f32,
    last: f32,
    hold_left: f32,
}

impl TrailBar {
    pub fn new(value: f32) -> Self {
        Self {
            shown: value,
            last: value,
            hold_left: 0.0,
        }
    }

    /// Advances by `dt` toward `value` (fractions 0..=1). New damage restarts the
    /// hold, so a burst reads as one chunk.
    pub fn update(&mut self, value: f32, dt: f32, hold: f32, rate: f32) {
        if value >= self.shown {
            self.shown = value;
            self.hold_left = 0.0;
        } else {
            if value < self.last {
                self.hold_left = hold;
            }
            if self.hold_left > 0.0 {
                self.hold_left -= dt;
            } else {
                self.shown = (self.shown - rate * dt).max(value);
            }
        }
        self.last = value;
    }
}

// ---------------------------------------------------------------------------
// Evidence: same-frame feedback
// ---------------------------------------------------------------------------

/// The simulation tick when this frame began, so presentation can prove a message
/// was produced by this frame's fixed step (`message.tick > FrameStartTick`).
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct FrameStartTick(pub u64);

/// Counts of the player's character hits whose feedback appeared on the very frame
/// the hit registered. Scenarios put it in their summary (gate G5).
#[derive(Resource, Debug, Default, Clone, Serialize)]
pub struct HitFeedbackStats {
    pub hits: u32,
    pub markers_same_frame: u32,
    pub numbers_same_frame: u32,
    pub sounds_same_frame: u32,
}

fn record_frame_start_tick(tick: Res<SimTick>, mut start: ResMut<FrameStartTick>) {
    start.0 = tick.0;
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FrameStartTick>()
            .init_resource::<HitFeedbackStats>()
            .add_systems(First, record_frame_start_tick);
        layout::build(app);
        systems::build(app);
    }
}
