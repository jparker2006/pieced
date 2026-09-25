//! Combat through the simulation seam: scripted `PlayerIntent` in, shots, damage,
//! ammo and messages out. Fixed 60 Hz ticks, seeded randomness.

use avian3d::prelude::*;
use bevy::prelude::*;
use pieced::{
    building::BuildTuning,
    combat::{CombatStats, GunTuning, Loadout},
    dummy::{Downed, Dummy, look_toward},
    player::{HEAD_CENTER, HEAD_RADIUS},
    shared::{
        ActiveTool, Ads, DamageDealt, Eliminated, EyeHeight, GameCue, Health, Layer, PieceHit,
        PieceKind, ShotFired, TICK_SECONDS, WeaponKind,
    },
    sim::Sim,
};

const RIFLE: ActiveTool = ActiveTool::Weapon(WeaponKind::Rifle);
const PUMP: ActiveTool = ActiveTool::Weapon(WeaponKind::Pump);

struct Arena {
    sim: Sim,
    player: Entity,
    dummy: Entity,
}

impl Arena {
    fn new(seed: u64) -> Self {
        let mut sim = Sim::with_seed(seed);
        sim.tuning_mut().dummy.stand_still = true;
        sim.record::<ShotFired>();
        sim.record::<DamageDealt>();
        sim.record::<Eliminated>();
        sim.record::<GameCue>();
        sim.record::<PieceHit>();
        let player = sim.player();
        let dummy = sim
            .world_mut()
            .query_filtered::<Entity, With<Dummy>>()
            .single(sim.world())
            .expect("one dummy");
        Self { sim, player, dummy }
    }

    fn place(&mut self, e: Entity, feet: Vec3) {
        self.sim
            .world_mut()
            .get_mut::<Transform>(e)
            .unwrap()
            .translation = feet;
    }

    /// Moves the dummy well out of the player's line of fire.
    fn park_dummy(&mut self) {
        self.place(self.dummy, Vec3::new(-18.0, 0.0, 18.0));
    }

    fn eye(&self, e: Entity) -> Vec3 {
        self.sim.feet(e) + Vec3::Y * self.sim.get::<EyeHeight>(e).0
    }

    fn aim_at(&mut self, point: Vec3) {
        let look = look_toward(point - self.eye(self.player));
        self.sim.set_look(self.player, look.yaw, look.pitch);
    }

    fn aim_forward(&self) -> Vec3 {
        self.sim
            .get::<pieced::shared::LookAngles>(self.player)
            .forward()
    }

    fn select(&mut self, tool: ActiveTool) {
        self.sim.player_intent().select = Some(tool);
        self.sim.tick();
    }

    /// Selects `tool` and waits out the switch lock.
    fn equip(&mut self, tool: ActiveTool) {
        self.select(tool);
        self.sim.run_seconds(0.5);
    }

    fn press_fire(&mut self) {
        self.hold_fire(true);
        self.sim.tick();
        self.sim.player_intent().fire = false;
    }

    fn hold_fire(&mut self, held: bool) {
        let mut i = self.sim.player_intent();
        i.fire = held;
        i.fire_pressed = held;
    }

    fn loadout(&self) -> &Loadout {
        self.sim.get::<Loadout>(self.player)
    }

    fn health(&self, e: Entity) -> Health {
        *self.sim.get::<Health>(e)
    }

    fn player_shots(&self) -> Vec<ShotFired> {
        self.sim
            .recorded::<ShotFired>()
            .into_iter()
            .filter(|s| s.shooter == self.player)
            .collect()
    }

    fn damage_to(&self, e: Entity) -> Vec<DamageDealt> {
        self.sim
            .recorded::<DamageDealt>()
            .into_iter()
            .filter(|d| d.target == e)
            .collect()
    }

    fn cues(&self) -> Vec<GameCue> {
        self.sim.recorded::<GameCue>()
    }

    fn clear(&mut self) {
        self.sim.clear_recorded::<ShotFired>();
        self.sim.clear_recorded::<DamageDealt>();
        self.sim.clear_recorded::<GameCue>();
        self.sim.clear_recorded::<PieceHit>();
        self.sim.clear_recorded::<Eliminated>();
    }
}

fn deviation_deg(shot: &ShotFired, i: usize, aim: Vec3) -> f32 {
    let dir = (shot.traces[i].end - shot.origin).normalize();
    dir.angle_between(aim).to_degrees()
}

