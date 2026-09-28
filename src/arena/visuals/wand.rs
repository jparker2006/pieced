//! The knight's crystal wand (docs/M3-SPEC.md → The grunt, item 10): every
//! knight figure whose character carries a [`Wand`] holds the `wand` model
//! (`art/blender/assets/wand.py`) in his right gauntlet, parented to his
//! `PivotArmR` joint so it swings with the arm.
//!
//! During the 0.4 s wind-up the right arm raises the wand ([`RAISE`] about
//! the shoulder, eased in over the first part of the wind-up, held through
//! the release, then lowered) and the crystal glows up: its material steps
//! through [`GLOW_STEPS`] shared emissive levels and a halo on the `Tip`
//! swells. At the release it flares, then fades back to a gentle idle glow.
//! All of it is visual only: the character and its hitboxes never move, and
//! the orb leaves from `orb::WAND_TIP`, which [`raised_tip`] ties to this pose.

use super::target::{TargetFigure, animate_knights};
use crate::{
    knight::{KnightAssets, KnightRig},
    look::{Halo, ModelDressed, Outline, ToonMaterial},
    models::{MODEL_FORWARD_FIX, ModelLibrary, ModelParts, spawn_model},
    orb::Wand,
    shared::{FreezableTime, GameCue},
    tuning::Tuning,
};
use bevy::prelude::*;

/// The model's name in `assets/models/manifest.json`.
pub const WAND_MODEL: &str = "wand";
/// The `PivotArmR` joint in the knight's model space, and where the wand's
/// grip sits in his right glove (the model's origin), at rest.
pub const SHOULDER_R: Vec3 = Vec3::new(0.2, 1.2, 0.0);
pub const GRIP_R: Vec3 = Vec3::new(0.266, 0.69, -0.03);
/// At rest the wand points forward and this far down (rad).
pub const REST_DOWN: f32 = 40.0 * std::f32::consts::PI / 180.0;
/// How far the wind-up raises the arm forward about the shoulder (rad).
pub const RAISE: f32 = 70.0 * std::f32::consts::PI / 180.0;
/// The wand's tip, ahead of the grip along the wand (m; `Tip` in the model).
pub const WAND_LENGTH: f32 = 0.42;
/// The wand is drawn this much bigger than modelled, cartoon-chunky in his
/// big glove so it reads from across the island.
pub const WAND_SCALE: f32 = 1.35;
/// The raise eases in over this fraction of the wind-up.
pub const RAISE_IN: f32 = 0.55;
/// After the release the arm holds up this long (s), then lowers over
/// [`LOWER_TIME`].
pub const HOLD_TIME: f32 = 0.15;
pub const LOWER_TIME: f32 = 0.35;
/// The release flare fades over this long (s).
pub const FLARE_TIME: f32 = 0.25;
/// Emissive steps of the crystal's glow, and their range.
pub const GLOW_STEPS: usize = 6;
pub const GLOW_IDLE: f32 = 0.35;
pub const GLOW_FULL: f32 = 2.6;
/// The crystal's glow colour (sRGB): hot orange.
pub const CRYSTAL_GLOW: Color = Color::srgb(1.0, 0.45, 0.12);

/// Model-space rotation of the wand at rest (it points along its -Z).
pub fn rest_rotation() -> Quat {
    Quat::from_rotation_x(-REST_DOWN)
}

/// The arm's extra rotation about the shoulder for a raise of `raise` (0..=1).
pub fn raise_rotation(raise: f32) -> Quat {
    Quat::from_rotation_x(RAISE * raise.clamp(0.0, 1.0))
}

/// Where the wand tip is in the knight's model space (feet at the origin)
/// with the arm raised by `raise` (0..=1) and no other pose. At 1 this is
/// the orb's launch point, `orb::WAND_TIP`.
pub fn raised_tip(raise: f32) -> Vec3 {
    let r = raise_rotation(raise);
    let dir = rest_rotation() * Vec3::NEG_Z;
    SHOULDER_R + r * ((GRIP_R - SHOULDER_R) + dir * WAND_LENGTH * WAND_SCALE)
}

/// How far the arm is raised (0..=1): easing up through the wind-up
/// (`progress` 0..=1, `None` when not winding), holding after the release
/// (`since_release`, s), then lowering.
pub fn raise_amount(progress: Option<f32>, since_release: f32) -> f32 {
    let smooth = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    };
    if let Some(p) = progress {
        return smooth(p / RAISE_IN);
    }
    if since_release < HOLD_TIME {
        1.0
    } else {
        1.0 - smooth((since_release - HOLD_TIME) / LOWER_TIME)
    }
}

