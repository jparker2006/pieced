//! Far-view motion: the turning galaxy, bobbing islands, ships on looping
//! splines and pulsing stained glass. Every system here is a pure transform or
//! parameter update driven by [`SkyClock`], so it runs headless and tests can
//! step it ([`FarMotionPlugin`] needs no renderer, window or assets on disk).

use crate::look::{FarMaterial, Halo, LookSettings};
use bevy::{core_pipeline::Skybox, prelude::*};
use std::{f32::consts::TAU, sync::Arc};

/// Seconds of far-view time. It follows real time (hit-stop frames don't
/// freeze the sky); scenarios can pause it or set it for repeatable shots.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct SkyClock {
    pub seconds: f64,
    pub paused: bool,
}

/// Longest step the clock takes in one frame (a stall doesn't teleport ships).
pub const MAX_CLOCK_STEP: f64 = 0.25;

// ---------------------------------------------------------------------------
// Galaxy
// ---------------------------------------------------------------------------

/// One revolution every 10 minutes (docs/M2-SPEC.md → The sky and far view).
pub const GALAXY_PERIOD_S: f64 = 600.0;

/// How the galaxy sky turns: about its own core's direction, so the spiral spins
/// in place upper-left of the spawn view while the stars wheel around it.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct GalaxySpin {
    /// World direction of the galaxy's core (the rotation axis).
    pub axis: Vec3,
    pub period_s: f64,
}

/// Marks the camera whose `Skybox` shows the galaxy.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct GalaxySky;

/// The galaxy's rotation after `seconds` (angle `TAU * seconds / period`).
pub fn galaxy_rotation(spin: &GalaxySpin, seconds: f64) -> Quat {
    let turns = (seconds / spin.period_s).rem_euclid(1.0);
    Quat::from_axis_angle(
        spin.axis.normalize(),
        (turns * std::f64::consts::TAU) as f32,
    )
}

// ---------------------------------------------------------------------------
// Islands
// ---------------------------------------------------------------------------

/// A far island floating up and down around `base`.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Bob {
    pub base: Vec3,
    /// Metres either side of `base` (1–3 m).
    pub amplitude: f32,
    /// Seconds per bob (6–10 s).
    pub period: f32,
    /// Radians, so neighbours are out of step.
    pub phase: f32,
}

impl Bob {
    /// Height offset from `base` at `seconds`.
    pub fn offset(&self, seconds: f64) -> f32 {
        let turns = (seconds / self.period as f64).rem_euclid(1.0) as f32;
        self.amplitude * (turns * TAU + self.phase).sin()
    }
}

// ---------------------------------------------------------------------------
// Ships
// ---------------------------------------------------------------------------

/// A closed Catmull-Rom loop flown at constant speed (arc-length sampled).
#[derive(Debug, Clone, PartialEq)]
pub struct ShipLoop {
    points: Vec<Vec3>,
    /// (distance along the loop, curve parameter) samples, distance ascending.
    table: Vec<(f32, f32)>,
    length: f32,
    /// Seconds per lap.
    lap_s: f32,
}

impl ShipLoop {
    const SAMPLES_PER_SPAN: usize = 48;

    /// A loop through `points` (at least 3), one lap every `lap_s` seconds.
    pub fn new(points: Vec<Vec3>, lap_s: f32) -> Self {
        assert!(points.len() >= 3, "a loop needs at least 3 points");
        let mut this = Self {
            points,
            table: Vec::new(),
            length: 0.0,
            lap_s,
        };
        let steps = this.points.len() * Self::SAMPLES_PER_SPAN;
        let mut last = this.curve(0.0);
        let mut d = 0.0;
        this.table.push((0.0, 0.0));
        for i in 1..=steps {
            let u = i as f32 / steps as f32 * this.points.len() as f32;
            let p = this.curve(u);
            d += p.distance(last);
            this.table.push((d, u));
            last = p;
        }
        this.length = d;
        this
    }

    pub fn points(&self) -> &[Vec3] {
        &self.points
    }

    pub fn length(&self) -> f32 {
        self.length
    }

    pub fn lap_seconds(&self) -> f32 {
        self.lap_s
    }

    pub fn speed(&self) -> f32 {
        self.length / self.lap_s
    }

    /// The curve at parameter `u` (0..points, wrapping).
    pub fn curve(&self, u: f32) -> Vec3 {
        let n = self.points.len();
        let u = u.rem_euclid(n as f32);
        let i = u.floor() as usize % n;
        let t = u - u.floor();
        let p = |k: isize| self.points[(i as isize + k).rem_euclid(n as isize) as usize];
        let (p0, p1, p2, p3) = (p(-1), p(0), p(1), p(2));
        let (t2, t3) = (t * t, t * t * t);
        0.5 * (2.0 * p1
            + (p2 - p0) * t
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
            + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3)
    }

