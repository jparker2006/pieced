//! The castle's magic and the magical sky (M3: D64, D67, D94; target
//! `M3-C4-citadel-mix.png`).
//!
//! - **Rune ring** ([`RuneRing`]): one golden hoop of runes orbiting the
//!   castle, tilted toward the arena, turning slowly about its own axis.
//! - **Lanterns** ([`Lantern`]): a swarm of paper sky lanterns drifting and
//!   bobbing round the castle, and streams of them rising off its towers into
//!   the sky. One shared mesh and material, so they draw as one instanced batch.
//! - **Embers** ([`Ember`]): gold sparkles drifting up off the castle's glow.
//! - **Motes** ([`Mote`]): soft specks floating round the arena and the far
//!   islands, each inside its own box (at most [`MAX_MOTES`]).
//! - **Shooting stars** ([`ShootingStars`]): a streak across the sky every
//!   8–20 s on a seeded schedule, from a pool of [`SHOOTING_STAR_POOL`].
//! - **Galaxy shimmer** ([`Twinkle`], [`SkyGlow`]): sparkles over the galaxy
//!   twinkling on a [`SHIMMER_PERIOD_S`] cycle, and teal-violet aurora ribbons
//!   ([`Aurora`]) drifting round it; both turn with the galaxy.
//!
//! Everything is a pure function of [`SkyClock`] time, so the motion runs
//! headless and tests can step it; the entities are spawned without meshes by
//! [`spawn_sky_magic`] and dressed once the far assets exist. Every mesh here
//! is built in code with the far models' vertex layout (position, normal,
//! COLOR_0), drawn by [`FarMaterial`] (unlit; the glows additive), so no new
//! pipeline is needed. The motion systems only write transforms and a few
//! material colours: nothing allocates per frame.

use super::{
    galaxy::sky_point,
    layout::FarLayout,
    motion::{GalaxySpin, SkyClock, galaxy_rotation},
};
use crate::look::{FarMaterial, LookSettings};
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use std::f32::consts::{PI, TAU};

// ---------------------------------------------------------------------------
// Numbers
// ---------------------------------------------------------------------------

/// The rune ring (station frame, metres): radius, band height, centre height
/// over the plaza, tilt toward the arena (its front dips), roll, and seconds
/// per turn.
pub const RING_RADIUS: f32 = 226.0;
pub const RING_BAND: f32 = 13.0;
pub const RING_HEIGHT: f32 = 132.0;
pub const RING_TILT_DEG: f32 = 34.0;
pub const RING_ROLL_DEG: f32 = 0.0;
pub const RING_PERIOD_S: f32 = 150.0;
/// Rune segments round the ring.
pub const RING_SEGMENTS: usize = 96;

/// Lanterns drifting round the castle, and the rising streams.
pub const SWARM_LANTERNS: usize = 150;
/// A lantern's glow, in lantern sizes across.
pub const LANTERN_GLOW: f32 = 3.2;
pub const LANTERN_STREAMS: usize = 4;
pub const LANTERNS_PER_STREAM: usize = 18;
/// Gold sparkles rising off the castle's glow.
pub const EMBERS: usize = 36;
/// The spec's cap on drifting motes (docs/M3-SPEC.md → The castle and the sky).
pub const MAX_MOTES: usize = 64;
pub const ARENA_MOTES: usize = 22;
pub const FAR_MOTES: usize = 26;
/// Sparkles twinkling over the galaxy.
pub const TWINKLES: usize = 28;
/// Seconds per galaxy shimmer (the spec's 4–8 s cycle).
pub const SHIMMER_PERIOD_S: f32 = 6.0;
/// Seconds per aurora breath (their brightness) and sway.
pub const AURORA_BREATH_S: f32 = 11.0;
pub const AURORA_SWAY_S: f32 = 48.0;
/// Radius (m, from the arena centre) of the sky layer: shimmer, aurora and
/// shooting stars sit here, behind every far model and inside the far plane.
pub const SKY_RADIUS: f32 = 1150.0;
/// Shooting stars: the gap between two (s) and the pool.
pub const SHOOTING_STAR_GAP_S: (f32, f32) = (8.0, 20.0);
pub const SHOOTING_STAR_POOL: usize = 3;
pub const SHOOTING_STAR_SEED: u64 = 0x5EED_57A2;

// ---------------------------------------------------------------------------
// Seeded randomness
// ---------------------------------------------------------------------------

/// SplitMix64: a tiny deterministic generator for placing things.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in 0..1.
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / 16_777_216.0
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }
}

fn wave(seconds: f64, period: f32, phase: f32) -> f32 {
    let turns = (seconds / period as f64).rem_euclid(1.0) as f32;
    (turns * TAU + phase).sin()
}

// ---------------------------------------------------------------------------
// Rune ring
// ---------------------------------------------------------------------------

/// The golden rune ring: turns about its own (tilted) axis once per `period_s`.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct RuneRing {
    /// The ring's tilt; its axis is `tilt * Y`.
    pub tilt: Quat,
    pub period_s: f32,
}

impl RuneRing {
    pub fn standard() -> Self {
        Self {
            tilt: Quat::from_rotation_z(RING_ROLL_DEG.to_radians())
                * Quat::from_rotation_x(-RING_TILT_DEG.to_radians()),
            period_s: RING_PERIOD_S,
        }
    }

    pub fn axis(&self) -> Vec3 {
        self.tilt * Vec3::Y
    }

    /// The ring's rotation at `seconds`.
    pub fn rotation(&self, seconds: f64) -> Quat {
        let turns = (seconds / self.period_s as f64).rem_euclid(1.0) as f32;
        self.tilt * Quat::from_rotation_y(turns * TAU)
    }
}

// ---------------------------------------------------------------------------
// Lanterns, embers, motes
// ---------------------------------------------------------------------------

/// How a lantern moves (station frame, metres and seconds).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LanternPath {
    /// Drifts round `home` in a slow loop and bobs up and down.
    Swarm {
        home: Vec3,
        drift: Vec2,
        drift_s: f32,
        bob: f32,
        bob_s: f32,
        phase: f32,
    },
    /// Rises from `from` along `rise` over `life_s`, swaying, then starts
    /// again (shrinking out at the top and growing in at the bottom).
    Stream {
        from: Vec3,
        rise: Vec3,
        life_s: f32,
        sway: f32,
        phase: f32,
    },
}

