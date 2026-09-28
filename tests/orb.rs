//! The grunt's wand and its spell orb through the simulation seam
//! (docs/M3-SPEC.md → The orb, Fairness): scripted `PlayerIntent` in, orbs,
//! damage and messages out. Fixed 60 Hz ticks, seeded, Waves mode.

use bevy::prelude::*;
use pieced::{
    arena::visuals::wand::{WAND_LENGTH, raised_tip},
    audio::wand::{VIEW_MARGIN, in_view},
    building::{Piece, PieceSlot, clear_pieces, place_piece},
    combat::Downed,
    dummy::look_toward,
    hud::damage_arrow::{DamageArrowTracking, DamageArrows, arrow_angle, arrow_offset},
    models::Sidecar,
    orb::{
        ORB_POOL, Orb, OrbHit, OrbImpact, OrbSlot, OrbTuning, REARM, WAND_TIP, Wand,
        recycle_all_orbs, wand_tip,
    },
    player::spawn_character,
    shared::{
        DamageDealt, DamageTarget, Eliminated, Facing, GameCue, GridCell, Health, LookAngles,
        PieceHit,
    },
    sim::Sim,
};

/// Knights stand this far north of the player (m).
const RANGE: f32 = 10.0;

struct Range {
    sim: Sim,
    player: Entity,
}

impl Range {
    fn new(seed: u64) -> Self {
        let mut sim = Sim::waves(seed);
        clear_pieces(sim.world_mut());
        sim.record::<GameCue>();
        sim.record::<DamageDealt>();
        sim.record::<Eliminated>();
        sim.record::<OrbImpact>();
        sim.record::<PieceHit>();
        let player = sim.player();
        sim.tick();
        Self { sim, player }
    }

    fn player_feet(&self) -> Vec3 {
        self.sim.feet(self.player)
    }

    /// A knight with a wand at `feet`, aiming his orbs at `target`.
    fn knight(&mut self, feet: Vec3, target: Vec3, fire_interval: f32) -> Entity {
        let look = orb_look(feet, target);
        let world = self.sim.world_mut();
        let mut commands = world.commands();
        let knight = spawn_character(
            &mut commands,
            feet,
            look,
            Health::full(100.0, 0.0),
            (Wand::new(fire_interval),),
        );
        world.flush();
        knight
    }

    /// A knight `RANGE` m north of the player, aiming at the player's chest.
    fn knight_ahead(&mut self) -> Entity {
        let feet = self.player_feet() + Vec3::new(0.0, 0.0, -RANGE);
        let chest = self.player_feet() + Vec3::Y * 0.9;
        self.knight(feet, chest, 1.5)
    }

    fn aim(&mut self, knight: Entity, target: Vec3) {
        let look = orb_look(self.sim.feet(knight), target);
        self.sim.set_look(knight, look.yaw, look.pitch);
    }

    fn fire(&mut self, knight: Entity, held: bool) {
        let mut intent = self.sim.intent(knight);
        intent.fire = held;
        intent.fire_pressed = held;
    }

    fn cues(&self) -> Vec<GameCue> {
        self.sim.recorded::<GameCue>()
    }

    fn windups(&self, knight: Entity) -> usize {
        self.cues()
            .iter()
            .filter(|c| matches!(c, GameCue::WandWindup { who } if *who == knight))
            .count()
    }

    fn fired(&self, knight: Entity) -> usize {
        self.cues()
            .iter()
            .filter(|c| matches!(c, GameCue::OrbFired { who } if *who == knight))
            .count()
    }

    fn damage_to(&self, e: Entity) -> Vec<DamageDealt> {
        self.sim
            .recorded::<DamageDealt>()
            .into_iter()
            .filter(|d| d.target == e)
            .collect()
    }

    fn active_orbs(&mut self) -> Vec<(Entity, Orb, Vec3)> {
        let world = self.sim.world_mut();
        world
            .query::<(Entity, &Orb, &Transform)>()
            .iter(world)
            .map(|(e, o, t)| (e, *o, t.translation))
            .collect()
    }

    fn entity_count(&mut self) -> usize {
        self.sim
            .world_mut()
            .query::<Entity>()
            .iter(self.sim.world())
            .count()
    }

