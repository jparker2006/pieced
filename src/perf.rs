//! Performance settings and levers for Milestone 4's GPU budget (chunk 0,
//! D99–D100).
//!
//! [`PerfTuning`] is a designer section of [`crate::tuning::Tuning`]: never
//! saved to `settings.json` (`#[serde(skip)]`), so the orchestrator can change
//! a default after Jake's sessions without a saved copy freezing it. Perf
//! knobs override it at startup ([`crate::perf_knobs::PerfKnobsPlugin`]).
//!
//! The pre-approved levers (D100), each measurable on and off:
//!
//! | lever | knob | default | where |
//! |---|---|---|---|
//! | outlines fade over 15–25 m (from 25–40 m) | – | **on** | [`crate::look::FADE_START`] |
//! | knights beyond 20 m animate at 30 Hz | – | **on** | [`AnimLod`] |
//! | the far layer at half resolution | `farres=half` | off | [`far_res`] |
//! | dynamic resolution 0.8–1.0 on Battery, with sharpening | `dynres=on` | off | [`DynamicResolution`] |
//! | overdraw cap on glows by screen coverage | `overdraw=cap` | off | [`cap_overdraw`] |
//!
//! Each works from state that is allocated once: the only per-frame work is
//! arithmetic over existing entities and buffers reused frame to frame.

pub mod far_res;

use crate::{
    look::{Halo, HaloSprite, HaloSystems, LookSettings, pack_halo},
    profile::LastGpu,
    render::{MainCamera, QualityPreset, VIEWMODEL_LAYER, WorldTarget},
    shared::AppState,
    tuning::Tuning,
    viewmodel::ViewmodelCamera,
};
use bevy::{
    anti_alias::contrast_adaptive_sharpening::ContrastAdaptiveSharpening,
    camera::visibility::RenderLayers, mesh::MeshTag, platform::collections::HashMap, prelude::*,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PerfTuning {
    /// Time the GPU passes on 1 frame in this many (1 = every frame, 0 =
    /// never). The spec keeps per-frame timing only if it costs ≤ 0.3 ms; it
    /// measured ~0.5 ms per timed frame offscreen, so it samples 1 in 8
    /// (see [`crate::gpu_timing`]).
    pub gpu_every: u32,
    /// Of the timed frames, every this-many-th is *bare* (first and last
    /// marks only), so the session can report the timing's own cost.
    pub gpu_bare_every: u32,
    /// Knights farther than this (m) from the camera animate at
    /// [`PerfTuning::knight_lod_hz`] (0 Hz = every frame).
    pub knight_lod_m: f32,
    pub knight_lod_hz: f32,
    /// `farres=half`: the far layer renders into its own half-resolution
    /// target, composited behind the world ([`far_res`]).
    pub far_half_res: bool,
    /// `dynres=on`: on the Battery preset, the 3D render scale follows the
    /// GPU time between `dynres_min` and `dynres_max`, one `dynres_step` at
    /// most every `dynres_hold_s`: down when the smoothed GPU total passes
    /// `dynres_down_ms`, up when it falls under `dynres_up_ms`.
    pub dynres: bool,
    pub dynres_min: f32,
    pub dynres_max: f32,
    pub dynres_step: f32,
    pub dynres_hold_s: f32,
    pub dynres_down_ms: f32,
    pub dynres_up_ms: f32,
    /// Contrast-adaptive sharpening strength while the scale is below 1.
    pub dynres_sharpening: f32,
    /// `overdraw=cap`: glows (halos, including spell and bolt halos) may
    /// cover at most this many screens in total; past it the oldest fade
    /// out first, and those nearer than `overdraw_keep_m` always stay.
    pub overdraw_cap: bool,
    pub overdraw_max_screens: f32,
    pub overdraw_keep_m: f32,
    pub overdraw_fade_s: f32,
}

impl Default for PerfTuning {
    fn default() -> Self {
        Self {
            gpu_every: 8,
            gpu_bare_every: 4,
            knight_lod_m: 20.0,
            knight_lod_hz: 30.0,
            far_half_res: false,
            dynres: false,
            dynres_min: 0.8,
            dynres_max: 1.0,
            dynres_step: 0.05,
            dynres_hold_s: 0.5,
            dynres_down_ms: 12.5,
            dynres_up_ms: 9.5,
            dynres_sharpening: 0.5,
            overdraw_cap: false,
            overdraw_max_screens: 1.5,
            overdraw_keep_m: 6.0,
            overdraw_fade_s: 0.25,
        }
    }
}

/// Client systems for the levers (the far layer adds its own when on).
pub struct PerfPlugin;

impl Plugin for PerfPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DynamicResolution>()
            .add_plugins(far_res::FarResPlugin)
            .add_systems(Update, (drive_dynamic_resolution, sync_sharpening).chain())
            .add_systems(
                PostUpdate,
                cap_overdraw
                    .after(HaloSystems)
                    .before(TransformSystems::Propagate),
            );
    }
}

