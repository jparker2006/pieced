//! The first-person gun models' layout. The rifle, the pump and the gloves are
//! Blender models (`art/blender/assets/guns.py`, `gloves.py`, loaded by
//! `crate::models`); this module reads their attach points from the embedded
//! sidecars and defines each gun's hip and aim-down-sights poses. The muzzle
//! flash and the build-mode blueprint tablet are still built here in code.
//!
//! Model space (a gun's, and the viewmodel item's): meters, -Z is the barrel
//! direction, +Y up, +X the gun's right; the origin is on the "ground" under the
//! pistol grip (use the attach points, not the origin).

use super::mesh::{ModelBuilder, linear, shade};
use crate::{
    models::{EMBEDDED_MODELS, Sidecar},
    palette::{GUN_ACCENT, MUZZLE, SHIELD, WOOD, WOOD_DARK, WOOD_LIGHT, WOOD_TRIM},
    shared::WeaponKind,
};
use bevy::{math::Affine3A, prelude::*};
use std::sync::LazyLock;

/// Where a gun sits in the viewmodel camera's space at the hip.
#[derive(Debug, Clone, Copy)]
pub struct HipPose {
    /// Where the gun's crystal socket sits (viewmodel camera space).
    pub anchor: Vec3,
    /// The gun's rotation as (pitch, yaw, roll) radians.
    pub euler: Vec3,
}

/// A gun's key points (model space) and its poses.
#[derive(Debug, Clone, Copy)]
pub struct GunSpec {
    /// The rear sight's notch: the point the eye looks through in ADS.
    pub sight: Vec3,
    /// The front sight, on the same level line.
    pub sight_front: Vec3,
    /// Tip of the barrel (muzzle flash and tracer origin).
    pub muzzle: Vec3,
    /// The crystal's centre (the `Crystal` part's pivot).
    pub socket: Vec3,
    /// The right glove's frame on the pistol grip.
    pub grip_r: Transform,
    /// The left glove's frame (on the pump, at the pump grip's rest position).
    pub grip_l: Transform,
    /// Hip pose: the rig translation (viewmodel camera space).
    pub hip: Vec3,
    /// Hip pose rotation as (pitch, yaw, roll) radians.
    pub hip_euler: Vec3,
    /// Distance from the eye to the rear sight in the ADS pose.
    pub ads_distance: f32,
    /// How many times farther out the gun is held at the hip than when its
    /// motion was tuned ([`MOTION_DEPTH`]). The sway, bob, kick, sprint,
    /// slide, reload and switch offsets are scaled by it at the hip, so they
    /// move the gun just as far on screen as before (and the muzzle flash as
    /// big): the gun moved, its feel did not.
    pub reach: f32,
}

/// The crystal socket's depth (m ahead of the eye) at which each gun's
/// motion offsets were tuned and felt good (S1 round 2's hip poses): the
/// rifle's, then the pump's. See [`GunSpec::reach`].
pub const MOTION_DEPTH: [f32; 2] = [0.57, 0.52];

impl GunSpec {
    /// Reads the attach points from a gun's sidecar and places it at `hip`;
    /// its motion was tuned with the socket `motion_depth` ahead of the eye.
    pub fn from_sidecar(
        side: &Sidecar,
        hip: HipPose,
        ads_distance: f32,
        motion_depth: f32,
    ) -> Self {
        let point = |name: &str| {
            side.attach(name)
                .unwrap_or_else(|| panic!("{}: no attach point {name}", side.name))
                .transform()
        };
        let socket = point("CrystalSocket").translation;
        let rotation = super::anim::euler(hip.euler);
        Self {
            sight: point("Sight").translation,
            sight_front: point("SightFront").translation,
            muzzle: point("MuzzleTip").translation,
            socket,
            grip_r: point("GripR"),
            grip_l: point("GripL"),
            hip: hip.anchor - rotation * socket,
            hip_euler: hip.euler,
            ads_distance,
            reach: -hip.anchor.z / motion_depth,
        }
    }
}

/// The embedded sidecar of a model (compiled in with the model).
pub fn sidecar(name: &str) -> Sidecar {
    let model = EMBEDDED_MODELS
        .iter()
        .find(|m| m.name == name)
        .unwrap_or_else(|| panic!("model {name} is not embedded"));
    Sidecar::parse(model.sidecar).unwrap_or_else(|e| panic!("{name}.json: {e}"))
}