    /// Ticks until `done`, at most `max`; the number of ticks run.
    fn tick_until(&mut self, max: u32, done: impl Fn(&Self) -> bool) -> u32 {
        for i in 1..=max {
            self.sim.tick();
            if done(self) {
                return i;
            }
        }
        panic!("condition not met within {max} ticks");
    }
}

/// The look that sends an orb from the wand tip of a knight at `feet` through
/// `target` (the orb flies along the look, from the tip).
fn orb_look(feet: Vec3, target: Vec3) -> LookAngles {
    let mut look = look_toward(target - (feet + Vec3::Y * 1.62));
    for _ in 0..4 {
        look = look_toward(target - wand_tip(feet, look.yaw));
    }
    look
}

fn tuning() -> OrbTuning {
    OrbTuning::default()
}

#[test]
fn holding_fire_winds_up_for_0_4_s_then_releases_an_orb() {
    let mut r = Range::new(1);
    let knight = r.knight_ahead();
    r.sim.tick();
    assert_eq!(r.windups(knight), 0, "no wind-up without the trigger");
    r.fire(knight, true);
    r.sim.tick();
    assert_eq!(r.windups(knight), 1, "the wind-up cue");
    let wand = *r.sim.get::<Wand>(knight);
    assert!(wand.is_winding());
    assert!((wand.windup.unwrap() - 0.4).abs() < 1e-4, "{wand:?}");
    let ticks = r.tick_until(60, |r| r.fired(knight) == 1);
    assert_eq!(ticks, 24, "0.4 s of wind-up at 60 Hz");
    assert!(!r.sim.get::<Wand>(knight).is_winding());
    assert_eq!(r.active_orbs().len(), 1);
    // No more without the trigger.
    r.fire(knight, false);
    r.sim.run_seconds(3.0);
    assert_eq!(r.windups(knight), 1);
    assert_eq!(r.fired(knight), 1);
}

#[test]
fn letting_go_during_the_windup_cancels_it() {
    let mut r = Range::new(11);
    let knight = r.knight_ahead();
    r.sim.tick();
    r.fire(knight, true);
    r.sim.ticks(15);
    assert!(r.sim.get::<Wand>(knight).is_winding());
    // He loses sight and lets go: the wind-up is cancelled.
    r.fire(knight, false);
    r.sim.tick();
    assert!(!r.sim.get::<Wand>(knight).is_winding());
    // The cancel didn't spend his 1.5 s interval: pressed again, he winds up
    // after the short re-arm.
    r.fire(knight, true);
    let rearm = r.tick_until(60, |r| r.windups(knight) == 2);
    assert!(
        rearm as f32 / 60.0 <= REARM + 0.02,
        "re-armed after {rearm} ticks"
    );
    // Lets go again at once: cancelled too.
    r.fire(knight, false);
    r.sim.run_seconds(2.0);
    assert_eq!(r.fired(knight), 0, "no orb from a cancelled wind-up");
    assert!(r.active_orbs().is_empty());
    assert!(r.damage_to(r.player).is_empty());
    // Held through, the next one fires.
    r.fire(knight, true);
    r.tick_until(60, |r| r.fired(knight) == 1);
}

#[test]
fn the_orb_leaves_the_wand_tip_at_30_m_s_and_recycles_at_60_m() {
    let mut r = Range::new(2);
    // Straight up from the middle of the island: nothing to hit.
    let feet = Vec3::new(-4.0, 0.0, -4.0);
    let knight = r.knight(feet, feet + Vec3::new(0.0, 100.0, -0.01), 1.5);
    r.sim.tick();
    r.fire(knight, true);
    r.tick_until(40, |r| r.fired(knight) == 1);
    r.fire(knight, false);
    let orbs = r.active_orbs();
    assert_eq!(orbs.len(), 1);
    let (orb_entity, orb, pos) = orbs[0];
    assert_eq!(orb.shooter, knight);
    let yaw = r.sim.get::<LookAngles>(knight).yaw;
    let tip = wand_tip(r.sim.feet(knight), yaw);
    assert!(
        orb.previous.distance(tip) < 1e-3,
        "starts at the wand tip {tip}, not {}",
        orb.previous
    );
    assert!((orb.velocity.length() - 30.0).abs() < 1e-3);
    assert!(orb.velocity.normalize().y > 0.95, "flies where he aims");
    // One tick of flight on the release tick, then half a metre per tick.
    assert!((pos.distance(tip) - 0.5).abs() < 1e-3);
    let mut last = pos;
    for _ in 0..10 {
        r.sim.tick();
        let now = r.sim.get::<Transform>(orb_entity).translation;
        assert!((now.distance(last) - 30.0 / 60.0).abs() < 1e-3);
        last = now;
    }
    // It flies its 60 m (120 ticks) and goes back to the pool, unhurt.
    let flown = r.tick_until(200, |r| {
        r.sim
            .recorded::<OrbImpact>()
            .iter()
            .any(|i| i.hit == OrbHit::Expired)
    });
    assert_eq!(11 + flown, 120, "60 m at 30 m/s");
    let impact = r.sim.recorded::<OrbImpact>()[0];
    assert_eq!(impact.orb, orb_entity);
    assert!((impact.point.distance(tip) - 60.0).abs() < 0.01);
    assert!(r.active_orbs().is_empty());
    assert!(
        r.sim.world().get::<Orb>(orb_entity).is_none(),
        "back in the pool"
    );
}

