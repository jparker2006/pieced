//! Per-frame cost attribution for the session log (M3 performance slice).
//!
//! Every frame the session log records where its interval went and what
//! happened in it, so a spike in Jake's play session can be traced to a cause
//! without ever running a timing session ourselves.
//!
//! **Where the time went.** Marker schedules are threaded between the main
//! schedules ([`MainScheduleOrder`]), around the physics step inside each
//! fixed tick ([`FixedMainScheduleOrder`]) and around the render world's
//! schedules ([`RenderScheduleOrder`]), plus markers at the render schedule's
//! set boundaries. Each marker stores one `Instant` (as nanoseconds since
//! process start) in a shared atomic slot: no allocation, a few nanoseconds.
//! Pipelined rendering is off, so a frame runs `main (N) → extract → render
//! (N)` on one thread, and the interval that ends at frame N's `Last` covers
//! render (N−1), the event-loop gap, then main (N). [`FrameCost`] splits it:
//!
//! | column | what it covers |
//! |---|---|
//! | `pre_ms` | `First` + `PreUpdate` (window and input events, state transitions) |
//! | `fixed_ms` | every fixed tick this frame (gameplay and physics) |
//! | `physics_ms` | the avian step inside those ticks (`FixedPostUpdate`) |
//! | `update_ms` | `Update` + `SpawnScene` |
//! | `post_ms` | `PostUpdate` (transforms, visibility, UI layout, audio) |
//! | `extract_ms` | render extract of the previous frame |
//! | `prepare_ms` | render specialize, queue, sort and prepare (excluding the drawable wait) |
//! | `acquire_ms` | `PrepareViews`: acquiring the swapchain drawable. With vsync this is where a frame waits for the display or for a GPU that is behind |
//! | `graph_ms` | `RenderSystems::Render`: first-use pipeline compiles, render graph encode, submit and present |
//! | `render_end_ms` | render cleanup and the frame limiter (no-vsync only) |
//! | `idle_ms` | from the end of rendering to the next frame's `First` (the winit event loop and the OS) |
//! | `vsync_dt_ms` | the interval between the last two drawable acquisitions: the display-paced cadence, for comparing with `dt_ms` (the CPU-side interval S2 measures) |
//! | `gpu_ms` | the GPU time of a timed frame, first mark to last (1 frame in 8 by default; it arrives a few frames late, and `gpu_frame` names the frame it measured; see [`crate::gpu_timing`]) |
//! | `press_latency_ms`, `motion_latency_ms` | input-to-present latency samples resolved this frame (see [`crate::latency`]) |
//!
//! **Pipelined rendering** (the default since the M4 performance follow-up;
//! `--knobs pipelined=off` turns it off): the render world of frame N runs
//! on its own thread beside main N+1, so its marks can't split this frame's
//! interval. The main-world buckets stay as above; of the render buckets,
//! `acquire_ms` and `vsync_dt_ms` come from each render frame's drawable
//! acquisition in turn (a small lock-free ring, so every presented interval
//! lands in exactly one row, a frame or two late), and the rest read zero
//! (`idle` then covers extract and the hand-off to the render thread).
//!
//! **What happened.** [`FrameCounters`] counts this frame's events (knights
//! spawned, orbs fired, pieces placed, cracked and broken, sounds started,
//! pipelines compiled) and what is alive (knights, orbs, particles, debris,
//! potions, damage numbers, voices, entities), read from messages, queries and
//! the effect pools without allocating.

