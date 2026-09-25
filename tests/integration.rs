//! Cross-slice behavior with every gameplay plugin running together: movement,
//! building, combat and the dummy (gates G4 and G6).

use bevy::prelude::*;
use pieced::{
    building::{Piece, PieceMap, PieceSlot, clear_pieces, place_piece},
    dummy::{Dummy, look_toward},
    player::{BODY_BOTTOM, BODY_TOP},
    shared::{
        ActiveTool, DamageDealt, DamageTarget, EyeHeight, Facing, GridCell, Hitbox, LEVEL_HEIGHT,
        PieceChange, PieceChanged, PieceKind, PreviousFeet, WeaponKind,
    },
    sim::Sim,
};

fn empty_sim() -> Sim {
    let mut sim = Sim::new();
    clear_pieces(sim.world_mut());
    sim.tick();
    sim
}

fn dummy(sim: &mut Sim) -> Entity {
    sim.world_mut()
        .query_filtered::<Entity, With<Dummy>>()
        .single(sim.world())
        .expect("one dummy")
}

fn put(sim: &mut Sim, entity: Entity, feet: Vec3) {
    sim.world_mut()
        .get_mut::<Transform>(entity)
        .unwrap()
        .translation = feet;
    if let Some(mut prev) = sim.world_mut().get_mut::<PreviousFeet>(entity) {
        prev.0 = feet;
    }
}

fn aim_at(sim: &mut Sim, point: Vec3) {
    let player = sim.player();
    let eye = sim.feet(player) + Vec3::Y * sim.get::<EyeHeight>(player).0;
    let look = look_toward(point - eye);
    sim.set_look(player, look.yaw, look.pitch);
}

fn cell_center(x: i32, z: i32) -> Vec3 {
    GridCell::new(x, z, 0).base_center()
}

#[test]
fn turbo_ramp_rush_with_real_movement_climbs_levels() {
    let mut sim = empty_sim();
    let player = sim.player();
    put(&mut sim, player, cell_center(4, 11));
    sim.set_look(player, Facing::North.yaw(), 8f32.to_radians());
    sim.player_intent().select = Some(ActiveTool::Build(PieceKind::Ramp));
    sim.tick();
    {
        let mut intent = sim.player_intent();
        intent.fire = true;
        intent.fire_pressed = true;
        intent.move_axis = Vec2::Y;
    }
    sim.run_seconds(4.0);
    let feet = sim.feet(player);
    let ramps = sim
        .world_mut()
        .query::<&Piece>()
        .iter(sim.world())
        .filter(|p| p.kind == PieceKind::Ramp)
        .count();
    assert!(ramps >= 3, "ramps placed while running: {ramps}");
    assert!(
        feet.y >= 2.0 * LEVEL_HEIGHT - 0.5,
        "climbed only to y = {:.2} with {ramps} ramps",
        feet.y
    );
}

#[test]
fn rifle_needs_at_least_a_second_to_break_a_real_wall() {
    let mut sim = empty_sim();
    let player = sim.player();
    sim.record::<PieceChanged>();
    put(&mut sim, player, cell_center(5, 8));
    // A wall on the north edge of the cell two ahead: about 6 m away.
    let wall = place_piece(
        sim.world_mut(),
        PieceSlot::wall(GridCell::new(5, 6, 0), Facing::North),
    )
    .expect("wall placed");
    let wall_center = sim.world().get::<Piece>(wall).unwrap().slot().center();
    aim_at(&mut sim, wall_center);
    sim.player_intent().select = Some(ActiveTool::Weapon(WeaponKind::Rifle));
    sim.ticks(20); // finish the weapon switch
    let start = sim.sim_tick();
    sim.player_intent().fire = true;
    sim.player_intent().fire_pressed = true;
    let mut broke_at = None;
    for _ in 0..(60 * 4) {
        sim.tick();
        aim_at(&mut sim, wall_center);
        if sim
            .recorded::<PieceChanged>()
            .iter()
            .any(|c| c.entity == wall && c.change == PieceChange::Destroyed)
        {
            broke_at = Some(sim.sim_tick());
            break;
        }
    }
    let seconds = (broke_at.expect("the wall breaks within 4 s") - start) as f32 / 60.0;
    assert!(
        (1.0..=2.5).contains(&seconds),
        "wall broke after {seconds:.2} s of rifle fire"
    );
}

#[test]
fn walls_stop_bullets_until_they_break() {
    let mut sim = empty_sim();
    sim.tuning_mut().dummy.stand_still = true;
    sim.record::<DamageDealt>();
    let player = sim.player();
    let target = dummy(&mut sim);
    put(&mut sim, player, cell_center(5, 9));
    put(&mut sim, target, cell_center(5, 5));
    sim.tick();
    place_piece(
        sim.world_mut(),
        PieceSlot::wall(GridCell::new(5, 7, 0), Facing::North),
    )
    .expect("wall placed");
    let chest = sim.feet(target) + Vec3::Y * 1.0;
    aim_at(&mut sim, chest);
    sim.player_intent().select = Some(ActiveTool::Weapon(WeaponKind::Rifle));
    sim.ticks(20);
    sim.clear_recorded::<DamageDealt>();
    sim.player_intent().fire = true;
    sim.ticks(30); // 0.5 s: the wall (200 HP) is still standing
    let hits = sim.recorded::<DamageDealt>();
    assert!(
        hits.iter().all(|d| d.target_kind == DamageTarget::Piece),
        "a bullet went through the wall"
    );
    assert!(hits.iter().any(|d| d.target_kind == DamageTarget::Piece));
    // Keep firing: once the wall breaks, the dummy starts taking hits.
    sim.ticks(120);
    assert!(
        sim.recorded::<DamageDealt>()
            .iter()
            .any(|d| d.target_kind == DamageTarget::Character && d.target == target)
    );
}

#[test]
fn crouched_body_hitbox_shrinks_from_the_top_and_stays_on_the_feet() {
    let mut sim = empty_sim();
    sim.tuning_mut().dummy.stand_still = true;
    let target = dummy(&mut sim);
    sim.tick();
    sim.intent(target).crouch = true;
    sim.ticks(30);
    let feet = sim.feet(target);
    let eye = sim.get::<EyeHeight>(target).0;
    let drop = sim
        .world()
        .resource::<pieced::tuning::Tuning>()
        .movement
        .eye_height
        - eye;
    assert!(drop > 0.3, "the dummy crouched (eye {eye})");
    let mut body = None;
    let mut q = sim
        .world_mut()
        .query::<(&Hitbox, &GlobalTransform, &avian3d::prelude::Collider)>();
    for (hitbox, global, collider) in q.iter(sim.world()) {
        if hitbox.owner == target && !hitbox.head {
            let capsule = collider.shape().as_capsule().expect("capsule body");
            let half = capsule.segment.length() / 2.0 + capsule.radius;
            let center = global.translation().y;
            body = Some((center - half, center + half));
        }
    }
    let (bottom, top) = body.expect("body hitbox");
    assert!(
        (bottom - (feet.y + BODY_BOTTOM)).abs() < 0.05,
        "body bottom {bottom:.2} vs feet {:.2}",
        feet.y
    );
    assert!(
        top <= feet.y + BODY_TOP - drop + 0.05,
        "crouched body top {top:.2} is still at standing height"
    );
    let _ = sim.world().resource::<PieceMap>();
}