#[test]
fn an_orb_hits_the_player_for_12_and_18_on_the_head() {
    for (head, expected) in [(false, 12.0), (true, 18.0)] {
        let mut r = Range::new(3);
        let knight = r.knight_ahead();
        let height = if head { 1.62 } else { 0.9 };
        let target = r.player_feet() + Vec3::Y * height;
        r.aim(knight, target);
        let before = r.sim.get::<Health>(r.player).total();
        r.fire(knight, true);
        r.tick_until(60, |r| !r.damage_to(r.player).is_empty());
        r.fire(knight, false);
        let hits = r.damage_to(r.player);
        assert_eq!(hits.len(), 1);
        let hit = &hits[0];
        assert_eq!(hit.source, Some(knight), "the damage names its knight");
        assert_eq!(hit.target_kind, DamageTarget::Character);
        assert_eq!(hit.headshot, head);
        assert!((hit.amount - expected).abs() < 1e-4, "{}", hit.amount);
        let after = r.sim.get::<Health>(r.player).total();
        assert!((before - after - expected).abs() < 1e-4);
        let impact = r.sim.recorded::<OrbImpact>()[0];
        assert_eq!(impact.hit, OrbHit::Player { head });
        assert_eq!(impact.target, Some(r.player));
    }
}

#[test]
fn a_wall_between_takes_the_orb_as_12_structure_damage() {
    let mut r = Range::new(4);
    let knight = r.knight_ahead();
    // The player's cell's north edge, between them.
    let cell = GridCell::containing(r.player_feet());
    let wall = place_piece(r.sim.world_mut(), PieceSlot::wall(cell, Facing::North))
        .expect("the wall fits");
    r.sim.tick(); // the physics step adds it to the query tree
    let hp = r.sim.get::<Piece>(wall).hp;
    r.fire(knight, true);
    r.tick_until(60, |r| !r.sim.recorded::<PieceHit>().is_empty());
    r.fire(knight, false);
    r.sim.run_seconds(1.0);
    let hits = r.sim.recorded::<PieceHit>();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].piece, wall);
    assert_eq!(hits[0].amount, tuning().structure_damage);
    assert_eq!(hits[0].amount, 12.0);
    assert_eq!(hits[0].source, Some(knight));
    assert!((r.sim.get::<Piece>(wall).hp - (hp - 12.0)).abs() < 1e-4);
    assert!(r.damage_to(r.player).is_empty(), "the wall stopped it");
    assert!(matches!(
        r.sim.recorded::<OrbImpact>()[0].hit,
        OrbHit::Piece(_)
    ));
}

#[test]
fn a_wand_poking_through_a_wall_cannot_fire_from_its_far_side() {
    let mut r = Range::new(5);
    let cell = GridCell::containing(r.player_feet());
    let wall_z = cell.min_corner().z;
    let wall = place_piece(r.sim.world_mut(), PieceSlot::wall(cell, Facing::North))
        .expect("the wall fits");
    // The knight stands just north of the wall (his body clear of it): his
    // wand tip (0.85 m ahead) is on the player's side of it.
    let feet = Vec3::new(r.player_feet().x, 0.0, wall_z - 0.45);
    let knight = r.knight(feet, r.player_feet() + Vec3::Y * 0.9, 1.5);
    r.sim.tick();
    let tip = wand_tip(feet, r.sim.get::<LookAngles>(knight).yaw);
    assert!(tip.z > wall_z + 0.2, "the tip pokes through");
    r.fire(knight, true);
    r.tick_until(60, |r| !r.sim.recorded::<OrbImpact>().is_empty());
    r.fire(knight, false);
    r.sim.run_seconds(1.0);
    let impact = r.sim.recorded::<OrbImpact>()[0];
    assert!(matches!(impact.hit, OrbHit::Piece(_)), "{impact:?}");
    assert_eq!(impact.target, Some(wall));
    assert!(r.damage_to(r.player).is_empty());
}