/// The crystal's glow (0..=1): rising through the wind-up, a flare at the
/// release fading over [`FLARE_TIME`], otherwise idle (0).
pub fn glow_amount(progress: Option<f32>, since_release: f32) -> f32 {
    match progress {
        Some(p) => 0.15 + 0.85 * p.clamp(0.0, 1.0).powf(1.5),
        None => (1.0 - since_release / FLARE_TIME).clamp(0.0, 1.0).powi(2),
    }
}

/// On a knight figure holding a wand.
#[derive(Component, Debug, Clone)]
pub struct FigureWand {
    /// The wand model's root (a child of his `PivotArmR`).
    pub model: Entity,
    /// The crystal's mesh and the `Tip` node, once the model is dressed.
    pub crystal: Option<Entity>,
    pub tip: Option<Entity>,
    /// Seconds since his last orb left.
    pub since_release: f32,
    /// The glow step on the crystal now.
    step: usize,
}

/// The crystal's glow materials, idle to full, shared by every wand.
#[derive(Resource, Debug, Clone)]
pub struct WandAssets {
    pub glow: [Handle<ToonMaterial>; GLOW_STEPS],
}

pub struct WandVisualsPlugin;

impl Plugin for WandVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ModelDressed>()
            .add_message::<GameCue>()
            .add_systems(Startup, create_wand_assets)
            .add_systems(Update, (attach_wands, dress_wands))
            .add_systems(
                PostUpdate,
                pose_wands
                    .after(animate_knights)
                    .before(TransformSystems::Propagate),
            );
    }
}

fn create_wand_assets(mut commands: Commands, mut materials: ResMut<Assets<ToonMaterial>>) {
    let glow = std::array::from_fn(|k| {
        let level = GLOW_IDLE + (GLOW_FULL - GLOW_IDLE) * k as f32 / (GLOW_STEPS - 1) as f32;
        materials.add(
            ToonMaterial::vertex_colored()
                .with_emissive(CRYSTAL_GLOW, level)
                .with_rim(1.0),
        )
    });
    commands.insert_resource(WandAssets { glow });
}

/// The wand root's transform under the knight's `PivotArmR` glTF node: the
/// grip and rest aim, from model space into the node's space (glTF nodes sit
/// under the model's forward fix).
fn wand_in_arm() -> Transform {
    let fix = MODEL_FORWARD_FIX;
    Transform {
        translation: fix.inverse() * (GRIP_R - SHOULDER_R),
        rotation: fix.inverse() * rest_rotation() * fix,
        scale: Vec3::splat(WAND_SCALE),
    }
}

/// Puts a wand in the right glove of every rigged knight whose character
/// carries one.
fn attach_wands(
    mut commands: Commands,
    library: Option<Res<ModelLibrary>>,
    figures: Query<(Entity, &TargetFigure, &KnightRig), Without<FigureWand>>,
    wands: Query<(), With<Wand>>,
) {
    let Some(library) = library else {
        return;
    };
    for (figure, target, rig) in &figures {
        if !wands.contains(target.owner) {
            continue;
        }
        // PivotArmR: see `knight::JOINTS`.
        let arm = rig.joints[4].0;
        let Some(model) = spawn_model(&mut commands, &library, WAND_MODEL, wand_in_arm()) else {
            warn_once!("knight wand: no `{WAND_MODEL}` model in the library");
            continue;
        };
        commands
            .entity(model)
            .insert((Outline::default(), ChildOf(arm)));
        commands.entity(figure).insert(FigureWand {
            model,
            crystal: None,
            tip: None,
            since_release: f32::MAX,
            step: usize::MAX,
        });
    }
}

/// Once a wand is dressed: the knight's warm-rim material on its brass and
/// wood, the glow material on its crystal, and a halo on its tip.
fn dress_wands(
    mut commands: Commands,
    mut dressed: MessageReader<ModelDressed>,
    parts: ModelParts,
    children: Query<&Children>,
    meshes: Query<(), With<Mesh3d>>,
    knight: Option<Res<KnightAssets>>,
    assets: Option<Res<WandAssets>>,
    mut figures: Query<&mut FigureWand>,
) {
    for event in dressed.read() {
        if event.name != WAND_MODEL {
            continue;
        }
        let Some(mut wand) = figures.iter_mut().find(|w| w.model == event.root) else {
            continue;
        };
        if let Some(knight) = &knight {
            for e in children.iter_descendants(event.root) {
                if meshes.contains(e) {
                    commands
                        .entity(e)
                        .insert(MeshMaterial3d(knight.material.clone()));
                }
            }
        }
        let crystal = parts.find(event.root, "Crystal").and_then(|node| {
            std::iter::once(node)
                .chain(children.iter_descendants(node))
                .find(|e| meshes.contains(*e))
        });
        if let (Some(crystal), Some(assets)) = (crystal, &assets) {
            commands
                .entity(crystal)
                .insert(MeshMaterial3d(assets.glow[0].clone()));
            wand.step = 0;
        }
        wand.crystal = crystal;
        wand.tip = parts.find(event.root, "Tip");
        if let Some(tip) = wand.tip {
            commands
                .entity(tip)
                .insert(Halo::new(CRYSTAL_GLOW, 0.3, 0.3));
        }
    }
}

