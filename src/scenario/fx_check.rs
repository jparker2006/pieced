//! Slice E — `fx_check`: a scripted tour of the viewmodel and effects, captured
//! mid-action: rifle fire (tracers + muzzle flash), rifle ADS, the rifle reload,
//! a pump blast at the dummy (sparks, hit pops, shield shimmer and break), the
//! pump rack and shell reload, wood chips and a piece break, an elimination
//! burst, and build mode with the gun lowered.

use super::{
    Director, DirectorStatus, ScenarioClock, capture, player_entity, set_look, teleport,
    with_intent,
};
use crate::{
    building::{PieceSlot, damage_piece, place_piece},
    combat::{Downed, Loadout},
    dummy::Dummy,
    shared::{
        ActiveTool, EyeHeight, Facing, GridCell, Health, PieceKind, PreviousFeet, WeaponKind,
    },
    tuning::Tuning,
};
use bevy::{platform::collections::HashSet, prelude::*};
use serde_json::{Value, json};

pub fn director(name: &str) -> Option<Box<dyn Director>> {
    (name == "fx_check").then(|| Box::new(FxCheck::default()) as Box<dyn Director>)
}

/// Where the dummy stands for the gun tests.
const DUMMY_SPOT: Vec3 = Vec3::new(2.0, 0.0, -6.0);
/// The wall that gets shot and broken, and where the player watches from.
const WALL_CELL: GridCell = GridCell::new(4, 7, 0);
const WALL_VIEW: Vec3 = Vec3::new(-6.0, 0.0, 8.8);

#[derive(Default)]
struct FxCheck {
    done: HashSet<&'static str>,
    /// Captures scheduled for a later frame.
    pending: Vec<(u64, String)>,
    wall: Option<Entity>,
    kill_frame: Option<u64>,
    shots: Vec<Value>,
}

impl FxCheck {
    fn once(&mut self, key: &'static str) -> bool {
        self.done.insert(key)
    }

    fn snap(&mut self, world: &mut World, clock: &ScenarioClock, name: &str) {
        capture(world, name, false);
        self.shots
            .push(json!({ "name": name, "t": clock.seconds, "frame": clock.frame }));
    }

    fn snap_later(&mut self, frame: u64, name: &str) {
        self.pending.push((frame, name.to_string()));
    }
}

fn dummy_entity(world: &mut World) -> Option<Entity> {
    world
        .query_filtered::<Entity, With<Dummy>>()
        .iter(world)
        .next()
}

/// Puts the dummy at `feet` with the given health, standing still.
fn place_dummy(world: &mut World, feet: Vec3, hp: f32, shield: f32) {
    world.resource_mut::<Tuning>().dummy.stand_still = true;
    let Some(dummy) = dummy_entity(world) else {
        return;
    };
    world.entity_mut(dummy).remove::<Downed>();
    if let Some(mut t) = world.get_mut::<Transform>(dummy) {
        t.translation = feet;
    }
    if let Some(mut p) = world.get_mut::<PreviousFeet>(dummy) {
        p.0 = feet;
    }
    if let Some(mut h) = world.get_mut::<Health>(dummy) {
        h.hp = hp.min(h.max_hp);
        h.shield = shield.min(h.max_shield);
    }
}

/// Aims the player's view at a world point.
fn aim_at(world: &mut World, target: Vec3) {
    let Some(player) = player_entity(world) else {
        return;
    };
    let feet = world
        .get::<Transform>(player)
        .map(|t| t.translation)
        .unwrap_or_default();
    let eye = feet + Vec3::Y * world.get::<EyeHeight>(player).map_or(1.62, |e| e.0);
    let d = target - eye;
    let yaw = (-d.x).atan2(-d.z);
    let pitch = d.y.atan2(Vec2::new(d.x, d.z).length());
    set_look(world, yaw, pitch);
}

/// True when the held rifle will fire on this frame's fixed tick.
fn rifle_fires_now(world: &mut World) -> bool {
    let Some(player) = player_entity(world) else {
        return false;
    };
    world.get::<Loadout>(player).is_some_and(|l| {
        !l.is_switching()
            && l.rifle.ammo > 0
            && !l.rifle.is_reloading()
            && l.rifle.cooldown - 1.0 / 60.0 <= 1e-4
    })
}

fn select(world: &mut World, tool: ActiveTool) {
    with_intent(world, |i| i.select = Some(tool));
}

impl Director for FxCheck {
    fn warmup_seconds(&self) -> f64 {
        1.0
    }