#[test]
fn orbs_pass_through_other_knights() {
    let mut r = Range::new(6);
    let knight = r.knight_ahead();
    // A second knight right on the line, half way.
    let between = r.player_feet() + Vec3::new(0.0, 0.0, -RANGE / 2.0);
    let friend = r.knight(between, r.player_feet(), 1.5);
    r.sim.tick();
    let friend_health = *r.sim.get::<Health>(friend);
    r.fire(knight, true);
    r.tick_until(60, |r| !r.damage_to(r.player).is_empty());
    r.fire(knight, false);
    assert_eq!(*r.sim.get::<Health>(friend), friend_health);
    assert!(r.damage_to(friend).is_empty());
    assert_eq!(r.damage_to(r.player)[0].source, Some(knight));
}

#[test]
fn the_wand_keeps_its_fire_interval() {
    for interval in [1.5, 0.9] {
        let mut r = Range::new(7);
        // Up into the sky, so nothing dies.
        let feet = Vec3::new(-4.0, 0.0, -4.0);
        let knight = r.knight(feet, feet + Vec3::new(0.0, 100.0, -0.01), interval);
        r.sim.tick();
        r.fire(knight, true);
        let mut fired = Vec::new();
        let mut windups = Vec::new();
        for _ in 0..(8.0 * 60.0) as u32 {
            r.sim.tick();
            let tick = r.sim.sim_tick();
            if r.fired(knight) > fired.len() {
                fired.push(tick);
            }
            if r.windups(knight) > windups.len() {
                windups.push(tick);
            }
        }
        let expected = (interval * 60.0).round() as u64;
        assert!(fired.len() >= 4, "{fired:?}");
        for pair in fired.windows(2) {
            assert_eq!(pair[1] - pair[0], expected, "release to release: {fired:?}");
        }
        for (w, f) in windups.iter().zip(&fired) {
            assert_eq!(f - w, 24, "every orb follows its 0.4 s wind-up");
        }
    }
}

#[test]
fn a_player_killed_by_orbs_is_downed_and_eliminated() {
    let mut r = Range::new(8);
    let knight = r.knight_ahead();
    r.sim
        .world_mut()
        .get_mut::<Health>(r.player)
        .unwrap()
        .shield = 0.0;
    r.sim.world_mut().get_mut::<Health>(r.player).unwrap().hp = 20.0;
    r.fire(knight, true);
    r.tick_until(300, |r| !r.sim.recorded::<Eliminated>().is_empty());
    let kill = &r.sim.recorded::<Eliminated>()[0];
    assert_eq!(kill.victim, r.player);
    assert_eq!(kill.by, Some(knight));
    assert!(r.sim.world().get::<Downed>(r.player).is_some());
    let hits = r.damage_to(r.player);
    assert_eq!(hits.len(), 2, "12 then the last 8");
    assert!(hits[1].killed && !hits[0].killed);
    // Orbs fly through a downed player.
    r.sim.run_seconds(3.0);
    assert_eq!(r.damage_to(r.player).len(), 2);
    assert!(r.fired(knight) >= 3);
}

