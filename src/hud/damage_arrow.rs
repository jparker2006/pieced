//! The damage-direction arrow (docs/M3-SPEC.md → Fairness, HUD and results
//! UI; D76): every hit on the player shows a red cartoon arrow around the
//! crosshair pointing at whoever landed it, for [`ARROW_SECONDS`], from the
//! frame the hit registers (M2's same-frame rule).
//!
//! The logic ([`DamageArrows`], [`DamageArrowTracking`]) runs without a window,
//! so the fairness tests can check an arrow is up on the hit tick; the HUD
//! ([`DamageArrowPlugin`]) adds the pooled UI arrows that draw it. The arrow
//! follows its source as the player turns (and as a live source moves), and
//! several can show at once (a pool of [`ARROW_POOL`]; a new hit from the
//! same source refreshes its arrow).

use crate::shared::{DamageDealt, DamageTarget, EyeHeight, FreezableTime, LookAngles, Player};
use bevy::{
    asset::RenderAssetUsages,
    image::Image,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

/// Arrows up at once.
pub const ARROW_POOL: usize = 6;
/// How long each arrow stays up (s); it fades over the last third.
pub const ARROW_SECONDS: f32 = 1.0;
/// Distance of the arrow's centre from the crosshair (px at 800 px tall).
pub const ARROW_RADIUS: f32 = 92.0;
/// The arrow image's size (px): it points up, toward -y.
pub const ARROW_SIZE: UVec2 = UVec2::new(56, 64);

/// Screen angle (radians, clockwise from straight up) of the arrow for a
/// source at `source`, seen from `eye` looking along `yaw`: 0 dead ahead,
/// +π/2 to the right, ±π behind.
pub fn arrow_angle(eye: Vec3, yaw: f32, source: Vec3) -> f32 {
    let local = Quat::from_rotation_y(-yaw) * (source - eye);
    if local.x.abs() < 1e-6 && local.z.abs() < 1e-6 {
        return 0.0;
    }
    local.x.atan2(-local.z)
}

/// Where an arrow at `angle` sits relative to the crosshair (px, y down).
pub fn arrow_offset(angle: f32, radius: f32) -> Vec2 {
    Vec2::new(angle.sin(), -angle.cos()) * radius
}

/// One damage arrow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DamageArrow {
    pub active: bool,
    /// Seconds since the hit that raised (or refreshed) it.
    pub age: f32,
    /// Who hit the player, and where they were (last seen).
    pub source: Option<Entity>,
    pub source_at: Vec3,
    /// The current screen angle ([`arrow_angle`]).
    pub angle: f32,
    /// The simulation tick of its latest hit.
    pub tick: u64,
}

impl Default for DamageArrow {
    fn default() -> Self {
        Self {
            active: false,
            age: 0.0,
            source: None,
            source_at: Vec3::ZERO,
            angle: 0.0,
            tick: 0,
        }
    }
}

/// The live arrows, and evidence for the fairness gate.
#[derive(Resource, Debug, Clone, Default)]
pub struct DamageArrows {
    pub arrows: [DamageArrow; ARROW_POOL],
    /// Hits on the player seen, and how many raised an arrow on their frame.
    pub hits: u32,
    pub raised: u32,
}

impl DamageArrows {
    pub fn live(&self) -> impl Iterator<Item = &DamageArrow> {
        self.arrows.iter().filter(|a| a.active)
    }

    /// Raises (or refreshes) the arrow for a hit from `source` at `at`.
    fn raise(&mut self, source: Option<Entity>, at: Vec3, tick: u64) {
        let slot = self
            .arrows
            .iter()
            .position(|a| a.active && source.is_some() && a.source == source)
            .or_else(|| self.arrows.iter().position(|a| !a.active))
            .unwrap_or_else(|| {
                // Recycle the oldest.
                (0..ARROW_POOL)
                    .max_by(|&a, &b| self.arrows[a].age.total_cmp(&self.arrows[b].age))
                    .unwrap_or(0)
            });
        self.arrows[slot] = DamageArrow {
            active: true,
            age: 0.0,
            source,
            source_at: at,
            angle: 0.0,
            tick,
        };
        self.hits += 1;
        self.raised += 1;
    }
}

/// The arrow logic, without any UI (headless tests add just this).
pub struct DamageArrowTracking;

impl Plugin for DamageArrowTracking {
    fn build(&self, app: &mut App) {
        Self::install(app);
    }
}

impl DamageArrowTracking {
    /// Adds the arrow logic to an app, even one already running (a test's
    /// headless simulation).
    pub fn install(app: &mut App) {
        app.init_resource::<DamageArrows>()
            .add_systems(Update, track_damage_arrows.in_set(DamageArrowSet));
    }
}