use crate::{
    audio::Voice,
    fx::{FxPools, spells::SpellPools},
    gpu_timing::{GpuResults, GpuSample},
    grunt::Grunt,
    hud::DamageNumber,
    orb::Orb,
    shared::{DamageDealt, GameCue, PieceChange, PieceChanged, ShotFired},
    telemetry::process_start,
    waves::PotionSlot,
};
use bevy::{
    app::{FixedMainScheduleOrder, MainScheduleOrder},
    ecs::{entity::Entities, schedule::ScheduleLabel, schedule::SingleThreadedExecutor},
    prelude::*,
    render::{
        Render, RenderApp, RenderScheduleOrder, RenderSystems,
        render_resource::{CachedPipelineState, PipelineCache},
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};

// ---------------------------------------------------------------------------
// Marks
// ---------------------------------------------------------------------------

/// A point in the frame where the clock is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mark {
    /// Before `First`.
    FrameStart,
    /// Before `RunFixedMainLoop`.
    FixedStart,
    /// After `RunFixedMainLoop`.
    FixedEnd,
    /// After `SpawnScene` (so `Update` + `SpawnScene`).
    UpdateEnd,
    /// After `PostUpdate`.
    PostEnd,
    /// After `Last`: the main world is done.
    MainEnd,
    /// Render world, before its `First` (extract is done).
    RenderStart,
    /// Render schedule, before `PrepareViews` (the drawable acquire).
    ViewsStart,
    /// Render schedule, after `PrepareViews`.
    ViewsEnd,
    /// Render schedule, before `RenderSystems::Render`.
    GraphStart,
    /// Render schedule, after `RenderSystems::Render`.
    GraphEnd,
    /// Render world, after `Render` (cleanup and the frame limiter done).
    RenderEnd,
}

impl Mark {
    pub const ALL: [Mark; 12] = [
        Mark::FrameStart,
        Mark::FixedStart,
        Mark::FixedEnd,
        Mark::UpdateEnd,
        Mark::PostEnd,
        Mark::MainEnd,
        Mark::RenderStart,
        Mark::ViewsStart,
        Mark::ViewsEnd,
        Mark::GraphStart,
        Mark::GraphEnd,
        Mark::RenderEnd,
    ];
}

/// The label of a one-system schedule that records one [`Mark`].
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct MarkAt(Mark);

/// Physics timing inside each fixed tick.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
enum TickMark {
    PhysicsStart,
    PhysicsEnd,
}

/// Drawable acquisitions remembered for pipelined rendering.
const ACQUIRE_RING: usize = 8;

/// The frame's clock readings, shared by the main and render worlds (the
/// same `Arc` is a resource in both). Every slot is written by one marker and
/// read in `Last`. Without pipelined rendering all of it happens on one
/// thread; with it, the render world's acquisitions reach the main world
/// through the ring (single writer, single reader).
#[derive(Debug, Default)]
pub struct ProfileSlots {
    marks: [AtomicU64; Mark::ALL.len()],
    /// The previous two `ViewsEnd` readings, for the display cadence.
    views_end_prev: AtomicU64,
    physics_start: AtomicU64,
    physics_ns: AtomicU64,
    ticks: AtomicU32,
    pipelines_ok: AtomicU32,
    pipelines_compiled: AtomicU32,
    /// Pipelined rendering is on: the render parts come from the ring.
    pipelined: AtomicBool,
    /// Each acquisition's end (ns) and its wait (ns), by sequence number.
    acquired_at: [AtomicU64; ACQUIRE_RING],
    acquire_wait: [AtomicU64; ACQUIRE_RING],
    /// Acquisitions written (render world) and read (main world).
    acquired: AtomicU64,
    acquired_read: AtomicU64,
}

/// Shared handle on [`ProfileSlots`].
#[derive(Resource, Debug, Clone, Default)]
pub struct FrameProfile(pub Arc<ProfileSlots>);

/// Nanoseconds since process start (the clock every mark uses).
pub fn now_ns() -> u64 {
    process_start().elapsed().as_nanos() as u64
}

impl FrameProfile {
    /// Records `mark` now.
    pub fn mark(&self, mark: Mark) {
        self.mark_at(mark, now_ns());
    }

    /// Records `mark` at `t_ns` (nanoseconds since process start).
    pub fn mark_at(&self, mark: Mark, t_ns: u64) {
        let slots = &self.0;
        match mark {
            Mark::FrameStart => {
                slots.physics_ns.store(0, Ordering::Relaxed);
                slots.ticks.store(0, Ordering::Relaxed);
            }
            Mark::ViewsEnd => {
                let before = slots.marks[Mark::ViewsEnd as usize].load(Ordering::Relaxed);
                slots.views_end_prev.store(before, Ordering::Relaxed);
                // Into the ring too (read under pipelined rendering).
                let start = slots.marks[Mark::ViewsStart as usize].load(Ordering::Relaxed);
                let n = slots.acquired.load(Ordering::Relaxed);
                let i = (n % ACQUIRE_RING as u64) as usize;
                slots.acquired_at[i].store(t_ns, Ordering::Relaxed);
                slots.acquire_wait[i].store(t_ns.saturating_sub(start), Ordering::Relaxed);
                slots.acquired.store(n + 1, Ordering::Release);
            }
            _ => {}
        }
        slots.marks[mark as usize].store(t_ns, Ordering::Relaxed);
    }

