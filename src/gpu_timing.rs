//! Per-pass GPU timing for the session log (M4 chunk 0, D99).
//!
//! **Why not Bevy's `RenderDiagnosticsPlugin`.** Bevy times a pass by writing
//! timestamps *inside* it, which Apple-silicon GPUs cannot do (Metal only
//! samples counters at stage boundaries: wgpu reports `TIMESTAMP_QUERY` but not
//! `TIMESTAMP_QUERY_INSIDE_PASSES`), and Bevy skips encoder-level timestamps
//! on macOS. So on Jake's M4 it only ever produced CPU times, and the M3
//! `gpu=on` knob logged nothing.
//!
//! **How this works.** Bevy 0.19 runs each camera's render graph as a
//! schedule (`Core3d` for the world and viewmodel cameras, `Core2d` for the UI
//! camera) whose passes are systems, each encoding its own command buffer. We
//! add *mark* systems between the passes. A mark is a one-thread compute pass
//! whose end-of-pass timestamp is a stage-boundary sample, which Metal supports.
//! Apple GPUs overlap neighbouring passes when nothing ties them together, so a
//! mark first copies one texel of the texture the previous pass wrote into a
//! tiny scratch buffer, and its compute reads that buffer: Metal's hazard
//! tracking then holds the mark until the pass it follows has finished. Every
//! mark also writes the scratch buffer, so marks never reorder among
//! themselves. A camera's last mark depends on its output image; the UI
//! camera's last pass writes the window's drawable, which cannot be read, so
//! its mark first writes one texel into the UI camera's main texture (which
//! that pass reads), waiting for the read to finish.
//!
//! | mark | after | depends on |
//! |---|---|---|
//! | `FarStart`, `MainStart` | nothing (the frame's first camera begins) | a write to its output image, so it waits for the previous frame's last read of it |
//! | `*Opaque` | `main_opaque_pass_3d` | the camera's main texture |
//! | `*Main` | the main (opaque + transparent) passes | the main texture |
//! | `*End` | `upscaling` into the camera's output | the output image |
//! | `UiPass` | `ui_pass` | the UI main texture |
//! | `UiEnd` | `upscaling` into the drawable | a write to the UI main texture |
//!
//! **Passes** ([`GpuPass`]), as the budget names them:
//!
//! - `world` = world camera start → after its opaque pass, plus the world
//!   camera's end → the viewmodel's opaque pass (the viewmodel camera starts
//!   where the world camera ends, which saves a mark): the toon world, props, pieces, knights, **the ink
//!   outline hulls** (they draw in the opaque pass), **the far layer** unless
//!   it has its own camera (`farres=half`), the skybox, and the gun.
//! - `outlines`: never split. The hull outlines are extra draws inside the
//!   opaque pass, and Metal cannot time inside a pass, so the column stays
//!   empty and their cost is part of `world` (measure it with
//!   `--knobs outline=off`).
//! - `far` = the far camera, only with `farres=half` (otherwise inside `world`).
//! - `effects` = after opaque → after the main passes: the transparent phase
//!   (halos, particles, spell bursts, ghosts), on both 3D cameras.
//! - `ui` = the viewmodel's end → after `ui_pass`: the HUD and the full-screen
//!   composite of the 3D image at native resolution.
//! - `post` = FXAA, sharpening and each camera's upscale/copy into its output,
//!   plus the UI camera's final copy into the drawable.
//! - `total` = first mark → last mark, including the gaps between cameras.
//!
//! **Cost and sampling.** A full frame has 9 marks (11 with the far camera):
//! each is a 1-texel copy and a 1-thread dispatch, but it stops the GPU
//! overlapping the passes it separates. Measured offscreen on the M4
//! (`tests/gpu_timing_offscreen.rs`), back-to-back marks are 0.07 ms apart
//! without a copy and 0.11–0.16 ms with one: about 1 ms per full frame, well
//! over the spec's 0.3 ms for always-on timing. Only 1 frame in [`PerfTuning::gpu_every`](crate::perf::PerfTuning)
//! is timed (default 8; `--knobs gpu=on` times every frame, `gpu=off` none).
//! Every [`PerfTuning::gpu_bare_every`](crate::perf::PerfTuning)-th timed frame
//! is *bare*: only the first and last marks, so `session.json` can report the
//! timing's own cost as the mean full total minus the mean bare total.
//!
//! Results come back a few frames later ([`GpuSample::frame`] says which frame
//! was measured) through [`GpuResults`], and ride in the session log's row of
//! the frame they arrive on (see [`crate::session`]).

