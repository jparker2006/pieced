//! Shield potions (docs/M3-SPEC.md → Waves, D80) through the simulation seam:
//! downed knights drop one about one time in ten (seeded), at their feet;
//! walking within 1 m gives +25 shield, spilling into health, never above
//! max; it vanishes after 20 s; the pool is fixed.
//!
//! Brains are frozen ([`GalleryFreeze`]) and knights are downed through the
//! signals combat uses (`Downed` plus a killing [`DamageDealt`]).

use bevy::prelude::*;
use pieced::{
    combat::Downed,
    grunt::Parked,
    shared::{
        DamageDealt, DamageTarget, GalleryFreeze, GameCue, Health, PreviousFeet, TICK_SECONDS,
    },
    sim::Sim,
    waves::{POTION_POOL, PoolGrunt, PotionSlot, Run, RunPhase, SkipBreak},
};

fn frozen(seed: u64, chance: f32) -> Sim {
    let mut sim = Sim::waves(seed);
    sim.world_mut().insert_resource(GalleryFreeze);
    sim.tuning_mut().waves.potion_chance = chance;
    sim.record::<GameCue>();
    sim
}

fn in_play(sim: &mut Sim) -> Vec<Entity> {
    let mut q = sim
        .world_mut()
        .query_filtered::<Entity, (With<PoolGrunt>, Without<Parked>, Without<Downed>)>();
    let mut v: Vec<Entity> = q.iter(sim.world()).collect();
    v.sort();
    v
}

/// Ticks until a knight is in play, and returns it.
fn next_knight(sim: &mut Sim) -> Entity {
    for _ in 0..3000 {
        if let Some(&g) = in_play(sim).first() {
            return g;
        }
        if matches!(sim.world().resource::<Run>().phase, RunPhase::Break { .. }) {
            sim.world_mut().write_message(SkipBreak);
        }
        sim.tick();
    }
    panic!("no knight arrived");
}

fn kill(sim: &mut Sim, grunt: Entity) {
    let player = sim.player();
    let tick = sim.sim_tick();
    let at = sim.feet(grunt);
    let hp = sim.get::<Health>(grunt).hp;
    sim.world_mut().get_mut::<Health>(grunt).unwrap().hp = 0.0;
    sim.world_mut().entity_mut(grunt).insert(Downed { tick });
    sim.world_mut().write_message(DamageDealt {
        source: Some(player),
        target: grunt,
        target_kind: DamageTarget::Character,
        amount: hp,
        to_shield: 0.0,
        headshot: false,
        shield_broke: false,
        killed: true,
        point: at + Vec3::Y,
        normal: Vec3::Z,
        tick,
    });
}

/// Downs the next knight to arrive and returns where it fell.
fn kill_next(sim: &mut Sim) -> Vec3 {
    let g = next_knight(sim);
    let at = sim.feet(g);
    kill(sim, g);
    sim.tick();
    at
}

fn live(sim: &mut Sim) -> Vec<(Entity, PotionSlot, Vec3)> {
    let mut v: Vec<_> = sim
        .world_mut()
        .query::<(Entity, &PotionSlot, &Transform)>()
        .iter(sim.world())
        .filter(|(_, p, _)| p.live.is_some())
        .map(|(e, p, t)| (e, *p, t.translation))
        .collect();
    v.sort_by_key(|(e, ..)| *e);
    v
}

fn drops(sim: &Sim) -> Vec<(Entity, Vec3)> {
    sim.recorded::<GameCue>()
        .iter()
        .filter_map(|c| match *c {
            GameCue::PotionDropped { potion, at } => Some((potion, at)),
            _ => None,
        })
        .collect()
}

fn picks(sim: &Sim) -> usize {
    sim.recorded::<GameCue>()
        .iter()
        .filter(|c| matches!(c, GameCue::PotionPicked { .. }))
        .count()
}

fn put_player(sim: &mut Sim, feet: Vec3) {
    let p = sim.player();
    sim.world_mut().get_mut::<Transform>(p).unwrap().translation = feet;
    sim.world_mut().get_mut::<PreviousFeet>(p).unwrap().0 = feet;
}

fn set_health(sim: &mut Sim, hp: f32, shield: f32) {
    let p = sim.player();
    let mut h = sim.world_mut().get_mut::<Health>(p).unwrap();
    h.hp = hp;
    h.shield = shield;
}

fn health(sim: &mut Sim) -> Health {
    let p = sim.player();
    *sim.get::<Health>(p)
}

#[test]
fn a_downed_knight_drops_a_potion_at_its_feet() {
    let mut sim = frozen(1, 1.0);
    assert_eq!(live(&mut sim).len(), 0);
    let at = kill_next(&mut sim);
    let potions = live(&mut sim);
    assert_eq!(potions.len(), 1, "one potion");
    let (entity, slot, shown) = potions[0];
    let potion = slot.live.unwrap();
    // Where the knight lay when its down counted (it may still settle a little).
    assert!(
        potion.at.distance(at) < 0.3,
        "at the knight's feet: {} vs {at}",
        potion.at
    );
    assert_eq!(shown, potion.at, "the slot stands there");
    let life = potion.expires - potion.dropped;
    assert_eq!(life, (20.0 / TICK_SECONDS).round() as u64, "20 s to live");
    assert_eq!(drops(&sim), vec![(entity, potion.at)], "the drop cue");
    assert_eq!(
        *sim.get::<Visibility>(entity),
        Visibility::Inherited,
        "shown"
    );

    let mut none = frozen(1, 0.0);
    kill_next(&mut none);
    assert!(live(&mut none).is_empty(), "no roll, no potion");
    assert!(drops(&none).is_empty());
}

