//! Every feel number in one live-editable, persisted resource. Each slice owns
//! its own sub-struct (defined in that slice's module); this file only aggregates.
//! All sub-structs use `#[serde(default)]` so older settings files keep loading.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Tuning {
    pub look: crate::input::LookTuning,
    pub movement: crate::movement::MovementTuning,
    pub building: crate::building::BuildTuning,
    pub combat: crate::combat::CombatTuning,
    pub dummy: crate::dummy::DummyTuning,
    pub feedback: crate::fx::FeedbackTuning,
    pub hud: crate::hud::HudTuning,
    pub audio: crate::audio::AudioTuning,
    pub graphics: crate::render::GraphicsTuning,
    /// The key bindings (D93: the Settings → Controls page). A menu setting:
    /// `load_or_default` keeps it from the file.
    pub bindings: crate::input::Bindings,
    // Designer tuning for Waves (M3). Never persisted: `settings.json` holds
    // every other section, so a saved copy would freeze these numbers and hide
    // play-test tuning changes from Jake's game.
    #[serde(skip)]
    pub grunt: crate::grunt::GruntTuning,
    #[serde(skip)]
    pub orb: crate::orb::OrbTuning,
    #[serde(skip)]
    pub waves: crate::waves::WavesTuning,
    /// Designer performance settings (M4 chunk 0): GPU timing and the D100
    /// levers. Never persisted, like the Waves sections.
    #[serde(skip)]
    pub perf: crate::perf::PerfTuning,
    /// Kill feedback feel (M4 chunk 1): designer numbers, never persisted.
    #[serde(skip)]
    pub kills: crate::fx::kills::KillFeelTuning,
    /// Weapon feel (M4 chunk 2): the draw, ADS weight, breathing and the
    /// render-only camera kick. Designer numbers, never persisted.
    #[serde(skip)]
    pub weapons: crate::viewmodel::WeaponFeelTuning,
    /// The adaptive score's designer numbers (M4 chunk 3): crossfades, ducks,
    /// mix levels. Never persisted; the Music and Effects sliders live in
    /// `audio`.
    #[serde(skip)]
    pub music: crate::audio::music::MusicTuning,
}

impl Tuning {
    /// Where settings persist: `<project>/userdata/settings.json` (git-ignored).
    /// `PIECED_ROOT` overrides the project directory.
    pub fn settings_path() -> std::path::PathBuf {
        std::env::var_os("PIECED_ROOT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")))
            .join("userdata")
            .join("settings.json")
    }

    /// Loads persisted settings, falling back to defaults on any problem.
    ///
    /// Only what the Settings menu edits is taken from the file (look, audio,
    /// graphics, HUD, feedback, aim friction and the key bindings). Designer numbers (movement,
    /// building, combat, the dummy) always come from the code: every section is
    /// saved, so otherwise the first save froze them and later tuning never
    /// reached the game.
    pub fn load_or_default(path: &std::path::Path) -> Self {
        let mut t: Self = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        let d = Self::default();
        let friction = (t.combat.aim_friction, t.combat.aim_friction_strength);
        t.movement = d.movement;
        t.building = d.building;
        t.combat = d.combat;
        (t.combat.aim_friction, t.combat.aim_friction_strength) = friction;
        t.dummy = d.dummy;
        // The kill hitstop is a designer feel number (M4), not a menu one.
        t.feedback.hitstop_on_kill = d.feedback.hitstop_on_kill;
        t.feedback.hitstop_frames = d.feedback.hitstop_frames;
        t
    }

    /// Saves settings atomically (write then rename).
    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_string_pretty(self).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_tolerate_missing_fields() {
        let dir = std::env::temp_dir().join(format!("pieced-tuning-{}", std::process::id()));
        let path = dir.join("settings.json");
        let mut t = Tuning::default();
        t.look.sensitivity = 0.009;
        t.save(&path).unwrap();
        assert_eq!(Tuning::load_or_default(&path), t);
        std::fs::write(&path, r#"{"look":{"fov_deg":80.0}}"#).unwrap();
        let partial = Tuning::load_or_default(&path);
        assert_eq!(partial.look.fov_deg, 80.0);
        assert_eq!(partial.movement, crate::movement::MovementTuning::default());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn designer_numbers_come_from_the_code_not_the_file() {
        let dir = std::env::temp_dir().join(format!("pieced-tuning-d-{}", std::process::id()));
        let path = dir.join("settings.json");
        let mut t = Tuning::default();
        t.combat.pump.damage = 1.0;
        t.combat.aim_friction = true;
        t.movement.run_speed = 1.0;
        t.look.sensitivity = 0.009;
        t.save(&path).unwrap();
        let loaded = Tuning::load_or_default(&path);
        assert_eq!(loaded.combat.pump, crate::combat::GunTuning::pump());
        assert_eq!(loaded.movement, crate::movement::MovementTuning::default());
        assert!(loaded.combat.aim_friction, "a menu setting survives");
        assert_eq!(loaded.look.sensitivity, 0.009);
        let _ = std::fs::remove_dir_all(dir);
    }
}
