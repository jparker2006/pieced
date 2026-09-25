//! Slice G — `latency`: input-to-frame latency probe (G3).
//!
//! **Primary method (render-submit clock).** At random moments a virtual input
//! "arrives" (a wall-clock instant chosen at random, not aligned to frames).
//! The first frame whose `PreUpdate` runs after it applies a large, sudden look
//! change through the player's `PlayerIntent` (the same point in the frame
//! where the keyboard/trackpad adapter writes real input), recording the
//! instant and the main-world frame number. The render sub-app records the
//! instant each frame's rendering was submitted and presented (right after
//! `RenderSystems::Render`, see [`telemetry::SubmitClock`]); the two are
//! matched by frame number. Pipelined rendering is off, so the frame that
//! applies the input is the frame rendered right after it.
//!
//! **Secondary check (GPU readback).** For a few trials the view is flipped
//! between looking down at the ground and up at the sky while screenshots are
//! requested on the frame before, the frame of, and the frames after the
//! change. The time until the first readback whose pixels show the change
//! arrives back on the CPU is an upper bound that includes GPU execution and
//! the readback copy; it also confirms which frame first shows the change.

use super::{
    Director, DirectorStatus, ScenarioClock, ScenarioRun, player_entity, set_look, with_intent,
};
use crate::{
    arena::ArenaLayout,
    dummy::Dummy,
    rng::Rng,
    shared::{LookAngles, PreviousFeet},
    telemetry::{self, MainFrame, SubmitClock},
    tuning::Tuning,
};
use bevy::{
    image::Image,
    prelude::*,
    render::{
        render_resource::TextureFormat,
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::PrimaryWindow,
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Render-submit samples.
pub const SAMPLES: usize = 120;
/// GPU-readback trials.
pub const READBACK_TRIALS: usize = 20;
/// The view flips between this pitch down and up (radians, about 46°).
pub const PITCH: f32 = 0.8;
/// One refresh of the 60 Hz display.
pub const REFRESH_MS: f64 = 1000.0 / 60.0;
/// Seconds before sampling starts.
pub const WARMUP_SECONDS: f64 = 4.0;
/// Latest virtual arrival after scheduling (random within this window).
const ARRIVAL_WINDOW: Duration = Duration::from_millis(40);
/// Give up sampling this long after the warm-up (the window may stay covered).
pub const MAX_SAMPLING_SECONDS: f64 = 240.0;
/// Minimum change of the sky score that counts as the view having flipped.
pub const FLIP_THRESHOLD: f32 = 20.0;
/// Screenshots per readback trial: the frame before the change, the frame of
/// the change and the next four.
const READBACK_OFFSETS: std::ops::RangeInclusive<i32> = -1..=4;

pub fn director(name: &str) -> Option<Box<dyn Director>> {
    (name == "latency").then(|| Box::new(Latency::new(0x01A7_E4C1)) as Box<dyn Director>)
}

/// Mean (blue − red) of a few sample points in the upper part of a window
/// capture: strongly positive for sky, near zero or negative for ground.
/// Points avoid the crosshair at the center and the HUD at the bottom.
pub fn sky_score(image: &Image) -> Option<f32> {
    sample_points(image).map(|(sky, _)| sky)
}

/// (mean blue − red, mean brightness) over the sample points.
fn sample_points(image: &Image) -> Option<(f32, f32)> {
    let data = image.data.as_deref()?;
    let bgra = match image.texture_descriptor.format {
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb => true,
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb => false,
        _ => return None,
    };
    let (w, h) = (image.width() as usize, image.height() as usize);
    let mut sum = 0.0;
    let mut luma = 0.0;
    let mut n = 0.0;
    for fy in [0.18, 0.26, 0.34] {
        for fx in [0.2, 0.32, 0.68, 0.8] {
            let (x, y) = ((w as f32 * fx) as usize, (h as f32 * fy) as usize);
            let i = (y * w + x) * 4;
            let px = data.get(i..i + 4)?;
            let (r, b) = if bgra { (px[2], px[0]) } else { (px[0], px[2]) };
            sum += b as f32 - r as f32;
            luma += (r as f32 + px[1] as f32 + b as f32) / 3.0;
            n += 1.0;
        }
    }
    Some((sum / n, luma / n))
}

#[derive(Debug, Clone)]
struct Sample {
    frame: u64,
    arrival: Instant,
    applied: Instant,
    /// When the previous frame sampled input.
    previous_poll: Option<Instant>,
    submitted: Option<Instant>,
}

#[derive(Debug, Clone)]
struct Capture {
    trial: usize,
    offset: i32,
    at: Instant,
    score: Option<f32>,
    brightness: Option<f32>,
}

#[derive(Debug, Clone)]
struct Trial {
    /// +1 when flipping up to the sky, −1 when flipping down.
    direction: f32,
    frame: Option<u64>,
    applied: Option<Instant>,
    submitted: Option<Instant>,
    /// Frames since the trial began.
    frames: i32,
    /// The window was covered at some point during the trial.
    occluded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Step {
    Setup,
    Primary,
    Readback,
    Done,
}

struct Latency {
    rng: Rng,
    step: Step,
    last_poll: Option<Instant>,
    /// The next virtual input, once scheduled.
    arrival: Option<Instant>,
    gap_frames: u32,
    /// Whether the view currently looks up.
    up: bool,
    samples: Vec<Sample>,
    trials: Vec<Trial>,
    captures: Arc<Mutex<Vec<Capture>>>,
    notes: Vec<String>,
    load_start: Option<[f64; 3]>,
    /// Samples dropped because the window was covered (not composited).
    skipped_occluded: u32,
    occluded_frames: u64,
}

impl Latency {
    fn new(seed: u64) -> Self {
        Self {
            rng: Rng::new(seed),
            step: Step::Setup,
            last_poll: None,
            arrival: None,
            gap_frames: 30,
            up: false,
            samples: Vec::new(),
            trials: Vec::new(),
            captures: Arc::new(Mutex::new(Vec::new())),
            notes: Vec::new(),
            load_start: None,
            skipped_occluded: 0,
            occluded_frames: 0,
        }
    }

    /// Flips the view between down and up through the intent's look delta.
    fn flip(&mut self, world: &mut World) {
        let pitch = player_entity(world)
            .and_then(|p| world.get::<LookAngles>(p))
            .map_or(0.0, |l| l.pitch);
        self.up = !self.up;
        let target = if self.up { PITCH } else { -PITCH };
        with_intent(world, |i| i.look_delta.y += target - pitch);
    }

    fn request_capture(&self, world: &mut World, trial: usize, offset: i32) {
        let captures = self.captures.clone();
        // Keep the first two trials' before/after frames as evidence.
        let save = (trial < 2 && offset <= 0)
            .then(|| world.get_resource::<ScenarioRun>().map(|r| r.out.clone()))
            .flatten()
            .map(|dir| dir.join(format!("latency-readback-t{trial}-f{offset}.png")));
        world.spawn(Screenshot::primary_window()).observe(
            move |capture: On<ScreenshotCaptured>| {
                let at = Instant::now();
                let points = sample_points(&capture.image);
                if let Ok(mut list) = captures.lock() {
                    list.push(Capture {
                        trial,
                        offset,
                        at,
                        score: points.map(|p| p.0),
                        brightness: points.map(|p| p.1),
                    });
                }
                if let Some(path) = save.clone() {
                    let image = capture.image.clone();
                    std::thread::spawn(move || {
                        if let Ok(image) = image.try_into_dynamic() {
                            let _ = image.to_rgb8().save(path);
                        }
                    });
                }
            },
        );
    }

    fn resolve_submits(&mut self, clock: &SubmitClock) {
        for s in self.samples.iter_mut().filter(|s| s.submitted.is_none()) {
            s.submitted = clock.submitted(s.frame);
        }
        for t in self.trials.iter_mut().filter(|t| t.submitted.is_none()) {
            t.submitted = t.frame.and_then(|f| clock.submitted(f));
        }
    }

    fn primary(&mut self, world: &mut World, frame: u64, now: Instant) {
        match self.arrival {
            Some(arrival) if now >= arrival => {
                self.flip(world);
                self.samples.push(Sample {
                    frame,
                    arrival,
                    applied: now,
                    previous_poll: self.last_poll,
                    submitted: None,
                });
                self.arrival = None;
                self.gap_frames = 3 + (self.rng.next_u64() % 12) as u32;
            }
            Some(_) => {}
            None if self.gap_frames > 0 => self.gap_frames -= 1,
            None => {
                let offset = ARRIVAL_WINDOW.mul_f32(self.rng.next_f32());
                self.arrival = Some(now + offset);
            }
        }
    }

    /// Advances the current readback trial by one frame: a capture on the frame
    /// before the flip, the flip with a capture, then captures of later frames.
    fn readback(&mut self, world: &mut World, frame: u64, now: Instant) {
        let index = self.trials.len().saturating_sub(1);
        let Some(trial) = self.trials.last_mut() else {
            return;
        };
        let offset = trial.frames - 1;
        trial.frames += 1;
        if offset == 0 {
            trial.frame = Some(frame);
            trial.applied = Some(now);
        }
        if READBACK_OFFSETS.contains(&offset) {
            if offset == 0 {
                self.flip(world);
            }
            self.request_capture(world, index, offset);
        }
    }

    fn trial_done(&self, index: usize) -> bool {
        let Some(trial) = self.trials.get(index) else {
            return true;
        };
        let expected = READBACK_OFFSETS.count();
        let got = self
            .captures
            .lock()
            .map(|c| c.iter().filter(|c| c.trial == index).count())
            .unwrap_or(0);
        got >= expected || trial.frames > 120
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

impl Director for Latency {
    fn update(&mut self, world: &mut World, clock: &ScenarioClock) -> DirectorStatus {
        let now = Instant::now();
        let frame = world.resource::<MainFrame>().0;
        let submits = world.resource::<SubmitClock>().clone();
        self.resolve_submits(&submits);
        // A covered window is not composited: presentation stops pacing and
        // readbacks come back black, so nothing is sampled while it is covered.
        let occluded = world
            .get_resource::<telemetry::RunConditions>()
            .is_some_and(|c| c.occluded_now);
        if occluded && self.step != Step::Setup {
            self.occluded_frames += 1;
        }
        if self.step != Step::Setup
            && self.step != Step::Done
            && clock.seconds > WARMUP_SECONDS + MAX_SAMPLING_SECONDS
        {
            self.notes.push(format!(
                "stopped after {MAX_SAMPLING_SECONDS} s of sampling with {} samples and {} readback trials",
                self.samples.len(),
                self.trials.len()
            ));
            self.step = Step::Done;
            self.gap_frames = 10;
        }
        match self.step {
            Step::Setup => {
                if self.load_start.is_none() {
                    self.load_start = telemetry::load_average();
                }
                world.resource_mut::<Tuning>().dummy.stand_still = true;
                // Park the dummy behind the player, out of view.
                let spawn = world.resource::<ArenaLayout>().player_spawn;
                let parked = spawn + Vec3::new(-20.0, 0.0, 8.0);
                if let Some(dummy) = world
                    .query_filtered::<Entity, With<Dummy>>()
                    .iter(world)
                    .next()
                {
                    if let Some(mut t) = world.get_mut::<Transform>(dummy) {
                        t.translation = parked;
                    }
                    if let Some(mut p) = world.get_mut::<PreviousFeet>(dummy) {
                        p.0 = parked;
                    }
                }
                set_look(world, 0.0, -PITCH);
                self.up = false;
                if clock.seconds >= WARMUP_SECONDS {
                    self.step = Step::Primary;
                }
            }
            Step::Primary if occluded => {
                if self.arrival.take().is_some() {
                    self.skipped_occluded += 1;
                }
            }
            Step::Primary => {
                self.primary(world, frame, now);
                if self.samples.len() >= SAMPLES && self.arrival.is_none() {
                    self.step = Step::Readback;
                    self.gap_frames = 20;
                }
            }
            Step::Readback => {
                let current = self.trials.len().checked_sub(1);
                if occluded && let Some(t) = self.trials.last_mut() {
                    t.occluded = true;
                }
                let finished = current.is_none_or(|i| self.trial_done(i));
                if !finished {
                    self.readback(world, frame, now);
                } else if self.gap_frames > 0 {
                    self.gap_frames -= 1;
                } else if occluded {
                    // Start trials only while the window is visible.
                } else if self.trials.iter().filter(|t| !t.occluded).count() >= READBACK_TRIALS {
                    self.step = Step::Done;
                    self.gap_frames = 10;
                } else {
                    self.trials.push(Trial {
                        direction: if self.up { -1.0 } else { 1.0 },
                        frame: None,
                        applied: None,
                        submitted: None,
                        frames: 0,
                        occluded: false,
                    });
                    self.readback(world, frame, now);
                    self.gap_frames = 10 + (self.rng.next_u64() % 10) as u32;
                }
            }
            Step::Done => {
                if self.gap_frames == 0 {
                    return DirectorStatus::Done;
                }
                self.gap_frames -= 1;
            }
        }
        self.last_poll = Some(now);
        DirectorStatus::Running
    }

    fn warmup_seconds(&self) -> f64 {
        WARMUP_SECONDS
    }

    fn summary(&mut self, world: &mut World) -> Value {
        let submits = world.resource::<SubmitClock>().clone();
        self.resolve_submits(&submits);
        let resolved: Vec<&Sample> = self
            .samples
            .iter()
            .filter(|s| s.submitted.is_some())
            .collect();
        let apply: Vec<f64> = resolved
            .iter()
            .map(|s| ms(s.submitted.unwrap_or(s.applied) - s.applied))
            .collect();
        let arrival: Vec<f64> = resolved
            .iter()
            .map(|s| ms(s.submitted.unwrap_or(s.applied) - s.arrival))
            .collect();
        let queue_wait: Vec<f64> = resolved.iter().map(|s| ms(s.applied - s.arrival)).collect();
        let poll_interval: Vec<f64> = resolved
            .iter()
            .filter_map(|s| s.previous_poll.map(|p| ms(s.applied - p)))
            .collect();
        let plus_refresh = |v: &[f64]| -> Vec<f64> { v.iter().map(|x| x + REFRESH_MS).collect() };
        let apply_stats = telemetry::latency_stats(&apply);
        let arrival_stats = telemetry::latency_stats(&arrival);

        // Readback trials.
        let captures = self.captures.lock().map(|c| c.clone()).unwrap_or_default();
        let mut readback_ms = Vec::new();
        let mut trials = Vec::new();
        let mut same_frame = 0;
        for (i, t) in self.trials.iter().enumerate() {
            let mine: Vec<&Capture> = captures.iter().filter(|c| c.trial == i).collect();
            let before = mine.iter().find(|c| c.offset == -1).and_then(|c| c.score);
            let first_changed = before.and_then(|b| {
                let mut after: Vec<&&Capture> = mine
                    .iter()
                    .filter(|c| c.offset >= 0)
                    .filter(|c| {
                        c.score
                            .is_some_and(|s| (s - b) * t.direction > FLIP_THRESHOLD)
                    })
                    .collect();
                after.sort_by_key(|c| c.offset);
                after.first().map(|c| (c.offset, c.at))
            });
            let latency = first_changed
                .zip(t.applied)
                .map(|((_, at), applied)| ms(at - applied));
            if !t.occluded {
                if let Some(l) = latency {
                    readback_ms.push(l);
                }
                if first_changed.is_some_and(|(o, _)| o == 0) {
                    same_frame += 1;
                }
            }
            let mut scores: Vec<(i32, Option<f32>, Option<f32>)> = mine
                .iter()
                .map(|c| (c.offset, c.score, c.brightness))
                .collect();
            scores.sort_by_key(|s| s.0);
            trials.push(json!({
                "trial": i,
                "window_covered": t.occluded,
                "direction": if t.direction > 0.0 { "down_to_up" } else { "up_to_down" },
                "frame_offset_sky_score_brightness": scores.iter().map(|(o, s, b)| json!([o, s, b])).collect::<Vec<_>>(),
                "first_changed_frame_offset": first_changed.map(|(o, _)| o),
                "input_to_readback_ms": latency,
                "input_to_render_submit_ms": t.applied.zip(t.submitted).map(|(a, s)| ms(s - a)),
            }));
        }

        let window = world
            .query_filtered::<&Window, With<PrimaryWindow>>()
            .iter(world)
            .next()
            .map(|w| {
                json!({
                    "present_mode": format!("{:?}", w.present_mode),
                    "desired_maximum_frame_latency": w.desired_maximum_frame_latency.map(|n| n.get()),
                    "mode": format!("{:?}", w.mode),
                })
            });
        let tuning = world.resource::<Tuning>();
        let gate = telemetry::evaluate_g3("arrival_to_render_submit", &arrival_stats);
        let gate_apply = telemetry::evaluate_g3("apply_to_render_submit", &apply_stats);
        json!({
            "measures": "G3 input-to-frame latency, software-measured.",
            "method": {
                "primary": "A virtual input arrives at a random wall-clock instant (uniform over a 40 ms window, so at a random phase of the frame). The first frame whose PreUpdate runs after it applies a sudden ±46° pitch change through PlayerIntent.look_delta (the same schedule point where the keyboard/trackpad adapter writes real input) and records the instant and the main-world frame number. The render sub-app records an instant right after RenderSystems::Render (render graph executed, command buffers submitted, swapchain present called) with the frame number extracted from the main world. Pipelined rendering is off, so frame N's render follows frame N's update directly.",
                "apply_to_render_submit": "From the frame's PreUpdate applying the input to render submit/present of that frame.",
                "arrival_to_render_submit": "From the virtual arrival to render submit/present of the frame that applied it; adds the wait for the next frame to pick the input up, as a real OS event queued while a frame runs would wait.",
                "estimated_input_to_photon_upper_bound": format!("Submit latency plus one refresh interval ({REFRESH_MS:.1} ms): with desired_maximum_frame_latency 1 the presented frame is assumed to be scanned out from the next vblank. Assumes the GPU finishes the frame within that refresh and the macOS compositor adds no extra frame of buffering (a windowed or composited path can add one)."),
                "secondary_gpu_readback": "Screenshots requested on the frame before, the frame of, and four frames after a sudden pitch flip between ground and sky; the time from applying the input until the first readback whose pixels show the flip arrives on the CPU (ScreenshotCaptured, delivered in a later main-world Update). An upper bound on GPU completion that includes the readback copy, buffer mapping and the delivery delay.",
            },
            "includes": [
                "(arrival basis) waiting for the next frame to sample input",
                "main schedule after input (fixed-step simulation, camera follow, transform propagation)",
                "extract, prepare and the wait for a swapchain drawable (vsync back-pressure lands here)",
                "render graph encoding, command submission and the present call",
            ],
            "excludes": [
                "the OS and winit delivering a physical trackpad or key event to the app (not measurable in software here)",
                "GPU execution after submit and the wait for vblank (bounded by the estimate)",
                "display scanout and pixel response",
            ],
            "samples_requested": SAMPLES,
            "samples_resolved": resolved.len(),
            "samples_skipped_while_window_covered": self.skipped_occluded,
            "frames_with_window_covered_while_sampling": self.occluded_frames,
            "apply_to_render_submit_ms": apply_stats,
            "arrival_to_render_submit_ms": arrival_stats,
            "arrival_wait_until_frame_samples_input_ms": telemetry::latency_stats(&queue_wait),
            "input_poll_interval_ms": telemetry::latency_stats(&poll_interval),
            "estimated_input_to_photon_upper_bound_ms": {
                "from_arrival": telemetry::latency_stats(&plus_refresh(&arrival)),
                "from_apply": telemetry::latency_stats(&plus_refresh(&apply)),
                "refresh_interval_ms": REFRESH_MS,
            },
            "gpu_readback": {
                "trials": self.trials.len(),
                "trials_with_window_visible": self.trials.iter().filter(|t| !t.occluded).count(),
                "trials_with_visible_change": readback_ms.len(),
                "change_visible_in_the_input_frame": same_frame,
                "input_to_readback_ms": telemetry::latency_stats(&readback_ms),
                "flip_threshold_sky_score": FLIP_THRESHOLD,
                "trial_detail": trials,
            },
            "window": window,
            "vsync": tuning.graphics.vsync,
            "frame_cap": tuning.graphics.frame_cap,
            "g3_threshold_median_ms": telemetry::g3::MEDIAN_MAX_MS,
            "gate": gate,
            "gate_apply_basis": gate_apply,
            "run_conditions": telemetry::run_conditions_json(world, self.load_start),
            "notes": self.notes,
        })
    }
}