// ---------------------------------------------------------------------------
// Knights beyond 20 m at 30 Hz
// ---------------------------------------------------------------------------

/// Whether a knight steps its animation this frame. Near knights (or any
/// knight just hit) step every frame with `dt`; far ones bank `dt` into
/// `pending` and step once a `1/hz` slot has passed (half a 60 Hz frame of
/// tolerance), with everything banked. Returns the step to take.
pub fn lod_due(
    pending: &mut f32,
    dt: f32,
    distance: f32,
    urgent: bool,
    far_m: f32,
    hz: f32,
) -> Option<f32> {
    let total = *pending + dt;
    let slow = hz > 0.0 && far_m > 0.0 && distance > far_m && !urgent;
    if slow && total < 0.95 / hz {
        *pending = total;
        return None;
    }
    *pending = 0.0;
    Some(total)
}

/// Per-knight banked time for [`lod_due`] (a system `Local`; one entry per
/// pooled figure, allocated once).
#[derive(Debug, Default)]
pub struct AnimLod {
    pending: HashMap<Entity, f32>,
}

impl AnimLod {
    pub fn step(
        &mut self,
        figure: Entity,
        dt: f32,
        distance: f32,
        urgent: bool,
        perf: &PerfTuning,
    ) -> Option<f32> {
        let pending = self.pending.entry(figure).or_insert(0.0);
        lod_due(
            pending,
            dt,
            distance,
            urgent,
            perf.knight_lod_m,
            perf.knight_lod_hz,
        )
    }
}

// ---------------------------------------------------------------------------
// Dynamic resolution (Battery only)
// ---------------------------------------------------------------------------

/// The dynamic render-scale factor (1 = off), multiplied into
/// `GraphicsTuning::render_scale` by [`crate::render`].
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct DynamicResolution {
    pub scale: f32,
    /// Smoothed GPU total (ms).
    pub smoothed_ms: Option<f32>,
    last_step_s: f64,
}

impl Default for DynamicResolution {
    fn default() -> Self {
        Self {
            scale: 1.0,
            smoothed_ms: None,
            last_step_s: f64::NEG_INFINITY,
        }
    }
}

impl DynamicResolution {
    /// How fast the smoothed GPU time follows new samples.
    pub const SMOOTHING: f32 = 0.3;

    /// Takes a GPU total at `now_s`; returns whether the scale moved.
    pub fn observe(&mut self, now_s: f64, gpu_ms: f32, t: &PerfTuning) -> bool {
        let ema = match self.smoothed_ms {
            None => gpu_ms,
            Some(m) => m + (gpu_ms - m) * Self::SMOOTHING,
        };
        self.smoothed_ms = Some(ema);
        if now_s - self.last_step_s < f64::from(t.dynres_hold_s) {
            return false;
        }
        let before = self.scale;
        if ema > t.dynres_down_ms {
            self.scale = (self.scale - t.dynres_step).max(t.dynres_min);
        } else if ema < t.dynres_up_ms {
            self.scale = (self.scale + t.dynres_step).min(t.dynres_max);
        }
        // Snap away float drift so 1.0 means exactly 1.0.
        self.scale = (self.scale * 1000.0).round() / 1000.0;
        let moved = self.scale != before;
        if moved {
            self.last_step_s = now_s;
        }
        moved
    }