/// The wind-up pose and glow, on top of the knight's animation.
#[allow(clippy::too_many_arguments)]
fn pose_wands(
    time: FreezableTime,
    tuning: Res<Tuning>,
    assets: Option<Res<WandAssets>>,
    mut cues: MessageReader<GameCue>,
    owners: Query<&Wand>,
    mut figures: Query<(&TargetFigure, &KnightRig, &mut FigureWand)>,
    mut transforms: Query<&mut Transform, Without<TargetFigure>>,
    mut halos: Query<&mut Halo>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    let fired: Vec<Entity> = cues
        .read()
        .filter_map(|c| match *c {
            GameCue::OrbFired { who } => Some(who),
            _ => None,
        })
        .collect();
    for (target, rig, mut wand) in &mut figures {
        let Ok(owner) = owners.get(target.owner) else {
            continue;
        };
        wand.since_release = if fired.contains(&target.owner) {
            0.0
        } else if wand.since_release < 1e6 {
            wand.since_release + dt
        } else {
            wand.since_release
        };
        let progress = owner.windup_progress(tuning.grunt.windup);
        let raise = raise_amount(progress, wand.since_release);
        if raise > 1e-4
            && let Ok(mut arm) = transforms.get_mut(rig.joints[4].0)
        {
            let fix = MODEL_FORWARD_FIX;
            arm.rotation = fix.inverse() * raise_rotation(raise) * fix * arm.rotation;
        }
        let glow = glow_amount(progress, wand.since_release);
        if let Some(assets) = &assets {
            let step = ((glow * (GLOW_STEPS - 1) as f32).round() as usize).min(GLOW_STEPS - 1);
            if step != wand.step
                && let Some(crystal) = wand.crystal
            {
                wand.step = step;
                commands
                    .entity(crystal)
                    .insert(MeshMaterial3d(assets.glow[step].clone()));
            }
        }
        if let Some(mut halo) = wand.tip.and_then(|t| halos.get_mut(t).ok()) {
            let want = Halo::new(CRYSTAL_GLOW, 0.3 + 0.55 * glow, 0.3 + 2.6 * glow);
            if *halo != want {
                *halo = want;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_raise_eases_up_holds_and_lowers() {
        assert_eq!(raise_amount(None, f32::MAX), 0.0);
        assert_eq!(raise_amount(Some(0.0), f32::MAX), 0.0);
        assert!((raise_amount(Some(RAISE_IN), f32::MAX) - 1.0).abs() < 1e-6);
        assert_eq!(raise_amount(Some(1.0), f32::MAX), 1.0);
        assert_eq!(raise_amount(None, 0.0), 1.0);
        assert_eq!(raise_amount(None, HOLD_TIME + LOWER_TIME), 0.0);
        let mid = raise_amount(None, HOLD_TIME + LOWER_TIME * 0.5);
        assert!(mid > 0.3 && mid < 0.7);
    }

    #[test]
    fn the_glow_builds_through_the_windup_and_flares_out() {
        assert!(glow_amount(Some(0.0), f32::MAX) < glow_amount(Some(0.5), f32::MAX));
        assert!((glow_amount(Some(1.0), f32::MAX) - 1.0).abs() < 1e-6);
        assert_eq!(glow_amount(None, 0.0), 1.0);
        assert_eq!(glow_amount(None, FLARE_TIME), 0.0);
        assert_eq!(glow_amount(None, f32::MAX), 0.0);
    }

    #[test]
    fn at_rest_the_wand_hangs_forward_and_down_by_his_hip() {
        let tip = raised_tip(0.0);
        assert!(tip.y < GRIP_R.y && tip.z < GRIP_R.z, "{tip}");
        assert!((tip.x - GRIP_R.x).abs() < 1e-6);
    }
}