    fn get(&self, mark: Mark) -> u64 {
        self.0.marks[mark as usize].load(Ordering::Relaxed)
    }

    /// Marks the render parts as coming from another thread (pipelined
    /// rendering; see the module docs).
    pub fn set_pipelined(&self, on: bool) {
        self.0.pipelined.store(on, Ordering::Relaxed);
    }

    /// The next drawable acquisition the main world hasn't read yet:
    /// (interval since the one before, its wait), in ms; zeros when none
    /// is new. Each acquisition is read once.
    fn next_acquisition(&self) -> (f32, f32) {
        let slots = &self.0;
        let written = slots.acquired.load(Ordering::Acquire);
        let mut k = slots.acquired_read.load(Ordering::Relaxed);
        if k >= written {
            return (0.0, 0.0);
        }
        // Fell behind by a whole ring (never in lockstep play): skip ahead.
        k = k.max(written.saturating_sub(ACQUIRE_RING as u64 - 1));
        let at = |n: u64| slots.acquired_at[(n % ACQUIRE_RING as u64) as usize].load(Ordering::Relaxed);
        let t = at(k);
        let wait = slots.acquire_wait[(k % ACQUIRE_RING as u64) as usize].load(Ordering::Relaxed);
        let dt = if k > 0 { t.saturating_sub(at(k - 1)) } else { 0 };
        slots.acquired_read.store(k + 1, Ordering::Relaxed);
        (dt as f32 * 1e-6, wait as f32 * 1e-6)
    }

    /// Records the start of one tick's physics step.
    pub fn physics_start(&self, t_ns: u64) {
        self.0.physics_start.store(t_ns, Ordering::Relaxed);
    }

    /// Records the end of one tick's physics step.
    pub fn physics_end(&self, t_ns: u64) {
        let start = self.0.physics_start.load(Ordering::Relaxed);
        self.0
            .physics_ns
            .fetch_add(t_ns.saturating_sub(start), Ordering::Relaxed);
        self.0.ticks.fetch_add(1, Ordering::Relaxed);
    }

    /// Records how many pipelines are compiled now; the difference from the
    /// last reading is this frame's compiles.
    pub fn pipelines_ready(&self, ok: u32) {
        let before = self.0.pipelines_ok.swap(ok, Ordering::Relaxed);
        // The first reading (from zero) is the boot warm-up, not a hitch.
        let compiled = if before == 0 {
            0
        } else {
            ok.saturating_sub(before)
        };
        self.0.pipelines_compiled.store(compiled, Ordering::Relaxed);
    }

