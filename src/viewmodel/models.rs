//! The first-person gun models, built from chunky primitives in code: a
//! SCAR-style assault rifle and a pump shotgun, plus the muzzle flash and a small
//! blueprint tablet for build mode.
//!
//! Model space: meters, -Z is the barrel direction, +Y up. Each gun is split into
//! the parts that animate separately (the rifle's magazine, the pump's forend and
//! the loading shell).

use super::mesh::{ModelBuilder, linear, shade};
use crate::palette::{
    GUN_ACCENT, GUN_METAL, GUN_POLYMER, MUZZLE, SHIELD, WOOD, WOOD_DARK, WOOD_LIGHT, WOOD_TRIM,
};
use bevy::{math::Affine3A, prelude::*};

/// Where each gun's key points are, in its model space.
#[derive(Debug, Clone, Copy)]
pub struct GunSpec {
    /// The point the eye looks through when aiming down sights.
    pub sight: Vec3,
    /// Tip of the barrel (muzzle flash and tracer origin).
    pub muzzle: Vec3,
    /// Hip pose in viewmodel-camera space.
    pub hip: Vec3,
    /// Hip pose rotation as (pitch, yaw, roll) radians.
    pub hip_euler: Vec3,
    /// Distance from the eye to the sight point in the ADS pose.
    pub ads_distance: f32,
}

pub const RIFLE: GunSpec = GunSpec {
    sight: Vec3::new(0.0, 0.1245, -0.061),
    muzzle: Vec3::new(0.0, 0.020, -0.815),
    hip: Vec3::new(0.205, -0.215, -0.50),
    hip_euler: Vec3::new(0.02, 0.03, -0.03),
    ads_distance: 0.25,
};

pub const PUMP: GunSpec = GunSpec {
    sight: Vec3::new(0.0, 0.060, -0.05),
    muzzle: Vec3::new(0.0, 0.024, -0.80),
    hip: Vec3::new(0.205, -0.205, -0.47),
    hip_euler: Vec3::new(0.025, 0.035, -0.03),
    ads_distance: 0.24,
};

/// The rifle magazine's seat in rifle model space (its local origin).
pub const RIFLE_MAG_SEAT: Vec3 = Vec3::new(0.0, -0.06, -0.075);
/// Forward tilt of the seated magazine (radians about +X).
pub const RIFLE_MAG_TILT: f32 = 0.14;
/// The pump forend's rest position in pump model space (its local origin).
pub const PUMP_FOREND_REST: Vec3 = Vec3::new(0.0, -0.016, -0.39);
/// How far the forend travels back when racking.
pub const PUMP_RACK_TRAVEL: f32 = 0.085;
/// Where a loaded shell disappears into the loading port (pump model space).
pub const PUMP_LOADING_PORT: Vec3 = Vec3::new(0.0, -0.05, -0.06);

/// Rifle parts: body, magazine, reticle dot.
pub struct RifleParts {
    pub body: ModelBuilder,
    pub mag: ModelBuilder,
    pub dot: ModelBuilder,
}

/// Pump parts: body, sliding forend, loading shell.
pub struct PumpParts {
    pub body: ModelBuilder,
    pub forend: ModelBuilder,
    pub shell: ModelBuilder,
}

fn v3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn zy(points: &[(f32, f32)]) -> Vec<Vec2> {
    points.iter().map(|&(z, y)| Vec2::new(z, y)).collect()
}