/// A floating sky lantern.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Lantern {
    pub path: LanternPath,
    pub size: f32,
}

impl Lantern {
    /// Position and scale at `seconds`.
    pub fn at(&self, seconds: f64) -> (Vec3, f32) {
        match self.path {
            LanternPath::Swarm {
                home,
                drift,
                drift_s,
                bob,
                bob_s,
                phase,
            } => {
                let a = wave(seconds, drift_s, phase);
                let b = wave(seconds, drift_s, phase + PI / 2.0);
                let p = home
                    + Vec3::new(
                        drift.x * a,
                        bob * wave(seconds, bob_s, phase * 1.7),
                        drift.y * b,
                    );
                (p, self.size)
            }
            LanternPath::Stream {
                from,
                rise,
                life_s,
                sway,
                phase,
            } => {
                let k = stream_fraction(seconds, life_s, phase);
                let side = Vec3::new(rise.z, 0.0, -rise.x).normalize_or_zero();
                let p = from + rise * k + side * sway * (k * 9.0 + phase * TAU).sin();
                (p, self.size * stream_scale(k))
            }
        }
    }
}

/// How far (0..1) along its stream a lantern is at `seconds`.
pub fn stream_fraction(seconds: f64, life_s: f32, phase: f32) -> f32 {
    ((seconds / life_s as f64).rem_euclid(1.0) as f32 + phase).fract()
}

/// A stream lantern's size (0..1) at fraction `k`: grows in over the first
/// 6%, shrinks out over the last 12%.
pub fn stream_scale(k: f32) -> f32 {
    (k / 0.06).min((1.0 - k) / 0.12).clamp(0.0, 1.0)
}

/// A gold sparkle rising off the castle's glow, shrinking as it goes.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Ember {
    pub from: Vec3,
    pub rise: Vec3,
    pub life_s: f32,
    pub phase: f32,
    pub size: f32,
}

impl Ember {
    pub fn at(&self, seconds: f64) -> (Vec3, f32) {
        let k = stream_fraction(seconds, self.life_s, self.phase);
        let s = (k / 0.1).min(1.0 - k).clamp(0.0, 1.0);
        (self.from + self.rise * k, self.size * s)
    }
}

/// A soft glowing speck floating inside the box `centre ± extent`.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Mote {
    pub centre: Vec3,
    pub extent: Vec3,
    /// Seconds per swing along x, y, z.
    pub periods: Vec3,
    pub phase: Vec3,
    pub size: f32,
}

impl Mote {
    pub fn at(&self, seconds: f64) -> Vec3 {
        self.centre
            + self.extent
                * Vec3::new(
                    wave(seconds, self.periods.x, self.phase.x),
                    wave(seconds, self.periods.y, self.phase.y),
                    wave(seconds, self.periods.z, self.phase.z),
                )
    }
}

// ---------------------------------------------------------------------------
// Galaxy shimmer and aurora
// ---------------------------------------------------------------------------

/// The frame of the galaxy's sky layer: turns with the galaxy (see
/// [`galaxy_rotation`]), so the shimmer and aurora stay on it.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct GalaxyLayer;

/// A sparkle over the galaxy that swells and fades on the shimmer cycle.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Twinkle {
    pub size: f32,
    pub phase: f32,
}

impl Twinkle {
    /// Scale at `seconds` (between 25% and 100% of `size`).
    pub fn scale(&self, seconds: f64) -> f32 {
        let k = 0.5 + 0.5 * wave(seconds, SHIMMER_PERIOD_S, self.phase);
        self.size * (0.25 + 0.75 * k * k)
    }
}

/// An aurora ribbon swaying about the galaxy's axis.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Aurora {
    pub axis: Vec3,
    /// Radians either side of its rest.
    pub sway: f32,
    pub phase: f32,
}

impl Aurora {
    pub fn rotation(&self, seconds: f64) -> Quat {
        Quat::from_axis_angle(
            self.axis,
            self.sway * wave(seconds, AURORA_SWAY_S, self.phase),
        )
    }
}

/// Breathing sky materials: the galaxy shimmer's sparkles and the aurora.
#[derive(Resource, Debug, Clone, Default)]
pub struct SkyGlow {
    pub twinkle: Option<Handle<FarMaterial>>,
    pub aurora: Option<Handle<FarMaterial>>,
}

/// Twinkle material brightness (0.55..1) at `seconds`: the galaxy shimmer.
pub fn shimmer_level(seconds: f64) -> f32 {
    0.775 + 0.225 * wave(seconds, SHIMMER_PERIOD_S, 0.0)
}

/// Aurora brightness (0.55..1) at `seconds`.
pub fn aurora_level(seconds: f64) -> f32 {
    0.775 + 0.225 * wave(seconds, AURORA_BREATH_S, 1.3)
}

pub const TWINKLE_COLOR: [f32; 3] = [0.85, 0.9, 1.0];
pub const AURORA_COLOR: [f32; 3] = [1.0, 1.0, 1.0];

// ---------------------------------------------------------------------------
// Shooting stars
// ---------------------------------------------------------------------------

/// One shooting star's pass across the sky (world directions from the arena
/// centre).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StarPass {
    pub index: u64,
    pub start_s: f64,
    pub duration_s: f32,
    pub from: Vec3,
    /// Unit tangent it sets off along (perpendicular to `from`).
    pub heading: Vec3,
    /// Radians of sky it crosses.
    pub arc: f32,
}

impl StarPass {
    /// Head direction at fraction `u` (0..1) of the pass.
    pub fn direction(&self, u: f32) -> Vec3 {
        let a = self.arc * u;
        (self.from * a.cos() + self.heading * a.sin()).normalize()
    }

    pub fn end_s(&self) -> f64 {
        self.start_s + self.duration_s as f64
    }
}

/// The seeded shooting-star schedule, with a cursor so a frame never rescans it.
#[derive(Resource, Debug, Clone)]
pub struct ShootingStars {
    pub seed: u64,
    cursor: Option<StarPass>,
}

impl Default for ShootingStars {
    fn default() -> Self {
        Self {
            seed: SHOOTING_STAR_SEED,
            cursor: None,
        }
    }
}

