//! Shield potions (docs/M3-SPEC.md → Waves, D80): a downed knight drops one
//! with `potion_chance` (seeded from the run); walking within
//! `potion_pickup_radius` gives `potion_shield`, spilling into health once the
//! shield is full; it vanishes after `potion_lifetime`.
//!
//! Potions live on a fixed pool of [`POTION_POOL`] [`PotionSlot`] entities
//! made at startup, so a drop allocates nothing. An idle slot waits hidden at
//! [`POTION_PARK`]; a live one stands at the knight's feet with its
//! `Visibility` inherited. Slice B's look (the bob and the cyan glow) hangs its
//! mesh on these entities and reads [`PotionSlot::live`].

use super::{Run, ticks};
use crate::{
    combat::Downed,
    shared::{GameCue, Health, Player, SimTick},
    tuning::Tuning,
};
use bevy::prelude::*;

/// Potions that can be on the ground at once. When every slot is live, a new
/// drop takes the slot that would vanish soonest.
pub const POTION_POOL: usize = 12;
/// Where idle potion slots wait: under the island, out of sight.
pub const POTION_PARK: Vec3 = Vec3::new(0.0, -80.0, 0.0);
/// How far above or below the potion the player's feet may be and still pick
/// it up (m): the player has to stand on the same floor.
pub const PICKUP_HEIGHT: f32 = 1.2;

/// A pooled potion.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq)]
pub struct PotionSlot {
    /// Set while the potion is on the ground.
    pub live: Option<LivePotion>,
}

/// A potion on the ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LivePotion {
    /// Where it lies: the downed knight's feet.
    pub at: Vec3,
    /// Tick it dropped, and the tick it vanishes.
    pub dropped: u64,
    pub expires: u64,
}

/// Drops the director queued this tick (knight feet positions), taken by
/// [`step_potions`]. Preallocated; cleared every tick.
#[derive(Resource, Debug)]
pub struct PendingPotions(pub Vec<Vec3>);

impl Default for PendingPotions {
    fn default() -> Self {
        Self(Vec::with_capacity(16))
    }
}

/// Makes the fixed pool (Waves runs only).
pub(super) fn spawn_potion_pool(mut commands: Commands) {
    for _ in 0..POTION_POOL {
        commands.spawn((
            Name::new("Shield potion"),
            PotionSlot::default(),
            Transform::from_translation(POTION_PARK),
            Visibility::Hidden,
        ));
    }
}

/// How `amount` of potion splits for `health`: shield first, the rest to
/// health, neither above its max. Returns (to shield, to health).
pub fn potion_split(health: &Health, amount: f32) -> (f32, f32) {
    let to_shield = amount.min((health.max_shield - health.shield).max(0.0));
    let to_hp = (amount - to_shield).min((health.max_hp - health.hp).max(0.0));
    (to_shield, to_hp)
}

/// Whether a player standing at `feet` is in reach of a potion at `at`.
pub fn in_reach(feet: Vec3, at: Vec3, radius: f32) -> bool {
    feet.xz().distance(at.xz()) <= radius && (feet.y - at.y).abs() <= PICKUP_HEIGHT
}

type SlotParts = (
    Entity,
    &'static mut PotionSlot,
    &'static mut Transform,
    &'static mut Visibility,
);

/// Drops queued potions, lets the player pick them up, and expires old ones.
pub(super) fn step_potions(
    run: Res<Run>,
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    mut pending: ResMut<PendingPotions>,
    mut slots: Query<SlotParts, Without<Player>>,
    mut players: Query<(Entity, &Transform, &mut Health, Has<Downed>), With<Player>>,
    mut cues: MessageWriter<GameCue>,
) {
    let now = tick.0;
    let t = &tuning.waves;

    for at in pending.0.drain(..) {
        // An idle slot, else the live one closest to vanishing.
        let Some((entity, mut slot, mut transform, mut visibility)) =
            slots.iter_mut().min_by_key(|(_, s, ..)| match s.live {
                None => (0, 0),
                Some(live) => (1, live.expires),
            })
        else {
            break;
        };
        slot.live = Some(LivePotion {
            at,
            dropped: now,
            expires: now + ticks(t.potion_lifetime),
        });
        transform.translation = at;
        visibility.set_if_neq(Visibility::Inherited);
        cues.write(GameCue::PotionDropped { potion: entity, at });
    }

    let mut player = players
        .iter_mut()
        .next()
        .filter(|(_, _, health, downed)| !downed && !health.is_dead() && !run.is_ended());

    for (entity, mut slot, mut transform, mut visibility) in &mut slots {
        let Some(live) = slot.live else {
            continue;
        };
        let mut gone = now >= live.expires;
        if !gone && let Some((who, feet, health, _)) = player.as_mut() {
            let (to_shield, to_hp) = potion_split(health, t.potion_shield);
            // A full player leaves it on the ground for later.
            if (to_shield > 0.0 || to_hp > 0.0)
                && in_reach(feet.translation, live.at, t.potion_pickup_radius)
            {
                health.shield += to_shield;
                health.hp += to_hp;
                cues.write(GameCue::PotionPicked {
                    who: *who,
                    potion: entity,
                    at: live.at,
                });
                gone = true;
            }
        }
        if gone {
            slot.live = None;
            transform.translation = POTION_PARK;
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
}

/// Every potion back to the pool (a restart).
pub(super) fn recycle_all_potions(world: &mut World) {
    let mut q = world.query::<(&mut PotionSlot, &mut Transform, &mut Visibility)>();
    for (mut slot, mut transform, mut visibility) in q.iter_mut(world) {
        slot.live = None;
        transform.translation = POTION_PARK;
        visibility.set_if_neq(Visibility::Hidden);
    }
    if let Some(mut pending) = world.get_resource_mut::<PendingPotions>() {
        pending.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_potion_fills_the_shield_then_spills_into_health() {
        let mut h = Health::full(100.0, 100.0);
        h.shield = 90.0;
        h.hp = 60.0;
        assert_eq!(potion_split(&h, 25.0), (10.0, 15.0));
        h.hp = 95.0;
        assert_eq!(potion_split(&h, 25.0), (10.0, 5.0));
        h.shield = 100.0;
        h.hp = 100.0;
        assert_eq!(potion_split(&h, 25.0), (0.0, 0.0));
    }

    #[test]
    fn reach_is_a_flat_radius_on_the_same_floor() {
        let at = Vec3::new(2.0, 0.0, 2.0);
        assert!(in_reach(Vec3::new(2.9, 0.0, 2.0), at, 1.0));
        assert!(!in_reach(Vec3::new(3.1, 0.0, 2.0), at, 1.0));
        assert!(!in_reach(Vec3::new(2.0, 3.0, 2.0), at, 1.0));
    }
}