/// The arrows are raised and aimed in this set (`Update`).
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DamageArrowSet;

/// The arrow logic and its pooled UI (client).
pub struct DamageArrowPlugin;

impl Plugin for DamageArrowPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(DamageArrowTracking)
            .add_systems(Startup, spawn_arrows)
            .add_systems(Update, draw_arrows.after(DamageArrowSet));
    }
}

fn track_damage_arrows(
    time: FreezableTime,
    mut damage: MessageReader<DamageDealt>,
    mut arrows: ResMut<DamageArrows>,
    player: Option<Single<(Entity, &Transform, &LookAngles, Option<&EyeHeight>), With<Player>>>,
    sources: Query<&Transform, Without<Player>>,
) {
    let Some(player) = player else {
        damage.clear();
        return;
    };
    let (me, feet, look, eye) = *player;
    let eye = feet.translation + Vec3::Y * eye.map_or(EyeHeight::default().0, |e| e.0);
    for hit in damage.read() {
        if hit.target != me || hit.target_kind != DamageTarget::Character || hit.amount <= 0.0 {
            continue;
        }
        // Point at the shooter; without one, back along the hit.
        let at = hit
            .source
            .and_then(|s| sources.get(s).ok())
            .map(|t| t.translation + Vec3::Y * 1.2)
            .unwrap_or(hit.point + hit.normal * 5.0);
        arrows.raise(hit.source, at, hit.tick);
    }
    let dt = time.delta_secs();
    for arrow in arrows.arrows.iter_mut().filter(|a| a.active) {
        arrow.age += dt;
        if arrow.age >= ARROW_SECONDS {
            arrow.active = false;
            continue;
        }
        if let Some(t) = arrow.source.and_then(|s| sources.get(s).ok()) {
            arrow.source_at = t.translation + Vec3::Y * 1.2;
        }
        arrow.angle = arrow_angle(eye, look.yaw, arrow.source_at);
    }
}

// ---------------------------------------------------------------------------
// UI
// ---------------------------------------------------------------------------

/// One pooled UI arrow (index into [`DamageArrows::arrows`]).
#[derive(Component, Debug, Clone, Copy)]
struct ArrowNode(usize);

/// The arrow's fill: a hot cartoon red.
const ARROW_RED: Color = Color::srgb(0.96, 0.16, 0.12);
/// Its highlight stripe.
const ARROW_LIGHT: Color = Color::srgb(1.0, 0.5, 0.38);

fn spawn_arrows(mut commands: Commands, images: Option<ResMut<Assets<Image>>>) {
    // Without an image store (a bare HUD test app) the arrows have nothing
    // to draw with; their logic still runs.
    let Some(mut images) = images else {
        return;
    };
    let image = images.add(arrow_image());
    commands
        .spawn((
            Name::new("Damage arrows"),
            Node {
                position_type: PositionType::Absolute,
                left: percent(50),
                top: percent(50),
                width: px(0),
                height: px(0),
                ..default()
            },
            GlobalZIndex(11),
        ))
        .with_children(|root| {
            for i in 0..ARROW_POOL {
                let (w, h) = (ARROW_SIZE.x as f32, ARROW_SIZE.y as f32);
                root.spawn((
                    ArrowNode(i),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(-w / 2.0),
                        top: px(-h / 2.0),
                        width: px(w),
                        height: px(h),
                        ..default()
                    },
                    ImageNode::new(image.clone()),
                    UiTransform::default(),
                    Visibility::Hidden,
                ));
            }
        });
}