pub fn rifle() -> RifleParts {
    let metal = GUN_METAL;
    let poly = GUN_POLYMER;
    let dark = shade(GUN_POLYMER, 0.55);
    let mut m = ModelBuilder::new();

    // Upper receiver (metal) and lower receiver (polymer).
    m.chamfer_box(
        v3(-0.030, 0.000, -0.30),
        v3(0.030, 0.062, 0.10),
        0.007,
        metal,
    );
    m.chamfer_box(v3(-0.027, -0.048, -0.13), v3(0.027, 0.0, 0.10), 0.005, poly);
    // Magwell.
    m.chamfer_box(
        v3(-0.025, -0.072, -0.115),
        v3(0.025, -0.040, -0.035),
        0.004,
        poly,
    );
    // Trigger guard and trigger.
    m.cube(v3(-0.008, -0.079, -0.030), v3(0.008, -0.072, 0.036), poly);
    m.cube(v3(-0.004, -0.068, 0.000), v3(0.004, -0.048, 0.012), metal);
    // Pistol grip, raked back.
    m.prism_x(
        &zy(&[
            (0.035, -0.046),
            (0.085, -0.046),
            (0.118, -0.158),
            (0.070, -0.165),
        ]),
        -0.019,
        0.019,
        poly,
    );
    // Ejection port (right) and charging handle + selector (left, accents).
    m.cube(
        v3(0.030, 0.016, -0.09),
        v3(0.033, 0.044, -0.01),
        shade(metal, 0.6),
    );
    m.chamfer_box(
        v3(-0.047, 0.026, -0.215),
        v3(-0.030, 0.045, -0.183),
        0.003,
        GUN_ACCENT,
    );
    m.cube(
        v3(-0.031, -0.031, 0.030),
        v3(-0.026, -0.020, 0.054),
        GUN_ACCENT,
    );

    // Handguard with vent slots on both sides.
    m.chamfer_box(
        v3(-0.033, -0.034, -0.56),
        v3(0.033, 0.058, -0.30),
        0.012,
        poly,
    );
    for i in 0..3 {
        let z0 = -0.535 + i as f32 * 0.075;
        m.cube(v3(-0.035, -0.010, z0), v3(-0.032, 0.012, z0 + 0.045), dark);
        m.cube(v3(0.032, -0.010, z0), v3(0.035, 0.012, z0 + 0.045), dark);
    }
    // Top rail with chunky teeth.
    let rail = shade(metal, 0.9);
    m.cube(v3(-0.017, 0.058, -0.555), v3(0.017, 0.070, 0.085), rail);
    for i in 0..15 {
        let z0 = -0.545 + i as f32 * 0.028;
        m.cube(v3(-0.019, 0.070, z0), v3(0.019, 0.077, z0 + 0.014), rail);
    }
    // Barrel, gas block, flash hider with an accent ring.
    m.tube_z(
        Vec2::new(0.0, 0.020),
        0.012,
        -0.745,
        -0.555,
        8,
        shade(metal, 0.8),
    );
    m.chamfer_box(
        v3(-0.017, 0.002, -0.63),
        v3(0.017, 0.040, -0.60),
        0.003,
        metal,
    );
    m.tube_z(
        Vec2::new(0.0, 0.020),
        0.018,
        -0.815,
        -0.745,
        8,
        shade(metal, 0.75),
    );
    m.tube_z(Vec2::new(0.0, 0.020), 0.0195, -0.758, -0.748, 8, GUN_ACCENT);
    // Folding front sight.
    m.chamfer_box(
        v3(-0.009, 0.077, -0.535),
        v3(0.009, 0.108, -0.515),
        0.002,
        metal,
    );

    // Holographic sight: a raised base and a slim open hood with an accent lip.
    m.chamfer_box(
        v3(-0.019, 0.077, -0.098),
        v3(0.019, 0.094, 0.004),
        0.003,
        metal,
    );
    m.cube(v3(-0.024, 0.092, -0.092), v3(-0.0195, 0.160, -0.034), poly);
    m.cube(v3(0.0195, 0.092, -0.092), v3(0.024, 0.160, -0.034), poly);
    m.chamfer_box(
        v3(-0.024, 0.155, -0.094),
        v3(0.024, 0.162, -0.032),
        0.002,
        poly,
    );
    m.cube(
        v3(-0.024, 0.162, -0.094),
        v3(0.024, 0.165, -0.084),
        GUN_ACCENT,
    );
    m.cube(
        v3(-0.016, 0.094, -0.03),
        v3(0.016, 0.099, 0.0),
        shade(metal, 0.7),
    );

    // Stock: top bar, lower strut, butt plate.
    m.chamfer_box(v3(-0.020, 0.008, 0.10), v3(0.020, 0.055, 0.30), 0.006, poly);
    m.bar(v3(0.0, -0.036, 0.10), v3(0.0, -0.024, 0.295), 0.011, poly);
    m.chamfer_box(
        v3(-0.026, -0.070, 0.295),
        v3(0.026, 0.066, 0.33),
        0.008,
        shade(poly, 0.8),
    );

    // Magazine in its own space (origin at the seat, hanging down).
    let mut mag = ModelBuilder::new();
    mag.chamfer_box(
        v3(-0.019, -0.165, -0.032),
        v3(0.019, 0.02, 0.032),
        0.004,
        metal,
    );
    mag.cube(
        v3(-0.0205, -0.120, -0.024),
        v3(0.0205, -0.112, 0.024),
        shade(metal, 0.75),
    );
    mag.cube(
        v3(-0.0205, -0.070, -0.024),
        v3(0.0205, -0.062, 0.024),
        shade(metal, 0.75),
    );
    mag.chamfer_box(
        v3(-0.022, -0.183, -0.037),
        v3(0.022, -0.165, 0.037),
        0.004,
        GUN_ACCENT,
    );

    // Reticle dot (drawn unlit) at the sight point, facing the eye.
    let mut dot = ModelBuilder::new();
    let r = 0.0016;
    let ring: Vec<Vec3> = (0..8)
        .map(|i| {
            let a = std::f32::consts::TAU * i as f32 / 8.0;
            Vec3::new(r * a.cos(), r * a.sin(), 0.0)
        })
        .collect();
    let c = linear(Color::WHITE, 1.0);
    dot.fan(Vec3::ZERO, &ring, c, c);

    RifleParts { body: m, mag, dot }
}

