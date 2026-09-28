//! The Waves screens (docs/M3-SPEC.md → HUD and results UI, D65; Waves,
//! D79–D84), drawn from [`RunSummary`]:
//!
//! - **The run HUD** ([`hud`]): wave, knights left and score in M2's cartoon
//!   frames along the top of the screen (the M1/M2 anchors don't move); the
//!   score pops when it ticks up.
//! - **The break** ([`hud`]): "WAVE n CLEARED!", a big countdown and "Press
//!   ⟨Enter⟩ to start" with the bound key's name; "WAVE n" as the next one
//!   starts. A cyan "+25" pops under the crosshair when a potion is drunk.
//! - **The death beat** ([`death`], client): slow motion, the view dropping
//!   to the grass, a soft vignette.
//! - **The results** ([`results`]): a cartoon card in the pause menu's style
//!   (T12) with the run's numbers, the best run beside them, a "NEW BEST!"
//!   burst and the seed; **Go again** and **Quit**.
//!
//! Every part is spawned once at startup (hidden) and only has its text and
//! visibility rewritten, into the strings it already owns, so nothing
//! allocates per event. The UI clocks read real time: the death beat's slow
//! motion never slows a menu.

pub mod death;
pub mod hud;
pub mod results;

use super::{Run, RunPhase, RunSummary};
use crate::hud::style::{INK, TEXT};
use bevy::{prelude::*, text::LetterSpacing};
use std::fmt::Write;

/// Spawns and drives the run HUD, the break, the "+25" and the results card.
pub struct WavesUiPlugin;

impl Plugin for WavesUiPlugin {
    fn build(&self, app: &mut App) {
        crate::hud::art::install(app);
        hud::build(app);
        results::build(app);
    }
}

/// Every Waves UI part a system rewrites (and a test reads).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunUi {
    /// The top strip: wave, knights left, score.
    Hud,
    Wave,
    Knights,
    Score,
    /// The frame round the score (it pops).
    ScoreFrame,
    /// The big centre banner: the break, or a new wave's title.
    Banner,
    BannerTitle,
    /// "NEXT WAVE IN" and the countdown badge.
    Countdown,
    CountdownValue,
    /// "Press ⟨key⟩ to start".
    Hint,
    HintKey,
    /// The cyan "+25" under the crosshair after a potion.
    PotionNumber,
    /// The death beat's vignette.
    Vignette,
    /// The results screen: the veil and the card.
    Results,
    ResultsWave,
    ResultsScore,
    ResultsEliminations,
    ResultsAccuracy,
    ResultsHeadshots,
    ResultsTime,
    BestWave,
    BestScore,
    BestEliminations,
    BestTime,
    NewBest,
    Seed,
    ResultsHint,
}

/// The results card's buttons.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultsButton {
    /// Starts a new run in place ([`RestartRun`](super::RestartRun)).
    GoAgain,
    /// Closes the game (chunk 5: back to the main menu).
    Quit,
}

// ---------------------------------------------------------------------------
// What the pause menu's Quit does
// ---------------------------------------------------------------------------

/// What the pause menu's Quit does in the current run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseQuit {
    /// A run in progress ends ([`EndRun`](super::EndRun)) and play resumes
    /// on its results (D84).
    EndRun,
    /// The run already ended (the death beat): resume; the results follow.
    Resume,
    /// No run, or its results are already up: close the game.
    Exit,
}

/// D84: quitting a run shows its results; a second Quit (on the results)
/// closes the game.
pub fn pause_quit(run: Option<&Run>) -> PauseQuit {
    match run {
        Some(run) if !run.is_ended() => PauseQuit::EndRun,
        Some(run) if !run.is_over() => PauseQuit::Resume,
        _ => PauseQuit::Exit,
    }
}

// ---------------------------------------------------------------------------
// Text (pure; written into existing strings)
// ---------------------------------------------------------------------------

/// Writes `n` with thousands separators ("12,450").
pub fn write_thousands(out: &mut String, n: u32) {
    let mut digits = [0u8; 10];
    let mut len = 0;
    let mut v = n;
    loop {
        digits[len] = (v % 10) as u8;
        len += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    for i in (0..len).rev() {
        out.push((b'0' + digits[i]) as char);
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
    }
}

/// Writes a run time as m:ss ("4:07"; "61:00" past an hour).
pub fn write_run_time(out: &mut String, seconds: f32) {
    let total = seconds.max(0.0).floor() as u32;
    let _ = write!(out, "{}:{:02}", total / 60, total % 60);
}

/// Writes an accuracy (0..=1) as a whole percentage ("43%").
pub fn write_accuracy(out: &mut String, accuracy: f32) {
    let _ = write!(
        out,
        "{}%",
        (accuracy.clamp(0.0, 1.0) * 100.0).round() as u32
    );
}

/// The whole seconds the break countdown shows: 10 at the start, 1 in its last
/// second.
pub fn countdown_shown(seconds_left: f32) -> u32 {
    (seconds_left - 1e-3).max(0.0).ceil() as u32
}

/// Rewrites `text` with `f` only when the result differs (so change detection
/// and layout only see real changes), reusing `scratch`'s buffer.
pub fn set_text(text: &mut Mut<Text>, scratch: &mut String, f: impl FnOnce(&mut String)) {
    scratch.clear();
    f(scratch);
    if text.0 != *scratch {
        text.0.clear();
        text.0.push_str(scratch);
    }
}

/// What the banner shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerKind {
    /// "WAVE n CLEARED!", the countdown and the hint (the break).
    Cleared { wave: u32, count: u32 },
    /// "WAVE n" as a wave starts.
    Wave(u32),
}