fn draw_arrows(
    arrows: Res<DamageArrows>,
    windows: Query<&Window>,
    mut nodes: Query<(
        &ArrowNode,
        &mut ImageNode,
        &mut UiTransform,
        &mut Visibility,
    )>,
) {
    let height = windows.iter().next().map_or(800.0, |w| w.height());
    let k = (height / 800.0).clamp(0.6, 2.5);
    for (node, mut image, mut transform, mut vis) in &mut nodes {
        let arrow = &arrows.arrows[node.0];
        let show = arrow.active;
        vis.set_if_neq(if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if !show {
            continue;
        }
        let t = arrow.age / ARROW_SECONDS;
        // A pop on the hit frame, then it fades over the last third.
        let pop = 1.0 + 0.35 * (1.0 - (arrow.age / 0.08).clamp(0.0, 1.0));
        let alpha = if t < 0.66 { 1.0 } else { (1.0 - t) / 0.34 }.clamp(0.0, 1.0);
        let offset = arrow_offset(arrow.angle, ARROW_RADIUS * k);
        let target = UiTransform {
            translation: Val2::px(offset.x, offset.y),
            scale: Vec2::splat(pop * k),
            rotation: Rot2::radians(arrow.angle),
        };
        if *transform != target {
            *transform = target;
        }
        let color = Color::WHITE.with_alpha(alpha);
        if image.color != color {
            image.color = color;
        }
    }
}

/// The arrow picture: a fat cartoon arrowhead over a short stem, red with a
/// light stripe and a thick dark ink outline, pointing up (-y). Drawn in code
/// (4× supersampled), once at startup.
pub fn arrow_image() -> Image {
    let (w, h) = (ARROW_SIZE.x as usize, ARROW_SIZE.y as usize);
    let ink = crate::hud::style::INK.to_srgba();
    let red = ARROW_RED.to_srgba();
    let light = ARROW_LIGHT.to_srgba();
    let mut data = vec![0u8; w * h * 4];
    const SS: usize = 4;
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let p = Vec2::new(
                        x as f32 + (sx as f32 + 0.5) / SS as f32,
                        y as f32 + (sy as f32 + 0.5) / SS as f32,
                    );
                    let d = arrow_distance(p, w as f32);
                    let c = if d < -3.2 {
                        // Inside the ink rim: the fill, with a light stripe
                        // down the left of the head.
                        if stripe(p, w as f32) { light } else { red }
                    } else if d < 0.0 {
                        ink
                    } else {
                        continue;
                    };
                    acc[0] += c.red;
                    acc[1] += c.green;
                    acc[2] += c.blue;
                    acc[3] += 1.0;
                }
            }
            let n = (SS * SS) as f32;
            let i = (y * w + x) * 4;
            if acc[3] > 0.0 {
                for k in 0..3 {
                    data[i + k] = ((acc[k] / acc[3]).clamp(0.0, 1.0) * 255.0).round() as u8;
                }
                data[i + 3] = ((acc[3] / n) * 255.0).round() as u8;
            }
        }
    }
    Image::new(
        Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// Signed distance (px, negative inside) to the arrow shape in a `w`-wide
/// image: a rounded triangle head (tip at the top) over a short stem.
fn arrow_distance(p: Vec2, w: f32) -> f32 {
    let cx = w / 2.0;
    let round = 3.0;
    // The head: a triangle shrunk by `round`, then grown back (rounded corners).
    let tip = Vec2::new(cx, 4.0 + round);
    let left = Vec2::new(4.0 + round, 38.0 - round * 0.5);
    let right = Vec2::new(w - 4.0 - round, 38.0 - round * 0.5);
    let head = triangle_distance(p, tip, right, left) - round;
    // The stem: a rounded box under the head.
    let half = Vec2::new(9.0, 12.0);
    let centre = Vec2::new(cx, 47.0);
    let q = (p - centre).abs() - (half - Vec2::splat(round));
    let stem = q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0) - round;
    head.min(stem)
}

fn stripe(p: Vec2, w: f32) -> bool {
    // A band parallel to the head's left edge, a little inside it.
    let tip = Vec2::new(w / 2.0, 10.0);
    let left = Vec2::new(12.0, 36.0);
    let edge = (left - tip).normalize();
    let normal = Vec2::new(-edge.y, edge.x);
    let d = (p - tip).dot(normal).abs();
    let along = (p - tip).dot(edge);
    d < 2.2 && along > 4.0 && along < (left - tip).length() - 2.0
}

/// Signed distance to a triangle (points in either winding).
fn triangle_distance(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> f32 {
    let edges = [(a, b), (b, c), (c, a)];
    let mut d = f32::MAX;
    for (u, v) in edges {
        let e = v - u;
        let t = ((p - u).dot(e) / e.length_squared()).clamp(0.0, 1.0);
        d = d.min((p - (u + e * t)).length());
    }
    let sign = |u: Vec2, v: Vec2| (v - u).perp_dot(p - u);
    let (s0, s1, s2) = (sign(a, b), sign(b, c), sign(c, a));
    let inside = (s0 >= 0.0 && s1 >= 0.0 && s2 >= 0.0) || (s0 <= 0.0 && s1 <= 0.0 && s2 <= 0.0);
    if inside { -d } else { d }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arrow_image_has_a_red_body_an_ink_rim_and_a_clear_background() {
        let image = arrow_image();
        let data = image.data.as_ref().unwrap();
        let w = ARROW_SIZE.x as usize;
        let px = |x: usize, y: usize| {
            let i = (y * w + x) * 4;
            [data[i], data[i + 1], data[i + 2], data[i + 3]]
        };
        assert_eq!(px(0, 0)[3], 0, "clear corner");
        let body = px(w / 2 + 6, 28);
        assert!(body[0] > 200 && body[1] < 90 && body[3] == 255, "{body:?}");
        let rim = px(w / 2, 6);
        assert!(rim[0] < 60 && rim[3] > 100, "{rim:?}");
    }
}