fn chest(feet: Vec3) -> Vec3 {
    feet + Vec3::Y * 1.1
}

// ---------------------------------------------------------------------------
// Rifle accuracy
// ---------------------------------------------------------------------------

#[test]
fn rifle_first_shot_is_accurate() {
    let base = GunTuning::rifle().base_spread_deg;
    for seed in 1..=10 {
        let mut a = Arena::new(seed);
        a.park_dummy();
        let aim = a.aim_forward();
        a.press_fire();
        let shots = a.player_shots();
        assert_eq!(shots.len(), 1, "fires on the press tick");
        let dev = deviation_deg(&shots[0], 0, aim);
        assert!(dev <= base + 1e-3, "seed {seed}: first shot off by {dev}°");
    }
}

#[test]
fn rifle_bloom_grows_while_spraying_and_recovers_after_a_pause() {
    let t = GunTuning::rifle();
    let mut a = Arena::new(3);
    a.park_dummy();
    let aim = a.aim_forward();
    assert!((a.loadout().rifle.spread_deg(&t, false) - t.base_spread_deg).abs() < 1e-6);

    a.hold_fire(true);
    a.sim.ticks(10 * 20);
    a.hold_fire(false);
    a.sim.tick();
    let shots = a.player_shots();
    assert!(shots.len() >= 20);
    let mut worst_late = 0.0_f32;
    for (k, shot) in shots.iter().enumerate() {
        let allowed = t.base_spread_deg + (k as f32 * t.bloom_per_shot_deg).min(t.bloom_max_deg);
        let dev = deviation_deg(shot, 0, aim);
        assert!(dev <= allowed + 1e-3, "shot {k}: {dev}° > {allowed}°");
        if k >= 8 {
            worst_late = worst_late.max(dev);
        }
    }
    assert!(
        worst_late > t.base_spread_deg + 0.5,
        "sustained fire should spread well past first-shot accuracy ({worst_late}°)"
    );
    let bloomed = a.loadout().rifle.spread_deg(&t, false);
    assert!((bloomed - (t.base_spread_deg + t.bloom_max_deg)).abs() < 1e-4);

    // Still bloomed just before the recovery delay (the last shot was 11 ticks ago)...
    a.sim.run_seconds(0.1);
    assert!(a.loadout().rifle.spread_deg(&t, false) > bloomed - 1e-4);
    // ...and fully recovered soon after it.
    let recover = t.bloom_recover_delay + t.bloom_max_deg / t.bloom_recover_deg_per_sec;
    a.sim.run_seconds(recover);
    assert!((a.loadout().rifle.spread_deg(&t, false) - t.base_spread_deg).abs() < 1e-6);

    a.clear();
    a.press_fire();
    let dev = deviation_deg(&a.player_shots()[0], 0, aim);
    assert!(
        dev <= t.base_spread_deg + 1e-3,
        "recovered shot off by {dev}°"
    );
}

#[test]
fn ads_tightens_rifle_spread() {
    let t = GunTuning::rifle();
    let mut a = Arena::new(4);
    a.park_dummy();
    a.hold_fire(true);
    a.sim.ticks(120);
    a.hold_fire(false);
    let hip = a.loadout().rifle.spread_deg(&t, false);
    let ads = a.loadout().rifle.spread_deg(&t, true);
    assert!((ads - hip * t.ads_spread_multiplier).abs() < 1e-5);
    assert!(ads < hip * 0.5, "ADS cuts spread by about 60%");
}

// ---------------------------------------------------------------------------
// Pump
// ---------------------------------------------------------------------------

#[test]
fn pump_pattern_is_identical_every_shot() {
    let t = GunTuning::pump();
    let mut a = Arena::new(5);
    a.park_dummy();
    a.equip(PUMP);
    let aim = a.aim_forward();
    for _ in 0..3 {
        a.press_fire();
        a.sim.run_seconds(1.0);
    }
    let shots = a.player_shots();
    assert_eq!(shots.len(), 3);
    for shot in &shots {
        assert_eq!(shot.weapon, WeaponKind::Pump);
        assert_eq!(shot.traces.len(), t.pellets as usize);
        for i in 0..shot.traces.len() {
            let dev = deviation_deg(shot, i, aim);
            assert!(dev <= t.pellet_spread_deg + 1e-3, "pellet {i} at {dev}°");
        }
    }
    for shot in &shots[1..] {
        for (p, q) in shot.traces.iter().zip(&shots[0].traces) {
            assert!(
                p.end.distance(q.end) < 1e-4,
                "pattern changed between shots"
            );
        }
    }
    // Same pattern for other seeds too: it's fixed, not random.
    let mut b = Arena::new(99);
    b.park_dummy();
    b.equip(PUMP);
    b.press_fire();
    for (p, q) in b.player_shots()[0].traces.iter().zip(&shots[0].traces) {
        assert!(p.end.distance(q.end) < 1e-4);
    }
}