#[test]
fn about_one_knight_in_ten_drops_one_and_the_seed_decides_which() {
    fn run(seed: u64, kills: usize) -> Vec<bool> {
        let mut sim = frozen(seed, 0.1);
        (0..kills)
            .map(|_| {
                let before = drops(&sim).len();
                kill_next(&mut sim);
                drops(&sim).len() > before
            })
            .collect()
    }
    let a = run(3, 150);
    let dropped = a.iter().filter(|d| **d).count();
    assert!(
        (7..=26).contains(&dropped),
        "{dropped} potions from 150 knights"
    );
    assert_eq!(a[..40], run(3, 40)[..], "the same seed, the same drops");
}

#[test]
fn walking_over_a_potion_gives_shield_then_health_never_above_max() {
    let mut sim = frozen(2, 1.0);
    let at = kill_next(&mut sim);
    assert_eq!(live(&mut sim).len(), 1);

    // Out of reach: nothing happens.
    set_health(&mut sim, 60.0, 90.0);
    put_player(&mut sim, at + Vec3::X * 1.5);
    sim.run_seconds(0.2);
    assert_eq!(live(&mut sim).len(), 1, "1.5 m away: still there");
    assert_eq!(health(&mut sim).shield, 90.0);

    // Within 1 m: +25, 10 to fill the shield, 15 spilling into health.
    put_player(&mut sim, at + Vec3::X * 0.8);
    sim.tick();
    let h = health(&mut sim);
    assert_eq!((h.shield, h.hp), (100.0, 75.0));
    assert!(live(&mut sim).is_empty(), "drunk");
    assert_eq!(picks(&sim), 1, "the pickup cue");

    // Nearly full: capped at max.
    put_player(&mut sim, Vec3::new(2.0, 0.0, 14.0));
    let at = kill_next(&mut sim);
    set_health(&mut sim, 95.0, 100.0);
    put_player(&mut sim, at);
    sim.tick();
    let h = health(&mut sim);
    assert_eq!(
        (h.shield, h.hp),
        (h.max_shield, h.max_hp),
        "never above max"
    );
    assert_eq!(picks(&sim), 2);

    // Full: it stays on the ground for later.
    put_player(&mut sim, Vec3::new(2.0, 0.0, 14.0));
    let at = kill_next(&mut sim);
    put_player(&mut sim, at);
    sim.run_seconds(0.5);
    assert_eq!(live(&mut sim).len(), 1, "a full player leaves it");
    set_health(&mut sim, 100.0, 10.0);
    sim.tick();
    assert!(live(&mut sim).is_empty());
    assert_eq!(health(&mut sim).shield, 35.0);
}

#[test]
fn a_potion_vanishes_after_twenty_seconds() {
    let mut sim = frozen(4, 1.0);
    kill_next(&mut sim);
    let (entity, ..) = live(&mut sim)[0];
    sim.run_seconds(19.8);
    assert_eq!(live(&mut sim).len(), 1, "still there at 19.8 s");
    sim.run_seconds(0.4);
    assert!(live(&mut sim).is_empty(), "gone after 20 s");
    assert_eq!(*sim.get::<Visibility>(entity), Visibility::Hidden);
    assert_eq!(picks(&sim), 0, "it vanished, nobody drank it");
}

#[test]
fn the_pool_is_fixed_and_the_oldest_potion_makes_room() {
    let mut sim = frozen(5, 1.0);
    let slots = |sim: &mut Sim| {
        sim.world_mut()
            .query::<&PotionSlot>()
            .iter(sim.world())
            .count()
    };
    assert_eq!(slots(&mut sim), POTION_POOL);
    const { assert!(POTION_POOL >= 12, "D80: a pool of at least 12") };
    for _ in 0..POTION_POOL {
        kill_next(&mut sim);
    }
    let full = live(&mut sim);
    assert_eq!(full.len(), POTION_POOL);
    let (oldest, ..) = *full
        .iter()
        .min_by_key(|(_, p, _)| p.live.unwrap().dropped)
        .unwrap();
    let at = kill_next(&mut sim);
    let after = live(&mut sim);
    assert_eq!(after.len(), POTION_POOL, "still twelve");
    assert_eq!(slots(&mut sim), POTION_POOL, "the pool never grows");
    let reused = after.iter().find(|(e, ..)| *e == oldest).unwrap();
    let spot = reused.1.live.unwrap().at;
    assert!(
        spot.distance(at) < 0.3,
        "the oldest made room: {spot} vs {at}"
    );
}