    /// The frame that ends at `Last` now, split into its parts: main-world
    /// parts from this frame, render parts from the previous frame's render
    /// (see the module docs). Parts that did not happen (no render world in
    /// headless tests) are zero.
    pub fn cost(&self) -> FrameCost {
        use Mark::*;
        let m = |mark| self.get(mark);
        let span = |from: u64, to: u64| -> f32 {
            if from == 0 || to < from {
                0.0
            } else {
                (to - from) as f32 * 1e-6
            }
        };
        let frame_start = m(FrameStart);
        // The render readings describe the previous frame's render only when
        // they come after the previous main world ended and before this frame
        // began.
        let main_end = m(MainEnd);
        let render_start = m(RenderStart);
        let pipelined = self.0.pipelined.load(Ordering::Relaxed);
        let render_ok = !pipelined
            && render_start >= main_end
            && render_start <= frame_start
            && main_end > 0;
        let r = |mark| if render_ok { m(mark) } else { 0 };
        let render_end = r(RenderEnd);
        let idle_from = if render_ok && render_end > 0 {
            render_end
        } else {
            main_end
        };
        let views_end = r(ViewsEnd);
        let views_prev = if render_ok {
            self.0.views_end_prev.load(Ordering::Relaxed)
        } else {
            0
        };
        let (ring_dt, ring_wait) = if pipelined {
            self.next_acquisition()
        } else {
            (0.0, 0.0)
        };
        FrameCost {
            pre_ms: span(frame_start, m(FixedStart)),
            fixed_ms: span(m(FixedStart), m(FixedEnd)),
            physics_ms: self.0.physics_ns.load(Ordering::Relaxed) as f32 * 1e-6,
            ticks: self.0.ticks.load(Ordering::Relaxed).min(255) as u8,
            update_ms: span(m(FixedEnd), m(UpdateEnd)),
            post_ms: span(m(UpdateEnd), m(PostEnd)),
            extract_ms: if render_ok {
                span(main_end, render_start)
            } else {
                0.0
            },
            prepare_ms: span(render_start, r(ViewsStart)) + span(views_end, r(GraphStart)),
            acquire_ms: if pipelined {
                ring_wait
            } else {
                span(r(ViewsStart), views_end)
            },
            graph_ms: span(r(GraphStart), r(GraphEnd)),
            render_end_ms: span(r(GraphEnd), render_end),
            idle_ms: span(idle_from, frame_start),
            vsync_dt_ms: if pipelined {
                ring_dt
            } else {
                span(views_prev, views_end)
            },
            press_latency_ms: 0.0,
            motion_latency_ms: 0.0,
            gpu: GpuSample::default(),
            counters: FrameCounters {
                pipelines_compiled: self
                    .0
                    .pipelines_compiled
                    .load(Ordering::Relaxed)
                    .min(u16::MAX as u32) as u16,
                ..default()
            },
        }
    }
}

// ---------------------------------------------------------------------------
// The row
// ---------------------------------------------------------------------------

/// What one frame cost and what happened in it. Small and `Copy`: it rides
/// in the session log's per-frame row.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FrameCost {
    pub pre_ms: f32,
    pub fixed_ms: f32,
    pub physics_ms: f32,
    pub ticks: u8,
    pub update_ms: f32,
    pub post_ms: f32,
    pub extract_ms: f32,
    pub prepare_ms: f32,
    pub acquire_ms: f32,
    pub graph_ms: f32,
    pub render_end_ms: f32,
    pub idle_ms: f32,
    pub vsync_dt_ms: f32,
    /// Input-to-present latency samples resolved this frame, for earlier
    /// frames (0 = none; see [`crate::latency`]).
    pub press_latency_ms: f32,
    pub motion_latency_ms: f32,
    /// A GPU timing sample that arrived this frame (for an earlier frame;
    /// `gpu.frame == 0` when none did). Its total is the `gpu_ms` column.
    pub gpu: GpuSample,
    pub counters: FrameCounters,
}

/// The time buckets, in the order the CSV lists them, for attribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bucket {
    Pre,
    Fixed,
    Update,
    Post,
    Extract,
    Prepare,
    Acquire,
    Graph,
    RenderEnd,
    Idle,
}

impl Bucket {
    pub const ALL: [Bucket; 10] = [
        Bucket::Pre,
        Bucket::Fixed,
        Bucket::Update,
        Bucket::Post,
        Bucket::Extract,
        Bucket::Prepare,
        Bucket::Acquire,
        Bucket::Graph,
        Bucket::RenderEnd,
        Bucket::Idle,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Bucket::Pre => "pre",
            Bucket::Fixed => "fixed",
            Bucket::Update => "update",
            Bucket::Post => "post",
            Bucket::Extract => "extract",
            Bucket::Prepare => "prepare",
            Bucket::Acquire => "acquire",
            Bucket::Graph => "graph",
            Bucket::RenderEnd => "render_end",
            Bucket::Idle => "idle",
        }
    }
}

impl FrameCost {
    pub fn bucket(&self, b: Bucket) -> f32 {
        match b {
            Bucket::Pre => self.pre_ms,
            Bucket::Fixed => self.fixed_ms,
            Bucket::Update => self.update_ms,
            Bucket::Post => self.post_ms,
            Bucket::Extract => self.extract_ms,
            Bucket::Prepare => self.prepare_ms,
            Bucket::Acquire => self.acquire_ms,
            Bucket::Graph => self.graph_ms,
            Bucket::RenderEnd => self.render_end_ms,
            Bucket::Idle => self.idle_ms,
        }
    }