/// The banner's title.
pub fn write_banner_title(out: &mut String, kind: BannerKind) {
    let _ = match kind {
        BannerKind::Cleared { wave, .. } => write!(out, "WAVE {wave} CLEARED!"),
        BannerKind::Wave(wave) => write!(out, "WAVE {wave}"),
    };
}

/// The break's banner for `summary`, if it's in one.
pub fn break_banner(summary: &RunSummary) -> Option<BannerKind> {
    match summary.phase {
        RunPhase::Break { .. } => Some(BannerKind::Cleared {
            wave: summary.wave,
            count: countdown_shown(summary.break_seconds_left.unwrap_or(0.0)),
        }),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Shared cartoon styling (the pause menu's T12 look)
// ---------------------------------------------------------------------------

/// The veil over the world behind the results (the pause menu's).
pub const BACKDROP: Color = Color::srgba(0.035, 0.03, 0.09, 0.58);
/// Button fills: the primary (Go again) in crystal blue, the rest in slate.
pub const BLUE: Color = Color::srgb(0.17, 0.43, 0.77);
pub const BLUE_HOVER: Color = Color::srgb(0.23, 0.53, 0.88);
pub const SLATE: Color = Color::srgb(0.16, 0.21, 0.28);
pub const SLATE_HOVER: Color = Color::srgb(0.22, 0.29, 0.38);
/// The brass frame round buttons and cards.
pub const GOLD_FRAME: Color = Color::srgb(0.9, 0.66, 0.22);
pub const GOLD_HOVER: Color = Color::srgb(1.0, 0.83, 0.36);
pub const RIVET: Color = Color::srgb(0.62, 0.65, 0.74);
/// Potion cyan (the shield's colour).
pub const POTION_CYAN: Color = crate::palette::SHIELD;

/// A title in the cartoon font with a hard ink drop.
pub fn title(value: &str, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(value),
        TextFont::from_font_size(size),
        TextColor(color),
        TextShadow {
            offset: Vec2::new(0.0, size * 0.08),
            color: INK,
        },
        LetterSpacing::Px(size * 0.04),
    )
}

/// White text in the cartoon font with the ink drop (the menus' labels).
pub fn label(value: &str, size: f32) -> impl Bundle {
    title(value, size, TEXT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::waves::{RunEnd, WavesTuning};

    fn written(f: impl FnOnce(&mut String)) -> String {
        let mut s = String::new();
        f(&mut s);
        s
    }

    #[test]
    fn numbers_print_like_a_scoreboard() {
        assert_eq!(written(|s| write_thousands(s, 0)), "0");
        assert_eq!(written(|s| write_thousands(s, 950)), "950");
        assert_eq!(written(|s| write_thousands(s, 12_450)), "12,450");
        assert_eq!(written(|s| write_thousands(s, 1_234_567)), "1,234,567");
        assert_eq!(written(|s| write_run_time(s, 247.9)), "4:07");
        assert_eq!(written(|s| write_run_time(s, 3.0)), "0:03");
        assert_eq!(written(|s| write_accuracy(s, 0.4321)), "43%");
        assert_eq!(written(|s| write_accuracy(s, 1.0)), "100%");
    }

    #[test]
    fn the_countdown_counts_whole_seconds_down_to_one() {
        assert_eq!(countdown_shown(10.0), 10);
        assert_eq!(countdown_shown(9.99), 10);
        assert_eq!(countdown_shown(7.0), 7);
        assert_eq!(countdown_shown(0.4), 1);
        assert_eq!(countdown_shown(0.0), 0);
    }

    #[test]
    fn pause_quit_ends_a_live_run_then_closes_the_game() {
        let t = WavesTuning::default();
        assert_eq!(pause_quit(None), PauseQuit::Exit);
        let mut run = Run::new(1, 0, &t);
        assert_eq!(pause_quit(Some(&run)), PauseQuit::EndRun);
        run.phase = RunPhase::Break { ends_tick: 100 };
        assert_eq!(pause_quit(Some(&run)), PauseQuit::EndRun);
        run.ended = Some(RunEnd::Eliminated);
        run.phase = RunPhase::Dying { until: 10 };
        assert_eq!(pause_quit(Some(&run)), PauseQuit::Resume);
        run.phase = RunPhase::Over { tick: 10 };
        assert_eq!(pause_quit(Some(&run)), PauseQuit::Exit);
    }
}