use crate::{
    render::{FAR_CAMERA_ORDER, MAIN_CAMERA_ORDER, UI_CAMERA_ORDER, VIEWMODEL_CAMERA_ORDER},
    telemetry::MainFrame,
    tuning::Tuning,
};
use bevy::{
    camera::NormalizedRenderTarget,
    core_pipeline::{
        core_3d::{main_opaque_pass_3d, main_transparent_pass_3d},
        schedule::{Core2d, Core2dSystems, Core3d, Core3dSystems},
        upscaling::upscaling,
    },
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        camera::ExtractedCamera,
        render_asset::RenderAssets,
        renderer::{
            RenderContext, RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue, ViewQuery,
        },
        texture::GpuImage,
        view::ViewTarget,
    },
    ui_render::ui_pass,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};

// ---------------------------------------------------------------------------
// Passes and marks (pure)
// ---------------------------------------------------------------------------

/// The budget's passes, in `frames.csv` order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuPass {
    World,
    Outlines,
    Far,
    Effects,
    Ui,
    Post,
}

impl GpuPass {
    pub const ALL: [GpuPass; 6] = [
        GpuPass::World,
        GpuPass::Outlines,
        GpuPass::Far,
        GpuPass::Effects,
        GpuPass::Ui,
        GpuPass::Post,
    ];

    pub fn name(self) -> &'static str {
        match self {
            GpuPass::World => "world",
            GpuPass::Outlines => "outlines",
            GpuPass::Far => "far",
            GpuPass::Effects => "effects",
            GpuPass::Ui => "ui",
            GpuPass::Post => "post",
        }
    }
}

/// The GPU columns of `frames.csv`, after the counters. `gpu_ms` (the total)
/// keeps its M3 place among the time columns.
pub const GPU_COLUMNS: [&str; 8] = [
    "gpu_frame",
    "gpu_full",
    "gpu_world_ms",
    "gpu_outlines_ms",
    "gpu_far_ms",
    "gpu_effects_ms",
    "gpu_ui_ms",
    "gpu_post_ms",
];

/// Where a mark sits in the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum GpuMark {
    FarStart,
    FarEnd,
    MainStart,
    MainOpaque,
    MainMain,
    MainEnd,
    ViewOpaque,
    ViewMain,
    ViewEnd,
    UiPass,
    UiEnd,
}

/// Marks per timed frame (at most).
pub const MARKS: usize = 11;

impl GpuMark {
    pub const ALL: [GpuMark; MARKS] = [
        GpuMark::FarStart,
        GpuMark::FarEnd,
        GpuMark::MainStart,
        GpuMark::MainOpaque,
        GpuMark::MainMain,
        GpuMark::MainEnd,
        GpuMark::ViewOpaque,
        GpuMark::ViewMain,
        GpuMark::ViewEnd,
        GpuMark::UiPass,
        GpuMark::UiEnd,
    ];

    /// Marks a bare frame writes: the first and last of a frame.
    pub fn in_bare_frame(self) -> bool {
        matches!(
            self,
            GpuMark::FarStart | GpuMark::MainStart | GpuMark::UiEnd
        )
    }
}

/// Which camera a view is, from its order ([`crate::render`]'s constants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraKind {
    Far,
    Main,
    Viewmodel,
    Ui,
}

impl CameraKind {
    pub fn of_order(order: isize) -> Option<Self> {
        match order {
            FAR_CAMERA_ORDER => Some(Self::Far),
            MAIN_CAMERA_ORDER => Some(Self::Main),
            VIEWMODEL_CAMERA_ORDER => Some(Self::Viewmodel),
            UI_CAMERA_ORDER => Some(Self::Ui),
            _ => None,
        }
    }
}

/// Where a mark system sits in a camera's schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkAt {
    Start,
    /// After the opaque pass (3D cameras only).
    Opaque,
    /// After every main pass (3D), or after `ui_pass` (the UI camera).
    Main,
    End,
}

impl MarkAt {
    const fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Start,
            1 => Self::Opaque,
            2 => Self::Main,
            _ => Self::End,
        }
    }
}

/// The mark a camera writes at a point, if it writes one there.
pub fn mark_for(kind: CameraKind, at: MarkAt) -> Option<GpuMark> {
    use {CameraKind as C, GpuMark as M, MarkAt as A};
    Some(match (kind, at) {
        (C::Far, A::Start) => M::FarStart,
        (C::Far, A::End) => M::FarEnd,
        (C::Far, _) => return None,
        (C::Main, A::Start) => M::MainStart,
        (C::Main, A::Opaque) => M::MainOpaque,
        (C::Main, A::Main) => M::MainMain,
        (C::Main, A::End) => M::MainEnd,
        // A camera that follows another starts where the previous one's
        // end mark is: one mark fewer (each costs ~0.1 ms of GPU).
        (C::Viewmodel, A::Start) => return None,
        (C::Viewmodel, A::Opaque) => M::ViewOpaque,
        (C::Viewmodel, A::Main) => M::ViewMain,
        (C::Viewmodel, A::End) => M::ViewEnd,
        (C::Ui, A::Start) => return None,
        (C::Ui, A::Opaque) => return None,
        (C::Ui, A::Main) => M::UiPass,
        (C::Ui, A::End) => M::UiEnd,
    })
}

