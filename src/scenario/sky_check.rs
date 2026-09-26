//! `sky_check`: evidence that the far view moves (docs/M2-SPEC.md → Testing;
//! gate S4). From the spawn, looking up between the galaxy (upper left) and the
//! station with its ships (right), it captures a frame every
//! [`INTERVAL`] seconds for [`DURATION`] seconds: `sky-00s.png` … `sky-20s.png`.
//! The galaxy turns, ships cross, the stained glass pulses and islands bob, so
//! consecutive frames must differ. The summary measures how much, over the sky
//! band at the top of the frame ([`SKY_BAND`]), clear of the gun, the HUD and
//! the knight standing still below.
//!
//! The script runs on game time (fixed ticks), so a hitch never skips a frame.

use super::{
    Director, DirectorStatus, ScenarioClock, ScenarioRun, set_look, teleport, with_intent,
};
use crate::{
    arena::ArenaLayout,
    scenario::gallery::GALLERY_FOV_DEG,
    shared::{ActiveTool, PlayerIntent, WeaponKind},
    tuning::Tuning,
};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

pub fn director(name: &str) -> Option<Box<dyn Director>> {
    (name == "sky_check").then(|| Box::new(SkyCheck::default()) as Box<dyn Director>)
}

/// Seconds between frames, and the span covered (frames at 0, 5, … 20 s).
pub const INTERVAL: f64 = 5.0;
pub const DURATION: f64 = 20.0;
/// Seconds before the first frame, so the far view has loaded and settled.
pub const LEAD_IN: f64 = 2.0;
/// The view from the spawn: yaw 5° right of north, 14° up. The galaxy core
/// (azimuth -21°, 28° up) sits upper left, the station (azimuth 30°) right.
pub const LOOK_YAW_DEG: f32 = -5.0;
pub const LOOK_PITCH_DEG: f32 = 14.0;
/// Fraction of the frame's height, from the top, the change is measured over.
pub const SKY_BAND: f32 = 0.55;
/// A pixel counts as changed when a channel moved by more than this (0-255).
pub const CHANGE_THRESHOLD: u8 = 12;

/// One captured frame's pixels (RGB8, row major).
#[derive(Debug, Clone)]
pub struct Frame {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// How two frames differ over the top `band` of the image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameDiff {
    /// Mean absolute channel difference (0-255).
    pub mean_abs: f64,
    /// Fraction of pixels with a channel moved by more than the threshold.
    pub changed: f64,
}

/// Compares two same-sized RGB8 frames over the top `band` of their height.
pub fn frame_diff(a: &Frame, b: &Frame, band: f32, threshold: u8) -> Option<FrameDiff> {
    if a.width != b.width || a.height != b.height || a.rgb.len() != b.rgb.len() {
        return None;
    }
    let rows = ((a.height as f32 * band).round() as usize).min(a.height as usize);
    let n = rows * a.width as usize;
    if n == 0 {
        return None;
    }
    let (mut sum, mut changed) = (0u64, 0u64);
    let (pixels_a, _) = a.rgb[..n * 3].as_chunks::<3>();
    let (pixels_b, _) = b.rgb[..n * 3].as_chunks::<3>();
    for (pa, pb) in pixels_a.iter().zip(pixels_b) {
        let mut most = 0u8;
        for (ca, cb) in pa.iter().zip(pb) {
            let d = ca.abs_diff(*cb);
            sum += u64::from(d);
            most = most.max(d);
        }
        changed += u64::from(most > threshold);
    }
    Some(FrameDiff {
        mean_abs: sum as f64 / (n * 3) as f64,
        changed: changed as f64 / n as f64,
    })
}

#[derive(Default)]
struct SkyCheck {
    start_tick: Option<u64>,
    next: u32,
    frames: Arc<Mutex<Vec<Frame>>>,
    names: Vec<String>,
}

impl SkyCheck {
    fn setup(world: &mut World) {
        {
            let mut tuning = world.resource_mut::<Tuning>();
            tuning.look.fov_deg = GALLERY_FOV_DEG;
            tuning.dummy.stand_still = true;
            tuning.hud.perf_overlay = false;
        }
        let spawn = world.resource::<ArenaLayout>().player_spawn;
        with_intent(world, |i| {
            *i = PlayerIntent {
                select: Some(ActiveTool::Weapon(WeaponKind::Rifle)),
                ..default()
            };
        });
        teleport(world, spawn);
        set_look(
            world,
            LOOK_YAW_DEG.to_radians(),
            LOOK_PITCH_DEG.to_radians(),
        );
    }

