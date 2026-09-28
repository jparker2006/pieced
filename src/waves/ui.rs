//! The chunk-1 run banner (docs/M3-SPEC.md → Waves → Chunk 1 placeholder): one
//! plain line in a small cartoon card at the top third of the screen.
//!
//! - After the player's elimination: "Wave 1 — N eliminations — press Enter to
//!   go again" (Enter is read by the input adapter, `input::run_keys`).
//! - When every grunt of the wave is down: "Wave 1 cleared!".
//!
//! Client only. Chunk 2 replaces it with the wave HUD and the results screen.

use super::{Run, RunPhase};
use crate::{
    hud::style::{PANEL, RIM, TEXT, ink, text},
    shared::{SimTick, TICK_SECONDS},
};
use bevy::prelude::*;

/// Spawns and updates the run banner.
pub struct WavesUiPlugin;

impl Plugin for WavesUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_banner)
            .add_systems(Update, update_banner);
    }
}

/// The banner's root node (hidden when there's nothing to say).
#[derive(Component, Debug)]
pub struct RunBanner;

/// The banner's line of text.
#[derive(Component, Debug)]
pub struct RunBannerText;

/// What the banner says for `run` at `tick`, if anything.
pub fn banner_line(run: &Run, tick: u64) -> Option<String> {
    match run.phase {
        RunPhase::Over { .. } => {
            let kills = run.eliminations;
            let noun = if kills == 1 {
                "elimination"
            } else {
                "eliminations"
            };
            Some(format!(
                "Wave {} \u{2014} {kills} {noun} \u{2014} press Enter to go again",
                run.wave
            ))
        }
        RunPhase::Break { ends_tick } => Some(format!(
            "Wave {} cleared! Next wave in {:.0}",
            run.wave,
            (ends_tick.saturating_sub(tick) as f32 * TICK_SECONDS).ceil()
        )),
        _ => None,
    }
}

fn spawn_banner(mut commands: Commands) {
    commands
        .spawn((
            Name::new("Run banner"),
            RunBanner,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                top: percent(28),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(20),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    padding: UiRect::axes(px(26), px(12)),
                    border: UiRect::all(px(3)),
                    border_radius: BorderRadius::all(px(16)),
                    ..default()
                },
                BackgroundColor(PANEL),
                BorderColor::all(RIM),
                ink(3.0),
            ))
            .with_children(|card| {
                card.spawn((RunBannerText, text("", 30.0, TEXT)));
            });
        });
}

fn update_banner(
    run: Option<Res<Run>>,
    tick: Option<Res<SimTick>>,
    mut banner: Query<&mut Visibility, With<RunBanner>>,
    mut line: Query<&mut Text, With<RunBannerText>>,
) {
    let said = run
        .as_deref()
        .and_then(|run| banner_line(run, tick.as_deref().map_or(0, |t| t.0)));
    for mut visibility in &mut banner {
        visibility.set_if_neq(if said.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    if let Some(said) = said {
        for mut text in &mut line {
            if text.0 != said {
                text.0 = said.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::waves::WavesTuning;

    #[test]
    fn the_banner_speaks_only_when_the_run_is_over_or_cleared() {
        let mut run = Run::new(1, 0, &WavesTuning::default());
        assert_eq!(banner_line(&run, 10), None);
        run.eliminations = 2;
        run.phase = RunPhase::Over { tick: 5 };
        assert_eq!(
            banner_line(&run, 10).as_deref(),
            Some("Wave 1 \u{2014} 2 eliminations \u{2014} press Enter to go again")
        );
        run.phase = RunPhase::Break { ends_tick: 700 };
        assert_eq!(
            banner_line(&run, 101).as_deref(),
            Some("Wave 1 cleared! Next wave in 10")
        );
    }
}