    fn parameter_at(&self, distance: f32) -> f32 {
        let d = distance.rem_euclid(self.length);
        let i = self.table.partition_point(|&(s, _)| s < d).max(1);
        let (s0, u0) = self.table[i - 1];
        let (s1, u1) = self.table[i.min(self.table.len() - 1)];
        let t = if s1 > s0 { (d - s0) / (s1 - s0) } else { 0.0 };
        u0 + (u1 - u0) * t
    }

    /// Position and unit heading `distance` metres along the loop.
    pub fn at_distance(&self, distance: f32) -> (Vec3, Vec3) {
        let u = self.parameter_at(distance);
        let p = self.curve(u);
        let ahead = self.curve(u + 0.01);
        (p, (ahead - p).normalize_or(Vec3::NEG_Z))
    }

    /// Where a ship that started at `start_fraction` of a lap is after `seconds`.
    pub fn at_time(&self, seconds: f64, start_fraction: f32) -> (Vec3, Vec3) {
        let laps = (seconds / self.lap_s as f64).rem_euclid(1.0) as f32 + start_fraction;
        self.at_distance(laps * self.length)
    }

    /// Distance from `p` to the loop (sampled densely).
    pub fn distance_to(&self, p: Vec3) -> f32 {
        let steps = self.points.len() * Self::SAMPLES_PER_SPAN * 4;
        (0..steps)
            .map(|i| {
                self.curve(i as f32 / steps as f32 * self.points.len() as f32)
                    .distance(p)
            })
            .fold(f32::MAX, f32::min)
    }
}

/// A ship flying `path` (shared by every ship on that loop).
#[derive(Component, Debug, Clone)]
pub struct ShipFlight {
    pub path: Arc<ShipLoop>,
    /// Where on the lap it was at time 0 (0..1).
    pub start: f32,
    /// Most roll into turns (radians).
    pub max_bank: f32,
}

impl ShipFlight {
    /// The ship's transform (translation and rotation) at `seconds`: facing
    /// along the loop and banked into its turns.
    pub fn transform(&self, seconds: f64) -> Transform {
        let (p, heading) = self.path.at_time(seconds, self.start);
        let (_, ahead) = self.path.at_time(seconds + 0.4, self.start);
        // Turning left (heading swinging toward +X × ...) rolls left.
        let turn = heading.cross(ahead).y;
        let bank = (turn * 6.0).clamp(-1.0, 1.0) * self.max_bank;
        let facing = Transform::from_translation(p).looking_to(heading, Vec3::Y);
        Transform {
            rotation: facing.rotation * Quat::from_rotation_z(bank),
            ..facing
        }
    }
}

// ---------------------------------------------------------------------------
// Stained glass
// ---------------------------------------------------------------------------

/// Seconds per stained-glass pulse (4–6 s).
pub const GLASS_PERIOD_S: f32 = 5.0;

/// Pulse level 0..1 at `seconds` for a part `phase` turns out of step.
pub fn glass_level(seconds: f64, phase: f32) -> f32 {
    let turns = (seconds / GLASS_PERIOD_S as f64).rem_euclid(1.0) as f32 + phase;
    0.5 + 0.5 * (turns * TAU).sin()
}

/// One pulsing glass material: brightness and emissive follow [`glass_level`].
#[derive(Debug, Clone)]
pub struct GlassPane {
    pub material: Handle<FarMaterial>,
    pub phase: f32,
    /// Base-color multiplier at level 0 and 1 (lights the glass in its own colors).
    pub brightness: (f32, f32),
    /// Emissive color (a pale wash) and its strength at level 0 and 1.
    pub emissive: Color,
    pub strength: (f32, f32),
}

impl GlassPane {
    /// (brightness, emissive strength) at `seconds`.
    pub fn at(&self, seconds: f64) -> (f32, f32) {
        let k = glass_level(seconds, self.phase);
        (
            self.brightness.0 + (self.brightness.1 - self.brightness.0) * k,
            self.strength.0 + (self.strength.1 - self.strength.0) * k,
        )
    }
}

/// Every pulsing glass material.
#[derive(Resource, Debug, Clone, Default)]
pub struct GlassPulse {
    pub panes: Vec<GlassPane>,
}

/// A halo that breathes with the glass: intensity from `low` to `high`.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct GlassGlow {
    pub phase: f32,
    pub low: f32,
    pub high: f32,
}

// ---------------------------------------------------------------------------
// Plugin and systems
// ---------------------------------------------------------------------------

/// The motion systems (headless-safe). `FarViewPlugin` adds it; tests add it to
/// a minimal app.
pub struct FarMotionPlugin;

/// Motion runs in `Update`, in this set, after the clock ticks.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FarMotionSet;