pub fn pump() -> PumpParts {
    let metal = GUN_METAL;
    let poly = GUN_POLYMER;
    let mut m = ModelBuilder::new();

    // Receiver with a sighting rib, accent stripe (left), ports.
    m.chamfer_box(
        v3(-0.031, -0.042, -0.17),
        v3(0.031, 0.046, 0.10),
        0.009,
        metal,
    );
    m.cube(
        v3(-0.006, 0.046, -0.17),
        v3(0.006, 0.052, 0.06),
        shade(metal, 0.8),
    );
    m.cube(
        v3(-0.0345, 0.004, -0.15),
        v3(-0.030, 0.015, 0.07),
        GUN_ACCENT,
    );
    m.cube(
        v3(0.030, 0.004, -0.12),
        v3(0.034, 0.034, -0.03),
        shade(metal, 0.55),
    );
    m.cube(
        v3(-0.020, -0.046, -0.12),
        v3(0.020, -0.041, 0.0),
        shade(metal, 0.55),
    );
    // Barrel with a vent rib and an accent bead.
    m.tube_z(
        Vec2::new(0.0, 0.024),
        0.0175,
        -0.80,
        -0.17,
        8,
        shade(metal, 0.85),
    );
    m.cube(v3(-0.006, 0.040, -0.795), v3(0.006, 0.047, -0.17), metal);
    m.chamfer_box(
        v3(-0.0055, 0.047, -0.792),
        v3(0.0055, 0.059, -0.778),
        0.002,
        GUN_ACCENT,
    );
    // Magazine tube, end cap and barrel clamp.
    m.tube_z(
        Vec2::new(0.0, -0.016),
        0.015,
        -0.68,
        -0.17,
        8,
        shade(metal, 0.7),
    );
    m.tube_z(Vec2::new(0.0, -0.016), 0.0178, -0.70, -0.68, 8, metal);
    m.chamfer_box(
        v3(-0.020, -0.034, -0.676),
        v3(0.020, 0.045, -0.652),
        0.004,
        metal,
    );
    // Trigger guard and trigger.
    m.cube(v3(-0.008, -0.079, -0.020), v3(0.008, -0.072, 0.070), poly);
    m.cube(v3(-0.006, -0.076, -0.020), v3(0.006, -0.042, -0.010), poly);
    m.cube(v3(-0.004, -0.070, 0.020), v3(0.004, -0.046, 0.030), metal);
    // Stock: wrist, body, butt pad, accent inlay (left).
    m.prism_x(
        &zy(&[(0.10, 0.040), (0.20, 0.036), (0.20, -0.066), (0.10, -0.042)]),
        -0.023,
        0.023,
        poly,
    );
    m.prism_x(
        &zy(&[
            (0.20, 0.036),
            (0.42, 0.022),
            (0.44, -0.020),
            (0.44, -0.115),
            (0.40, -0.120),
            (0.20, -0.066),
        ]),
        -0.023,
        0.023,
        poly,
    );
    m.prism_x(
        &zy(&[
            (0.44, -0.020),
            (0.456, -0.020),
            (0.456, -0.115),
            (0.44, -0.115),
        ]),
        -0.024,
        0.024,
        shade(poly, 0.6),
    );
    m.cube(
        v3(-0.0245, -0.010, 0.27),
        v3(-0.022, -0.002, 0.36),
        GUN_ACCENT,
    );

    // Forend (origin at its rest center) with grip grooves, accent ring, action bars.
    let mut f = ModelBuilder::new();
    f.chamfer_box(
        v3(-0.036, -0.034, -0.11),
        v3(0.036, 0.030, 0.11),
        0.012,
        poly,
    );
    for i in 0..4 {
        let z = -0.07 + i as f32 * 0.047;
        f.cube(
            v3(-0.0375, -0.026, z - 0.006),
            v3(0.0375, 0.022, z + 0.006),
            shade(poly, 0.55),
        );
    }
    f.cube(
        v3(-0.0368, -0.030, -0.113),
        v3(0.0368, 0.026, -0.103),
        GUN_ACCENT,
    );
    for x in [-0.026, 0.026] {
        f.bar(v3(x, 0.0, 0.10), v3(x, 0.0, 0.27), 0.004, metal);
    }

    // A shell (origin at its brass base, pointing -Z).
    let mut s = ModelBuilder::new();
    s.tube_z(Vec2::ZERO, 0.0115, -0.068, -0.014, 8, GUN_ACCENT);
    s.tube_z(Vec2::ZERO, 0.0125, -0.014, 0.0, 8, shade(MUZZLE, 0.85));

    PumpParts {
        body: m,
        forend: f,
        shell: s,
    }
}

