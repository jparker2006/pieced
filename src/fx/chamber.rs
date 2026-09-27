//! The crackling energy inside each gun's crystal chamber (D50; targets T02
//! and T04): a soft glow round the crystal that breathes, a few lightning
//! arcs that strike round the inside of the glass and re-strike somewhere new
//! every few hundredths of a second, and sparkle motes drifting round the
//! crystal. It all rides on the gun's model root, so it kicks, squashes,
//! reloads and hides with the gun.
//!
//! **Brightness follows the magazine** ([`CrystalGlow`]): a full crystal
//! crackles with every arc lit and bright; as rounds go the arcs thin out and
//! dim, and an empty crystal only sputters now and then.
//!
//! **Pooled and cheap:** each chamber gets [`CHAMBER_ARCS`] arcs,
//! [`CHAMBER_MOTES`] motes and one [`Halo`] when its gun model is dressed, and
//! nothing more is ever made. The arc shapes ([`ARC_SHAPES`] per colour) are
//! built at startup with the other spell shapes; a crackle only swaps mesh
//! handles and moves transforms. Everything shares the one additive
//! [`super::material::SpellMaterial`], whose viewmodel pipeline is already
//! warmed up for the muzzle flash.
//!
//! **Freezable:** the clock reads [`FreezableTime`], so the gallery's freeze
//! holds each arc where it struck.

use super::{
    material::glow_tag,
    sim::FxRng,
    spells::{SpellAssets, SpellGlow},
};
use crate::{
    look::{Halo, ModelDressed, NoOutline},
    render::VIEWMODEL_LAYER,
    shared::{FreezableTime, WeaponKind},
    viewmodel::{CrystalGlow, ViewmodelSet, VmModel, crystal_color, models::gun_spec},
};
use bevy::{camera::visibility::RenderLayers, light::NotShadowCaster, mesh::MeshTag, prelude::*};
use std::f32::consts::PI;

/// Lightning arcs per chamber.
pub const CHAMBER_ARCS: usize = 4;
/// Sparkle motes per chamber.
pub const CHAMBER_MOTES: usize = 4;
/// Arc shapes built per colour (the game rolls and flips them too).
pub const ARC_SHAPES: usize = 6;
/// Seconds between crackles (each re-strikes every arc), shortest and longest.
pub const CRACKLE: (f32, f32) = (0.045, 0.09);
/// An arc's glow (the spell material's intensity) at full crystal glow.
pub const ARC_GLOW: f32 = 4.2;
/// A mote's glow at full crystal glow.
pub const MOTE_GLOW: f32 = 2.0;
/// The inner glow's halo: size (m) and intensity at full crystal glow.
pub const GLOW_HALO_SIZE: f32 = 0.16;
pub const GLOW_HALO_INTENSITY: f32 = 0.55;
/// Mote radius (m).
pub const MOTE_SIZE: f32 = 0.011;
/// The inner glow sits this far toward the side of the gun that faces the
/// camera (model -X), so it sorts in front of the glass and adds over it.
const GLOW_TOWARD_CAMERA: Vec3 = Vec3::new(-0.035, 0.0, 0.0);

/// One piece of a chamber's energy.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChamberEnergy {
    pub kind: WeaponKind,
    pub role: EnergyRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnergyRole {
    /// The breathing glow round the crystal (a halo).
    Glow,
    Arc(usize),
    Mote(usize),
}

/// Where one arc struck: shape, roll about the gun's axis, which end of the
/// crystal (flip), size and brightness.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ArcStrike {
    pub lit: bool,
    pub shape: usize,
    pub roll: f32,
    pub flip: bool,
    pub scale: f32,
    pub bright: f32,
}

/// The chance an arc is lit at a crackle, for a crystal glowing at `level`
/// (0.25 empty, 1 full; see [`CrystalGlow`]).
pub fn arc_chance(level: f32) -> f32 {
    let fill = ((level - 0.25) / 0.75).clamp(0.0, 1.0);
    0.12 + 0.88 * fill
}

/// Arcs strike round the half of the glass facing the player (model -X), top
/// to bottom: there they sort in front of the glass and add over it at full
/// strength, instead of showing dimmed through its front wall.
pub const ARC_ROLL: (f32, f32) = (PI - 1.3, PI + 1.3);

/// A fresh strike for one arc at glow `level`.
pub fn strike(rng: &mut FxRng, level: f32) -> ArcStrike {
    let lit = rng.f() < arc_chance(level);
    ArcStrike {
        lit,
        shape: rng.pick(ARC_SHAPES),
        roll: rng.range(ARC_ROLL.0, ARC_ROLL.1),
        flip: rng.f() < 0.5,
        scale: rng.range(0.85, 1.12),
        bright: rng.range(0.7, 1.0),
    }
}