#[test]
fn pump_falloff_numbers_match_the_spec() {
    let t = GunTuning::pump();
    assert_eq!(t.falloff(8.0), 1.0);
    assert!((t.falloff(15.0) - 0.3).abs() < 1e-6);
    assert!((t.falloff(11.5) - 0.65).abs() < 1e-5);
    assert!((t.falloff(30.0) - 0.3).abs() < 1e-6);
    let r = GunTuning::rifle();
    assert_eq!(r.damage_at(15.0, false), 28.0);
    assert!((r.damage_at(50.0, false) - 28.0 * 0.7).abs() < 1e-4);
}

/// Fires one pump shot at the dummy's lower chest from `range` meters and returns
/// (damage dealt, expected from the traces, per-body-pellet damages).
fn pump_shot_at(range: f32) -> (f32, f32, Vec<f32>) {
    let t = GunTuning::pump();
    let mut a = Arena::new(6);
    a.equip(PUMP);
    let player_feet = a.sim.feet(a.player);
    let dummy_feet = player_feet + Vec3::NEG_Z * range;
    a.place(a.dummy, dummy_feet);
    a.aim_at(dummy_feet + Vec3::Y * 0.75);
    a.press_fire();
    let shot = &a.player_shots()[0];
    let head = dummy_feet + Vec3::Y * HEAD_CENTER;
    let mut expected = 0.0;
    let mut body = Vec::new();
    for tr in shot.traces.iter().filter(|tr| tr.hit == Some(a.dummy)) {
        let dist = tr.end.distance(shot.origin);
        let is_head = tr.end.distance(head) <= HEAD_RADIUS + 1e-3;
        let dmg = t.damage_at(dist, is_head);
        expected += dmg;
        if !is_head {
            body.push(dmg);
        }
    }
    let dealt: f32 = a.damage_to(a.dummy).iter().map(|d| d.amount).sum();
    (dealt, expected, body)
}

#[test]
fn pump_does_full_damage_at_8m_and_falls_off_by_15m() {
    let (dealt, expected, body) = pump_shot_at(8.0);
    assert!(body.len() >= 4, "most of the pattern lands at 8 m");
    assert!(body.iter().all(|d| (*d - 10.0).abs() < 1e-4), "{body:?}");
    assert!((dealt - expected).abs() < 1e-3, "{dealt} vs {expected}");

    let (dealt, expected, body) = pump_shot_at(15.0);
    assert!(!body.is_empty());
    assert!(
        body.iter().all(|d| (3.0..3.5).contains(d)),
        "about 30% per pellet at 15 m: {body:?}"
    );
    assert!((dealt - expected).abs() < 1e-3, "{dealt} vs {expected}");
}