    /// CPU time spent on the frame's own work (every bucket but the drawable
    /// wait and the idle gap).
    pub fn work_ms(&self) -> f32 {
        Bucket::ALL
            .iter()
            .filter(|b| !matches!(b, Bucket::Acquire | Bucket::Idle))
            .map(|b| self.bucket(*b))
            .sum()
    }
}

/// This frame's events and what is alive at its end.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameCounters {
    pub knights: u16,
    pub knights_spawned: u16,
    pub orbs: u16,
    pub orbs_fired: u16,
    /// Player gun shots.
    pub shots: u16,
    /// Damage events (characters and pieces).
    pub damage: u16,
    pub placed: u16,
    pub cracked: u16,
    pub broken: u16,
    /// Live effect particles (sparks, dust).
    pub particles: u16,
    /// Live debris chunks.
    pub debris: u16,
    /// Live spell sparks, bolts and glows.
    pub spell_fx: u16,
    pub potions: u16,
    pub damage_numbers: u16,
    pub voices: u16,
    pub voices_started: u16,
    /// Render pipelines that finished compiling this frame (first-use
    /// shader compiles; the boot warm-up is excluded).
    pub pipelines_compiled: u16,
    pub entities: u32,
}

/// The per-frame event counters, in CSV order.
pub const COUNTER_COLUMNS: [&str; 18] = [
    "knights",
    "knights_spawned",
    "orbs",
    "orbs_fired",
    "shots",
    "damage",
    "placed",
    "cracked",
    "broken",
    "particles",
    "debris",
    "spell_fx",
    "potions",
    "damage_numbers",
    "voices",
    "voices_started",
    "pipelines_compiled",
    "entities",
];

impl FrameCounters {
    /// The counters in [`COUNTER_COLUMNS`] order.
    pub fn values(&self) -> [u32; 18] {
        [
            self.knights.into(),
            self.knights_spawned.into(),
            self.orbs.into(),
            self.orbs_fired.into(),
            self.shots.into(),
            self.damage.into(),
            self.placed.into(),
            self.cracked.into(),
            self.broken.into(),
            self.particles.into(),
            self.debris.into(),
            self.spell_fx.into(),
            self.potions.into(),
            self.damage_numbers.into(),
            self.voices.into(),
            self.voices_started.into(),
            self.pipelines_compiled.into(),
            self.entities,
        ]
    }
}

/// The time columns, in CSV order (after `frame,t_ms,dt_ms,state,occluded`).
pub const TIME_COLUMNS: [&str; 15] = [
    "pre_ms",
    "fixed_ms",
    "physics_ms",
    "ticks",
    "update_ms",
    "post_ms",
    "extract_ms",
    "prepare_ms",
    "acquire_ms",
    "graph_ms",
    "render_end_ms",
    "idle_ms",
    "vsync_dt_ms",
    "gpu_ms",
    // Filled with the frame's own work (every bucket but acquire and idle).
    "work_ms",
];

// ---------------------------------------------------------------------------
// The plugin
// ---------------------------------------------------------------------------

/// The frame's counters, gathered in `Last` in [`ProfileSystems`].
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct LastCounters(pub FrameCounters);

/// The GPU timing sample that arrived this frame, if any (see
/// [`crate::gpu_timing`]).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct LastGpu(pub GpuSample);

/// The counter gathering in `Last`; readers of [`LastCounters`] run after.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProfileSystems;

/// Adds the frame markers (main, fixed and render worlds) and the counters.
/// Works headless (no render world: the render parts stay zero).
pub struct FrameProfilePlugin;

fn mark_system(mark: Mark) -> impl FnMut(Res<FrameProfile>) + Send + Sync + 'static {
    move |profile: Res<FrameProfile>| profile.mark(mark)
}