/// An arc's glow intensity for a strike at crystal glow `level`.
pub fn arc_intensity(strike: &ArcStrike, level: f32) -> f32 {
    if strike.lit {
        ARC_GLOW * strike.bright * level.clamp(0.0, 1.4)
    } else {
        0.0
    }
}

/// An arc's transform in the gun's model space, about the crystal `socket`.
pub fn arc_transform(strike: &ArcStrike, socket: Vec3) -> Transform {
    let flip = if strike.flip { -1.0 } else { 1.0 };
    Transform::from_translation(socket)
        .with_rotation(Quat::from_rotation_z(strike.roll))
        .with_scale(Vec3::new(strike.scale, strike.scale, strike.scale * flip))
}

/// Where the `i`-th mote drifts `t` seconds in (relative to the socket), and
/// how big its twinkle makes it (0..=1).
pub fn mote_pose(i: usize, t: f32) -> (Vec3, f32) {
    let k = i as f32;
    let a = t * (1.1 + 0.37 * k) * if i.is_multiple_of(2) { 1.0 } else { -1.0 } + k * 1.9;
    let r = 0.031 + 0.01 * (t * 1.3 + k * 2.1).sin();
    let z = (0.045 - 0.03 * k) + 0.012 * (t * 0.9 + k).sin();
    let twinkle = (t * (7.0 + k) + k * 1.7).sin().abs();
    (Vec3::new(r * a.cos(), r * a.sin(), z), twinkle)
}

/// The inner glow's breathing (about 0.8..1.2) `t` seconds in.
pub fn glow_breath(t: f32) -> f32 {
    1.0 + 0.14 * (t * 6.3).sin() + 0.06 * (t * 17.0).sin()
}

#[derive(Debug, Clone, Copy, Default)]
struct Chamber {
    next: f32,
    arcs: [ArcStrike; CHAMBER_ARCS],
    /// A little flash on the glow as a crackle strikes (decays).
    flare: f32,
}

/// The crackle clocks and strikes, per gun.
#[derive(Resource, Debug)]
pub struct ChamberState {
    rng: FxRng,
    clock: f32,
    chambers: [Chamber; 2],
}

impl Default for ChamberState {
    fn default() -> Self {
        Self {
            rng: FxRng::new(0xA2C5),
            clock: 0.0,
            chambers: [Chamber::default(); 2],
        }
    }
}

impl ChamberState {
    /// Seconds of energy time so far (it stops while the gallery freezes).
    pub fn clock(&self) -> f32 {
        self.clock
    }

    /// The current strike of a gun's `i`-th arc.
    pub fn arc(&self, kind: WeaponKind, i: usize) -> ArcStrike {
        self.chambers[index(kind)].arcs[i]
    }
}

fn index(kind: WeaponKind) -> usize {
    match kind {
        WeaponKind::Rifle => 0,
        WeaponKind::Pump => 1,
    }
}

pub(super) fn add_systems(app: &mut App) {
    app.init_resource::<ChamberState>()
        .add_systems(Update, attach_chamber_energy)
        .add_systems(
            PostUpdate,
            animate_chamber_energy
                .after(ViewmodelSet)
                .before(TransformSystems::Propagate),
        );
}

/// Once a gun model is dressed, gives its chamber its arcs, motes and glow.
fn attach_chamber_energy(
    mut commands: Commands,
    mut dressed: MessageReader<ModelDressed>,
    roles: Query<&VmModel>,
    assets: Option<Res<SpellAssets>>,
) {
    let Some(assets) = assets else {
        dressed.clear();
        return;
    };
    let layer = RenderLayers::layer(VIEWMODEL_LAYER);
    for event in dressed.read() {
        let Ok(&VmModel::Gun(kind)) = roles.get(event.root) else {
            continue;
        };
        let socket = gun_spec(kind).socket;
        commands.spawn((
            Name::new(format!("{kind:?} chamber glow")),
            ChamberEnergy {
                kind,
                role: EnergyRole::Glow,
            },
            Halo::new(crystal_color(kind), GLOW_HALO_SIZE, 0.0),
            Transform::from_translation(socket + GLOW_TOWARD_CAMERA),
            Visibility::Inherited,
            layer.clone(),
            ChildOf(event.root),
        ));
        let glowing = |role: EnergyRole, mesh: Handle<Mesh>| {
            (
                ChamberEnergy { kind, role },
                Mesh3d(mesh),
                MeshMaterial3d(assets.material.clone()),
                glow_tag(Color::WHITE, 0.0),
                Transform::from_translation(socket),
                Visibility::Hidden,
                layer.clone(),
                NotShadowCaster,
                NoOutline,
                ChildOf(event.root),
            )
        };
        for i in 0..CHAMBER_ARCS {
            commands.spawn((
                Name::new(format!("{kind:?} chamber arc")),
                glowing(EnergyRole::Arc(i), assets.arc(kind, 0)),
            ));
        }
        for i in 0..CHAMBER_MOTES {
            commands.spawn((
                Name::new(format!("{kind:?} chamber mote")),
                glowing(EnergyRole::Mote(i), assets.mote(kind)),
            ));
        }
    }
}

type EnergyQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static ChamberEnergy,
        &'static mut Transform,
        &'static mut Visibility,
        Option<&'static mut Mesh3d>,
        Option<&'static mut MeshTag>,
        Option<&'static mut Halo>,
    ),
    Without<SpellGlow>,
>;

/// Crackles every chamber: re-strikes the arcs on the crackle clock, drifts
/// the motes, breathes the glow, all scaled by the crystal's glow.
fn animate_chamber_energy(
    time: FreezableTime,
    glow: Option<Res<CrystalGlow>>,
    assets: Option<Res<SpellAssets>>,
    mut state: ResMut<ChamberState>,
    mut energy: EnergyQuery,
) {
    let Some(assets) = assets else { return };
    let glow = glow.map(|g| *g).unwrap_or_default();
    let dt = time.delta_secs();
    let st = &mut *state;
    st.clock += dt;
    let clock = st.clock;
    for kind in [WeaponKind::Rifle, WeaponKind::Pump] {
        let level = glow.of(kind);
        let chamber = &mut st.chambers[index(kind)];
        chamber.flare = (chamber.flare - dt * 6.0).max(0.0);
        // Frozen time never re-strikes: the arcs hold where they are.
        if clock >= chamber.next && (dt > 0.0 || chamber.next == 0.0) {
            chamber.next = clock + st.rng.range(CRACKLE.0, CRACKLE.1);
            for arc in &mut chamber.arcs {
                *arc = strike(&mut st.rng, level);
            }
            if chamber.arcs.iter().any(|a| a.lit) {
                chamber.flare = 1.0;
            }
        }
    }
    for (piece, mut tf, mut vis, mesh, tag, halo) in &mut energy {
        let kind = piece.kind;
        let level = glow.of(kind);
        let socket = gun_spec(kind).socket;
        let chamber = &st.chambers[index(kind)];
        match piece.role {
            EnergyRole::Glow => {
                if let Some(mut halo) = halo {
                    let intensity =
                        GLOW_HALO_INTENSITY * level * (glow_breath(clock) + 0.25 * chamber.flare);
                    let want = Halo::new(crystal_color(kind), GLOW_HALO_SIZE, intensity);
                    if *halo != want {
                        *halo = want;
                    }
                }
            }
            EnergyRole::Arc(i) => {
                let arc = chamber.arcs[i];
                let intensity = arc_intensity(&arc, level);
                if intensity <= 0.0 {
                    vis.set_if_neq(Visibility::Hidden);
                    continue;
                }
                if let Some(mut mesh) = mesh {
                    let want = assets.arc(kind, arc.shape);
                    if mesh.0 != want {
                        mesh.0 = want;
                    }
                }
                if let Some(mut tag) = tag {
                    let want = glow_tag(Color::WHITE, intensity);
                    if *tag != want {
                        *tag = want;
                    }
                }
                tf.set_if_neq(arc_transform(&arc, socket));
                vis.set_if_neq(Visibility::Inherited);
            }
            EnergyRole::Mote(i) => {
                let (at, twinkle) = mote_pose(i, clock);
                let size = MOTE_SIZE * (0.45 + 0.55 * twinkle);
                tf.set_if_neq(
                    Transform::from_translation(socket + at)
                        .with_rotation(Quat::from_rotation_z(clock * 2.0 + i as f32))
                        .with_scale(Vec3::splat(size * 2.0)),
                );
                if let Some(mut tag) = tag {
                    let want = glow_tag(Color::WHITE, MOTE_GLOW * level.clamp(0.0, 1.4));
                    if *tag != want {
                        *tag = want;
                    }
                }
                vis.set_if_neq(Visibility::Inherited);
            }
        }
    }
}