/// One timed frame's GPU time per pass (milliseconds). `frame == 0` means no
/// sample.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GpuSample {
    /// The main-world frame that was measured.
    pub frame: u64,
    /// Every mark was written (a bare frame has only the total).
    pub full: bool,
    pub total_ms: Option<f32>,
    /// In [`GpuPass::ALL`] order.
    pub passes: [Option<f32>; 6],
}

impl GpuSample {
    pub fn is_some(&self) -> bool {
        self.frame != 0
    }

    pub fn pass(&self, pass: GpuPass) -> Option<f32> {
        self.passes[pass as usize]
    }
}

/// Builds a sample from the marks' GPU timestamps (`ticks[mark]`, `None` when
/// the mark was not written) and the timestamp period (ns per tick).
pub fn sample_from_marks(
    frame: u64,
    full: bool,
    ticks: &[Option<u64>; MARKS],
    period_ns: f32,
) -> GpuSample {
    use GpuMark as M;
    // Marks run in `GpuMark::ALL` order: one that is missing, zero or earlier
    // than the one before it (a stale value) is dropped.
    let mut clean = [None; MARKS];
    let mut previous = 0;
    for (out, tick) in clean.iter_mut().zip(ticks) {
        if let Some(t) = tick.filter(|&t| t > 0 && t >= previous) {
            *out = Some(t);
            previous = t;
        }
    }
    let t = |m: M| clean[m as usize];
    let ms = |ticks: u64| (ticks as f64 * f64::from(period_ns) / 1.0e6) as f32;
    let span = |a: M, b: M| match (t(a), t(b)) {
        (Some(a), Some(b)) => Some(ms(b - a)),
        _ => None,
    };
    let sum = |spans: &[Option<f32>]| {
        spans
            .iter()
            .flatten()
            .fold(None, |acc: Option<f32>, v| Some(acc.unwrap_or(0.0) + v))
    };
    let mut passes = [None; 6];
    if full {
        passes[GpuPass::World as usize] = sum(&[
            span(M::MainStart, M::MainOpaque),
            span(M::MainEnd, M::ViewOpaque),
        ]);
        passes[GpuPass::Far as usize] = span(M::FarStart, M::FarEnd);
        passes[GpuPass::Effects as usize] = sum(&[
            span(M::MainOpaque, M::MainMain),
            span(M::ViewOpaque, M::ViewMain),
        ]);
        passes[GpuPass::Ui as usize] = span(M::ViewEnd, M::UiPass);
        passes[GpuPass::Post as usize] = sum(&[
            span(M::MainMain, M::MainEnd),
            span(M::ViewMain, M::ViewEnd),
            span(M::UiPass, M::UiEnd),
        ]);
    }
    let first = clean.iter().flatten().next();
    let last = clean.iter().flatten().last();
    let total_ms = match (first, last) {
        (Some(a), Some(b)) if b > a => Some(ms(b - a)),
        _ => None,
    };
    GpuSample {
        frame,
        full,
        total_ms,
        passes,
    }
}

/// Which frames are timed: every `every`-th (0 = none), and of those every
/// `bare_every`-th is bare (0 = never).
pub fn timed(frame: u64, every: u32, bare_every: u32) -> Option<bool> {
    if every == 0 || !frame.is_multiple_of(u64::from(every)) {
        return None;
    }
    let index = frame / u64::from(every);
    let bare = bare_every > 0 && index.is_multiple_of(u64::from(bare_every));
    Some(!bare)
}

// ---------------------------------------------------------------------------
// Shared with the main world
// ---------------------------------------------------------------------------

/// The newest finished sample, handed from the render world to the main world
/// (the same `Arc` is a resource in both). The main world takes it once, so
/// each sample appears in exactly one `frames.csv` row.
#[derive(Resource, Debug, Clone, Default)]
pub struct GpuResults(Arc<Mutex<Option<GpuSample>>>);

impl GpuResults {
    pub fn publish(&self, sample: GpuSample) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(sample);
        }
    }

    pub fn take(&self) -> Option<GpuSample> {
        self.0.lock().ok().and_then(|mut s| s.take())
    }
}

// ---------------------------------------------------------------------------
// The plugin
// ---------------------------------------------------------------------------

