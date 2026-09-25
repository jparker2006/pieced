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
    pub fn load_or_default(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
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
}