impl ShootingStars {
    /// The gap (s) before pass `index` (after the previous one began).
    fn gap(&self, index: u64) -> f32 {
        let mut r = Rng::new(self.seed ^ index.wrapping_mul(0x2545_F491_4F6C_DD1D));
        r.range(SHOOTING_STAR_GAP_S.0, SHOOTING_STAR_GAP_S.1)
    }

    /// Pass `index`, given when it starts.
    fn pass(&self, index: u64, start_s: f64) -> StarPass {
        let mut r = Rng::new(self.seed ^ 0xA5A5 ^ index.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        r.next_u64();
        // Upper sky over the ways the player looks from the arena.
        let from = sky_point(r.range(-80.0, 110.0), r.range(28.0, 62.0));
        let east = Vec3::Y.cross(from).normalize();
        let up = from.cross(east).normalize();
        // Heading down and to one side.
        let psi = r.range(-160.0f32, -20.0).to_radians();
        let heading = (east * psi.cos() + up * psi.sin()).normalize();
        StarPass {
            index,
            start_s,
            duration_s: r.range(1.0, 1.6),
            from,
            heading,
            arc: r.range(16.0f32, 28.0).to_radians(),
        }
    }

    /// The first `n` passes, in order (for tests and review renders).
    pub fn passes(&self, n: usize) -> Vec<StarPass> {
        let mut out = Vec::with_capacity(n);
        let mut t = 0.0f64;
        for i in 0..n as u64 {
            t += self.gap(i) as f64;
            out.push(self.pass(i, t));
        }
        out
    }

    /// The latest pass that has started by `seconds`, if any.
    pub fn latest(&mut self, seconds: f64) -> Option<StarPass> {
        let first_start = self.gap(0) as f64;
        if seconds < first_start {
            self.cursor = None;
            return None;
        }
        let mut current = match self.cursor {
            Some(c) if c.start_s <= seconds => c,
            _ => self.pass(0, first_start),
        };
        loop {
            let next_start = current.start_s + self.gap(current.index + 1) as f64;
            if next_start > seconds {
                break;
            }
            current = self.pass(current.index + 1, next_start);
        }
        self.cursor = Some(current);
        Some(current)
    }
}

/// One of the pooled shooting-star streaks.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShootingStar {
    pub slot: usize,
}

/// Length (m) and width of a streak at full stretch.
pub const STREAK_LENGTH: f32 = 150.0;
pub const STREAK_WIDTH: f32 = 6.0;

/// A streak's transform for pass `pass` at `seconds`, or `None` when it isn't
/// flying. Its head leads along local -Z, the tail trails along +Z.
pub fn streak_transform(pass: &StarPass, seconds: f64) -> Option<Transform> {
    if seconds < pass.start_s || seconds > pass.end_s() {
        return None;
    }
    let u = ((seconds - pass.start_s) / pass.duration_s as f64) as f32;
    let head = pass.direction(u);
    let ahead = pass.direction(u + 0.01);
    let envelope = (u * PI).sin().max(0.0).sqrt();
    let p = head * SKY_RADIUS;
    let t = Transform::from_translation(p).looking_to((ahead - head).normalize(), head);
    Some(t.with_scale(Vec3::new(
        STREAK_WIDTH,
        STREAK_WIDTH,
        STREAK_LENGTH * envelope.max(0.02),
    )))
}

// ---------------------------------------------------------------------------
// The layout of the magic
// ---------------------------------------------------------------------------

/// What [`spawn_sky_magic`] puts where. Built from the [`FarLayout`] with a
/// fixed seed, so it is the same every launch.
#[derive(Resource, Debug, Clone)]
pub struct SkyMagic {
    pub ring: RuneRing,
    pub lanterns: Vec<Lantern>,
    pub embers: Vec<Ember>,
    /// Motes in world space.
    pub motes: Vec<Mote>,
    /// Twinkles: (sky direction, twinkle).
    pub twinkles: Vec<(Vec3, Twinkle)>,
    /// Aurora ribbons: (azimuth range, elevation, curtain height) in degrees.
    pub auroras: Vec<AuroraSpec>,
}

/// One aurora ribbon: a curtain over the sky from `az.0` to `az.1` (degrees),
/// its foot at `el.0` rising to `el.1` along it, `height` degrees tall.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AuroraSpec {
    pub az: (f32, f32),
    pub el: (f32, f32),
    pub height: f32,
    pub waves: f32,
    pub sway_deg: f32,
    pub phase: f32,
}

/// Where the rising streams leave the castle (station frame: -Z toward the
/// arena, +Y up from the plaza; +X is the arena's left seen from spawn), and
/// where they rise to.
const STREAMS: [(Vec3, Vec3); LANTERN_STREAMS] = [
    // Off the keep's crown, straight up and a little toward the arena.
    (Vec3::new(0.0, 300.0, 30.0), Vec3::new(10.0, 330.0, -90.0)),
    // Off the flanking towers, up and out to each side.
    (
        Vec3::new(56.0, 230.0, -60.0),
        Vec3::new(180.0, 300.0, -120.0),
    ),
    (
        Vec3::new(-56.0, 230.0, -60.0),
        Vec3::new(-200.0, 290.0, -60.0),
    ),
    // Off the great side tower on the right, up the sky's right.
    (
        Vec3::new(-136.0, 230.0, 6.0),
        Vec3::new(-150.0, 320.0, 60.0),
    ),
];