/// The muzzle flash in muzzle space (-Z out of the barrel): three crossed
/// flame petals and a front star, hot white at the core fading to amber.
pub fn muzzle_flash() -> ModelBuilder {
    let mut m = ModelBuilder::new();
    let hot = linear(Color::srgb(1.0, 0.97, 0.85), 1.0);
    let amber = linear(MUZZLE, 0.85);
    let tip = linear(MUZZLE, 0.0);
    let (w, len) = (0.030, 0.17);
    for k in 0..3 {
        let rot = Affine3A::from_rotation_z(std::f32::consts::PI * k as f32 / 3.0);
        m.with(rot, |m| {
            let pts = [
                v3(0.0, 0.0, 0.01),
                v3(w, 0.0, -len * 0.3),
                v3(0.0, 0.0, -len),
                v3(-w, 0.0, -len * 0.3),
            ];
            m.poly_colored(&pts, &[hot, amber, tip, amber], Vec3::Y);
        });
    }
    let star = |outer: f32, inner: f32, points: usize, z: f32, twist: f32| -> Vec<Vec3> {
        (0..points * 2)
            .map(|i| {
                let a = twist + std::f32::consts::PI * i as f32 / points as f32;
                let r = if i % 2 == 0 { outer } else { inner };
                Vec3::new(r * a.cos(), r * a.sin(), z)
            })
            .collect()
    };
    m.fan(
        v3(0.0, 0.0, -0.015),
        &star(0.060, 0.022, 6, -0.015, 0.0),
        hot,
        tip,
    );
    m.fan(
        v3(0.0, 0.0, -0.02),
        &star(0.028, 0.014, 5, -0.02, 0.3),
        hot,
        amber,
    );
    m
}