#[test]
fn orbs_come_from_a_fixed_pool_over_five_minutes_of_fire() {
    let mut r = Range::new(9);
    // Eight knights firing fast: some at the player, most up into the sky
    // (two-second flights), so the pool fills and recycles.
    let mut knights = Vec::new();
    for i in 0..8 {
        let a = i as f32 / 8.0 * std::f32::consts::TAU;
        let feet = Vec3::new(-2.0 + 6.0 * a.cos(), 0.0, -2.0 + 6.0 * a.sin());
        let target = if i < 2 {
            r.player_feet() + Vec3::Y * 0.9
        } else {
            feet + Vec3::new(0.3 * a.cos(), 100.0, 0.3 * a.sin())
        };
        let k = r.knight(feet, target, 0.45);
        knights.push(k);
    }
    r.sim.tick();
    for &k in &knights {
        r.fire(k, true);
    }
    r.sim.run_seconds(10.0);
    let settled = r.entity_count();
    let orbs = r
        .sim
        .world_mut()
        .query::<&OrbSlot>()
        .iter(r.sim.world())
        .count();
    assert_eq!(orbs, ORB_POOL);
    let mut most_in_flight = 0;
    for _ in 0..29 {
        for _ in 0..10 {
            r.sim.run_seconds(1.0);
            most_in_flight = most_in_flight.max(r.active_orbs().len());
        }
        assert_eq!(r.entity_count(), settled, "no entity per shot");
        r.sim.clear_recorded::<GameCue>();
        r.sim.clear_recorded::<OrbImpact>();
        r.sim.clear_recorded::<DamageDealt>();
    }
    assert_eq!(most_in_flight, ORB_POOL, "the pool filled and recycled");
    let orbs = r
        .sim
        .world_mut()
        .query::<&OrbSlot>()
        .iter(r.sim.world())
        .count();
    assert_eq!(orbs, ORB_POOL);
}

#[test]
fn a_restart_clears_the_sky_and_keeps_the_pool() {
    let slots = |r: &mut Range| {
        r.sim
            .world_mut()
            .query::<&OrbSlot>()
            .iter(r.sim.world())
            .count()
    };
    let mut r = Range::new(12);
    let feet = Vec3::new(-4.0, 0.0, -4.0);
    let knight = r.knight(feet, feet + Vec3::new(0.0, 100.0, -0.01), 0.45);
    r.sim.tick();
    r.fire(knight, true);
    r.sim.run_seconds(1.5);
    assert!(r.active_orbs().len() >= 2);
    let count = r.entity_count();
    // Recycled: nothing in flight, every slot kept.
    recycle_all_orbs(r.sim.world_mut());
    assert!(r.active_orbs().is_empty());
    assert_eq!(slots(&mut r), ORB_POOL);
    assert_eq!(r.entity_count(), count);
    // Or despawned outright (a restart that sweeps every `Orb`): the pool
    // refills its slots, and firing goes on.
    r.sim.run_seconds(1.0);
    let flying: Vec<Entity> = {
        let world = r.sim.world_mut();
        world
            .query_filtered::<Entity, With<Orb>>()
            .iter(world)
            .collect()
    };
    assert!(!flying.is_empty());
    for e in flying {
        r.sim.world_mut().despawn(e);
    }
    r.sim.tick();
    assert_eq!(slots(&mut r), ORB_POOL);
    let fired = r.fired(knight);
    r.sim.run_seconds(1.0);
    assert!(r.fired(knight) > fired);
}

#[test]
fn the_damage_arrow_points_at_its_source() {
    use std::f32::consts::{FRAC_PI_2, PI};
    let eye = Vec3::new(2.0, 1.62, 14.0);
    let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
    // Looking north (yaw 0 looks along -Z).
    assert!(close(
        arrow_angle(eye, 0.0, eye + Vec3::new(0.0, 3.0, -10.0)),
        0.0
    ));
    assert!(close(
        arrow_angle(eye, 0.0, eye + Vec3::X * 10.0),
        FRAC_PI_2
    ));
    assert!(close(
        arrow_angle(eye, 0.0, eye - Vec3::X * 10.0),
        -FRAC_PI_2
    ));
    assert!(close(arrow_angle(eye, 0.0, eye + Vec3::Z * 10.0).abs(), PI));
    // Turned to face east: east is dead ahead and north is on the left.
    let east = -FRAC_PI_2;
    assert!(close(arrow_angle(eye, east, eye + Vec3::X * 10.0), 0.0));
    assert!(close(
        arrow_angle(eye, east, eye - Vec3::Z * 10.0),
        -FRAC_PI_2
    ));
    // On screen (y down): straight up for 0, to the right for π/2.
    assert!(arrow_offset(0.0, 90.0).abs_diff_eq(Vec2::new(0.0, -90.0), 1e-3));
    assert!(arrow_offset(FRAC_PI_2, 90.0).abs_diff_eq(Vec2::new(90.0, 0.0), 1e-3));
}

