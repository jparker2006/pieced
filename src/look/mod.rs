//! Milestone 2 "Spellbound" rendering foundations (client only): the toon
//! material and its shared lighting, ink outlines, the far-layer material and
//! halo billboards, blob shadows, and pipeline warm-up behind the Boot gate.
//! See docs/M2-SPEC.md → Rendering.

use bevy::prelude::*;

pub struct LookPlugin;

impl Plugin for LookPlugin {
    fn build(&self, _app: &mut App) {}
}