/// Adds the marks, the readback and [`GpuResults`]. Does nothing without a
/// render world or where the GPU has no timestamp queries.
pub struct GpuTimingPlugin;

impl Plugin for GpuTimingPlugin {
    fn build(&self, app: &mut App) {
        let results = GpuResults::default();
        app.insert_resource(results.clone());
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .insert_resource(results)
            .init_resource::<TimingFrame>()
            .add_systems(ExtractSchedule, extract_timing_frame)
            .add_systems(
                Render,
                (
                    begin_timed_frame.before(RenderSystems::Render),
                    map_timed_frame
                        .after(RenderSystems::Render)
                        .before(RenderSystems::Cleanup),
                ),
            )
            .add_systems(
                RenderGraph,
                resolve_timed_frame
                    .after(RenderGraphSystems::Render)
                    .before(RenderGraphSystems::Submit),
            )
            .add_systems(
                Core3d,
                (
                    mark_3d::<0>.before(Core3dSystems::Prepass),
                    mark_3d::<1>
                        .after(main_opaque_pass_3d)
                        .before(main_transparent_pass_3d),
                    mark_3d::<2>
                        .after(Core3dSystems::MainPass)
                        .before(Core3dSystems::EarlyPostProcess),
                    mark_3d::<3>.after(upscaling),
                ),
            )
            .add_systems(
                Core2d,
                (
                    mark_2d::<0>.before(Core2dSystems::Prepass),
                    mark_2d::<2>.after(ui_pass).before(upscaling),
                    mark_2d::<3>.after(upscaling),
                ),
            );
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        let world = render_app.world_mut();
        let Some(device) = world.get_resource::<RenderDevice>() else {
            return;
        };
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            info!(
                "GPU timing: no timestamp queries on this GPU; frames.csv GPU columns stay empty"
            );
            return;
        }
        let period_ns = world.resource::<RenderQueue>().get_timestamp_period();
        let timer = GpuTimer::new(device.wgpu_device(), period_ns);
        world.insert_resource(timer);
    }
}

/// This frame's timing request, from the main world.
#[derive(Resource, Debug, Clone, Copy, Default)]
struct TimingFrame {
    frame: u64,
    every: u32,
    bare_every: u32,
}

fn extract_timing_frame(
    mut out: ResMut<TimingFrame>,
    frame: Extract<Option<Res<MainFrame>>>,
    tuning: Extract<Option<Res<Tuning>>>,
) {
    let perf = tuning
        .as_deref()
        .map(|t| t.perf.clone())
        .unwrap_or_default();
    *out = TimingFrame {
        frame: frame.as_deref().map_or(0, |f| f.0),
        every: perf.gpu_every,
        bare_every: perf.gpu_bare_every,
    };
}

/// Timing slots. A timed frame's slot waits for its GPU work to finish,
/// then is resolved in a later frame and read back: about four frames with
/// `gpu=on` (every frame timed), so six slots; a frame is only timed when a
/// slot is free.
const SLOTS: usize = 6;
/// Bytes per slot in the resolve buffer (`resolve_query_set` offsets must be
/// multiples of 256).
const SLOT_BYTES: u64 = 256;
/// Where the marks' texel copies land in the scratch buffer (a copy's buffer
/// offset must be a multiple of the texel size; 256 suits every format).
const TEXEL_OFFSET: u64 = 256;
const SCRATCH_BYTES: u64 = 512;

const MARK_SHADER: &str = r"
@group(0) @binding(0) var<storage, read_write> scratch: array<u32, 128>;

@compute @workgroup_size(1)
fn mark() {
    // Reads the copied texel (index 64 = byte 256) and writes the head, so the
    // pass depends on the copy and every later mark depends on this one.
    scratch[0] = scratch[0] + scratch[64] + 1u;
}
";

/// A slot's life: `FREE` → `RECORDING` (its frame's marks) → `SUBMITTED`
/// (waiting for that frame's GPU work) → `DONE` → `RESOLVING` (the resolve
/// is encoded in a later frame: Metal writes a pass's end timestamp after the
/// pass, untracked, so resolving in the same frame can read a stale value)
/// → `IN_FLIGHT` (mapping) → `MAPPED` (or `FAILED`) → read → `FREE`.
const FREE: u8 = 0;
const RECORDING: u8 = 1;
const SUBMITTED: u8 = 2;
const DONE: u8 = 3;
const RESOLVING: u8 = 4;
const IN_FLIGHT: u8 = 5;
const MAPPED: u8 = 6;
const FAILED: u8 = 7;

struct Slot {
    readback: wgpu::Buffer,
    /// Shared with the wgpu callbacks.
    state: Arc<AtomicU8>,
    frame: u64,
    full: bool,
    /// Bit per [`GpuMark`] written in its frame.
    written: u16,
}