#[test]
fn every_hit_on_the_player_raises_an_arrow_on_its_tick() {
    let mut r = Range::new(10);
    DamageArrowTracking::install(&mut r.sim.app);
    // A knight 10 m east of the player, who looks north.
    let feet = r.player_feet() + Vec3::X * RANGE;
    let knight = r.knight(feet, r.player_feet() + Vec3::Y * 0.9, 1.5);
    r.sim.tick();
    assert_eq!(r.sim.world().resource::<DamageArrows>().live().count(), 0);
    r.fire(knight, true);
    for _ in 0..3 {
        let hits_before = r.damage_to(r.player).len();
        r.tick_until(120, |r| r.damage_to(r.player).len() > hits_before);
        let hit = r.damage_to(r.player).last().cloned().unwrap();
        let arrows = r.sim.world().resource::<DamageArrows>();
        let arrow = arrows
            .live()
            .find(|a| a.source == Some(knight))
            .expect("an arrow for the knight");
        assert_eq!(arrow.tick, hit.tick, "raised on the hit tick");
        assert!(
            (arrow.angle - std::f32::consts::FRAC_PI_2).abs() < 0.05,
            "points right, at the knight: {}",
            arrow.angle
        );
    }
    // One knight, one arrow, refreshed by each hit; it fades a second after the last.
    assert_eq!(r.sim.world().resource::<DamageArrows>().live().count(), 1);
    r.fire(knight, false);
    r.sim.run_seconds(2.5);
    assert_eq!(r.sim.world().resource::<DamageArrows>().live().count(), 0);
}

#[test]
fn off_screen_knights_are_told_apart_from_visible_ones() {
    let eye = Vec3::ZERO;
    let look = Quat::IDENTITY; // along -Z
    let (vfov, aspect) = (70f32.to_radians(), 1.6);
    let seen = |p: Vec3| in_view(eye, look, vfov, aspect, p, VIEW_MARGIN);
    assert!(seen(Vec3::new(0.0, 0.0, -10.0)), "dead ahead");
    assert!(
        seen(Vec3::new(5.0, -1.0, -10.0)),
        "a little right and below"
    );
    assert!(!seen(Vec3::new(0.0, 0.0, 10.0)), "behind");
    assert!(!seen(Vec3::new(20.0, 0.0, -10.0)), "far to the right");
    assert!(!seen(Vec3::new(-20.0, 0.0, -10.0)), "far to the left");
    assert!(!seen(Vec3::new(0.0, 9.0, -10.0)), "above the frame");
    assert!(!seen(Vec3::new(0.0, 0.0, -0.05)), "at the eye");
    // The horizontal edge (about 48° at 16:10) sits outside the margin.
    let edge = (35f32.to_radians().tan() * aspect).atan();
    let at_edge = Vec3::new(edge.tan() * 10.0 * 0.97, 0.0, -10.0);
    assert!(!seen(at_edge), "the very edge counts as off-screen");
    // Turned around, the knight behind is now on screen.
    let turned = Quat::from_rotation_y(std::f32::consts::PI);
    assert!(in_view(
        eye,
        turned,
        vfov,
        aspect,
        Vec3::new(0.0, 0.0, 10.0),
        VIEW_MARGIN
    ));
}

#[test]
fn the_wand_tip_matches_the_raised_gauntlet_and_the_model() {
    // Right of the eye, below it and ahead: the orb reads as coming from
    // his raised right hand, not his face.
    const {
        assert!(WAND_TIP.x > 0.15 && WAND_TIP.x < 0.4);
        assert!(WAND_TIP.y > 1.0 && WAND_TIP.y < 1.5);
        assert!(WAND_TIP.z < -0.5 && WAND_TIP.z > -1.1);
    }
    // The simulation's launch point is the posed model's tip, fully raised.
    let posed = raised_tip(1.0);
    assert!(posed.abs_diff_eq(WAND_TIP, 0.01), "{posed} vs {WAND_TIP}");
    // And the model's `Tip` is where the pose assumes, within its budget.
    let side = Sidecar::parse(include_str!("../assets/models/wand.json")).unwrap();
    let tip = side.attach("Tip").expect("the wand has a Tip").position();
    assert!(
        tip.abs_diff_eq(Vec3::new(0.0, 0.0, -WAND_LENGTH), 1e-3),
        "{tip}"
    );
    assert!(side.triangles <= 400, "{} triangles", side.triangles);
    assert!(side.part("Crystal").is_some());
}