    fn update(&mut self, world: &mut World, clock: &ScenarioClock) -> DirectorStatus {
        let t = clock.seconds;
        let frame = clock.frame;
        let due: Vec<String> = self
            .pending
            .iter()
            .filter(|(f, _)| *f == frame)
            .map(|(_, n)| n.clone())
            .collect();
        for name in due {
            self.snap(world, clock, &name);
        }
        let chest = DUMMY_SPOT + Vec3::Y * 1.05;

        // Rifle from 9 m.
        if t < 5.8 {
            if self.once("setup") {
                place_dummy(world, DUMMY_SPOT, 100.0, 100.0);
                teleport(world, Vec3::new(2.0, 0.0, 3.0));
            }
            aim_at(world, chest);
        }
        if t >= 1.2 && self.once("hip") {
            self.snap(world, clock, "01_hip_idle");
        }
        if (1.5..2.4).contains(&t) {
            with_intent(world, |i| i.fire = true);
            if t >= 1.9 && !self.done.contains("rifle_fire") && rifle_fires_now(world) {
                self.once("rifle_fire");
                self.snap(world, clock, "02_rifle_fire");
                self.snap_later(frame + 2, "02b_rifle_fire_after");
            }
        }
        if t >= 2.4 && self.once("release1") {
            with_intent(world, |i| i.fire = false);
        }
        if t >= 2.6 && self.once("ads_on") {
            place_dummy(world, DUMMY_SPOT, 100.0, 100.0);
            with_intent(world, |i| i.ads_toggle_pressed = true);
        }
        if t >= 2.95 && self.once("ads_still") {
            self.snap(world, clock, "03_rifle_ads");
        }
        if (3.05..3.45).contains(&t) {
            with_intent(world, |i| i.fire = true);
            if t >= 3.15 && !self.done.contains("ads_fire") && rifle_fires_now(world) {
                self.once("ads_fire");
                self.snap(world, clock, "03b_rifle_ads_fire");
            }
        }
        if t >= 3.45 && self.once("release2") {
            with_intent(world, |i| {
                i.fire = false;
                i.ads_toggle_pressed = true;
            });
        }
        if t >= 3.7 && self.once("reload") {
            with_intent(world, |i| i.reload_pressed = true);
        }
        if t >= 4.15 && self.once("reload_a") {
            self.snap(world, clock, "04_rifle_reload_drop");
        }
        if t >= 5.0 && self.once("reload_b") {
            self.snap(world, clock, "04b_rifle_reload_insert");
        }

        // Pump from 4.5 m.
        if t >= 5.8 && self.once("pump_setup") {
            place_dummy(world, DUMMY_SPOT, 100.0, 100.0);
            teleport(world, Vec3::new(2.4, 0.0, -1.5));
            select(world, ActiveTool::Weapon(WeaponKind::Pump));
        }
        if t >= 5.87 && self.once("switch_mid") {
            self.snap(world, clock, "05_switch_mid");
        }
        if (5.8..8.3).contains(&t) {
            aim_at(world, chest + Vec3::new(0.25, -0.1, 0.0));
        }
        if t >= 6.5 && self.once("pump_fire") {
            with_intent(world, |i| i.fire_pressed = true);
            self.snap(world, clock, "06_pump_blast");
            self.snap_later(frame + 3, "06b_pump_blast_after");
            self.snap_later(frame + 18, "07_pump_rack");
        }
        if t >= 7.6 && self.once("pump_reload") {
            with_intent(world, |i| i.reload_pressed = true);
        }
        if t >= 7.95 && self.once("pump_reload_snap") {
            self.snap(world, clock, "08_pump_reload");
        }

        // Wood chips and a piece break.
        if t >= 8.3 && self.once("wall_setup") {
            select(world, ActiveTool::Weapon(WeaponKind::Rifle));
            teleport(world, WALL_VIEW);
            self.wall = place_piece(world, PieceSlot::wall(WALL_CELL, Facing::North)).ok();
        }
        let wall_center = PieceSlot::wall(WALL_CELL, Facing::North).center();
        if (8.3..11.0).contains(&t) {
            aim_at(world, wall_center + Vec3::new(0.3, -0.2, 0.0));
        }
        if (8.9..9.5).contains(&t) {
            with_intent(world, |i| i.fire = true);
            if t >= 9.2 && !self.done.contains("chips") && rifle_fires_now(world) {
                self.once("chips");
                self.snap(world, clock, "09_piece_hits");
            }
        }
        if t >= 9.5 && self.once("release3") {
            with_intent(world, |i| i.fire = false);
        }
        if t >= 9.8 && self.once("break") {
            if let Some(wall) = self.wall {
                damage_piece(world, wall, 10_000.0);
            }
            self.snap_later(frame + 8, "10_piece_break_a");
            self.snap_later(frame + 22, "10b_piece_break_b");
            self.snap_later(frame + 50, "10c_piece_break_c");
        }

        // Elimination from 7 m.
        if t >= 11.0 && self.once("elim_setup") {
            place_dummy(world, DUMMY_SPOT, 20.0, 0.0);
            teleport(world, Vec3::new(2.0, 0.0, 1.0));
        }
        if (11.0..12.8).contains(&t) {
            if self.kill_frame.is_none() {
                aim_at(world, chest);
            }
            let downed = dummy_entity(world).is_some_and(|d| world.get::<Downed>(d).is_some());
            if t >= 11.4 && !downed && self.kill_frame.is_none() {
                with_intent(world, |i| i.fire = true);
            }
            if downed && self.kill_frame.is_none() {
                self.kill_frame = Some(frame);
                with_intent(world, |i| i.fire = false);
                self.snap(world, clock, "11_elim_a");
                self.snap_later(frame + 7, "11b_elim_b");
                self.snap_later(frame + 18, "11c_elim_c");
            }
        }

        // Build mode: the gun lowers away and the blueprint comes up.
        if t >= 12.8 && self.once("build") {
            with_intent(world, |i| i.fire = false);
            select(world, ActiveTool::Build(PieceKind::Wall));
            set_look(world, 0.0, -0.25);
        }
        if t >= 12.87 && self.once("build_mid") {
            self.snap(world, clock, "12a_to_build_mid");
        }
        if t >= 13.4 && self.once("build_snap") {
            self.snap(world, clock, "12_build_mode");
        }
        if t >= 13.8 {
            DirectorStatus::Done
        } else {
            DirectorStatus::Running
        }
    }

    fn summary(&mut self, _world: &mut World) -> Value {
        json!({ "captures": self.shots, "kill_frame": self.kill_frame })
    }
}