#[test]
fn one_point_blank_pump_shot_never_kills_from_full() {
    for range in [1.0, 1.5, 2.0, 3.0, 5.0] {
        for aim_height in [0.9, 1.2, 1.62] {
            let mut a = Arena::new(7);
            a.equip(PUMP);
            let dummy_feet = a.sim.feet(a.player) + Vec3::NEG_Z * range;
            a.place(a.dummy, dummy_feet);
            a.aim_at(dummy_feet + Vec3::Y * aim_height);
            a.press_fire();
            let dealt: f32 = a.damage_to(a.dummy).iter().map(|d| d.amount).sum();
            assert!(dealt > 0.0);
            assert!(!a.health(a.dummy).is_dead(), "killed at {range} m");
            assert!(a.sim.recorded::<Eliminated>().is_empty());
            if aim_height < 1.3 {
                assert!(dealt <= 100.0 + 1e-3, "body shot did {dealt}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Damage rules
// ---------------------------------------------------------------------------

#[test]
fn headshots_do_one_and_a_half_times_damage() {
    let mut a = Arena::new(8);
    let dummy_feet = Vec3::new(2.0, 0.0, 4.0);
    a.place(a.dummy, dummy_feet);
    a.aim_at(dummy_feet + Vec3::Y * HEAD_CENTER);
    a.press_fire();
    let hits = a.damage_to(a.dummy);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].headshot);
    assert!((hits[0].amount - 42.0).abs() < 1e-4, "{}", hits[0].amount);

    a.clear();
    a.sim.run_seconds(1.0);
    a.aim_at(chest(dummy_feet));
    a.press_fire();
    let hits = a.damage_to(a.dummy);
    assert_eq!(hits.len(), 1);
    assert!(!hits[0].headshot);
    assert!((hits[0].amount - 28.0).abs() < 1e-4);
}

#[test]
fn shield_absorbs_damage_before_health() {
    let mut a = Arena::new(9);
    let dummy_feet = Vec3::new(2.0, 0.0, 4.0);
    a.place(a.dummy, dummy_feet);
    a.aim_at(chest(dummy_feet));
    let mut seen = Vec::new();
    for _ in 0..5 {
        a.press_fire();
        seen.push(a.health(a.dummy));
        a.sim.run_seconds(1.0);
    }
    let shields: Vec<f32> = seen.iter().map(|h| h.shield).collect();
    let hps: Vec<f32> = seen.iter().map(|h| h.hp).collect();
    assert_eq!(shields, vec![72.0, 44.0, 16.0, 0.0, 0.0]);
    assert_eq!(hps, vec![100.0, 100.0, 100.0, 88.0, 60.0]);
    let hits = a.damage_to(a.dummy);
    assert_eq!(hits[0].to_shield, 28.0);
    assert!(hits[3].shield_broke && hits[3].to_shield == 16.0);
    assert!(hits.iter().all(|h| !h.killed));
}

// ---------------------------------------------------------------------------
// Reloads, switching, buffering and build mode
// ---------------------------------------------------------------------------

#[test]
fn rifle_reloads_a_full_magazine_in_two_seconds() {
    let mut a = Arena::new(10);
    a.park_dummy();
    a.hold_fire(true);
    a.sim.ticks(41); // shots at 0, 10, 20, 30, 40
    a.hold_fire(false);
    assert_eq!(a.loadout().rifle.ammo, 25);
    a.sim.run_seconds(0.5);
    a.clear();

    a.sim.player_intent().reload_pressed = true;
    a.sim.tick();
    assert!(a.loadout().rifle.is_reloading());
    assert!(a.cues().contains(&GameCue::ReloadStart {
        who: a.player,
        weapon: WeaponKind::Rifle
    }));
    // The trigger does nothing mid-reload.
    a.hold_fire(true);
    a.sim.ticks(118);
    assert_eq!(a.loadout().rifle.ammo, 25);
    assert!(a.player_shots().is_empty(), "no firing while reloading");
    a.hold_fire(false);
    a.sim.tick(); // 119 ticks after the press
    assert_eq!(a.loadout().rifle.ammo, 25);
    a.sim.tick(); // 2.0 s
    assert_eq!(a.loadout().rifle.ammo, 30);
    assert!(!a.loadout().rifle.is_reloading());
    assert!(a.cues().contains(&GameCue::ReloadDone {
        who: a.player,
        weapon: WeaponKind::Rifle
    }));
}

#[test]
fn rifle_auto_reloads_on_empty() {
    let mut a = Arena::new(11);
    a.park_dummy();
    a.hold_fire(true);
    a.sim.ticks(10 * 30 + 130);
    let shots = a.player_shots();
    assert!(shots.len() >= 31, "fire resumes after the reload");
    let gap = shots[30].tick - shots[29].tick;
    assert_eq!(gap, 120, "empty magazine reloads for exactly 2.0 s");
    assert!(a.cues().iter().any(|c| matches!(
        c,
        GameCue::ReloadStart {
            weapon: WeaponKind::Rifle,
            ..
        }
    )));
}

#[test]
fn pump_reloads_shell_by_shell() {
    let mut a = Arena::new(12);
    a.park_dummy();
    a.equip(PUMP);
    for _ in 0..3 {
        a.press_fire();
        a.sim.run_seconds(1.0);
    }
    assert_eq!(a.loadout().pump.ammo, 2);
    a.clear();
    a.sim.player_intent().reload_pressed = true;
    a.sim.tick();
    let mut ammo = Vec::new();
    for _ in 0..100 {
        a.sim.tick();
        ammo.push(a.loadout().pump.ammo);
    }
    // One shell every 0.5 s (30 ticks) after the press.
    assert_eq!(ammo[28], 2);
    assert_eq!(ammo[29], 3);
    assert_eq!(ammo[58], 3);
    assert_eq!(ammo[59], 4);
    assert_eq!(ammo[89], 5);
    assert!(!a.loadout().pump.is_reloading());
    let shells = a
        .cues()
        .iter()
        .filter(|c| matches!(c, GameCue::ReloadShell { .. }))
        .count();
    assert_eq!(shells, 3);
    assert!(a.cues().contains(&GameCue::ReloadDone {
        who: a.player,
        weapon: WeaponKind::Pump
    }));
}

#[test]
fn firing_interrupts_the_pump_reload_once_a_shell_is_in() {
    let mut a = Arena::new(13);
    a.park_dummy();
    a.equip(PUMP);
    for _ in 0..3 {
        a.press_fire();
        a.sim.run_seconds(1.0);
    }
    a.sim.player_intent().reload_pressed = true;
    a.sim.tick();
    a.sim.ticks(35);
    assert_eq!(a.loadout().pump.ammo, 3);
    assert!(a.loadout().pump.is_reloading());
    a.clear();
    a.press_fire();
    assert_eq!(a.player_shots().len(), 1, "fire interrupts the reload");
    assert_eq!(a.loadout().pump.ammo, 2);
    assert!(!a.loadout().pump.is_reloading());
    a.sim.run_seconds(2.0);
    assert_eq!(a.loadout().pump.ammo, 2, "the reload stays cancelled");
}

#[test]
fn empty_pump_auto_reloads_and_a_press_with_no_shell_does_not_fire() {
    let mut a = Arena::new(14);
    a.park_dummy();
    a.equip(PUMP);
    for _ in 0..5 {
        a.press_fire();
        a.sim.run_seconds(1.0);
    }
    // Auto-reload started on the last shot; wait for it to finish.
    a.sim.run_seconds(2.0);
    assert_eq!(a.loadout().pump.ammo, 5);
    assert_eq!(a.player_shots().len(), 5);

    // Empty it again, then press before the first shell lands.
    for _ in 0..4 {
        a.press_fire();
        a.sim.run_seconds(1.0);
    }
    a.clear();
    a.press_fire(); // last shell; the reload starts on this tick
    assert_eq!(a.loadout().pump.ammo, 0);
    assert!(a.loadout().pump.is_reloading());
    a.sim.ticks(4);
    a.press_fire(); // 5 ticks in: nothing loaded, and the buffer expires first
    a.sim.ticks(40);
    assert_eq!(a.player_shots().len(), 1);
    assert!(a.loadout().pump.is_reloading(), "reload not interrupted");
    assert_eq!(a.loadout().pump.ammo, 1);
}

#[test]
fn weapon_switch_locks_firing_for_0_2_seconds() {
    // Pump → rifle with the trigger held: the first shot lands exactly 12 ticks later.
    let mut a = Arena::new(15);
    a.park_dummy();
    a.equip(PUMP);
    a.clear();
    a.sim.player_intent().select = Some(RIFLE);
    a.hold_fire(true);
    a.sim.tick();
    let switched = a.sim.sim_tick();
    a.sim.ticks(20);
    let first = a.player_shots()[0].tick;
    assert_eq!(first - switched, 12, "0.2 s switch lock");
    assert!(
        a.cues()
            .iter()
            .any(|c| matches!(c, GameCue::WeaponSwitch { .. }))
    );

    // Rifle → pump, pressing inside the lock: fires the moment the lock ends.
    a.hold_fire(false);
    a.sim.run_seconds(1.0);
    a.clear();
    a.select(PUMP);
    let switched = a.sim.sim_tick();
    a.sim.ticks(2);
    a.press_fire(); // 3 ticks after the switch, 9 ticks (150 ms) before it's ready
    a.sim.ticks(20);
    let shots = a.player_shots();
    assert_eq!(shots.len(), 1);
    assert_eq!(shots[0].weapon, WeaponKind::Pump);
    assert_eq!(shots[0].tick - switched, 12);
}

#[test]
fn pump_buffers_a_press_up_to_150ms_early() {
    let mut a = Arena::new(16);
    a.park_dummy();
    a.equip(PUMP);
    a.clear();
    a.press_fire();
    let first = a.player_shots()[0].tick;
    // 9 ticks (150 ms) before the pump is ready: fires on the ready tick.
    a.sim.ticks(44);
    a.press_fire();
    a.sim.ticks(20);
    let shots = a.player_shots();
    assert_eq!(shots.len(), 2);
    assert_eq!(
        shots[1].tick - first,
        54,
        "fires the moment the 0.9 s cooldown ends"
    );

    // 10 ticks (167 ms) early: the press is dropped.
    let second = shots[1].tick;
    let now = a.sim.sim_tick();
    a.sim.ticks((second + 44 - now) as u32 - 1);
    a.press_fire();
    a.sim.ticks(30);
    assert_eq!(a.player_shots().len(), 2, "too-early press is not buffered");
}

#[test]
fn guns_do_not_fire_in_build_mode() {
    let mut a = Arena::new(17);
    a.park_dummy();
    a.select(ActiveTool::Build(PieceKind::Wall));
    a.sim.run_seconds(0.5);
    a.clear();
    for _ in 0..60 {
        a.hold_fire(true);
        a.sim.tick();
    }
    assert!(a.player_shots().is_empty());
    assert_eq!(a.loadout().rifle.ammo, 30);
}

#[test]
fn ads_toggles_and_turns_off_for_build_sprint_and_rifle_reload() {
    let mut a = Arena::new(18);
    a.park_dummy();
    let ads = |a: &Arena| a.sim.get::<Ads>(a.player).0;
    let toggle = |a: &mut Arena| {
        a.sim.player_intent().ads_toggle_pressed = true;
        a.sim.tick();
    };

    toggle(&mut a);
    assert!(ads(&a));
    assert!(a.cues().contains(&GameCue::AdsChanged {
        who: a.player,
        ads: true
    }));
    toggle(&mut a);
    assert!(!ads(&a));

    // Entering build mode drops ADS, and it can't be turned on while building.
    toggle(&mut a);
    assert!(ads(&a));
    a.select(ActiveTool::Build(PieceKind::Ramp));
    assert!(!ads(&a));
    toggle(&mut a);
    assert!(!ads(&a));

    // Sprinting drops it.
    a.equip(RIFLE);
    toggle(&mut a);
    assert!(ads(&a));
    {
        let mut i = a.sim.player_intent();
        i.sprint = true;
        i.move_axis = Vec2::Y;
    }
    a.sim.tick();
    assert!(!ads(&a));
    {
        let mut i = a.sim.player_intent();
        i.sprint = false;
        i.move_axis = Vec2::ZERO;
    }

    // Reloading the rifle drops it.
    a.press_fire();
    toggle(&mut a);
    assert!(ads(&a));
    a.sim.player_intent().reload_pressed = true;
    a.sim.tick();
    assert!(!ads(&a));
    toggle(&mut a);
    assert!(!ads(&a), "can't aim while the rifle reloads");
    a.sim.run_seconds(2.0);
    toggle(&mut a);
    assert!(ads(&a), "ADS works again after the reload");
}

// ---------------------------------------------------------------------------
// THE TTK CONTRACT
// ---------------------------------------------------------------------------

/// Continuous rifle fire at the chest of a standing dummy 15 m away, from full 200.
/// Returns (time to kill in seconds, shots fired).
fn rifle_ttk_at_15m(seed: u64) -> (f32, usize) {
    let mut a = Arena::new(seed);
    let player_feet = a.sim.feet(a.player);
    let dummy_feet = player_feet + Vec3::NEG_Z * 15.0;
    a.place(a.dummy, dummy_feet);
    a.aim_at(chest(dummy_feet));
    assert_eq!(a.health(a.dummy).total(), 200.0);
    a.hold_fire(true);
    let start = a.sim.sim_tick() + 1;
    for _ in 0..240 {
        a.sim.tick();
        if !a.sim.recorded::<Eliminated>().is_empty() {
            break;
        }
    }
    let kill = a.sim.recorded::<Eliminated>();
    assert_eq!(kill.len(), 1, "seed {seed}: dummy not killed in 4 s");
    assert_eq!(kill[0].victim, a.dummy);
    let ttk = (kill[0].tick - start) as f32 * TICK_SECONDS;
    // The HUD readout measures first damage to elimination.
    let first_damage = a.damage_to(a.dummy)[0].tick;
    let readout = a.sim.world().resource::<CombatStats>().last_ttk.unwrap();
    assert!((readout - (kill[0].tick - first_damage) as f32 * TICK_SECONDS).abs() < 1e-4);
    (ttk, a.player_shots().len())
}

#[test]
fn ttk_contract_rifle_kills_standing_dummy_at_15m_in_1_to_2_seconds() {
    let mut results = Vec::new();
    for seed in 1..=12 {
        let (ttk, shots) = rifle_ttk_at_15m(seed);
        results.push((seed, ttk, shots));
    }
    println!("seed, ttk s, shots: {results:?}");
    for (seed, ttk, _) in &results {
        assert!(
            (1.0..=2.0).contains(ttk),
            "seed {seed}: TTK {ttk:.3} s outside 1.0–2.0 s"
        );
    }
}

#[test]
fn ttk_contract_a_wall_soaks_at_least_one_second_of_rifle_fire() {
    let rifle = GunTuning::rifle();
    let wall_hp = BuildTuning::default().wall_hp;
    let shots = rifle.shots_to_break(wall_hp);
    assert!(
        shots as f32 * rifle.fire_interval >= 1.0,
        "{shots} shots × {} s",
        rifle.fire_interval
    );

    // And in the simulation: a piece between the player and the dummy takes the
    // bullets (structure damage per hit), the dummy takes none.
    let mut a = Arena::new(19);
    let wall = a
        .sim
        .world_mut()
        .spawn((
            RigidBody::Static,
            Collider::cuboid(4.0, 3.0, 0.2),
            CollisionLayers::new(Layer::Piece, LayerMask::ALL),
            Transform::from_xyz(2.0, 1.5, 4.0),
        ))
        .id();
    a.sim.tick(); // the physics step adds it to the query tree
    let dummy_feet = a.sim.feet(a.dummy);
    a.aim_at(chest(dummy_feet));
    a.hold_fire(true);
    a.sim.run_seconds(2.0);
    let hits = a.sim.recorded::<PieceHit>();
    assert!(hits.len() >= 8);
    assert!(
        hits.iter()
            .all(|h| h.piece == wall && h.amount == rifle.structure_damage)
    );
    assert!(
        a.damage_to(a.dummy).is_empty(),
        "the first hit stops the bullet"
    );
    let shots = a.player_shots();
    let mut total = 0.0;
    let breaking = shots
        .iter()
        .find(|_| {
            total += rifle.structure_damage;
            total >= wall_hp
        })
        .unwrap();
    let soak = (breaking.tick - shots[0].tick) as f32 * TICK_SECONDS;
    assert!(soak >= 1.0, "wall broke after {soak} s of fire");
    assert!(
        shots[0].traces[0].hit == Some(wall),
        "trace reports the piece"
    );
}

#[test]
fn pump_pellets_damage_pieces_by_structure_damage() {
    let mut a = Arena::new(20);
    let wall = a
        .sim
        .world_mut()
        .spawn((
            RigidBody::Static,
            Collider::cuboid(4.0, 3.0, 0.2),
            CollisionLayers::new(Layer::Piece, LayerMask::ALL),
            Transform::from_xyz(2.0, 1.5, 11.0),
        ))
        .id();
    a.equip(PUMP);
    a.aim_at(Vec3::new(2.0, 1.5, 11.0));
    a.press_fire();
    let hits = a.sim.recorded::<PieceHit>();
    assert_eq!(hits.len(), 1, "one aggregated hit per piece per shot");
    assert_eq!(hits[0].piece, wall);
    assert!((hits[0].amount - 100.0).abs() < 1e-4, "{}", hits[0].amount);
}

// ---------------------------------------------------------------------------
// Hit registration
// ---------------------------------------------------------------------------

#[test]
fn hits_register_on_a_dummy_that_moves_every_tick() {
    // avian syncs hitbox positions after the fixed update, so a naive ray query
    // would test last tick's hitboxes. Teleport the dummy further than its own
    // width every tick and aim at where it is *now*: every shot must land.
    let mut a = Arena::new(21);
    {
        let mut t = a.sim.tuning_mut();
        t.combat.rifle.base_spread_deg = 0.0;
        t.combat.rifle.bloom_per_shot_deg = 0.0;
    }
    a.hold_fire(true);
    let mut x = -8.0;
    for _ in 0..70 {
        x += 0.9;
        if x > 10.0 {
            x = -8.0;
        }
        let feet = Vec3::new(x, 0.0, 0.0);
        a.place(a.dummy, feet);
        a.aim_at(chest(feet));
        a.sim.tick();
    }
    let shots = a.player_shots();
    assert_eq!(shots.len(), 7);
    assert!(shots.iter().all(|s| s.traces[0].hit == Some(a.dummy)));
    assert_eq!(a.damage_to(a.dummy).len(), 7);

    // And a dummy strafing at run speed, tracked with the default rifle.
    let mut b = Arena::new(22);
    b.hold_fire(true);
    let speed = 5.5 / 60.0;
    let mut feet = Vec3::new(0.0, 0.0, 6.0);
    for _ in 0..45 {
        feet.x += speed;
        b.place(b.dummy, feet);
        b.aim_at(chest(feet));
        b.sim.tick();
    }
    let hits = b.damage_to(b.dummy).len();
    assert_eq!(hits, b.player_shots().len(), "every tracked shot lands");
}

#[test]
fn dead_center_shots_always_hit() {
    // One exact shot per tick at the dummy's chest or head as it steps across the
    // arena. (parry's GJK capsule cast missed some of these; see combat.rs.)
    let mut a = Arena::new(25);
    {
        let mut t = a.sim.tuning_mut();
        t.combat.rifle.base_spread_deg = 0.0;
        t.combat.rifle.bloom_per_shot_deg = 0.0;
        t.combat.rifle.fire_interval = TICK_SECONDS;
        t.combat.rifle.magazine = 10_000;
    }
    // Re-create the loadout so the bigger magazine applies.
    let tuning = a
        .sim
        .world()
        .resource::<pieced::tuning::Tuning>()
        .combat
        .clone();
    let player = a.player;
    a.sim
        .world_mut()
        .get_mut::<Loadout>(player)
        .unwrap()
        .rifle
        .ammo = tuning.rifle.magazine;
    a.hold_fire(true);
    let mut shots = 0;
    for i in 0..600 {
        let x = -8.0 + i as f32 * 0.03;
        let h = [0.6, 0.8, 1.0, 1.1, 1.2, 1.62][i % 6];
        let feet = Vec3::new(x, 0.0, 0.0);
        a.place(a.dummy, feet);
        *a.sim.world_mut().get_mut::<Health>(a.dummy).unwrap() = Health::default();
        a.aim_at(Vec3::new(x, h, 0.0));
        a.sim.tick();
        shots += 1;
    }
    let fired = a.player_shots();
    assert_eq!(fired.len(), shots);
    let missed: Vec<_> = fired
        .iter()
        .filter(|s| s.traces[0].hit != Some(a.dummy))
        .map(|s| s.tick)
        .collect();
    assert!(
        missed.is_empty(),
        "missed dead-center shots at ticks {missed:?}"
    );
}

#[test]
fn combat_stats_track_shots_hits_and_headshots() {
    let mut a = Arena::new(23);
    let dummy_feet = Vec3::new(2.0, 0.0, 4.0);
    a.place(a.dummy, dummy_feet);
    a.aim_at(dummy_feet + Vec3::Y * HEAD_CENTER);
    a.press_fire();
    a.sim.run_seconds(1.0);
    a.aim_at(chest(dummy_feet));
    a.press_fire();
    a.sim.run_seconds(1.0);
    a.aim_at(Vec3::new(20.0, 1.0, 4.0));
    a.press_fire();
    let stats = a.sim.world().resource::<CombatStats>().clone();
    assert_eq!((stats.shots, stats.hits, stats.headshots), (3, 2, 1));
    assert!((stats.accuracy() - 2.0 / 3.0).abs() < 1e-6);
    assert!((stats.headshot_rate() - 0.5).abs() < 1e-6);
    assert!(stats.last_ttk.is_none());
}

#[test]
fn eliminated_characters_are_downed_and_reported() {
    let mut a = Arena::new(24);
    let dummy_feet = Vec3::new(2.0, 0.0, 4.0);
    a.place(a.dummy, dummy_feet);
    a.sim
        .world_mut()
        .get_mut::<Health>(a.dummy)
        .unwrap()
        .apply(190.0);
    a.aim_at(chest(dummy_feet));
    a.press_fire();
    let kills = a.sim.recorded::<Eliminated>();
    assert_eq!(kills.len(), 1);
    assert_eq!(kills[0].by, Some(a.player));
    assert!(a.damage_to(a.dummy)[0].killed);
    assert!(a.sim.world().get::<Downed>(a.dummy).is_some());
    assert_eq!(a.sim.world().resource::<CombatStats>().eliminations, 1);
}
