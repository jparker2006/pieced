//! Slice F — `hud_check`: a scripted fight that captures the HUD and menus for
//! review: rifle hits on the shield and the body (hitmarkers, blue and white damage
//! numbers), headshots, the kill marker, a reload, the pump reticle, build mode,
//! a damaged wall's HP bar, the pause menu, the settings page, the tuning panel,
//! and the F3 overlay (on throughout). The summary counts how many hits showed their
//! hitmarker, damage number and sound on the frame the hit registered.
//!
//! The script advances on game events (hits, the kill), not wall-clock alone, so it
//! captures the same moments on a slow machine.

use super::{
    Director, DirectorStatus, ScenarioClock, capture, player_entity, set_look, teleport,
    with_intent,
};
use crate::{
    combat::CombatStats,
    dummy::{Dummy, look_toward},
    hud::HitFeedbackStats,
    menu::{MenuPage, MenuState},
    player::HEAD_CENTER,
    shared::{ActiveTool, EyeHeight, GridCell, Health, PieceKind, PreviousFeet, WeaponKind},
    tuning::Tuning,
};
use bevy::prelude::*;
use serde_json::{Value, json};

pub fn director(name: &str) -> Option<Box<dyn Director>> {
    (name == "hud_check").then(|| Box::new(HudCheck::default()) as Box<dyn Director>)
}

/// Dummy distance in front of the player for the fight (m).
const FIGHT_RANGE: f32 = 11.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Phase {
    #[default]
    Setup,
    Fight,
    Reload,
    Pump,
    Build,
    Piece,
    Pause,
    Settings,
    Panel,
    Done,
}

#[derive(Default)]
struct HudCheck {
    phase: Phase,
    /// Scenario seconds when the current phase began.
    since: f64,
    captured: Vec<&'static str>,
    kill_at: Option<f64>,
    wall_at: Option<Vec3>,
}

impl HudCheck {
    fn go(&mut self, phase: Phase, now: f64) {
        self.phase = phase;
        self.since = now;
    }

    fn capture_once(&mut self, world: &mut World, name: &'static str) {
        if !self.captured.contains(&name) {
            self.captured.push(name);
            capture(world, name, false);
        }
    }
}

fn dummy_entity(world: &mut World) -> Option<Entity> {
    world
        .query_filtered::<Entity, With<Dummy>>()
        .iter(world)
        .next()
}

fn feet_of(world: &World, entity: Option<Entity>) -> Option<Vec3> {
    entity
        .and_then(|e| world.get::<Transform>(e))
        .map(|t| t.translation)
}

/// Points the player's view at `target`.
fn aim_at(world: &mut World, target: Vec3) {
    let Some(player) = player_entity(world) else {
        return;
    };
    let feet = feet_of(world, Some(player)).unwrap_or_default();
    let eye = world.get::<EyeHeight>(player).map(|e| e.0).unwrap_or(1.62);
    let look = look_toward(target - (feet + Vec3::Y * eye));
    set_look(world, look.yaw, look.pitch);
}

/// Holds the trigger (with a fresh press edge when it starts).
fn hold_fire(world: &mut World, firing: bool) {
    with_intent(world, |i| {
        i.fire_pressed |= firing && !i.fire;
        i.fire = firing;
    });
}

fn select(world: &mut World, tool: ActiveTool) {
    with_intent(world, |i| i.select = Some(tool));
}

