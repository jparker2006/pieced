//! Foundation: the headless simulation steps deterministically and routes all
//! control through `PlayerIntent`.

use pieced::{
    shared::{ActiveTool, GameCue, Health, LookAngles, PieceKind, PlayerIntent},
    sim::Sim,
};

#[test]
fn one_fixed_tick_per_update() {
    let mut sim = Sim::new();
    let start = sim.sim_tick();
    sim.ticks(10);
    assert_eq!(sim.sim_tick() - start, 10);
}

#[test]
fn player_spawns_with_full_health_and_rifle() {
    let mut sim = Sim::new();
    let player = sim.player();
    assert_eq!(*sim.get::<Health>(player), Health::full(100.0, 100.0));
    assert_eq!(*sim.get::<ActiveTool>(player), ActiveTool::default());
}

#[test]
fn intent_selection_switches_tool_and_edges_clear() {
    let mut sim = Sim::new();
    sim.record::<GameCue>();
    let player = sim.player();
    sim.player_intent().select = Some(ActiveTool::Build(PieceKind::Wall));
    sim.tick();
    assert_eq!(
        *sim.get::<ActiveTool>(player),
        ActiveTool::Build(PieceKind::Wall)
    );
    assert_eq!(
        sim.get::<PlayerIntent>(player).select,
        None,
        "edge consumed"
    );
    assert!(
        sim.recorded::<GameCue>()
            .iter()
            .any(|c| matches!(c, GameCue::WeaponSwitch { .. }))
    );
}

#[test]
fn look_delta_is_applied_before_the_fixed_step() {
    let mut sim = Sim::new();
    let player = sim.player();
    let before = *sim.get::<LookAngles>(player);
    sim.player_intent().look_delta = bevy::math::Vec2::new(0.25, 0.1);
    sim.tick();
    let after = *sim.get::<LookAngles>(player);
    assert!((after.pitch - before.pitch - 0.1).abs() < 1e-5);
    assert!(sim.get::<PlayerIntent>(player).look_delta == bevy::math::Vec2::ZERO);
}

#[test]
fn health_absorbs_with_shield_first() {
    let mut h = Health::default();
    let split = h.apply(130.0);
    assert_eq!((split.to_shield, split.to_hp), (100.0, 30.0));
    assert!(split.shield_broke && !split.killed);
    let split = h.apply(500.0);
    assert!(split.killed && h.hp == 0.0);
    assert_eq!(h.apply(10.0).to_hp, 0.0, "no damage after death");
}