/// The build-mode prop: a small blueprint tablet.
pub fn blueprint() -> ModelBuilder {
    let mut m = ModelBuilder::new();
    let paper = shade(SHIELD, 0.5);
    let line = shade(SHIELD, 0.95);
    m.chamfer_box(
        v3(-0.07, -0.010, -0.052),
        v3(0.07, 0.0, 0.052),
        0.004,
        shade(SHIELD, 0.32),
    );
    m.cube(v3(-0.064, 0.0, -0.046), v3(0.064, 0.003, 0.046), paper);
    for i in 0..3 {
        let x = -0.032 + i as f32 * 0.032;
        m.cube(
            v3(x - 0.001, 0.003, -0.042),
            v3(x + 0.001, 0.0036, 0.042),
            line,
        );
        let z = -0.023 + i as f32 * 0.023;
        m.cube(
            v3(-0.06, 0.003, z - 0.001),
            v3(0.06, 0.0036, z + 0.001),
            line,
        );
    }
    m.cube(v3(0.048, 0.0, -0.050), v3(0.068, 0.006, -0.038), GUN_ACCENT);
    m
}

/// A miniature wooden piece that stands on the blueprint (base at y = 0).
pub fn mini_piece(kind: crate::shared::PieceKind) -> ModelBuilder {
    use crate::shared::PieceKind;
    let mut m = ModelBuilder::new();
    match kind {
        PieceKind::Wall => {
            m.chamfer_box(v3(-0.05, 0.0, -0.007), v3(0.05, 0.075, 0.007), 0.002, WOOD);
            m.cube(v3(-0.052, 0.0, -0.009), v3(0.052, 0.008, 0.009), WOOD_TRIM);
            m.cube(
                v3(-0.052, 0.067, -0.009),
                v3(0.052, 0.075, 0.009),
                WOOD_TRIM,
            );
            m.cube(
                v3(-0.004, 0.008, -0.008),
                v3(0.004, 0.067, 0.008),
                WOOD_DARK,
            );
        }
        PieceKind::Floor => {
            m.chamfer_box(v3(-0.05, 0.0, -0.05), v3(0.05, 0.009, 0.05), 0.002, WOOD);
            for i in 0..3 {
                let x = -0.025 + i as f32 * 0.025;
                m.cube(
                    v3(x - 0.002, 0.009, -0.048),
                    v3(x + 0.002, 0.0105, 0.048),
                    WOOD_DARK,
                );
            }
            m.cube(v3(-0.052, 0.0, -0.052), v3(0.052, 0.006, -0.046), WOOD_TRIM);
            m.cube(v3(-0.052, 0.0, 0.046), v3(0.052, 0.006, 0.052), WOOD_TRIM);
        }
        PieceKind::Ramp => {
            m.prism_x(
                &zy(&[(0.05, 0.0), (0.05, 0.008), (-0.05, 0.075), (-0.05, 0.0)]),
                -0.05,
                0.05,
                WOOD_LIGHT,
            );
            m.cube(
                v3(-0.052, 0.0, -0.052),
                v3(-0.044, 0.075, -0.044),
                WOOD_TRIM,
            );
            m.cube(v3(0.044, 0.0, -0.052), v3(0.052, 0.075, -0.044), WOOD_TRIM);
        }
    }
    m
}