impl SkyMagic {
    pub fn new(layout: &FarLayout) -> Self {
        let mut r = Rng::new(0x00CA_571E);
        let mut lanterns = Vec::new();
        for i in 0..SWARM_LANTERNS {
            // Most in front of the castle and round its flanks, some behind.
            let behind = i % 5 == 4;
            let a = if behind {
                r.range(0.0, TAU)
            } else {
                PI + r.range(-1.9, 1.9)
            };
            let near = i % 3 == 0;
            let radius = if near {
                r.range(90.0, 190.0)
            } else {
                r.range(170.0, 330.0)
            };
            let height = if near {
                r.range(40.0, 300.0)
            } else {
                r.range(10.0, 360.0)
            };
            // `a` = 0 is behind the castle (+Z), PI toward the arena (-Z).
            let home = Vec3::new(a.sin() * radius, height, a.cos() * radius);
            lanterns.push(Lantern {
                path: LanternPath::Swarm {
                    home,
                    drift: Vec2::new(r.range(6.0, 16.0), r.range(6.0, 16.0)),
                    drift_s: r.range(40.0, 80.0),
                    bob: r.range(2.5, 6.0),
                    bob_s: r.range(5.0, 9.0),
                    phase: r.range(0.0, TAU),
                },
                size: r.range(4.2, 6.8),
            });
        }
        for (from, rise) in STREAMS {
            for k in 0..LANTERNS_PER_STREAM {
                let jitter = Vec3::new(r.range(-14.0, 14.0), 0.0, r.range(-14.0, 14.0));
                lanterns.push(Lantern {
                    path: LanternPath::Stream {
                        from: from + jitter,
                        rise: rise * r.range(0.85, 1.15),
                        life_s: r.range(52.0, 64.0),
                        sway: r.range(4.0, 12.0),
                        phase: (k as f32 + r.range(-0.3, 0.3)) / LANTERNS_PER_STREAM as f32,
                    },
                    size: r.range(4.8, 7.2),
                });
            }
        }
        let embers = (0..EMBERS)
            .map(|i| {
                let a = r.range(0.0, TAU);
                let d = r.range(0.0, 70.0);
                Ember {
                    from: Vec3::new(a.sin() * d, r.range(170.0, 290.0), a.cos() * d - 20.0),
                    rise: Vec3::new(
                        r.range(-40.0, 40.0),
                        r.range(120.0, 190.0),
                        r.range(-60.0, 0.0),
                    ),
                    life_s: r.range(16.0, 26.0),
                    phase: i as f32 / EMBERS as f32 + r.range(-0.02, 0.02),
                    size: r.range(2.2, 3.6),
                }
            })
            .collect();
        // Motes: round the arena (small, near), then round the far islands and
        // the castle (big, far away).
        let mut motes = Vec::new();
        for _ in 0..ARENA_MOTES {
            motes.push(Mote {
                centre: Vec3::new(
                    r.range(-30.0, 30.0),
                    r.range(4.0, 18.0),
                    r.range(-30.0, 30.0),
                ),
                extent: Vec3::new(r.range(3.0, 7.0), r.range(1.2, 3.0), r.range(3.0, 7.0)),
                periods: Vec3::new(r.range(14.0, 26.0), r.range(7.0, 12.0), r.range(16.0, 30.0)),
                phase: Vec3::new(r.range(0.0, TAU), r.range(0.0, TAU), r.range(0.0, TAU)),
                size: r.range(0.18, 0.32),
            });
        }
        let islands: Vec<Vec3> = layout.all_islands().map(|i| i.piece.position).collect();
        for k in 0..FAR_MOTES {
            let near = if k % 3 == 0 {
                layout.station.position
                    + Vec3::new(r.range(-200.0, 200.0), r.range(0.0, 220.0), 0.0)
            } else {
                islands[(k * 7) % islands.len()]
            };
            motes.push(Mote {
                centre: near
                    + Vec3::new(
                        r.range(-30.0, 30.0),
                        r.range(10.0, 50.0),
                        r.range(-30.0, 30.0),
                    ),
                extent: Vec3::new(r.range(10.0, 24.0), r.range(5.0, 12.0), r.range(10.0, 24.0)),
                periods: Vec3::new(r.range(18.0, 34.0), r.range(9.0, 15.0), r.range(18.0, 34.0)),
                phase: Vec3::new(r.range(0.0, TAU), r.range(0.0, TAU), r.range(0.0, TAU)),
                size: r.range(2.4, 4.0),
            });
        }
        // Twinkles scattered over the galaxy's disc (its core is `layout.galaxy`).
        let centre = layout.galaxy.normalize();
        let right = centre.cross(Vec3::Y).normalize();
        let up = right.cross(centre);
        let twinkles = (0..TWINKLES)
            .map(|i| {
                let rr = r.f32().sqrt() * 0.42;
                let a = r.range(0.0, TAU);
                let dir = (centre + right * rr * a.cos() + up * rr * a.sin() * 0.75).normalize();
                let size = if i % 5 == 0 {
                    r.range(26.0, 36.0)
                } else {
                    r.range(12.0, 22.0)
                };
                (
                    dir,
                    Twinkle {
                        size,
                        phase: r.range(0.0, TAU),
                    },
                )
            })
            .collect();
        let auroras = vec![
            // Sweeping under the galaxy from the left.
            AuroraSpec {
                az: (-78.0, -8.0),
                el: (12.0, 22.0),
                height: 11.0,
                waves: 2.5,
                sway_deg: 5.0,
                phase: 0.0,
            },
            // Arching over the galaxy.
            AuroraSpec {
                az: (-52.0, 6.0),
                el: (46.0, 40.0),
                height: 9.0,
                waves: 2.0,
                sway_deg: 4.0,
                phase: 2.1,
            },
            // The right of the sky, behind the castle and up past it.
            AuroraSpec {
                az: (40.0, 100.0),
                el: (30.0, 44.0),
                height: 12.0,
                waves: 3.0,
                sway_deg: 5.0,
                phase: 4.2,
            },
        ];
        Self {
            ring: RuneRing::standard(),
            lanterns,
            embers,
            motes,
            twinkles,
            auroras,
        }
    }
}

// ---------------------------------------------------------------------------
// Spawning (pure) and dressing
// ---------------------------------------------------------------------------

/// Which shared mesh and material an entity of the magic draws with.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MagicPart {
    Ring,
    Lantern,
    /// A lantern's soft orange glow (a child of the lantern).
    LanternGlow,
    Ember,
    Mote,
    Twinkle,
    Streak,
    Aurora(usize),
}