impl Plugin for FrameProfilePlugin {
    fn build(&self, app: &mut App) {
        let profile = FrameProfile::default();
        app.insert_resource(profile.clone())
            .init_resource::<LastCounters>()
            .init_resource::<LastGpu>();
        use bevy::app::{RunFixedMainLoop, SpawnScene};
        for mark in [
            Mark::FrameStart,
            Mark::FixedStart,
            Mark::FixedEnd,
            Mark::UpdateEnd,
            Mark::PostEnd,
            Mark::MainEnd,
        ] {
            add_mark_schedule(app.world_mut(), MarkAt(mark), mark_system(mark));
        }
        {
            let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
            order.insert_before(First, MarkAt(Mark::FrameStart));
            order.insert_before(RunFixedMainLoop, MarkAt(Mark::FixedStart));
            order.insert_after(RunFixedMainLoop, MarkAt(Mark::FixedEnd));
            order.insert_after(SpawnScene, MarkAt(Mark::UpdateEnd));
            order.insert_after(PostUpdate, MarkAt(Mark::PostEnd));
            order.insert_after(Last, MarkAt(Mark::MainEnd));
        }
        add_mark_schedule(
            app.world_mut(),
            TickMark::PhysicsStart,
            |p: Res<FrameProfile>| p.physics_start(now_ns()),
        );
        add_mark_schedule(
            app.world_mut(),
            TickMark::PhysicsEnd,
            |p: Res<FrameProfile>| p.physics_end(now_ns()),
        );
        {
            let mut fixed = app.world_mut().resource_mut::<FixedMainScheduleOrder>();
            fixed.insert_before(FixedPostUpdate, TickMark::PhysicsStart);
            fixed.insert_after(FixedPostUpdate, TickMark::PhysicsEnd);
        }
        app.add_systems(Last, gather_counters.in_set(ProfileSystems));

        // Under pipelined rendering (the default) the render world runs
        // beside the next frame's main world, so its marks can't split the
        // interval: the acquisitions go through the ring instead (see the
        // module docs).
        let pipelined =
            app.is_plugin_added::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>();
        profile.set_pipelined(pipelined);
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.insert_resource(profile);
            render_app.add_systems(
                Render,
                count_pipelines
                    .after(RenderSystems::Render)
                    .before(RenderSystems::Cleanup),
            );
            let world = render_app.world_mut();
            add_mark_schedule(
                world,
                MarkAt(Mark::RenderStart),
                mark_system(Mark::RenderStart),
            );
            add_mark_schedule(world, MarkAt(Mark::RenderEnd), mark_system(Mark::RenderEnd));
            if let Some(mut order) = world.get_resource_mut::<RenderScheduleOrder>() {
                order.labels.insert(0, MarkAt(Mark::RenderStart).intern());
                order.labels.push(MarkAt(Mark::RenderEnd).intern());
            }
            render_app.add_systems(
                Render,
                (
                    mark_system(Mark::ViewsStart)
                        .after(RenderSystems::Specialize)
                        .before(RenderSystems::PrepareViews),
                    mark_system(Mark::ViewsEnd)
                        .after(RenderSystems::PrepareViews)
                        .before(RenderSystems::Queue),
                    mark_system(Mark::GraphStart)
                        .after(RenderSystems::Prepare)
                        .before(RenderSystems::Render),
                    mark_system(Mark::GraphEnd)
                        .after(RenderSystems::Render)
                        .before(RenderSystems::Cleanup),
                ),
            );
        }
    }
}

/// Adds a one-system, single-threaded schedule (no task-pool round trip).
fn add_mark_schedule<M>(
    world: &mut World,
    label: impl ScheduleLabel + Clone,
    system: impl IntoScheduleConfigs<bevy::ecs::system::ScheduleSystem, M>,
) {
    let mut schedule = Schedule::new(label);
    schedule.set_executor(SingleThreadedExecutor::new());
    schedule.add_systems(system);
    world.add_schedule(schedule);
}

/// Counts compiled pipelines (a few hundred entries, no allocation).
fn count_pipelines(profile: Res<FrameProfile>, cache: Option<Res<PipelineCache>>) {
    let Some(cache) = cache else { return };
    let ok = cache
        .pipelines()
        .filter(|p| matches!(p.state, CachedPipelineState::Ok(_)))
        .count();
    profile.pipelines_ready(ok as u32);
}