    /// Screenshots the window to `<evidence>/<name>.png` and keeps its pixels.
    fn capture(&mut self, world: &mut World, name: String) {
        let out = world.resource::<ScenarioRun>().out.clone();
        world
            .resource_mut::<ScenarioRun>()
            .screenshots
            .push(format!("{name}.png"));
        self.names.push(name.clone());
        let path = out.join(format!("{name}.png"));
        let frames = self.frames.clone();
        world.spawn(Screenshot::primary_window()).observe(
            move |capture: On<ScreenshotCaptured>| {
                let image = capture.image.clone();
                let path = path.clone();
                let frames = frames.clone();
                let name = name.clone();
                std::thread::spawn(move || {
                    let Ok(image) = image.try_into_dynamic() else {
                        eprintln!("screenshot conversion failed: {}", path.display());
                        return;
                    };
                    let rgb = image.to_rgb8();
                    if let Err(e) = rgb.save(&path) {
                        eprintln!("screenshot not saved to {}: {e}", path.display());
                    }
                    let (width, height) = rgb.dimensions();
                    if let Ok(mut frames) = frames.lock() {
                        frames.push(Frame {
                            name,
                            width,
                            height,
                            rgb: rgb.into_raw(),
                        });
                    }
                });
            },
        );
    }
}

impl Director for SkyCheck {
    fn update(&mut self, world: &mut World, clock: &ScenarioClock) -> DirectorStatus {
        let start = *self.start_tick.get_or_insert_with(|| {
            Self::setup(world);
            clock.tick
        });
        // Keep the view steady (and the trigger up) all run.
        set_look(
            world,
            LOOK_YAW_DEG.to_radians(),
            LOOK_PITCH_DEG.to_radians(),
        );
        let now = (clock.tick - start) as f64 / 60.0 - LEAD_IN;
        let shots = (DURATION / INTERVAL).round() as u32 + 1;
        if self.next < shots && now >= self.next as f64 * INTERVAL {
            let name = format!("sky-{:02}s", (self.next as f64 * INTERVAL).round() as u32);
            self.capture(world, name);
            self.next += 1;
        }
        // A second after the last frame, so its readback has landed.
        if self.next >= shots && now >= DURATION + 1.0 {
            DirectorStatus::Done
        } else {
            DirectorStatus::Running
        }
    }

    fn warmup_seconds(&self) -> f64 {
        LEAD_IN
    }

    fn summary(&mut self, _world: &mut World) -> Value {
        let mut frames = self.frames.lock().map(|f| f.clone()).unwrap_or_default();
        frames.sort_by(|a, b| a.name.cmp(&b.name));
        let diffs: Vec<Value> = frames
            .windows(2)
            .map(|w| {
                let d = frame_diff(&w[0], &w[1], SKY_BAND, CHANGE_THRESHOLD);
                json!({
                    "from": w[0].name,
                    "to": w[1].name,
                    "mean_abs_diff": d.map(|d| d.mean_abs),
                    "changed_fraction": d.map(|d| d.changed),
                })
            })
            .collect();
        let differ = !diffs.is_empty()
            && frames.len() == self.names.len()
            && diffs
                .iter()
                .all(|d| d["changed_fraction"].as_f64().is_some_and(|c| c > 0.001));
        json!({
            "frames": self.names,
            "frames_read": frames.len(),
            "sky_band": SKY_BAND,
            "change_threshold": CHANGE_THRESHOLD,
            "diffs": diffs,
            "frames_differ": differ,
            "look": { "yaw_deg": LOOK_YAW_DEG, "pitch_deg": LOOK_PITCH_DEG },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> Frame {
        let mut rgb = Vec::new();
        for y in 0..h {
            for x in 0..w {
                rgb.extend(f(x, y));
            }
        }
        Frame {
            name: String::new(),
            width: w,
            height: h,
            rgb,
        }
    }

    #[test]
    fn frame_diff_counts_changes_in_the_sky_band_only() {
        let a = frame(10, 10, |_, _| [20, 30, 40]);
        // Only the bottom half changes: outside a 0.5 band.
        let b = frame(
            10,
            10,
            |_, y| if y >= 5 { [200, 30, 40] } else { [20, 30, 40] },
        );
        let d = frame_diff(&a, &b, 0.5, 12).unwrap();
        assert_eq!(d.changed, 0.0);
        assert_eq!(d.mean_abs, 0.0);
        // One changed pixel in the band's 50.
        let c = frame(10, 10, |x, y| {
            if (x, y) == (3, 1) {
                [20, 90, 40]
            } else {
                [20, 30, 40]
            }
        });
        let d = frame_diff(&a, &c, 0.5, 12).unwrap();
        assert!((d.changed - 1.0 / 50.0).abs() < 1e-9);
        assert!((d.mean_abs - 60.0 / 150.0).abs() < 1e-9);
        // Small noise stays under the threshold.
        let n = frame(10, 10, |_, _| [25, 30, 40]);
        assert_eq!(frame_diff(&a, &n, 0.5, 12).unwrap().changed, 0.0);
        // Sizes must match.
        assert!(frame_diff(&a, &frame(5, 10, |_, _| [0; 3]), 0.5, 12).is_none());
    }
}
