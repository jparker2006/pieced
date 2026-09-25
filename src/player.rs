//! Characters in the simulation: spawning (player and any other combatant),
//! look application, tool selection and the per-tick bookkeeping every slice relies on.

use crate::{
    arena::ArenaLayout,
    shared::{
        ActiveTool, Ads, AppState, Character, EyeHeight, GameCue, Health, Hitbox, Layer,
        LookAngles, Player, PlayerIntent, PreviousFeet, SimSet, SimTick,
    },
    tuning::Tuning,
};
use avian3d::prelude::*;
use bevy::prelude::*;

/// Writers of intents in `PreUpdate` (device adapter, scenario director) run in
/// this set; look is applied right after it, before the fixed step.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IntentWriters;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_player)
            .configure_sets(PreUpdate, IntentWriters.after(bevy::input::InputSystems))
            .add_systems(
                PreUpdate,
                apply_look
                    .after(IntentWriters)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                FixedFirst,
                (count_tick, record_previous_feet).run_if(in_state(AppState::Playing)),
            )
            .add_systems(FixedUpdate, sync_active_tool.in_set(SimSet::Tool))
            .add_systems(
                FixedLast,
                clear_intent_edges.run_if(in_state(AppState::Playing)),
            );
    }
}

/// Body hitbox (capsule) dimensions for a standing character, relative to the feet.
pub const BODY_RADIUS: f32 = 0.33;
pub const BODY_BOTTOM: f32 = 0.05;
pub const BODY_TOP: f32 = 1.45;
/// Head hitbox (sphere) center height and radius, relative to the feet.
pub const HEAD_CENTER: f32 = 1.62;
pub const HEAD_RADIUS: f32 = 0.2;

/// Spawns a combatant at `feet` looking along `look`, with `extra` components (such
/// as [`Player`]) inserted in the same spawn. Returns the character entity.
/// Slices attach their own per-character components with an `On<Add, Character>`
/// observer rather than editing this function.
pub fn spawn_character(
    commands: &mut Commands,
    feet: Vec3,
    look: LookAngles,
    health: Health,
    extra: impl Bundle,
) -> Entity {
    let body_len = (BODY_TOP - BODY_BOTTOM - 2.0 * BODY_RADIUS).max(0.0);
    let character = commands
        .spawn((
            Name::new("Character"),
            Character,
            Transform::from_translation(feet),
            PreviousFeet(feet),
            look,
            EyeHeight::default(),
            health,
            PlayerIntent::default(),
            ActiveTool::default(),
            Ads::default(),
            RigidBody::Kinematic,
            extra,
        ))
        .id();
    commands.spawn((
        Name::new("Body hitbox"),
        Hitbox {
            owner: character,
            head: false,
        },
        Collider::capsule(BODY_RADIUS, body_len),
        CollisionLayers::new(Layer::Body, LayerMask::NONE),
        Sensor,
        Transform::from_xyz(0.0, (BODY_BOTTOM + BODY_TOP) / 2.0, 0.0),
        ChildOf(character),
    ));
    commands.spawn((
        Name::new("Head hitbox"),
        Hitbox {
            owner: character,
            head: true,
        },
        Collider::sphere(HEAD_RADIUS),
        CollisionLayers::new(Layer::Head, LayerMask::NONE),
        Sensor,
        Transform::from_xyz(0.0, HEAD_CENTER, 0.0),
        ChildOf(character),
    ));
    character
}

fn spawn_player(mut commands: Commands, layout: Res<ArenaLayout>, tuning: Res<Tuning>) {
    let health = Health::full(tuning.combat.max_hp, tuning.combat.max_shield);
    spawn_character(
        &mut commands,
        layout.player_spawn,
        layout.player_look,
        health,
        Player,
    );
}

fn apply_look(mut q: Query<(&mut PlayerIntent, &mut LookAngles)>) {
    for (mut intent, mut look) in &mut q {
        let delta = std::mem::take(&mut intent.look_delta);
        if delta != Vec2::ZERO {
            look.add(delta.x, delta.y);
        }
    }
}

fn count_tick(mut tick: ResMut<SimTick>) {
    tick.0 += 1;
}

fn record_previous_feet(mut q: Query<(&Transform, &mut PreviousFeet)>) {
    for (transform, mut prev) in &mut q {
        prev.0 = transform.translation;
    }
}

fn sync_active_tool(
    mut q: Query<(Entity, &PlayerIntent, &mut ActiveTool)>,
    mut cues: MessageWriter<GameCue>,
) {
    for (entity, intent, mut tool) in &mut q {
        if let Some(selected) = intent.select
            && *tool != selected
        {
            *tool = selected;
            cues.write(GameCue::WeaponSwitch {
                who: entity,
                tool: selected,
            });
        }
    }
}

fn clear_intent_edges(mut q: Query<&mut PlayerIntent>) {
    for mut intent in &mut q {
        intent.clear_edges();
    }
}