/// Spawns the magic's entities (no meshes yet: [`dress_sky_magic`] adds them):
/// the ring, lanterns and embers in the station's frame; the motes in the far
/// view; the shimmer, aurora and shooting stars in the galaxy layer.
pub fn spawn_sky_magic(
    mut commands: Commands,
    layout: Res<FarLayout>,
    stations: Query<Entity, With<super::FarStation>>,
    roots: Query<Entity, With<super::FarView>>,
) {
    let magic = SkyMagic::new(&layout);
    let station = stations.iter().next();
    let root = roots.iter().next();
    let in_station = |e: &mut EntityCommands| {
        if let Some(s) = station {
            e.insert(ChildOf(s));
        }
    };
    let mut e = commands.spawn((
        Name::new("Rune ring"),
        magic.ring,
        MagicPart::Ring,
        Transform::from_translation(Vec3::Y * RING_HEIGHT).with_rotation(magic.ring.rotation(0.0)),
        Visibility::default(),
    ));
    in_station(&mut e);
    for (i, lantern) in magic.lanterns.iter().enumerate() {
        let (p, s) = lantern.at(0.0);
        let mut e = commands.spawn((
            Name::new(format!("Lantern {i}")),
            *lantern,
            MagicPart::Lantern,
            Transform::from_translation(p)
                .with_scale(Vec3::splat(s))
                .with_rotation(Quat::from_rotation_y(i as f32 * 0.7)),
            Visibility::default(),
        ));
        in_station(&mut e);
        e.with_child((
            MagicPart::LanternGlow,
            Transform::from_xyz(0.0, 0.8, 0.0).with_scale(Vec3::splat(LANTERN_GLOW)),
            Visibility::default(),
        ));
    }
    for (i, ember) in magic.embers.iter().enumerate() {
        let (p, s) = ember.at(0.0);
        let mut e = commands.spawn((
            Name::new(format!("Ember {i}")),
            *ember,
            MagicPart::Ember,
            Transform::from_translation(p).with_scale(Vec3::splat(s)),
            Visibility::default(),
        ));
        in_station(&mut e);
    }
    for (i, mote) in magic.motes.iter().enumerate() {
        let mut e = commands.spawn((
            Name::new(format!("Mote {i}")),
            *mote,
            MagicPart::Mote,
            Transform::from_translation(mote.at(0.0)).with_scale(Vec3::splat(mote.size)),
            Visibility::default(),
        ));
        if let Some(r) = root {
            e.insert(ChildOf(r));
        }
    }
    let mut layer = commands.spawn((
        Name::new("Galaxy layer"),
        GalaxyLayer,
        Transform::IDENTITY,
        Visibility::default(),
    ));
    if let Some(r) = root {
        layer.insert(ChildOf(r));
    }
    let layer = layer.id();
    for (i, (dir, twinkle)) in magic.twinkles.iter().enumerate() {
        commands.spawn((
            Name::new(format!("Twinkle {i}")),
            *twinkle,
            MagicPart::Twinkle,
            Transform::from_translation(*dir * SKY_RADIUS)
                .looking_to(-*dir, Vec3::Y)
                .with_scale(Vec3::splat(twinkle.scale(0.0))),
            Visibility::default(),
            ChildOf(layer),
        ));
    }
    let axis = layout.galaxy.normalize();
    for (i, spec) in magic.auroras.iter().enumerate() {
        commands.spawn((
            Name::new(format!("Aurora {i}")),
            Aurora {
                axis,
                sway: spec.sway_deg.to_radians(),
                phase: spec.phase,
            },
            MagicPart::Aurora(i),
            Transform::IDENTITY,
            Visibility::default(),
            ChildOf(layer),
        ));
    }
    for slot in 0..SHOOTING_STAR_POOL {
        let mut e = commands.spawn((
            Name::new(format!("Shooting star {slot}")),
            ShootingStar { slot },
            MagicPart::Streak,
            Transform::IDENTITY,
            Visibility::Hidden,
        ));
        if let Some(r) = root {
            e.insert(ChildOf(r));
        }
    }
    commands.insert_resource(magic);
}

/// The magic's shared meshes and materials.
#[derive(Resource, Debug, Clone)]
pub struct MagicAssets {
    pub ring_mesh: Handle<Mesh>,
    pub lantern_mesh: Handle<Mesh>,
    pub glow_mesh: Handle<Mesh>,
    pub spark_mesh: Handle<Mesh>,
    pub streak_mesh: Handle<Mesh>,
    pub aurora_meshes: Vec<Handle<Mesh>>,
    pub ring: Handle<FarMaterial>,
    pub lantern: Handle<FarMaterial>,
    pub lantern_glow: Handle<FarMaterial>,
    pub ember: Handle<FarMaterial>,
    pub mote: Handle<FarMaterial>,
    pub twinkle: Handle<FarMaterial>,
    pub streak: Handle<FarMaterial>,
    pub aurora: Handle<FarMaterial>,
}

fn additive_rgb(rgb: [f32; 3]) -> FarMaterial {
    FarMaterial {
        base_color: Color::linear_rgba(rgb[0], rgb[1], rgb[2], 0.0),
        alpha_mode: AlphaMode::Add,
        haze: 0.0,
        ..default()
    }
}

impl MagicAssets {
    pub fn new(
        magic: &SkyMagic,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<FarMaterial>,
    ) -> Self {
        Self {
            ring_mesh: meshes.add(rune_ring_mesh(RING_RADIUS, RING_BAND, RING_SEGMENTS)),
            lantern_mesh: meshes.add(lantern_mesh()),
            glow_mesh: meshes.add(glow_mesh(6)),
            spark_mesh: meshes.add(spark_mesh()),
            streak_mesh: meshes.add(streak_mesh()),
            aurora_meshes: magic
                .auroras
                .iter()
                .map(|a| meshes.add(aurora_mesh(a)))
                .collect(),
            ring: materials.add(additive_rgb([1.25, 1.1, 0.9])),
            // Unlit and barely hazed: the lanterns glow in their own colours.
            lantern: materials
                .add(FarMaterial::new(Color::linear_rgb(0.95, 0.9, 0.9)).with_haze(0.12)),
            lantern_glow: materials.add(additive_rgb([0.5, 0.2, 0.05])),
            ember: materials.add(additive_rgb([1.3, 0.9, 0.35])),
            mote: materials.add(additive_rgb([0.55, 0.85, 0.8])),
            twinkle: materials.add(additive_rgb(TWINKLE_COLOR)),
            streak: materials.add(additive_rgb([1.2, 1.2, 1.3])),
            aurora: materials.add(additive_rgb(AURORA_COLOR)),
        }
    }

