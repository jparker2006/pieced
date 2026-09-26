//! The target figure: every non-player character is drawn as the knight
//! (docs/M2-SPEC.md → The knight), replacing Milestone 1's training-dummy figure.
//! Only the look changes; the dummy's behavior and hitboxes are untouched.
//!
//! A [`TargetFigure`] is a top-level entity that follows its owner with render
//! interpolation and faces its look direction. It carries the knight model (from
//! `models::spawn_model`, toon-dressed and ink-outlined by `look`, with the
//! knight's warm-rim material), a [`BlobShadow`] under the boots, and the
//! knight's [`KnightAnim`], fed each frame with the owner's velocity, grounded
//! state, jumps, landings, hits and elimination (see `crate::knight`). When the
//! owner is eliminated the knight shows X eyes for `knight::KO_TIME`, then the
//! figure hides until respawn, when it pops back in.

use crate::{
    app::BootGate,
    combat::Downed,
    knight::{
        self, KNIGHT_MODEL, KnightAnim, KnightEvent, KnightInput, KnightModel, KnightRig,
        RespawnSparkle, SPARKLE_TIME,
    },
    look::{BlobShadow, ModelDressed, Outline},
    models::{ModelLibrary, spawn_model},
    movement::Motor,
    shared::{
        AppState, Character, DamageDealt, DamageTarget, EyeHeight, GameCue, Health, LookAngles,
        Player, PreviousFeet,
    },
};
use bevy::prelude::*;

/// Blob shadow under a standing figure (a little wider than the body capsule).
pub const FIGURE_SHADOW_RADIUS: f32 = 0.45;
/// The [`BootGate`] key held until every figure's knight is rigged, so its
/// draws are warmed up behind the loading screen.
pub const KNIGHT_GATE: &str = "knight";
/// Never hold Boot for the knight longer than this many frames.
pub const KNIGHT_GATE_TIMEOUT: u32 = 600;

/// The visible figure of a non-player character.
#[derive(Component, Debug)]
pub struct TargetFigure {
    pub owner: Entity,
}

/// Spawns, follows and animates the figures. Client only; needs `LookPlugin` and
/// `ModelsPlugin` for the model to appear (without them the figure is an empty,
/// still-animated entity).
pub struct TargetFigurePlugin;

impl Plugin for TargetFigurePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BootGate>();
        app.world_mut().resource_mut::<BootGate>().hold(KNIGHT_GATE);
        app.add_message::<ModelDressed>()
            .add_message::<DamageDealt>()
            .add_message::<GameCue>()
            .add_systems(Startup, knight::create_knight_assets)
            .add_systems(Update, (attach_knight_models, release_knight_gate))
            .add_systems(
                PostUpdate,
                (
                    knight::rig_knights,
                    pose_target_figures,
                    animate_knights,
                    knight::fade_sparkles,
                )
                    .chain()
                    .before(TransformSystems::Propagate),
            )
            .add_observer(spawn_target_figure);
    }
}

fn spawn_target_figure(
    add: On<Add, Character>,
    players: Query<(), With<Player>>,
    owners: Query<&Transform>,
    mut commands: Commands,
) {
    // The player is invisible in first person.
    if players.contains(add.entity) {
        return;
    }
    let at = owners.get(add.entity).copied().unwrap_or_default();
    commands.spawn((
        Name::new("Target figure"),
        TargetFigure { owner: add.entity },
        KnightAnim::new(add.entity.to_bits()),
        BlobShadow::new(FIGURE_SHADOW_RADIUS),
        Transform::from_translation(at.translation),
        Visibility::default(),
    ));
}

/// Puts the knight model under each new figure once the model library exists.
fn attach_knight_models(
    figures: Query<Entity, (With<TargetFigure>, Without<KnightModel>)>,
    library: Option<Res<ModelLibrary>>,
    mut commands: Commands,
) {
    let Some(library) = library else {
        return;
    };
    for figure in &figures {
        let Some(model) = spawn_model(&mut commands, &library, KNIGHT_MODEL, Transform::IDENTITY)
        else {
            error!("target figure: no `{KNIGHT_MODEL}` model in the library");
            commands
                .entity(figure)
                .insert(KnightModel(Entity::PLACEHOLDER));
            continue;
        };
        commands
            .entity(model)
            .insert((Outline::default(), ChildOf(figure)));
        commands.entity(figure).insert(KnightModel(model));
    }
}

/// Lets Boot finish once every figure's knight is rigged (and warm-up items
/// registered), or there is nothing to wait for: no model library, no figures,
/// or no knight model.
fn release_knight_gate(
    mut gate: ResMut<BootGate>,
    library: Option<Res<ModelLibrary>>,
    figures: Query<(Option<&KnightModel>, Has<KnightRig>), With<TargetFigure>>,
    mut frames: Local<u32>,
) {
    if !gate.held().any(|k| k == KNIGHT_GATE) {
        return;
    }
    *frames += 1;
    let done = match library {
        None => *frames > 2,
        Some(library) => {
            let missing = library.failed().iter().any(|f| f == KNIGHT_MODEL);
            library.is_ready()
                && figures.iter().all(|(model, rigged)| {
                    rigged || missing || model.is_some_and(|m| m.0 == Entity::PLACEHOLDER)
                })
        }
    };
    if done || *frames > KNIGHT_GATE_TIMEOUT {
        if !done {
            warn!("target figure: the knight wasn't rigged in time; not holding Boot");
        }
        gate.release(KNIGHT_GATE);
    }
}

