//! Settings preview live (M4 chunk 5, D110). The Settings card is
//! translucent over the live world (the pause blur freezes the world only on
//! the pause menu's main page, [`super::blur`]), so a graphics change shows
//! at once; and while Settings is open:
//!
//! - moving the **Master volume** or **Effects** slider plays a sample
//!   ([`SAMPLE`]) at the new level (at most every [`SAMPLE_EVERY`]); the
//!   **Music** slider is heard in the score itself;
//! - moving **Camera effects** plays a damage nudge at the new strength
//!   ([`CameraNudgePreview`]).
//!
//! It compares four numbers per frame; nothing allocated.

use super::{MenuPage, MenuState};
use crate::{audio::Sfx, camera_feel::CameraNudgePreview, tuning::Tuning};
use bevy::prelude::*;

/// The sound the volume sliders preview with.
pub const SAMPLE: Sfx = Sfx::KillConfirm;
/// At most one sample per this many seconds while a slider is dragged.
pub const SAMPLE_EVERY: f64 = 0.25;

pub(super) fn build(app: &mut App) {
    app.add_message::<CameraNudgePreview>()
        .init_resource::<SettingsPreview>()
        .add_systems(Update, preview_settings);
}

/// What the preview last saw, and how many previews it played (tests).
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct SettingsPreview {
    seen: Option<[f32; 3]>,
    last_sample: Option<f64>,
    pub samples: u32,
    pub nudges: u32,
}

fn preview_settings(
    time: Res<Time<Real>>,
    tuning: Res<Tuning>,
    menu: Res<MenuState>,
    mut preview: ResMut<SettingsPreview>,
    mut queue: Option<ResMut<crate::audio::PlayQueue>>,
    mut nudges: MessageWriter<CameraNudgePreview>,
) {
    let now = [
        tuning.audio.master_volume,
        tuning.audio.effects_volume,
        tuning.feedback.camera_effects,
    ];
    let open = menu.menu_visible() && menu.page == MenuPage::Settings;
    let Some(seen) = preview.seen.replace(now) else {
        return;
    };
    if !open || seen == now {
        return;
    }
    let t = time.elapsed_secs_f64();
    let due = preview
        .last_sample
        .is_none_or(|last| t - last >= SAMPLE_EVERY);
    if (seen[0] != now[0] || seen[1] != now[1]) && due {
        preview.last_sample = Some(t);
        preview.samples += 1;
        if let Some(queue) = queue.as_mut() {
            queue.push(SAMPLE, None, t);
        }
    }
    if seen[2] != now[2] {
        preview.nudges += 1;
        nudges.write(CameraNudgePreview);
    }
}