impl Slot {
    fn state(&self) -> u8 {
        self.state.load(Ordering::Acquire)
    }

    fn set(&self, state: u8) {
        self.state.store(state, Ordering::Release);
    }
}

/// The render world's timing state (present only with timestamp queries).
#[derive(Resource)]
struct GpuTimer {
    query_set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    scratch: wgpu::Buffer,
    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
    slots: [Slot; SLOTS],
    period_ns: f32,
    /// The slot recording this frame, if it is timed.
    current: Option<usize>,
}

const SLOT_LABELS: [&str; SLOTS] = [
    "pieced_gpu_timing_0",
    "pieced_gpu_timing_1",
    "pieced_gpu_timing_2",
    "pieced_gpu_timing_3",
    "pieced_gpu_timing_4",
    "pieced_gpu_timing_5",
];

impl GpuTimer {
    fn new(device: &wgpu::Device, period_ns: f32) -> Self {
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("pieced_gpu_timing"),
            ty: wgpu::QueryType::Timestamp,
            count: (MARKS * SLOTS) as u32,
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pieced_gpu_timing_resolve"),
            size: SLOT_BYTES * SLOTS as u64,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let scratch = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pieced_gpu_timing_scratch"),
            size: SCRATCH_BYTES,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pieced_gpu_timing_mark"),
            source: wgpu::ShaderSource::Wgsl(MARK_SHADER.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("pieced_gpu_timing_mark"),
            layout: None,
            module: &module,
            entry_point: Some("mark"),
            compilation_options: Default::default(),
            cache: None,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pieced_gpu_timing_mark"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: scratch.as_entire_binding(),
            }],
        });
        let slots = std::array::from_fn(|i| Slot {
            readback: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(SLOT_LABELS[i]),
                size: SLOT_BYTES,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            state: Arc::new(AtomicU8::new(FREE)),
            frame: 0,
            full: false,
            written: 0,
        });
        Self {
            query_set,
            resolve,
            scratch,
            pipeline,
            bind_group,
            slots,
            period_ns,
            current: None,
        }
    }

    /// The query index for `mark` in the current slot, if this frame is timed
    /// and the mark belongs in it.
    fn index(&self, mark: GpuMark) -> Option<u32> {
        let slot = self.current?;
        if !self.slots[slot].full && !mark.in_bare_frame() {
            return None;
        }
        Some((slot * MARKS + mark as usize) as u32)
    }

    /// Reads a mapped slot's timestamps into a sample and frees the slot.
    fn read(&self, i: usize) -> GpuSample {
        let slot = &self.slots[i];
        let mut ticks = [None; MARKS];
        {
            let data = slot.readback.slice(..).get_mapped_range();
            for (m, tick) in ticks.iter_mut().enumerate() {
                if slot.written & (1 << m) != 0 {
                    let bytes: [u8; 8] = data[m * 8..m * 8 + 8].try_into().unwrap_or([0; 8]);
                    *tick = Some(u64::from_le_bytes(bytes));
                }
            }
        }
        slot.readback.unmap();
        slot.set(FREE);
        sample_from_marks(slot.frame, slot.full, &ticks, self.period_ns)
    }
}

/// Reads back finished slots and decides whether this frame is timed.
fn begin_timed_frame(
    timer: Option<ResMut<GpuTimer>>,
    request: Res<TimingFrame>,
    results: Res<GpuResults>,
) {
    let Some(mut timer) = timer else { return };
    for i in 0..SLOTS {
        match timer.slots[i].state() {
            MAPPED => {
                let sample = timer.read(i);
                if sample.total_ms.is_some() {
                    results.publish(sample);
                }
            }
            FAILED => timer.slots[i].set(FREE),
            _ => {}
        }
    }
    timer.current = None;
    let Some(full) = timed(request.frame, request.every, request.bare_every) else {
        return;
    };
    let Some(i) = (0..SLOTS).find(|&i| timer.slots[i].state() == FREE) else {
        return;
    };
    let slot = &mut timer.slots[i];
    slot.set(RECORDING);
    slot.frame = request.frame;
    slot.full = full;
    slot.written = 0;
    timer.current = Some(i);
}