    pub fn mesh_and_material(&self, part: MagicPart) -> (Handle<Mesh>, Handle<FarMaterial>) {
        match part {
            MagicPart::Ring => (self.ring_mesh.clone(), self.ring.clone()),
            MagicPart::Lantern => (self.lantern_mesh.clone(), self.lantern.clone()),
            MagicPart::LanternGlow => (self.glow_mesh.clone(), self.lantern_glow.clone()),
            MagicPart::Ember => (self.spark_mesh.clone(), self.ember.clone()),
            MagicPart::Mote => (self.spark_mesh.clone(), self.mote.clone()),
            MagicPart::Twinkle => (self.spark_mesh.clone(), self.twinkle.clone()),
            MagicPart::Streak => (self.streak_mesh.clone(), self.streak.clone()),
            MagicPart::Aurora(i) => (self.aurora_meshes[i].clone(), self.aurora.clone()),
        }
    }
}

/// Builds the magic's meshes and materials (after [`spawn_sky_magic`]).
pub fn create_magic_assets(
    mut commands: Commands,
    magic: Res<SkyMagic>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FarMaterial>>,
    mut glow: ResMut<SkyGlow>,
) {
    let assets = MagicAssets::new(&magic, &mut meshes, &mut materials);
    glow.twinkle = Some(assets.twinkle.clone());
    glow.aurora = Some(assets.aurora.clone());
    commands.insert_resource(assets);
}

/// Gives every magic entity its mesh and material.
pub fn dress_sky_magic(
    mut commands: Commands,
    assets: Res<MagicAssets>,
    parts: Query<(Entity, &MagicPart), Without<Mesh3d>>,
) {
    for (e, part) in &parts {
        let (mesh, material) = assets.mesh_and_material(*part);
        commands
            .entity(e)
            .insert((Mesh3d(mesh), MeshMaterial3d(material)));
    }
}

// ---------------------------------------------------------------------------
// Motion
// ---------------------------------------------------------------------------

pub fn turn_rune_ring(clock: Res<SkyClock>, mut rings: Query<(&RuneRing, &mut Transform)>) {
    for (ring, mut t) in &mut rings {
        t.rotation = ring.rotation(clock.seconds);
    }
}

pub fn float_lanterns(
    clock: Res<SkyClock>,
    mut lanterns: Query<(&Lantern, &mut Transform), Without<Ember>>,
    mut embers: Query<(&Ember, &mut Transform), Without<Lantern>>,
) {
    for (lantern, mut t) in &mut lanterns {
        let (p, s) = lantern.at(clock.seconds);
        t.translation = p;
        t.scale = Vec3::splat(s);
    }
    for (ember, mut t) in &mut embers {
        let (p, s) = ember.at(clock.seconds);
        t.translation = p;
        t.scale = Vec3::splat(s);
    }
}

pub fn drift_motes(clock: Res<SkyClock>, mut motes: Query<(&Mote, &mut Transform)>) {
    for (mote, mut t) in &mut motes {
        t.translation = mote.at(clock.seconds);
    }
}

/// Turns the galaxy layer with the galaxy (unless `skyrot=off`), twinkles its
/// sparkles, sways the aurora and breathes both materials.
#[allow(clippy::type_complexity)]
pub fn shimmer_galaxy(
    clock: Res<SkyClock>,
    spin: Option<Res<GalaxySpin>>,
    settings: Option<Res<LookSettings>>,
    glow: Res<SkyGlow>,
    mut materials: ResMut<Assets<FarMaterial>>,
    mut layers: Query<&mut Transform, (With<GalaxyLayer>, Without<Twinkle>, Without<Aurora>)>,
    mut twinkles: Query<(&Twinkle, &mut Transform), (Without<GalaxyLayer>, Without<Aurora>)>,
    mut auroras: Query<(&Aurora, &mut Transform), (Without<GalaxyLayer>, Without<Twinkle>)>,
) {
    let t = clock.seconds;
    let turning = settings.is_none_or(|s| s.sky_rotation);
    if let Some(spin) = spin
        && turning
    {
        let rotation = galaxy_rotation(&spin, t);
        for mut layer in &mut layers {
            layer.rotation = rotation;
        }
    }
    for (twinkle, mut tr) in &mut twinkles {
        tr.scale = Vec3::splat(twinkle.scale(t));
    }
    for (aurora, mut tr) in &mut auroras {
        tr.rotation = aurora.rotation(t);
    }
    let breathe = |m: &mut FarMaterial, rgb: [f32; 3], k: f32| {
        m.base_color = Color::linear_rgba(rgb[0] * k, rgb[1] * k, rgb[2] * k, 0.0);
    };
    if let Some(h) = &glow.twinkle
        && let Some(mut m) = materials.get_mut(h)
    {
        breathe(&mut m, TWINKLE_COLOR, shimmer_level(t));
    }
    if let Some(h) = &glow.aurora
        && let Some(mut m) = materials.get_mut(h)
    {
        breathe(&mut m, AURORA_COLOR, aurora_level(t));
    }
}

