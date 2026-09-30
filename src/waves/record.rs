//! What a Waves run leaves behind (docs/M3-SPEC.md → Waves, D81 and D89): the
//! personal best in `userdata/best.json` and one line per run in
//! `userdata/runs.jsonl`.
//!
//! Nothing is written unless a [`RunStore`] directory is set: the native game
//! sets it to `userdata/` ([`RunStore::user`]); the headless test app leaves it
//! empty (the best then lives only in memory), and tests that check the files
//! point it at a temporary directory.

use crate::tuning::Tuning;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

/// Where the best run and the run log live. `dir: None` persists nothing.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct RunStore {
    pub dir: Option<PathBuf>,
}

impl RunStore {
    /// The game's store: the directory of [`Tuning::settings_path`]
    /// (`<project or PIECED_ROOT>/userdata`, git-ignored).
    pub fn user() -> Self {
        Self {
            dir: Tuning::settings_path().parent().map(Path::to_path_buf),
        }
    }

    /// A store in `dir` (tests use a temporary directory).
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: Some(dir.into()),
        }
    }

    pub fn best_path(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join("best.json"))
    }

    pub fn runs_path(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join("runs.jsonl"))
    }

    /// The saved personal best, if there is one and it parses.
    pub fn load_best(&self) -> Option<BestRun> {
        let text = std::fs::read_to_string(self.best_path()?).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Saves `best` atomically (write, then rename). No-op without a directory.
    pub fn save_best(&self, best: &BestRun) -> std::io::Result<()> {
        let Some(path) = self.best_path() else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_string_pretty(best).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(tmp, path)
    }

    /// Appends one JSON line to `runs.jsonl`. No-op without a directory.
    pub fn append_run(&self, line: &RunLogLine) -> std::io::Result<()> {
        let Some(path) = self.runs_path() else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut text = serde_json::to_string(line).map_err(std::io::Error::other)?;
        text.push('\n');
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?
            .write_all(text.as_bytes())
    }
}

/// The personal best (D81): the highest wave reached, score as the tiebreak.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BestRun {
    pub wave: u32,
    pub score: u32,
    pub seed: u64,
    pub eliminations: u32,
    pub run_seconds: f32,
}

impl BestRun {
    /// Whether this run beats `other`: a higher wave, or the same wave with a
    /// higher score.
    pub fn beats(&self, other: &BestRun) -> bool {
        (self.wave, self.score) > (other.wave, other.score)
    }
}

impl From<&RunResults> for BestRun {
    fn from(r: &RunResults) -> Self {
        Self {
            wave: r.wave,
            score: r.score,
            seed: r.seed,
            eliminations: r.eliminations,
            run_seconds: r.run_seconds,
        }
    }
}

/// The loaded personal best (loaded at startup, updated at each run's end).
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct PersonalBest(pub Option<BestRun>);

/// A run's results-screen numbers (D84). Live while the run goes; final once
/// it ends.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunResults {
    pub seed: u64,
    /// The wave reached (the wave being fought, or just cleared, at the end).
    pub wave: u32,
    /// The wave the run started on (D115: 1, 6 or 10).
    pub start_wave: u32,
    pub score: u32,
    pub eliminations: u32,
    /// The player's hits per shot, 0..=1 (`CombatStats::accuracy`).
    pub accuracy: f32,
    /// Shots with a head hit (`CombatStats::headshots`).
    pub headshots: u32,
    /// Simulated seconds from the run's start to its end (or now).
    pub run_seconds: f32,
}

/// How a run ended.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEnd {
    /// The player was eliminated.
    #[default]
    Eliminated,
    /// Quit from the pause menu ([`EndRun`](super::EndRun)).
    Quit,
}

/// What killed the player (D89): the orb's source knight, its wave and where
/// it stood, and how long since the player's previous hit.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeathCause {
    /// The wave the firing knight arrived in (`None` when the source wasn't a
    /// wave knight).
    pub source_wave: Option<u32>,
    /// Where the firing knight's feet were on the killing tick.
    pub source_position: Option<[f32; 3]>,
    /// Seconds between the player's previous hit and the killing one (`None`
    /// when the killing hit was the first).
    pub seconds_since_previous_hit: Option<f32>,
}

/// One line of `runs.jsonl`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RunLogLine {
    pub seed: u64,
    /// The build's git commit (`PIECED_GIT_COMMIT`), "-dirty" when built from
    /// uncommitted changes.
    pub commit: String,
    pub wave: u32,
    /// The wave the run started on (D115: 1, 6 or 10; 0 in lines written
    /// before start at wave). Runs above 1 never touch the best.
    pub start_wave: u32,
    pub score: u32,
    pub run_seconds: f32,
    pub eliminations: u32,
    pub accuracy: f32,
    pub headshots: u32,
    pub ended: RunEnd,
    /// Present when the player was killed by damage with a known source.
    pub cause: Option<DeathCause>,
    /// Wall-clock end time (Unix seconds).
    pub unix_time: u64,
}

impl RunLogLine {
    pub fn new(results: &RunResults, ended: RunEnd, cause: Option<DeathCause>) -> Self {
        Self {
            seed: results.seed,
            commit: build_commit(),
            wave: results.wave,
            start_wave: results.start_wave.max(1),
            score: results.score,
            run_seconds: results.run_seconds,
            eliminations: results.eliminations,
            accuracy: results.accuracy,
            headshots: results.headshots,
            ended,
            cause,
            unix_time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        }
    }
}

/// The commit this binary was built from.
pub fn build_commit() -> String {
    let commit = env!("PIECED_GIT_COMMIT");
    if env!("PIECED_GIT_DIRTY") == "1" {
        format!("{commit}-dirty")
    } else {
        commit.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn best(wave: u32, score: u32) -> BestRun {
        BestRun {
            wave,
            score,
            ..default()
        }
    }

    #[test]
    fn the_wave_wins_and_score_breaks_ties() {
        assert!(best(5, 100).beats(&best(4, 9000)));
        assert!(!best(4, 9000).beats(&best(5, 100)));
        assert!(best(5, 300).beats(&best(5, 200)));
        assert!(
            !best(5, 200).beats(&best(5, 200)),
            "a tie is not a new best"
        );
    }

    #[test]
    fn a_store_without_a_directory_writes_nothing() {
        let store = RunStore::default();
        assert!(store.save_best(&best(3, 10)).is_ok());
        assert!(store.append_run(&RunLogLine::default()).is_ok());
        assert_eq!(store.load_best(), None);
    }
}