/// The rifle at the hip (S1 round 3): tucked into the lower-right corner as
/// T01, T03, T05, T10 and T11 paint it, the stock leaving the frame's corner,
/// the glove low on the right and the barrel running up and left so the muzzle
/// sits just below and right of the crosshair, the vista clear. Held about as
/// far out as before, but lower, further right and pointing almost straight
/// ahead (perspective angles it toward the crosshair), and upright.
pub const RIFLE_HIP: HipPose = HipPose {
    anchor: Vec3::new(0.307, -0.2, -0.6),
    euler: Vec3::new(0.13, 0.07, -0.05),
};
/// The pump at the hip (T04): low right, a little farther out than the rifle
/// and turned left so its bell faces up and left and reads as a horn from the
/// side, the violet chamber by the corner. T04 catches it mid-kick, which
/// lifts it up and right onto the target's framing.
pub const PUMP_HIP: HipPose = HipPose {
    anchor: Vec3::new(0.336, -0.234, -0.68),
    euler: Vec3::new(0.14, 0.42, -0.1),
};

/// The rifle's gallery-only "inspect" pose (target T02): turned well toward the
/// camera and rolled upright, crossing the frame from the bottom-right corner
/// to the muzzle up and left of centre, big, with its whole left side (rune
/// window, chamber, crystal) facing you. Only the gallery shows it
/// (`super::ViewmodelInspect`); play never does.
pub const RIFLE_INSPECT: HipPose = HipPose {
    anchor: Vec3::new(0.098, -0.087, -0.62),
    euler: Vec3::new(0.34, 0.84, 0.24),
};

impl HipPose {
    /// The rig translation that puts `spec`'s crystal socket at this pose's
    /// anchor (as [`GunSpec::hip`] does for the hip pose).
    pub fn rig_translation(&self, spec: &GunSpec) -> Vec3 {
        self.anchor - super::anim::euler(self.euler) * spec.socket
    }
}

pub static RIFLE: LazyLock<GunSpec> =
    LazyLock::new(|| GunSpec::from_sidecar(&sidecar("rifle"), RIFLE_HIP, 0.30, MOTION_DEPTH[0]));
pub static PUMP: LazyLock<GunSpec> =
    LazyLock::new(|| GunSpec::from_sidecar(&sidecar("pump"), PUMP_HIP, 0.28, MOTION_DEPTH[1]));

/// The blueprint tablet's hip pose (no model, no ADS, no muzzle).
pub const BLUEPRINT: GunSpec = GunSpec {
    sight: Vec3::ZERO,
    sight_front: Vec3::ZERO,
    muzzle: Vec3::ZERO,
    socket: Vec3::ZERO,
    grip_r: Transform::IDENTITY,
    grip_l: Transform::IDENTITY,
    hip: Vec3::new(0.165, -0.15, -0.40),
    hip_euler: Vec3::new(0.62, -0.32, 0.10),
    ads_distance: 0.3,
    reach: 1.0,
};

pub fn gun_spec(kind: WeaponKind) -> &'static GunSpec {
    match kind {
        WeaponKind::Rifle => &RIFLE,
        WeaponKind::Pump => &PUMP,
    }
}

/// The Blender model for each gun.
pub fn gun_model(kind: WeaponKind) -> &'static str {
    match kind {
        WeaponKind::Rifle => "rifle",
        WeaponKind::Pump => "pump",
    }
}

/// The gloves model (`GloveR`, `GloveL`, placed at each gun's grips).
pub const GLOVES_MODEL: &str = "gloves";

/// How far the rack pulls the pump grip back (model +Z).
pub const PUMP_RACK_TRAVEL: f32 = 0.09;

fn v3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn zy(points: &[(f32, f32)]) -> Vec<Vec2> {
    points.iter().map(|&(z, y)| Vec2::new(z, y)).collect()
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
            m.chamfer_box(
                v3(-0.04, 0.0, -0.006),
                v3(0.04, 0.06, 0.006),
                0.002,
                WOOD_LIGHT,
            );
            m.cube(v3(-0.042, 0.0, -0.008), v3(0.042, 0.007, 0.008), WOOD_TRIM);
            m.cube(v3(-0.042, 0.053, -0.008), v3(0.042, 0.06, 0.008), WOOD_TRIM);
            m.cube(v3(-0.0035, 0.007, -0.007), v3(0.0035, 0.053, 0.007), WOOD);
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
        PieceKind::Cone => {
            // A little plank pyramid: four sloped faces meeting at a peak.
            let (w, h) = (0.05, 0.038);
            let apex = v3(0.0, h, 0.0);
            let base = [
                v3(-w, 0.0, -w),
                v3(w, 0.0, -w),
                v3(w, 0.0, w),
                v3(-w, 0.0, w),
            ];
            let inside = v3(0.0, h * 0.3, 0.0);
            for k in 0..4 {
                let (a, b) = (base[k], base[(k + 1) % 4]);
                let color = if k % 2 == 0 { WOOD_LIGHT } else { WOOD };
                m.poly(&[a, b, apex], inside, linear(color, 1.0));
                // A plank line across each face.
                let (c, d) = (a.lerp(apex, 0.5), b.lerp(apex, 0.5));
                m.bar(c, d, 0.0012, WOOD_DARK);
            }
            m.poly(&base, inside, linear(WOOD_TRIM, 1.0));
        }
    }
    m
}