/// What a mark waits for before it samples the clock.
enum Dependency<'a> {
    None,
    /// Copy one texel of this texture (the previous pass wrote it).
    Read(&'a wgpu::Texture),
    /// Write one texel into this texture (the previous pass read it).
    Write(&'a wgpu::Texture),
}

fn write_mark(
    timer: &mut GpuTimer,
    encoder: &mut wgpu::CommandEncoder,
    mark: GpuMark,
    dependency: Dependency,
) {
    let Some(index) = timer.index(mark) else {
        return;
    };
    let one = wgpu::Extent3d {
        width: 1,
        height: 1,
        depth_or_array_layers: 1,
    };
    let texel = |texture| wgpu::TexelCopyTextureInfo {
        texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
    };
    let copyable = |t: &wgpu::Texture, usage| {
        t.usage().contains(usage)
            && t.sample_count() == 1
            && t.format()
                .block_copy_size(None)
                .is_some_and(|n| u64::from(n) <= TEXEL_OFFSET)
    };
    match dependency {
        Dependency::None => {}
        Dependency::Read(texture) => {
            if copyable(texture, wgpu::TextureUsages::COPY_SRC) {
                encoder.copy_texture_to_buffer(
                    texel(texture),
                    wgpu::TexelCopyBufferInfo {
                        buffer: &timer.scratch,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: TEXEL_OFFSET,
                            bytes_per_row: None,
                            rows_per_image: None,
                        },
                    },
                    one,
                );
            }
        }
        Dependency::Write(texture) => {
            if copyable(texture, wgpu::TextureUsages::COPY_DST) {
                encoder.copy_buffer_to_texture(
                    wgpu::TexelCopyBufferInfo {
                        buffer: &timer.scratch,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: None,
                            rows_per_image: None,
                        },
                    },
                    texel(texture),
                    one,
                );
            }
        }
    }
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("pieced_gpu_mark"),
            timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                query_set: &timer.query_set,
                beginning_of_pass_write_index: None,
                end_of_pass_write_index: Some(index),
            }),
        });
        pass.set_pipeline(&timer.pipeline);
        pass.set_bind_group(0, &timer.bind_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    if let Some(slot) = timer.current {
        timer.slots[slot].written |= 1 << mark as usize;
    }
}

/// The output image's texture, for cameras that render into an image.
fn image_texture<'a>(
    camera: &ExtractedCamera,
    images: &'a RenderAssets<GpuImage>,
) -> Option<&'a wgpu::Texture> {
    match camera.target.as_ref()? {
        NormalizedRenderTarget::Image(target) => images.get(&target.handle).map(|i| &*i.texture),
        _ => None,
    }
}

fn mark_3d<const AT: u8>(
    view: ViewQuery<(&ExtractedCamera, &ViewTarget)>,
    timer: Option<ResMut<GpuTimer>>,
    images: Res<RenderAssets<GpuImage>>,
    mut ctx: RenderContext,
) {
    let Some(mut timer) = timer else { return };
    if timer.current.is_none() {
        return;
    }
    let (camera, target) = view.into_inner();
    let Some(mark) =
        CameraKind::of_order(camera.order).and_then(|k| mark_for(k, MarkAt::from_u8(AT)))
    else {
        return;
    };
    let output = image_texture(camera, &images);
    let dependency = match MarkAt::from_u8(AT) {
        // The frame's first camera: wait until the previous frame is done
        // reading its output (it is fully overwritten this frame).
        MarkAt::Start if mark.in_bare_frame() => output.map_or(Dependency::None, Dependency::Write),
        MarkAt::Start => Dependency::None,
        MarkAt::Opaque | MarkAt::Main => Dependency::Read(target.main_texture()),
        MarkAt::End => output.map_or(Dependency::None, Dependency::Read),
    };
    write_mark(&mut timer, ctx.command_encoder(), mark, dependency);
}

fn mark_2d<const AT: u8>(
    view: ViewQuery<(&ExtractedCamera, &ViewTarget)>,
    timer: Option<ResMut<GpuTimer>>,
    images: Res<RenderAssets<GpuImage>>,
    mut ctx: RenderContext,
) {
    let Some(mut timer) = timer else { return };
    if timer.current.is_none() {
        return;
    }
    let (camera, target) = view.into_inner();
    let Some(mark) =
        CameraKind::of_order(camera.order).and_then(|k| mark_for(k, MarkAt::from_u8(AT)))
    else {
        return;
    };
    let dependency = match MarkAt::from_u8(AT) {
        MarkAt::Start => Dependency::None,
        MarkAt::Opaque | MarkAt::Main => Dependency::Read(target.main_texture()),
        MarkAt::End => match image_texture(camera, &images) {
            Some(texture) => Dependency::Read(texture),
            // The drawable can't be read: wait for `upscaling`'s read of the
            // main texture by writing to it.
            None => Dependency::Write(target.main_texture()),
        },
    };
    write_mark(&mut timer, ctx.command_encoder(), mark, dependency);
}

