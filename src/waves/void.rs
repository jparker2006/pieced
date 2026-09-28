//! The void (docs/M3-SPEC.md → The grunt, item 12; D78): a knight knocked
//! through the barrier falls off the island and counts as an elimination.
//!
//! Movement does the knocking off ([`VoidFall`]: the barrier stops the player
//! only, a shoved knight crossing the barrier line is flung out over the
//! margin and falls under gravity). This module does the rest, in the fixed
//! step:
//!
//! - while it flies, the knight's controls are dead ([`silence_fallers`]): no
//!   steering, no wand wind-up, so it never casts on the way down;
//! - the moment its feet are [`VOID_DEPTH`] below the island top it is
//!   eliminated ([`void_eliminations`]): [`Downed`], an [`Eliminated`] credited
//!   to whoever knocked it (the last pump that shoved it), a player
//!   elimination in [`CombatStats`], and a [`VoidKill`] so the wave director
//!   adds the void bonus. A knight shot dead on the way down is an ordinary
//!   kill (no bonus).
//!
//! The player never falls: it never gets a knockback, so the barrier and the
//! movement bounds always hold it.

use super::VoidKill;
use crate::{
    combat::{CombatStats, Downed},
    movement::{ISLAND_TOP, VOID_DEPTH, VoidFall},
    orb::Wand,
    shared::{Eliminated, Player, PlayerIntent, SimSet, SimTick},
};
use bevy::prelude::*;

pub(super) fn build(app: &mut App) {
    app.add_systems(FixedUpdate, silence_fallers.in_set(SimSet::Tool))
        .add_systems(
            FixedUpdate,
            void_eliminations
                .in_set(SimSet::Resolve)
                .before(super::read_run_messages),
        );
}

/// A knight flying off the island does nothing: its intent is cleared after
/// its brain runs (and before the wand reads it), so any wind-up is let go.
pub fn silence_fallers(mut fallers: Query<(&mut PlayerIntent, Option<&mut Wand>), With<VoidFall>>) {
    for (mut intent, wand) in &mut fallers {
        if *intent != PlayerIntent::default() {
            *intent = PlayerIntent::default();
        }
        if let Some(mut wand) = wand
            && wand.windup.is_some()
        {
            wand.windup = None;
        }
    }
}

/// Counts a falling knight as eliminated once it is [`VOID_DEPTH`] below the
/// island top.
pub fn void_eliminations(
    mut commands: Commands,
    tick: Res<SimTick>,
    fallers: Query<(Entity, &Transform, &VoidFall), Without<Downed>>,
    players: Query<(), With<Player>>,
    mut stats: ResMut<CombatStats>,
    mut eliminated: MessageWriter<Eliminated>,
    mut void: MessageWriter<VoidKill>,
) {
    for (knight, transform, fall) in &fallers {
        if transform.translation.y > ISLAND_TOP - VOID_DEPTH {
            continue;
        }
        commands.entity(knight).insert(Downed { tick: tick.0 });
        eliminated.write(Eliminated {
            victim: knight,
            by: fall.by,
            position: transform.translation,
            tick: tick.0,
        });
        if fall.by.is_some_and(|by| players.contains(by)) {
            stats.eliminations += 1;
        }
        void.write(VoidKill { knight });
    }
}
