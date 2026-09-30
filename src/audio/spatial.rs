//! How a spatial sound reaches each ear (M4 chunk 6).
//!
//! Bevy 0.19 plays spatial sounds through rodio 0.22's `Spatial` source,
//! which sets each output channel's gain from the emitter's distance to each
//! ear. [`rodio_ear_gains`] is that formula, copied so tests can check what
//! a player hears. Its difference term is turned around: the channel whose
//! ear is *farther* from the emitter gets the louder share, so with Bevy's
//! own [`SpatialListener::new`] (left ear at −X) every spatial cue in the
//! game played from the mirrored side. [`listener`] places the ears the other
//! way round, so the game's panning is right: a knight on your right is
//! louder in your right speaker (the off-screen warning included).

use bevy::{audio::SpatialListener, prelude::*};

/// The gap between the ears (m), before the spatial scale.
pub const EAR_GAP: f32 = 0.25;

/// The listener the main camera carries: the ears swapped against rodio's
/// reversed difference term (see the module docs), so output channel 0 (the
/// left speaker) is loudest for sounds on the camera's left.
pub fn listener() -> SpatialListener {
    SpatialListener {
        left_ear_offset: Vec3::X * EAR_GAP / 2.0,
        right_ear_offset: Vec3::X * EAR_GAP / -2.0,
    }
}

/// rodio 0.22.2 `Spatial::set_positions`: the gains of output channels 0 and
/// 1 for an emitter and the two ears it is given (all already scaled).
pub fn rodio_ear_gains(emitter: Vec3, left_ear: Vec3, right_ear: Vec3) -> (f32, f32) {
    let left_dist_sq = left_ear.distance_squared(emitter);
    let right_dist_sq = right_ear.distance_squared(emitter);
    let max_diff = left_ear.distance(right_ear);
    let left_dist = left_dist_sq.sqrt();
    let right_dist = right_dist_sq.sqrt();
    let left_diff = (((left_dist - right_dist) / max_diff + 1.0) / 4.0 + 0.5).min(1.0);
    let right_diff = (((right_dist - left_dist) / max_diff + 1.0) / 4.0 + 0.5).min(1.0);
    let left_dist_mod = (1.0 / left_dist_sq).min(1.0);
    let right_dist_mod = (1.0 / right_dist_sq).min(1.0);
    (left_diff * left_dist_mod, right_diff * right_dist_mod)
}

/// The (left speaker, right speaker) gains Bevy gives an emitter at `at` for
/// `listener` on a camera at `camera`, with spatial scale `scale` (what
/// `bevy_audio` passes rodio).
pub fn speaker_gains(
    camera: &GlobalTransform,
    listener: &SpatialListener,
    scale: f32,
    at: Vec3,
) -> (f32, f32) {
    let left = camera.transform_point(listener.left_ear_offset) * scale;
    let right = camera.transform_point(listener.right_ear_offset) * scale;
    rodio_ear_gains(at * scale, left, right)
}