    /// Back to full resolution (the lever is off, or not on Battery).
    pub fn reset(&mut self) {
        if self.scale != 1.0 || self.smoothed_ms.is_some() {
            *self = Self::default();
        }
    }
}

fn drive_dynamic_resolution(
    time: Res<Time<Real>>,
    tuning: Res<Tuning>,
    gpu: Option<Res<LastGpu>>,
    mut dynres: ResMut<DynamicResolution>,
) {
    let perf = &tuning.perf;
    if !perf.dynres || tuning.graphics.preset != QualityPreset::Battery {
        dynres.reset();
        return;
    }
    let Some(total) = gpu.and_then(|g| g.0.total_ms) else {
        return;
    };
    let now = time.elapsed_secs_f64();
    // Only write on a move, so the render target isn't marked changed.
    let mut next = dynres.clone();
    if next.observe(now, total, perf) {
        *dynres = next;
    } else {
        dynres.bypass_change_detection().smoothed_ms = next.smoothed_ms;
    }
}

/// With `dynres=on`, the last 3D camera (the viewmodel's, which also runs
/// FXAA) gets contrast-adaptive sharpening, on whenever the scale is below 1
/// and throughout Boot, so its pipeline compiles behind the loading screen.
fn sync_sharpening(
    mut commands: Commands,
    tuning: Res<Tuning>,
    dynres: Res<DynamicResolution>,
    state: Res<State<AppState>>,
    mut cameras: Query<(Entity, Option<&mut ContrastAdaptiveSharpening>), With<ViewmodelCamera>>,
) {
    if !tuning.perf.dynres {
        return;
    }
    let enabled = *state.get() == AppState::Boot || dynres.scale < 0.999;
    for (camera, cas) in &mut cameras {
        match cas {
            Some(mut cas) => {
                if cas.enabled != enabled {
                    cas.enabled = enabled;
                }
            }
            None => {
                commands.entity(camera).insert(ContrastAdaptiveSharpening {
                    enabled,
                    sharpening_strength: tuning.perf.dynres_sharpening,
                    denoise: false,
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Overdraw cap
// ---------------------------------------------------------------------------

/// The fraction of the screen a camera-facing disc of `diameter_m` at
/// `distance_m` covers, for a vertical field of view `fov_y` and an aspect
/// ratio (width / height). At most 1.
pub fn screen_coverage(diameter_m: f32, distance_m: f32, fov_y: f32, aspect: f32) -> f32 {
    if distance_m <= 0.01 {
        return 1.0;
    }
    // Radius in half-screen-heights; the screen is 2 × 2·aspect of those.
    let r = 0.5 * diameter_m / (distance_m * (0.5 * fov_y).tan());
    (std::f32::consts::PI * r * r / (4.0 * aspect.max(0.1))).min(1.0)
}

/// One glow for [`overdraw_victims`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glow {
    pub coverage: f32,
    pub distance: f32,
    /// Seconds since it appeared.
    pub age_s: f32,
}

/// Which glows to fade so their total coverage fits `cap` screens: the
/// oldest first (then the farthest), never one nearer than `keep_m`.
/// `order` is scratch space (reused, so nothing allocates once it has grown);
/// the victims' indices are left in `victims`.
pub fn overdraw_victims(
    glows: &[Glow],
    cap: f32,
    keep_m: f32,
    order: &mut Vec<usize>,
    victims: &mut Vec<usize>,
) {
    victims.clear();
    let mut total: f32 = glows.iter().map(|g| g.coverage).sum();
    if total <= cap {
        return;
    }
    order.clear();
    order.extend((0..glows.len()).filter(|&i| glows[i].distance >= keep_m));
    order.sort_unstable_by(|&a, &b| {
        let (a, b) = (&glows[a], &glows[b]);
        b.age_s
            .total_cmp(&a.age_s)
            .then(b.distance.total_cmp(&a.distance))
    });
    for &i in order.iter() {
        if total <= cap {
            break;
        }
        total -= glows[i].coverage;
        victims.push(i);
    }
}

/// A glow billboard's state under the overdraw cap.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct OverdrawState {
    /// When its owner last became visible (s, virtual time).
    pub shown_at: f32,
    /// 1 = full, 0 = faded out (hidden).
    pub fade: f32,
    /// This lever hid it.
    pub hidden: bool,
}

/// Scratch buffers kept between frames.
#[derive(Default)]
pub struct OverdrawScratch {
    glows: Vec<Glow>,
    entities: Vec<Entity>,
    order: Vec<usize>,
    victims: Vec<usize>,
}

/// `overdraw=cap`: fades glows past the coverage cap (see [`PerfTuning`]).
/// Halos include the spell, bolt, crystal and far-glass glows; particles
/// and spell meshes are drawn by `crate::fx` and aren't capped yet.
pub fn cap_overdraw(
    mut commands: Commands,
    time: Res<Time>,
    tuning: Res<Tuning>,
    settings: Res<LookSettings>,
    target: Option<Res<WorldTarget>>,
    camera: Query<(&GlobalTransform, &Projection), With<MainCamera>>,
    owners: Query<(&Halo, &InheritedVisibility)>,
    mut sprites: Query<(
        Entity,
        &HaloSprite,
        &GlobalTransform,
        Option<&RenderLayers>,
        Option<&mut OverdrawState>,
        &mut MeshTag,
        &mut Visibility,
    )>,
    mut scratch: Local<OverdrawScratch>,
) {
    let perf = &tuning.perf;
    if !perf.overdraw_cap {
        return;
    }
    let Some((eye, Projection::Perspective(projection))) = camera.iter().next() else {
        return;
    };
    let aspect = target.map_or(16.0 / 10.0, |t| t.size.x as f32 / t.size.y.max(1) as f32);
    let eye = eye.translation();
    let now = time.elapsed_secs();
    let scratch = &mut *scratch;
    scratch.glows.clear();
    scratch.entities.clear();
    for (entity, sprite, at, layers, state, ..) in &sprites {
        if layers.is_some_and(|l| l.intersects(&RenderLayers::layer(VIEWMODEL_LAYER))) {
            continue;
        }
        let Ok((_, shown)) = owners.get(sprite.owner) else {
            continue;
        };
        let Some(state) = state else {
            commands.entity(entity).insert(OverdrawState {
                shown_at: now,
                fade: 1.0,
                hidden: false,
            });
            continue;
        };
        if !shown.get() {
            continue;
        }
        let distance = at.translation().distance(eye);
        scratch.glows.push(Glow {
            coverage: screen_coverage(at.scale().x, distance, projection.fov, aspect),
            distance,
            age_s: now - state.shown_at,
        });
        scratch.entities.push(entity);
    }
    overdraw_victims(
        &scratch.glows,
        perf.overdraw_max_screens,
        perf.overdraw_keep_m,
        &mut scratch.order,
        &mut scratch.victims,
    );
    let rate = time.delta_secs() / perf.overdraw_fade_s.max(1e-3);
    for (entity, sprite, _, _, state, mut tag, mut visibility) in &mut sprites {
        let Some(mut state) = state else { continue };
        let Ok((halo, shown)) = owners.get(sprite.owner) else {
            continue;
        };
        if !shown.get() {
            // Hidden by its owner: it reappears new and in full.
            if state.fade < 1.0 || state.hidden {
                state.fade = 1.0;
                state.hidden = false;
            }
            state.shown_at = now;
            continue;
        }
        let victim = scratch
            .entities
            .iter()
            .position(|e| *e == entity)
            .is_some_and(|i| scratch.victims.contains(&i));
        let goal = if victim { 0.0 } else { 1.0 };
        if state.fade == goal {
            continue;
        }
        state.fade = if victim {
            (state.fade - rate).max(0.0)
        } else {
            (state.fade + rate).min(1.0)
        };
        tag.set_if_neq(MeshTag(pack_halo(halo.color, halo.intensity * state.fade)));
        let hide = state.fade <= 0.0;
        if hide != state.hidden {
            state.hidden = hide;
            let wanted = if hide || !settings.halos {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
            visibility.set_if_neq(wanted);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn far_knights_step_at_30_hz_with_all_the_time_banked() {
        let dt = 1.0 / 60.0;
        let mut pending = 0.0;
        let steps: Vec<Option<f32>> = (0..6)
            .map(|_| lod_due(&mut pending, dt, 30.0, false, 20.0, 30.0))
            .collect();
        assert_eq!(steps.iter().filter(|s| s.is_some()).count(), 3);
        let total: f32 = steps.iter().flatten().sum();
        assert!((total - 6.0 * dt).abs() < 1e-5, "no time is lost");
        assert!(steps.iter().flatten().all(|s| (s - 2.0 * dt).abs() < 1e-5));
        // Near, or just hit: every frame.
        let mut pending = 0.0;
        assert_eq!(lod_due(&mut pending, dt, 12.0, false, 20.0, 30.0), Some(dt));
        assert_eq!(lod_due(&mut pending, dt, 30.0, false, 20.0, 30.0), None);
        assert_eq!(
            lod_due(&mut pending, dt, 30.0, true, 20.0, 30.0),
            Some(2.0 * dt)
        );
        // 0 Hz turns the lever off.
        assert_eq!(lod_due(&mut 0.0, dt, 90.0, false, 20.0, 0.0), Some(dt));
    }

    #[test]
    fn dynamic_resolution_steps_once_per_hold_with_hysteresis() {
        let t = PerfTuning::default();
        let mut d = DynamicResolution::default();
        // Over budget: one step down, then nothing for half a second.
        assert!(d.observe(0.0, 16.0, &t));
        assert_eq!(d.scale, 0.95);
        assert!(!d.observe(0.2, 16.0, &t));
        assert!(!d.observe(0.45, 16.0, &t));
        assert!(d.observe(0.55, 16.0, &t));
        assert_eq!(d.scale, 0.9);
        // Never below 0.8.
        for i in 0..20 {
            d.observe(1.0 + i as f64, 30.0, &t);
        }
        assert_eq!(d.scale, 0.8);
        // Between the thresholds it holds: no pumping.
        for i in 0..40 {
            d.observe(30.0 + i as f64, 11.0, &t);
        }
        assert_eq!(d.scale, 0.8);
        // Well under budget: back up to 1.0 and no further.
        for i in 0..40 {
            d.observe(100.0 + i as f64, 6.0, &t);
        }
        assert_eq!(d.scale, 1.0);
        // The smoothing rides out a single spike.
        let mut d = DynamicResolution::default();
        d.observe(0.0, 10.0, &t);
        assert!(!d.observe(5.0, 15.0, &t));
        assert_eq!(d.scale, 1.0);
        assert!(d.smoothed_ms.unwrap() < 12.5);
        d.reset();
        assert_eq!(d, DynamicResolution::default());
    }

    #[test]
    fn coverage_is_the_projected_disc_share_of_the_screen() {
        let fov = 70f32.to_radians();
        let near = screen_coverage(1.0, 2.0, fov, 1.6);
        let far = screen_coverage(1.0, 4.0, fov, 1.6);
        assert!((near / far - 4.0).abs() < 1e-3, "inverse square");
        assert_eq!(screen_coverage(50.0, 1.0, fov, 1.6), 1.0);
        assert!(screen_coverage(0.3, 60.0, fov, 1.6) < 1e-4);
    }

    #[test]
    fn past_the_cap_the_oldest_fade_and_the_nearest_stay() {
        let g = |coverage, distance, age_s| Glow {
            coverage,
            distance,
            age_s,
        };
        let glows = [
            g(0.5, 3.0, 9.0),  // oldest but near: kept
            g(0.4, 20.0, 5.0), // oldest far one: first to go
            g(0.4, 30.0, 1.0),
            g(0.4, 25.0, 3.0), // second to go
            g(0.2, 12.0, 0.1),
        ];
        let (mut order, mut victims) = (Vec::new(), Vec::new());
        overdraw_victims(&glows, 1.2, 6.0, &mut order, &mut victims);
        assert_eq!(victims, [1, 3]);
        overdraw_victims(&glows, 5.0, 6.0, &mut order, &mut victims);
        assert!(victims.is_empty(), "under the cap nothing fades");
        // Only near glows left: they stay even over the cap.
        overdraw_victims(&glows[..1], 0.1, 6.0, &mut order, &mut victims);
        assert!(victims.is_empty());
    }
}