/// Encodes the resolve of a slot whose frame has finished on the GPU.
fn encode_resolve(timer: &GpuTimer, i: usize, encoder: &mut wgpu::CommandEncoder) {
    let first = (i * MARKS) as u32;
    let offset = i as u64 * SLOT_BYTES;
    encoder.resolve_query_set(
        &timer.query_set,
        first..first + MARKS as u32,
        &timer.resolve,
        offset,
    );
    encoder.copy_buffer_to_buffer(
        &timer.resolve,
        offset,
        &timer.slots[i].readback,
        0,
        SLOT_BYTES,
    );
}

/// After every camera: resolves the slots whose frames have finished on the
/// GPU.
fn resolve_timed_frame(timer: Option<Res<GpuTimer>>, mut ctx: RenderContext) {
    let Some(timer) = timer else { return };
    for i in 0..SLOTS {
        if timer.slots[i].state() == DONE {
            encode_resolve(&timer, i, ctx.command_encoder());
            timer.slots[i].set(RESOLVING);
        }
    }
}

/// After submission: waits for the timed frame's GPU work, and maps the slots
/// resolved this frame. Each wgpu callback is a boxed closure: the only
/// allocations, two per timed frame.
fn map_timed_frame(timer: Option<ResMut<GpuTimer>>, queue: Res<RenderQueue>) {
    let Some(mut timer) = timer else { return };
    if let Some(i) = timer.current.take() {
        let slot = &timer.slots[i];
        if slot.written == 0 {
            slot.set(FREE);
        } else {
            slot.set(SUBMITTED);
            let state = slot.state.clone();
            queue.on_submitted_work_done(move || state.store(DONE, Ordering::Release));
        }
    }
    for slot in &timer.slots {
        if slot.state() == RESOLVING {
            slot.set(IN_FLIGHT);
            let state = slot.state.clone();
            slot.readback
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    state.store(
                        if result.is_ok() { MAPPED } else { FAILED },
                        Ordering::Release,
                    );
                });
        }
    }
}