/// Whether a character counts as eliminated for its figure: downed awaiting
/// respawn, or dead.
pub fn figure_hidden(health: Option<&Health>, downed: bool) -> bool {
    downed || health.is_some_and(Health::is_dead)
}

/// Follows each figure's owner: interpolated feet, look yaw, crouch height.
pub fn pose_target_figures(
    mut commands: Commands,
    fixed: Res<Time<Fixed>>,
    state: Res<State<AppState>>,
    owners: Query<
        (
            &Transform,
            Option<&PreviousFeet>,
            Option<&LookAngles>,
            Option<&EyeHeight>,
        ),
        (With<Character>, Without<TargetFigure>),
    >,
    mut figures: Query<(Entity, &TargetFigure, &mut Transform)>,
) {
    let alpha = if *state.get() == AppState::Playing {
        fixed.overstep_fraction().clamp(0.0, 1.0)
    } else {
        1.0
    };
    for (entity, figure, mut transform) in &mut figures {
        let Ok((owner, previous, look, eye)) = owners.get(figure.owner) else {
            commands.entity(entity).despawn();
            continue;
        };
        let feet = previous.map_or(owner.translation, |p| p.0.lerp(owner.translation, alpha));
        let yaw = look.map_or(0.0, |l| l.yaw);
        let crouch = eye.map_or(1.0, |e| (e.0 / EyeHeight::default().0).clamp(0.6, 1.0));
        transform.translation = feet;
        transform.rotation = Quat::from_rotation_y(yaw);
        transform.scale = Vec3::new(1.0, crouch, 1.0);
    }
}

/// Feeds each knight its owner's motion and moments, steps its animation and
/// writes the pose (and the figure's visibility).
#[allow(clippy::type_complexity)]
pub fn animate_knights(
    time: Res<Time>,
    mut damage: MessageReader<DamageDealt>,
    mut cues: MessageReader<GameCue>,
    owners: Query<
        (
            Option<&Motor>,
            Option<&Health>,
            Has<Downed>,
            Option<&LookAngles>,
        ),
        With<Character>,
    >,
    mut figures: Query<(
        Entity,
        &TargetFigure,
        &mut KnightAnim,
        Option<&KnightRig>,
        &mut Visibility,
    )>,
    mut transforms: Query<&mut Transform, Without<TargetFigure>>,
    mut visibility: Query<&mut Visibility, Without<TargetFigure>>,
    mut commands: Commands,
) {
    // World directions into an owner's model space (its figure faces its yaw).
    let local = |owner: Entity, v: Vec3| {
        let yaw = owners
            .get(owner)
            .ok()
            .and_then(|o| o.3)
            .map_or(0.0, |l| l.yaw);
        Quat::from_rotation_y(-yaw) * v
    };
    let mut events: Vec<(Entity, KnightEvent)> = Vec::new();
    for hit in damage.read() {
        if hit.target_kind != DamageTarget::Character || hit.amount <= 0.0 {
            continue;
        }
        // The shot pushes into the body: against the hit surface's normal.
        let push = Vec3::new(-hit.normal.x, 0.0, -hit.normal.z);
        let push = if push.length_squared() > 1e-6 {
            local(hit.target, push)
        } else {
            Vec3::Z
        };
        events.push((
            hit.target,
            KnightEvent::Hit {
                push,
                headshot: hit.headshot,
            },
        ));
    }
    for cue in cues.read() {
        match *cue {
            GameCue::Jump { who } => events.push((who, KnightEvent::Jump)),
            GameCue::Land { who, speed } => events.push((who, KnightEvent::Land { speed })),
            _ => {}
        }
    }
    let dt = time.delta_secs();
    for (entity, figure, mut anim, rig, mut shown) in &mut figures {
        for (_, event) in events.iter().filter(|(who, _)| *who == figure.owner) {
            anim.event(*event);
        }
        let input = match owners.get(figure.owner) {
            Ok((motor, health, downed, _)) => KnightInput {
                velocity: local(figure.owner, motor.map_or(Vec3::ZERO, |m| m.velocity)),
                grounded: motor.is_none_or(|m| m.grounded),
                downed: figure_hidden(health, downed),
            },
            Err(_) => KnightInput::default(),
        };
        let was_down = anim.is_downed();
        let pose = anim.step(dt, &input);
        if was_down && !anim.is_downed() {
            commands.spawn((
                Name::new("Respawn sparkle"),
                RespawnSparkle { left: SPARKLE_TIME },
                knight::sparkle_halo(0.0),
                Transform::from_xyz(0.0, 1.0, 0.0),
                ChildOf(entity),
            ));
        }
        shown.set_if_neq(if pose.visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if let Some(rig) = rig {
            knight::write_pose(&pose, rig, &mut transforms, &mut visibility);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn figure_hides_when_owner_is_down() {
        let mut health = Health::default();
        assert!(!figure_hidden(Some(&health), false));
        assert!(figure_hidden(Some(&health), true));
        health.apply(1000.0);
        assert!(figure_hidden(Some(&health), false));
        assert!(!figure_hidden(None, false));
    }
}