pub fn fly_shooting_stars(
    clock: Res<SkyClock>,
    mut stars: ResMut<ShootingStars>,
    mut streaks: Query<(&ShootingStar, &mut Transform, &mut Visibility)>,
) {
    let t = clock.seconds;
    let latest = stars.latest(t);
    for (star, mut tr, mut vis) in &mut streaks {
        let shown = latest
            .filter(|p| p.index as usize % SHOOTING_STAR_POOL == star.slot)
            .and_then(|p| streak_transform(&p, t));
        match shown {
            Some(new) => {
                *tr = new;
                vis.set_if_neq(Visibility::Inherited);
            }
            None => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Meshes
// ---------------------------------------------------------------------------

/// Triangle soup with flat normals and vertex colours (linear rgb).
#[derive(Default)]
struct Soup {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
}

impl Soup {
    fn tri(&mut self, p: [Vec3; 3], c: [[f32; 3]; 3]) {
        let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or(Vec3::Y);
        for k in 0..3 {
            self.positions.push(p[k].to_array());
            self.normals.push(n.to_array());
            self.colors.push([c[k][0], c[k][1], c[k][2], 1.0]);
        }
    }

    /// A quad a-b-c-d (counter-clockwise from its front), one colour per corner.
    fn quad(&mut self, p: [Vec3; 4], c: [[f32; 3]; 4]) {
        self.tri([p[0], p[1], p[2]], [c[0], c[1], c[2]]);
        self.tri([p[0], p[2], p[3]], [c[0], c[2], c[3]]);
    }

    /// Both faces of a quad (for additive glows seen from either side).
    fn quad2(&mut self, p: [Vec3; 4], c: [[f32; 3]; 4]) {
        self.quad(p, c);
        self.quad([p[3], p[2], p[1], p[0]], [c[3], c[2], c[1], c[0]]);
    }

    fn mesh(self) -> Mesh {
        let n = self.positions.len() as u32;
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32((0..n).collect()))
    }
}

fn scale3(c: [f32; 3], k: f32) -> [f32; 3] {
    [c[0] * k, c[1] * k, c[2] * k]
}

/// Gold of the ring (linear): its rails, its runes, the faint band between.
const RING_GOLD: [f32; 3] = [1.0, 0.62, 0.16];
const RUNE_GOLD: [f32; 3] = [1.0, 0.8, 0.36];

/// The rune ring: a hoop of radius `radius` round +Y, `band` tall: two bright
/// gold rails, a faint glowing band between them and a rune on every segment
/// (a diamond, with little bars between), each face drawn from both sides.
pub fn rune_ring_mesh(radius: f32, band: f32, segments: usize) -> Mesh {
    let mut s = Soup::default();
    let at = |a: f32, y: f32, r: f32| Vec3::new(a.cos() * r, y, a.sin() * r);
    let h = band / 2.0;
    let rail = band * 0.13;
    let faint = scale3(RING_GOLD, 0.16);
    for i in 0..segments {
        let a0 = i as f32 / segments as f32 * TAU;
        let a1 = (i + 1) as f32 / segments as f32 * TAU;
        let am = (a0 + a1) / 2.0;
        let quad = |s: &mut Soup, y0: f32, y1: f32, c0: [f32; 3], c1: [f32; 3]| {
            s.quad2(
                [
                    at(a0, y0, radius),
                    at(a1, y0, radius),
                    at(a1, y1, radius),
                    at(a0, y1, radius),
                ],
                [c0, c0, c1, c1],
            );
        };
        // Rails, and the band between (brighter toward the rails).
        quad(&mut s, h - rail, h, RING_GOLD, RING_GOLD);
        quad(&mut s, -h, -h + rail, RING_GOLD, RING_GOLD);
        quad(&mut s, -h + rail, 0.0, faint, scale3(faint, 0.4));
        quad(&mut s, 0.0, h - rail, scale3(faint, 0.4), faint);
        // A rune: a diamond on odd segments, a pair of bars on even ones, a
        // hair outside the band (the glows add, so no fighting).
        let r = radius + 0.4;
        let da = (a1 - a0) * 0.32;
        let ry = h - rail * 1.6;
        if i % 2 == 1 {
            s.quad2(
                [
                    at(am, -ry, r),
                    at(am + da, 0.0, r),
                    at(am, ry, r),
                    at(am - da, 0.0, r),
                ],
                [RUNE_GOLD; 4],
            );
        } else {
            for k in [-0.5f32, 0.5] {
                let c = am + k * da;
                let w = da * 0.18;
                s.quad2(
                    [
                        at(c - w, -ry * 0.7, r),
                        at(c + w, -ry * 0.7, r),
                        at(c + w, ry * 0.7, r),
                        at(c - w, ry * 0.7, r),
                    ],
                    [scale3(RUNE_GOLD, 0.8); 4],
                );
            }
        }
    }
    s.mesh()
}

/// Lantern colours (linear): the glowing paper, redder at the top, the hot
/// opening at the bottom and the dark top.
const PAPER: [f32; 3] = [1.0, 0.36, 0.08];
const PAPER_TOP: [f32; 3] = [0.7, 0.1, 0.035];
const FLAME: [f32; 3] = [1.0, 0.8, 0.34];
const LANTERN_CAP: [f32; 3] = [0.3, 0.07, 0.04];

/// A paper sky lantern about 1.5 units tall (C2, C4): a square body widening
/// toward its top, glowing orange with a hot gold opening underneath.
pub fn lantern_mesh() -> Mesh {
    let mut s = Soup::default();
    let ring = |w: f32, y: f32| {
        [
            Vec3::new(-w, y, -w),
            Vec3::new(w, y, -w),
            Vec3::new(w, y, w),
            Vec3::new(-w, y, w),
        ]
    };
    let bottom = ring(0.42, 0.0);
    let mid = ring(0.55, 0.75);
    let top = ring(0.6, 1.4);
    let crown = ring(0.36, 1.55);
    for k in 0..4 {
        let j = (k + 1) % 4;
        s.quad(
            [bottom[j], bottom[k], mid[k], mid[j]],
            [FLAME, FLAME, PAPER, PAPER],
        );
        s.quad(
            [mid[j], mid[k], top[k], top[j]],
            [PAPER, PAPER, PAPER_TOP, PAPER_TOP],
        );
        s.quad(
            [top[j], top[k], crown[k], crown[j]],
            [PAPER_TOP, PAPER_TOP, LANTERN_CAP, LANTERN_CAP],
        );
    }
    // The dark crown on top and the glowing opening underneath.
    s.quad([crown[3], crown[2], crown[1], crown[0]], [LANTERN_CAP; 4]);
    s.quad([bottom[0], bottom[1], bottom[2], bottom[3]], [FLAME; 4]);
    s.mesh()
}

/// A soft round sparkle, 1 unit across (see [`glow_mesh`]).
pub fn spark_mesh() -> Mesh {
    glow_mesh(8)
}

/// A soft round glow, 1 unit across: three crossed `sides`-gons, white at the
/// centre fading to nothing at the rim (additive), so it glows from any side.
pub fn glow_mesh(sides: usize) -> Mesh {
    let mut s = Soup::default();
    let white = [1.0; 3];
    let clear = [0.0; 3];
    for axis in 0..3 {
        let basis = |a: f32| {
            let (u, v) = (a.cos() * 0.5, a.sin() * 0.5);
            match axis {
                0 => Vec3::new(u, v, 0.0),
                1 => Vec3::new(u, 0.0, v),
                _ => Vec3::new(0.0, u, v),
            }
        };
        for i in 0..sides {
            let a0 = i as f32 / sides as f32 * TAU;
            let a1 = (i + 1) as f32 / sides as f32 * TAU;
            s.tri([Vec3::ZERO, basis(a0), basis(a1)], [white, clear, clear]);
            s.tri([Vec3::ZERO, basis(a1), basis(a0)], [white, clear, clear]);
        }
    }
    s.mesh()
}

/// A shooting star, unit length: a bright head at the origin and a tail
/// trailing to +Z, as two crossed ribbons seen from both sides.
pub fn streak_mesh() -> Mesh {
    let mut s = Soup::default();
    let head = [1.0, 1.0, 1.0];
    let tail = [0.35, 0.45, 0.8];
    let clear = [0.0; 3];
    for u in [Vec3::X, Vec3::Y] {
        let w = 0.5;
        // A diamond flare at the head.
        s.quad2(
            [-u * w * 2.2, Vec3::Z * -0.015, u * w * 2.2, Vec3::Z * 0.04],
            [clear, head, clear, head],
        );
        // The tail, bright along its spine, tapering and fading to +Z.
        s.quad2(
            [Vec3::ZERO, u * w, u * w * 0.1 + Vec3::Z, Vec3::Z],
            [head, clear, clear, tail],
        );
        s.quad2(
            [-u * w, Vec3::ZERO, Vec3::Z, -u * w * 0.1 + Vec3::Z],
            [clear, head, tail, clear],
        );
    }
    s.mesh()
}

/// Aurora colours (linear): the bright teal foot, the violet body.
const AURORA_TEAL: [f32; 3] = [0.05, 0.42, 0.36];
const AURORA_VIOLET: [f32; 3] = [0.2, 0.08, 0.36];

/// An aurora curtain on the sky layer: from the foot's azimuth and elevation
/// along the spec's arc, rising `height` degrees, bright teal at its foot,
/// violet above, fading out at the top and both ends; wavy along its length.
pub fn aurora_mesh(spec: &AuroraSpec) -> Mesh {
    let mut s = Soup::default();
    let n = 36;
    let point = |az: f32, el: f32| sky_point(az, el) * SKY_RADIUS;
    let rows = [0.0f32, 0.18, 0.55, 1.0];
    let row_color = [
        scale3(AURORA_TEAL, 0.25),
        AURORA_TEAL,
        AURORA_VIOLET,
        [0.0; 3],
    ];
    for i in 0..n {
        let col = |k: usize| {
            let t = k as f32 / n as f32;
            let az = spec.az.0 + (spec.az.1 - spec.az.0) * t;
            let el = spec.el.0
                + (spec.el.1 - spec.el.0) * t
                + 2.2 * (t * spec.waves * TAU + spec.phase).sin();
            let height = spec.height * (0.7 + 0.3 * (t * spec.waves * 1.7 * TAU).cos());
            let fade = (t / 0.18).min((1.0 - t) / 0.18).clamp(0.0, 1.0);
            let fade = fade * fade * (3.0 - 2.0 * fade);
            // The curtain leans a little along its length, like drifting light.
            let lean = 3.0 * (t * spec.waves * TAU).cos();
            (az, el, height, fade, lean)
        };
        let (a0, e0, h0, f0, l0) = col(i);
        let (a1, e1, h1, f1, l1) = col(i + 1);
        for r in 0..rows.len() - 1 {
            let (y0, y1) = (rows[r], rows[r + 1]);
            let p = [
                point(a0 + l0 * y0, e0 + h0 * y0),
                point(a1 + l1 * y0, e1 + h1 * y0),
                point(a1 + l1 * y1, e1 + h1 * y1),
                point(a0 + l0 * y1, e0 + h0 * y1),
            ];
            let c = [
                scale3(row_color[r], f0),
                scale3(row_color[r], f1),
                scale3(row_color[r + 1], f1),
                scale3(row_color[r + 1], f0),
            ];
            s.quad2(p, c);
        }
    }
    s.mesh()
}

/// Triangles the magic draws (for the far layer's budget).
pub fn magic_triangles(magic: &SkyMagic) -> u32 {
    let tris = |m: &Mesh| m.count_vertices() as u32 / 3;
    let spark = tris(&spark_mesh());
    tris(&rune_ring_mesh(RING_RADIUS, RING_BAND, RING_SEGMENTS))
        + (tris(&lantern_mesh()) + tris(&glow_mesh(6))) * magic.lanterns.len() as u32
        + spark * (magic.embers.len() + magic.motes.len() + magic.twinkles.len()) as u32
        + tris(&streak_mesh()) * SHOOTING_STAR_POOL as u32
        + magic
            .auroras
            .iter()
            .map(|a| tris(&aurora_mesh(a)))
            .sum::<u32>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_magic_meshes_face_out_and_carry_far_attributes() {
        for mesh in [
            rune_ring_mesh(100.0, 10.0, 24),
            lantern_mesh(),
            spark_mesh(),
            streak_mesh(),
        ] {
            assert!(mesh.attribute(Mesh::ATTRIBUTE_POSITION).is_some());
            assert!(mesh.attribute(Mesh::ATTRIBUTE_NORMAL).is_some());
            assert!(mesh.attribute(Mesh::ATTRIBUTE_COLOR).is_some());
            assert_eq!(mesh.count_vertices() % 3, 0);
        }
        // The lantern's sides face outward.
        let mesh = lantern_mesh();
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!()
        };
        for tri in pos.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|k| Vec3::from_array(tri[k]));
            let n = (b - a).cross(c - a);
            let centre = (a + b + c) / 3.0 - Vec3::Y * 0.75;
            if n.y.abs() < 0.5 * n.length() {
                assert!(
                    n.dot(Vec3::new(centre.x, 0.0, centre.z)) > 0.0,
                    "side faces out"
                );
            }
        }
    }

    #[test]
    fn the_schedule_is_seeded_and_the_cursor_agrees_with_a_rescan() {
        let mut stars = ShootingStars::default();
        let passes = stars.passes(8);
        for t in [0.0, 5.0, 30.0, 61.0, 90.0, 12.0, 100.0] {
            let expected = passes.iter().rev().find(|p| p.start_s <= t).copied();
            assert_eq!(stars.latest(t), expected, "at {t} s");
        }
        assert_eq!(ShootingStars::default().passes(8), passes);
    }
}