/// Writes one timed frame's marks with every kind of dependency on the
/// texture formats the game's cameras use (the world image, the UI camera's
/// main texture on the drawable's format, an HDR target), then resolves and
/// reads them back like the game does, and checks that the device reported no
/// validation error (Bevy quits on one) and that every mark came back in
/// order. Returns the gaps between the marks (ms). For the offscreen probe
/// (`tests/gpu_timing_offscreen.rs`), where a window's path can't run.
#[doc(hidden)]
pub fn self_test(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Vec<f32>, String> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut timer = GpuTimer::new(device, queue.get_timestamp_period());
    timer.slots[0].set(RECORDING);
    timer.slots[0].full = true;
    timer.slots[0].written = 0;
    timer.current = Some(0);
    let textures: Vec<wgpu::Texture> = [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba16Float,
    ]
    .into_iter()
    .map(|format| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pieced_gpu_timing_self_test"),
            size: wgpu::Extent3d {
                width: 64,
                height: 32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    })
    .collect();
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let mut marks = GpuMark::ALL.into_iter();
    for texture in &textures {
        for dependency in [
            Dependency::None,
            Dependency::Read(texture),
            Dependency::Write(texture),
        ] {
            if let Some(mark) = marks.next() {
                write_mark(&mut timer, &mut encoder, mark, dependency);
            }
        }
    }
    let written = timer.slots[0].written;
    queue.submit([encoder.finish()]);
    let wait = |what: &str| {
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map(|_| ())
            .map_err(|e| format!("{what}: {e}"))
    };
    wait("marks")?;
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encode_resolve(&timer, 0, &mut encoder);
    queue.submit([encoder.finish()]);
    timer.slots[0]
        .readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, |_| {});
    wait("readback")?;
    let mut popped = std::pin::pin!(scope.pop());
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    if let std::task::Poll::Ready(Some(error)) = popped.as_mut().poll(&mut cx) {
        return Err(format!("validation error: {error}"));
    }
    let data = timer.slots[0].readback.slice(..).get_mapped_range();
    let mut out = Vec::new();
    let mut previous = 0u64;
    for m in 0..MARKS {
        if written & (1 << m) == 0 {
            continue;
        }
        let bytes: [u8; 8] = data[m * 8..m * 8 + 8].try_into().unwrap_or([0; 8]);
        let tick = u64::from_le_bytes(bytes);
        if tick == 0 || tick < previous {
            return Err(format!("mark {m} came back as {tick} after {previous}"));
        }
        if previous > 0 {
            out.push(((tick - previous) as f64 * f64::from(timer.period_ns) / 1.0e6) as f32);
        }
        previous = tick;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticks(marks: &[(GpuMark, u64)]) -> [Option<u64>; MARKS] {
        let mut t = [None; MARKS];
        for (m, v) in marks {
            t[*m as usize] = Some(*v);
        }
        t
    }

    #[test]
    fn passes_come_from_the_marks_between_them() {
        use GpuMark as M;
        // Nanosecond ticks: main 1–6.5 ms, viewmodel to 7.6 ms, UI to 9.7.
        let t = ticks(&[
            (M::MainStart, 1_000_000),
            (M::MainOpaque, 5_000_000),
            (M::MainMain, 6_000_000),
            (M::MainEnd, 6_500_000),
            (M::ViewOpaque, 7_000_000),
            (M::ViewMain, 7_100_000),
            (M::ViewEnd, 7_600_000),
            (M::UiPass, 9_200_000),
            (M::UiEnd, 9_700_000),
        ]);
        let s = sample_from_marks(42, true, &t, 1.0);
        let near = |a: Option<f32>, b: f32| a.is_some_and(|a| (a - b).abs() < 1e-4);
        assert_eq!(s.frame, 42);
        assert!(near(s.pass(GpuPass::World), 4.0 + 0.5), "{s:?}");
        assert!(near(s.pass(GpuPass::Effects), 1.0 + 0.1));
        assert!(near(s.pass(GpuPass::Post), 0.5 + 0.5 + 0.5));
        assert!(near(s.pass(GpuPass::Ui), 1.6));
        assert!(near(s.total_ms, 8.7));
        // The passes add up to the total.
        let sum: f32 = s.passes.iter().flatten().sum();
        assert!((sum - 8.7).abs() < 1e-4);
        // Outlines never split; far only with its own camera.
        assert_eq!(s.pass(GpuPass::Outlines), None);
        assert_eq!(s.pass(GpuPass::Far), None);

        // With the far camera, and a period that isn't 1 ns.
        let t = ticks(&[
            (M::FarStart, 100),
            (M::FarEnd, 200),
            (M::MainStart, 210),
            (M::UiEnd, 900),
        ]);
        let s = sample_from_marks(7, true, &t, 10.0);
        assert!(near(s.pass(GpuPass::Far), 0.001));
        assert!(near(s.total_ms, 0.008));
        assert_eq!(s.pass(GpuPass::World), None, "no opaque mark");
    }

    #[test]
    fn a_bare_frame_has_only_a_total_and_bad_marks_are_ignored() {
        use GpuMark as M;
        let t = ticks(&[(M::MainStart, 1_000), (M::UiEnd, 11_000)]);
        let s = sample_from_marks(3, false, &t, 1000.0);
        assert!(s.passes.iter().all(Option::is_none));
        assert!((s.total_ms.unwrap() - 10.0).abs() < 1e-4);
        // Unwritten (zero) marks and backwards spans give nothing.
        let t = ticks(&[
            (M::MainStart, 0),
            (M::MainOpaque, 5),
            (M::MainEnd, 9),
            (M::ViewOpaque, 4),
        ]);
        let s = sample_from_marks(3, true, &t, 1.0);
        assert_eq!(s.pass(GpuPass::World), None);
        assert!(s.total_ms.is_some());
        assert_eq!(
            sample_from_marks(1, true, &[None; MARKS], 1.0).total_ms,
            None
        );
    }

    #[test]
    fn every_camera_marks_its_own_points() {
        assert_eq!(mark_for(CameraKind::Ui, MarkAt::Opaque), None);
        assert_eq!(mark_for(CameraKind::Viewmodel, MarkAt::Start), None);
        assert_eq!(mark_for(CameraKind::Far, MarkAt::Main), None);
        assert_eq!(
            mark_for(CameraKind::Viewmodel, MarkAt::End),
            Some(GpuMark::ViewEnd)
        );
        assert_eq!(
            CameraKind::of_order(MAIN_CAMERA_ORDER),
            Some(CameraKind::Main)
        );
        assert_eq!(CameraKind::of_order(99), None);
        let bare: Vec<GpuMark> = GpuMark::ALL
            .into_iter()
            .filter(|m| m.in_bare_frame())
            .collect();
        assert_eq!(
            bare,
            [GpuMark::FarStart, GpuMark::MainStart, GpuMark::UiEnd]
        );
    }

    #[test]
    fn one_frame_in_every_is_timed_and_some_are_bare() {
        let timed_frames: Vec<(u64, bool)> = (1..=64)
            .filter_map(|f| timed(f, 8, 4).map(|full| (f, full)))
            .collect();
        assert_eq!(timed_frames.len(), 8);
        assert_eq!(timed_frames.iter().filter(|(_, full)| !full).count(), 2);
        assert!(timed_frames.iter().all(|(f, _)| f % 8 == 0));
        assert_eq!(timed(8, 0, 4), None, "0 turns timing off");
        assert!(
            (1..100).all(|f| timed(f, 1, 0) == Some(true)),
            "every frame, never bare"
        );
    }
}
