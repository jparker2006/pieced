//! Performance settings for Milestone 4's GPU budget (chunk 0, D99–D100).
//!
//! [`PerfTuning`] is a designer section of [`crate::tuning::Tuning`]: never
//! saved to `settings.json` (`#[serde(skip)]`), so the orchestrator can change
//! a default after Jake's sessions without a saved copy freezing it. Perf
//! knobs (`--knobs gpu=…`) override it at startup
//! ([`crate::perf_knobs::PerfKnobsPlugin`]).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PerfTuning {
    /// Time the GPU passes on 1 frame in this many (1 = every frame, 0 =
    /// never). The spec keeps per-frame timing only if it costs ≤ 0.3 ms;
    /// until Jake's sessions measure that, it samples 1 in 8
    /// (see [`crate::gpu_timing`]).
    pub gpu_every: u32,
    /// Of the timed frames, every this-many-th is *bare* (first and last
    /// marks only), so the session can report the timing's own cost.
    pub gpu_bare_every: u32,
}

impl Default for PerfTuning {
    fn default() -> Self {
        Self {
            gpu_every: 8,
            gpu_bare_every: 4,
        }
    }
}
