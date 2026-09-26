//! Milestone 2 "Spellbound" model library (client only): loads the Blender-made
//! glTF models listed in `assets/models/manifest.json` during Boot, exposes their
//! named parts and sidecar data (attach points, part bounds), and fixes glTF's
//! +Z-forward convention to Bevy's -Z. See docs/M2-SPEC.md → Asset pipeline.

use bevy::prelude::*;

pub struct ModelsPlugin;

impl Plugin for ModelsPlugin {
    fn build(&self, _app: &mut App) {}
}