fn clamp16(n: usize) -> u16 {
    n.min(u16::MAX as usize) as u16
}

/// The live things [`gather_counters`] counts.
type LiveQueries<'w, 's> = (
    Query<'w, 's, (), With<Grunt>>,
    Query<'w, 's, (), Added<Grunt>>,
    Query<'w, 's, (), With<Orb>>,
    Query<'w, 's, &'static PotionSlot>,
    Query<'w, 's, &'static DamageNumber>,
    Query<'w, 's, (), With<Voice>>,
    Query<'w, 's, (), Added<Voice>>,
);

#[allow(clippy::too_many_arguments)]
fn gather_counters(
    mut out: ResMut<LastCounters>,
    mut gpu_out: ResMut<LastGpu>,
    (grunts, new_grunts, orbs, potions, numbers, voices, new_voices): LiveQueries,
    mut cues: MessageReader<GameCue>,
    mut shots: MessageReader<ShotFired>,
    mut damage: MessageReader<DamageDealt>,
    mut pieces: MessageReader<PieceChanged>,
    fx: Option<Res<FxPools>>,
    spells: Option<Res<SpellPools>>,
    entities: &Entities,
    gpu: Option<Res<GpuResults>>,
) {
    let mut c = FrameCounters {
        knights: clamp16(grunts.iter().count()),
        knights_spawned: clamp16(new_grunts.iter().count()),
        orbs: clamp16(orbs.iter().count()),
        orbs_fired: clamp16(
            cues.read()
                .filter(|c| matches!(c, GameCue::OrbFired { .. }))
                .count(),
        ),
        shots: clamp16(shots.read().count()),
        damage: clamp16(damage.read().count()),
        potions: clamp16(potions.iter().filter(|p| p.live.is_some()).count()),
        damage_numbers: clamp16(numbers.iter().filter(|n| n.active).count()),
        voices: clamp16(voices.iter().count()),
        voices_started: clamp16(new_voices.iter().count()),
        entities: entities.count_spawned(),
        ..default()
    };
    for p in pieces.read() {
        match p.change {
            PieceChange::Placed => c.placed += 1,
            PieceChange::Cracked(_) => c.cracked += 1,
            PieceChange::Destroyed => c.broken += 1,
        }
    }
    if let Some(fx) = fx {
        let (particles, debris) = fx.live();
        c.particles = clamp16(particles);
        c.debris = clamp16(debris);
    }
    if let Some(spells) = spells {
        let k = spells.counts();
        c.spell_fx = clamp16(k.glow.0 + k.solid.0 + k.bolts.0 + k.halos.0);
    }
    out.0 = c;
    gpu_out.0 = gpu.and_then(|g| g.take()).unwrap_or_default();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_splits_the_interval_by_marks() {
        let p = FrameProfile::default();
        let ms = |v: f64| (v * 1e6) as u64;
        // Previous frame: main ended at 100, render 100.5..112.
        p.mark_at(Mark::MainEnd, ms(100.0));
        p.mark_at(Mark::RenderStart, ms(100.5));
        p.mark_at(Mark::ViewsStart, ms(101.5));
        p.mark_at(Mark::ViewsEnd, ms(108.0));
        p.mark_at(Mark::GraphStart, ms(108.5));
        p.mark_at(Mark::GraphEnd, ms(111.5));
        p.mark_at(Mark::RenderEnd, ms(112.0));
        // This frame's main world.
        p.mark_at(Mark::FrameStart, ms(112.25));
        p.mark_at(Mark::FixedStart, ms(113.0));
        p.physics_start(ms(113.5));
        p.physics_end(ms(114.0));
        p.physics_start(ms(114.5));
        p.physics_end(ms(115.0));
        p.mark_at(Mark::FixedEnd, ms(115.5));
        p.mark_at(Mark::UpdateEnd, ms(117.5));
        p.mark_at(Mark::PostEnd, ms(118.5));
        let c = p.cost();
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
        assert!(near(c.pre_ms, 0.75), "{c:?}");
        assert!(near(c.fixed_ms, 2.5));
        assert!(near(c.physics_ms, 1.0));
        assert_eq!(c.ticks, 2);
        assert!(near(c.update_ms, 2.0));
        assert!(near(c.post_ms, 1.0));
        assert!(near(c.extract_ms, 0.5));
        assert!(near(c.prepare_ms, 1.5));
        assert!(near(c.acquire_ms, 6.5));
        assert!(near(c.graph_ms, 3.0));
        assert!(near(c.render_end_ms, 0.5));
        assert!(near(c.idle_ms, 0.25));
        // The first acquisition has no previous one.
        assert!(near(c.vsync_dt_ms, 0.0));
        assert!(near(
            c.work_ms(),
            0.75 + 2.5 + 2.0 + 1.0 + 0.5 + 1.5 + 3.0 + 0.5
        ));
    }

    #[test]
    fn render_parts_are_zero_without_a_render_world() {
        let p = FrameProfile::default();
        p.mark_at(Mark::MainEnd, 5_000_000);
        p.mark_at(Mark::FrameStart, 6_000_000);
        p.mark_at(Mark::FixedStart, 6_500_000);
        let c = p.cost();
        assert_eq!(c.acquire_ms, 0.0);
        assert_eq!(c.extract_ms, 0.0);
        assert_eq!(c.graph_ms, 0.0);
        assert!((c.idle_ms - 1.0).abs() < 1e-4);
        assert!((c.pre_ms - 0.5).abs() < 1e-4);
    }

    #[test]
    fn pipelines_compiled_counts_new_pipelines_after_the_first_reading() {
        let p = FrameProfile::default();
        p.pipelines_ready(120);
        assert_eq!(p.cost().counters.pipelines_compiled, 0);
        p.pipelines_ready(120);
        assert_eq!(p.cost().counters.pipelines_compiled, 0);
        p.pipelines_ready(123);
        assert_eq!(p.cost().counters.pipelines_compiled, 3);
    }

    #[test]
    fn pipelined_rows_take_each_acquisition_once_in_order() {
        let p = FrameProfile::default();
        p.set_pipelined(true);
        let ms = |v: f64| (v * 1e6) as u64;
        // Two acquisitions land before the first row, none before the second.
        p.mark_at(Mark::ViewsStart, ms(8.0));
        p.mark_at(Mark::ViewsEnd, ms(10.0));
        p.mark_at(Mark::ViewsStart, ms(22.0));
        p.mark_at(Mark::ViewsEnd, ms(26.7));
        let first = p.cost();
        assert_eq!(first.vsync_dt_ms, 0.0, "the first has no previous one");
        assert!((first.acquire_ms - 2.0).abs() < 1e-3, "{first:?}");
        let second = p.cost();
        assert!((second.vsync_dt_ms - 16.7).abs() < 1e-3, "{second:?}");
        assert!((second.acquire_ms - 4.7).abs() < 1e-3);
        let third = p.cost();
        assert_eq!((third.vsync_dt_ms, third.acquire_ms), (0.0, 0.0), "nothing new");
        p.mark_at(Mark::ViewsStart, ms(40.0));
        p.mark_at(Mark::ViewsEnd, ms(43.4));
        assert!((p.cost().vsync_dt_ms - 16.7).abs() < 1e-3);
        // The serial render buckets stay zero: they can't split the interval.
        assert_eq!(p.cost().graph_ms, 0.0);
    }

    #[test]
    fn vsync_cadence_is_the_gap_between_acquisitions() {
        let p = FrameProfile::default();
        let ms = |v: f64| (v * 1e6) as u64;
        for (i, t) in [10.0, 26.7].iter().enumerate() {
            p.mark_at(Mark::MainEnd, ms(t - 2.0));
            p.mark_at(Mark::RenderStart, ms(t - 1.5));
            p.mark_at(Mark::ViewsStart, ms(t - 1.0));
            p.mark_at(Mark::ViewsEnd, ms(*t));
            p.mark_at(Mark::RenderEnd, ms(t + 1.0));
            p.mark_at(Mark::FrameStart, ms(t + 1.5));
            let c = p.cost();
            if i == 1 {
                assert!((c.vsync_dt_ms - 16.7).abs() < 1e-3, "{c:?}");
            }
        }
    }
}