impl Director for HudCheck {
    fn update(&mut self, world: &mut World, clock: &ScenarioClock) -> DirectorStatus {
        let now = clock.seconds;
        let dt = now - self.since;
        let player = player_entity(world);
        let feet = feet_of(world, player).unwrap_or_default();
        match self.phase {
            Phase::Setup => {
                {
                    let mut tuning = world.resource_mut::<Tuning>();
                    tuning.dummy.stand_still = true;
                    tuning.hud.perf_overlay = true;
                }
                // Stand the dummy in front of the player, close enough to read.
                let spot = feet + Vec3::NEG_Z * FIGHT_RANGE;
                if let Some(dummy) = dummy_entity(world) {
                    if let Some(mut t) = world.get_mut::<Transform>(dummy) {
                        t.translation = spot;
                    }
                    if let Some(mut p) = world.get_mut::<PreviousFeet>(dummy) {
                        p.0 = spot;
                    }
                }
                self.go(Phase::Fight, now);
            }
            Phase::Fight => {
                let dummy = dummy_entity(world);
                let shield = dummy
                    .and_then(|d| world.get::<Health>(d))
                    .map(|h| h.shield)
                    .unwrap_or(0.0);
                if let Some(at) = feet_of(world, dummy) {
                    // Body shots through the shield, then the head.
                    let height = if shield > 0.0 { 1.1 } else { HEAD_CENTER };
                    aim_at(world, at + Vec3::Y * height);
                }
                let stats = world.resource::<CombatStats>().clone();
                let firing = dt > 0.8 && self.kill_at.is_none() && dt < 8.0;
                hold_fire(world, firing);
                if stats.hits >= 2 {
                    self.capture_once(world, "hud-fight-shield");
                }
                if shield <= 0.0 && stats.hits >= 5 {
                    self.capture_once(world, "hud-fight");
                }
                if stats.headshots >= 1 {
                    self.capture_once(world, "hud-headshot");
                }
                if self.kill_at.is_none() && stats.eliminations > 0 {
                    self.kill_at = Some(now);
                }
                if let Some(kill) = self.kill_at
                    && now >= kill + 0.03
                {
                    self.capture_once(world, "hud-kill");
                    self.go(Phase::Reload, now);
                } else if dt >= 8.0 {
                    self.go(Phase::Reload, now);
                }
            }
            Phase::Reload => {
                hold_fire(world, false);
                if dt < 0.05 {
                    with_intent(world, |i| i.reload_pressed = true);
                }
                if dt >= 0.9 {
                    self.capture_once(world, "hud-reload");
                }
                if dt >= 2.4 {
                    select(world, ActiveTool::Weapon(WeaponKind::Pump));
                    self.go(Phase::Pump, now);
                }
            }
            Phase::Pump => {
                if dt >= 0.6 {
                    self.capture_once(world, "hud-pump");
                    select(world, ActiveTool::Build(PieceKind::Wall));
                    self.go(Phase::Build, now);
                }
            }
            Phase::Build => {
                aim_at(world, feet + Vec3::new(0.0, 0.6, -4.0));
                if dt >= 0.5 {
                    // The ghost preview first...
                    self.capture_once(world, "hud-build");
                }
                if dt >= 0.6 && self.wall_at.is_none() {
                    // ...then place it.
                    let edge_z = GridCell::containing(feet).min_corner().z;
                    self.wall_at = Some(Vec3::new(feet.x + 0.7, 1.3, edge_z));
                    with_intent(world, |i| {
                        i.fire_pressed = true;
                        i.fire = true;
                    });
                } else if self.wall_at.is_some() {
                    hold_fire(world, false);
                }
                if dt >= 0.9 {
                    // Step back and draw the rifle to shoot the new wall.
                    teleport(world, feet + Vec3::new(0.0, 0.0, 5.0));
                    select(world, ActiveTool::Weapon(WeaponKind::Rifle));
                    self.go(Phase::Piece, now);
                }
            }
            Phase::Piece => {
                if let Some(wall) = self.wall_at {
                    aim_at(world, wall);
                }
                hold_fire(world, (0.5..1.0).contains(&dt));
                if dt >= 1.4 {
                    self.capture_once(world, "hud-piece");
                    self.go(Phase::Pause, now);
                }
            }
            Phase::Pause => {
                hold_fire(world, false);
                let mut menu = world.resource_mut::<MenuState>();
                menu.menu_open = true;
                menu.page = MenuPage::Main;
                if dt >= 0.4 {
                    self.capture_once(world, "menu-pause");
                    self.go(Phase::Settings, now);
                }
            }
            Phase::Settings => {
                world.resource_mut::<MenuState>().page = MenuPage::Settings;
                if dt >= 0.4 {
                    self.capture_once(world, "menu-settings");
                    self.go(Phase::Panel, now);
                }
            }
            Phase::Panel => {
                world.resource_mut::<MenuState>().panel_open = true;
                if dt >= 0.5 {
                    self.capture_once(world, "tuning-panel");
                }
                if dt >= 0.8 {
                    let mut menu = world.resource_mut::<MenuState>();
                    menu.panel_open = false;
                    menu.menu_open = false;
                    self.go(Phase::Done, now);
                }
            }
            Phase::Done => {
                if dt >= 0.3 {
                    return DirectorStatus::Done;
                }
            }
        }
        DirectorStatus::Running
    }

    fn warmup_seconds(&self) -> f64 {
        1.0
    }

    fn summary(&mut self, world: &mut World) -> Value {
        let feedback = world.resource::<HitFeedbackStats>().clone();
        let stats = world.resource::<CombatStats>().clone();
        json!({
            "hit_feedback": feedback,
            "combat": {
                "shots": stats.shots,
                "hits": stats.hits,
                "headshots": stats.headshots,
                "eliminations": stats.eliminations,
                "last_ttk": stats.last_ttk,
            },
            "kill_at_s": self.kill_at,
            "captures": self.captured,
        })
    }
}