impl Plugin for FarMotionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SkyClock>()
            .init_resource::<GlassPulse>()
            .add_systems(
                Update,
                (
                    tick_sky_clock,
                    (spin_galaxy, bob_islands, fly_ships, pulse_glass),
                )
                    .chain()
                    .in_set(FarMotionSet),
            );
    }
}

pub fn tick_sky_clock(time: Res<Time<Real>>, mut clock: ResMut<SkyClock>) {
    if !clock.paused {
        clock.seconds += time.delta_secs_f64().min(MAX_CLOCK_STEP);
    }
}

/// Turns every galaxy skybox (unless `skyrot=off`, which freezes it).
pub fn spin_galaxy(
    clock: Res<SkyClock>,
    spin: Option<Res<GalaxySpin>>,
    settings: Option<Res<LookSettings>>,
    mut skies: Query<&mut Skybox, With<GalaxySky>>,
) {
    let Some(spin) = spin else { return };
    if settings.is_some_and(|s| !s.sky_rotation) {
        return;
    }
    let rotation = galaxy_rotation(&spin, clock.seconds);
    for mut sky in &mut skies {
        sky.rotation = rotation;
    }
}

pub fn bob_islands(clock: Res<SkyClock>, mut islands: Query<(&Bob, &mut Transform)>) {
    for (bob, mut transform) in &mut islands {
        transform.translation = bob.base + Vec3::Y * bob.offset(clock.seconds);
    }
}

pub fn fly_ships(clock: Res<SkyClock>, mut ships: Query<(&ShipFlight, &mut Transform)>) {
    for (flight, mut transform) in &mut ships {
        let t = flight.transform(clock.seconds);
        transform.translation = t.translation;
        transform.rotation = t.rotation;
    }
}

pub fn pulse_glass(
    clock: Res<SkyClock>,
    pulse: Res<GlassPulse>,
    mut materials: ResMut<Assets<FarMaterial>>,
    mut glows: Query<(&GlassGlow, &mut Halo)>,
) {
    for pane in &pulse.panes {
        let (brightness, strength) = pane.at(clock.seconds);
        if let Some(mut m) = materials.get_mut(&pane.material) {
            m.base_color = Color::linear_rgb(brightness, brightness, brightness);
            m.emissive = pane.emissive;
            m.emissive_strength = strength;
        }
    }
    for (glow, mut halo) in &mut glows {
        let k = glass_level(clock.seconds, glow.phase);
        halo.intensity = glow.low + (glow.high - glow.low) * k;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_catmull_rom_loop_passes_through_its_points_and_closes() {
        let pts = vec![
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(0.0, 2.0, 10.0),
            Vec3::new(-10.0, 0.0, 0.0),
            Vec3::new(0.0, -2.0, -10.0),
        ];
        let path = ShipLoop::new(pts.clone(), 20.0);
        for (i, p) in pts.iter().enumerate() {
            assert!(path.curve(i as f32).distance(*p) < 1e-4);
        }
        assert!(path.curve(4.0).distance(pts[0]) < 1e-4, "closed");
        // A loop round a 10 m "circle" is a bit over 2π·10 long.
        assert!((60.0..70.0).contains(&path.length()), "{}", path.length());
        // Constant speed: equal times cover equal arcs.
        let (a, _) = path.at_time(0.0, 0.0);
        let (b, _) = path.at_time(1.0, 0.0);
        let (c, _) = path.at_time(2.0, 0.0);
        let (d1, d2) = (a.distance(b), b.distance(c));
        assert!((d1 - d2).abs() < 0.05 * d1, "{d1} vs {d2}");
        assert!((path.speed() - path.length() / 20.0).abs() < 1e-4);
    }

    #[test]
    fn ships_face_along_their_loop() {
        let path = Arc::new(ShipLoop::new(
            vec![
                Vec3::new(100.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 100.0),
                Vec3::new(-100.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, -100.0),
            ],
            30.0,
        ));
        let ship = ShipFlight {
            path: path.clone(),
            start: 0.0,
            max_bank: 0.4,
        };
        for t in [0.0, 3.0, 11.5] {
            let tr = ship.transform(t);
            let (_, heading) = path.at_time(t, 0.0);
            assert!(tr.forward().dot(heading) > 0.9, "t {t}");
            // Banked, but never upside down.
            assert!(tr.up().y > 0.8);
        }
    }

    #[test]
    fn galaxy_turns_about_its_core() {
        let spin = GalaxySpin {
            axis: Vec3::new(-0.3, 0.47, -0.82).normalize(),
            period_s: GALAXY_PERIOD_S,
        };
        let r = galaxy_rotation(&spin, 150.0);
        assert!(
            (r * spin.axis - spin.axis).length() < 1e-5,
            "the core stays put"
        );
        assert!((r.to_axis_angle().1 - TAU / 4.0).abs() < 1e-4);
        assert!(galaxy_rotation(&spin, 600.0).angle_between(Quat::IDENTITY) < 1e-4);
    }
}
