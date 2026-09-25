//! `smoke`: a five-second sanity run — walk forward while turning, one screenshot.

use super::{Director, DirectorStatus, ScenarioClock, capture, with_intent};
use bevy::prelude::*;

pub fn director(name: &str) -> Option<Box<dyn Director>> {
    (name == "smoke").then(|| Box::new(Smoke::default()) as Box<dyn Director>)
}

#[derive(Default)]
struct Smoke {
    captured: bool,
}

impl Director for Smoke {
    fn update(&mut self, world: &mut World, clock: &ScenarioClock) -> DirectorStatus {
        let duration = clock.requested_seconds.unwrap_or(5.0);
        let turning = clock.seconds < duration * 0.8;
        with_intent(world, |intent| {
            intent.move_axis = if turning { Vec2::Y } else { Vec2::ZERO };
            intent.look_delta.x += if turning { 0.01 } else { 0.0 };
        });
        if !self.captured && clock.seconds >= 2.0 {
            capture(world, "smoke", true);
            self.captured = true;
        }
        if clock.seconds >= duration {
            DirectorStatus::Done
        } else {
            DirectorStatus::Running
        }
    }
}
