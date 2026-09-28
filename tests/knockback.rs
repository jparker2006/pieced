//! Pump knockback (docs/M3-SPEC.md → The grunt, item 11; D78) through the
//! simulation seam: scripted shots in, where the knight ends up out.
//!
//! A point-blank pump with every pellet landing shoves a knight about
//! `pump_knockback` (4 m) away from the shooter; a far pump much less; the
//! rifle not at all; the player never. The shove slides with the knight's own
//! collision, so it never carries a knight through a wall.

use bevy::prelude::*;
use pieced::{
    building::{self, PieceSlot},
    dummy::look_toward,
    grunt::Grunt,
    movement::Knockback,
    player,
    shared::{
        ActiveTool, DamageDealt, EyeHeight, Facing, GridCell, Health, LookAngles, WeaponKind,
    },
    sim::Sim,
};

/// The player stands here in every test, looking north (-Z) along x = 2,
/// a lane clear of props and initial cover.
const PLAYER_FEET: Vec3 = Vec3::new(2.0, 0.0, 14.7);

/// A knight (a grunt without a brain) standing at `feet`, tough enough to
/// survive the shots.
fn spawn_knight(sim: &mut Sim, feet: Vec3) -> Entity {
    let world = sim.world_mut();
    let mut commands = world.commands();
    let knight = player::spawn_character(
        &mut commands,
        feet,
        LookAngles::default(),
        Health::full(5000.0, 0.0),
        Grunt,
    );
    world.flush();
    knight
}

fn place(sim: &mut Sim, who: Entity, feet: Vec3) {
    let mut tf = sim.world_mut().get_mut::<Transform>(who).unwrap();
    tf.translation = feet;
}

fn aim(sim: &mut Sim, who: Entity, at: Vec3) {
    let eye = sim.feet(who) + Vec3::Y * sim.get::<EyeHeight>(who).0;
    let look = look_toward(at - eye);
    sim.set_look(who, look.yaw, look.pitch);
}

/// `shooter` switches to `weapon`, aims at `target`'s chest and fires once.
fn shoot(sim: &mut Sim, shooter: Entity, target: Entity, weapon: WeaponKind) {
    sim.intent(shooter).select = Some(ActiveTool::Weapon(weapon));
    sim.run_seconds(0.4);
    let chest = sim.feet(target) + Vec3::Y * 1.0;
    aim(sim, shooter, chest);
    sim.intent(shooter).fire_pressed = true;
    sim.tick();
}

/// Settled start: the player at [`PLAYER_FEET`], a knight at `knight_feet`.
fn setup(knight_feet: Vec3) -> (Sim, Entity, Entity) {
    let mut sim = Sim::new();
    let player = sim.player();
    place(&mut sim, player, PLAYER_FEET);
    let knight = spawn_knight(&mut sim, knight_feet);
    sim.run_seconds(0.3);
    (sim, player, knight)
}

/// Horizontal distance the knight is pushed by one `weapon` shot from the player.
fn push_from(knight_feet: Vec3, weapon: WeaponKind) -> (f32, Vec3) {
    let (mut sim, player, knight) = setup(knight_feet);
    sim.record::<DamageDealt>();
    let before = sim.feet(knight);
    shoot(&mut sim, player, knight, weapon);
    let hits = sim
        .recorded::<DamageDealt>()
        .iter()
        .filter(|d| d.target == knight)
        .count();
    assert_eq!(hits, 1, "the {weapon:?} shot hit the knight");
    sim.run_seconds(1.5);
    let after = sim.feet(knight);
    assert!(
        sim.world().get::<Knockback>(knight).is_none(),
        "the shove is spent after 1.5 s"
    );
    ((after - before).with_y(0.0).length(), after - before)
}

#[test]
fn a_point_blank_pump_blasts_a_knight_about_four_metres_away() {
    let knight_feet = PLAYER_FEET - Vec3::Z * 1.5;
    let (travel, delta) = push_from(knight_feet, WeaponKind::Pump);
    let expected = pieced::tuning::Tuning::default().grunt.pump_knockback;
    assert!(
        (travel - expected).abs() <= 0.25 * expected,
        "pushed {travel:.2} m, expected about {expected} m"
    );
    assert!(
        delta.z < -0.9 * travel,
        "pushed away from the shooter (north): {delta}"
    );
}

#[test]
fn a_far_pump_pushes_much_less() {
    let near = push_from(PLAYER_FEET - Vec3::Z * 1.5, WeaponKind::Pump).0;
    let far = push_from(PLAYER_FEET - Vec3::Z * 12.0, WeaponKind::Pump).0;
    assert!(
        far < 0.35 * near,
        "far pump pushed {far:.2} m vs {near:.2} m point blank"
    );
}

#[test]
fn the_rifle_never_pushes() {
    let (travel, _) = push_from(PLAYER_FEET - Vec3::Z * 1.5, WeaponKind::Rifle);
    assert!(travel < 0.01, "the rifle pushed {travel:.3} m");
}

#[test]
fn the_player_is_never_pushed() {
    // Another armed character pumps the player at point blank.
    let mut sim = Sim::new();
    let player = sim.player();
    place(&mut sim, player, PLAYER_FEET);
    let shooter = {
        let world = sim.world_mut();
        let mut commands = world.commands();
        let e = player::spawn_character(
            &mut commands,
            PLAYER_FEET - Vec3::Z * 1.5,
            LookAngles::default(),
            Health::default(),
            (),
        );
        world.flush();
        e
    };
    sim.run_seconds(0.3);
    sim.record::<DamageDealt>();
    let before = sim.feet(player);
    shoot(&mut sim, shooter, player, WeaponKind::Pump);
    assert!(
        sim.recorded::<DamageDealt>()
            .iter()
            .any(|d| d.target == player),
        "the pump hit the player"
    );
    sim.run_seconds(1.0);
    assert!(sim.world().get::<Knockback>(player).is_none());
    let moved = (sim.feet(player) - before).with_y(0.0).length();
    assert!(moved < 0.01, "the player was pushed {moved:.3} m");
}

#[test]
fn a_knocked_knight_never_passes_through_a_wall() {
    // The wall runs along z = 12 (the north edge of cell (6, 9)), 1.5 m behind
    // the knight.
    let knight_feet = PLAYER_FEET - Vec3::Z * 1.5;
    let (mut sim, player, knight) = setup(knight_feet);
    building::place_piece(
        sim.world_mut(),
        PieceSlot::wall(GridCell::new(6, 9, 0), Facing::North),
    )
    .expect("wall placed behind the knight");
    sim.tick();
    shoot(&mut sim, player, knight, WeaponKind::Pump);
    for _ in 0..90 {
        sim.tick();
        let z = sim.feet(knight).z;
        assert!(z > 12.0, "the knight went through the wall (z = {z:.2})");
    }
    let travel = (sim.feet(knight) - knight_feet).with_y(0.0).length();
    assert!(travel < 1.5, "stopped by the wall after {travel:.2} m");
}
