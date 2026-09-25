//! Slice F: the pure rules behind the HUD, menus, settings persistence and the
//! synthesized sound bank.

use bevy::prelude::*;
use pieced::{
    audio::{
        AudioTuning, Sfx, SfxCategory, VoiceDecision, choose_voice,
        synth::{SAMPLE_RATE, WavInfo, encode_wav},
    },
    combat::{GunState, GunTuning},
    hud::{
        MarkerKind, NumberKind, TrailBar, damage_label, number_motion, project_to_screen,
        spread_to_pixels,
    },
    menu::{Debounce, Setting, SettingKind},
    palette,
    render::QualityPreset,
    shared::DamageTarget,
    tuning::Tuning,
};

// ---------------------------------------------------------------------------
// Crosshair and projection
// ---------------------------------------------------------------------------

#[test]
fn spread_converts_to_pixels_through_the_vertical_fov() {
    let fov = 70f32.to_radians();
    assert_eq!(spread_to_pixels(0.0, fov, 800.0), 0.0);
    // A direction at the edge of the vertical FOV lands on the screen edge.
    assert!((spread_to_pixels(35.0, fov, 800.0) - 400.0).abs() < 0.01);
    // The rifle's full bloom is a clearly visible gap, but not a huge one.
    let rifle = GunTuning::rifle();
    let full = spread_to_pixels(rifle.bloom_max_deg + rifle.base_spread_deg, fov, 800.0);
    assert!((10.0..30.0).contains(&full), "full bloom = {full} px");
    // Zooming in (ADS narrows the FOV) makes the same angle bigger on screen.
    assert!(spread_to_pixels(1.0, fov * 0.75, 800.0) > spread_to_pixels(1.0, fov, 800.0));
}

#[test]
fn crosshair_gap_grows_with_bloom_and_tightens_on_ads() {
    let rifle = GunTuning::rifle();
    let mut gun = GunState::new(&rifle);
    let fov = 70f32.to_radians();
    let first = spread_to_pixels(gun.spread_deg(&rifle, false), fov, 800.0);
    gun.bloom_deg = rifle.bloom_max_deg;
    let bloomed = spread_to_pixels(gun.spread_deg(&rifle, false), fov, 800.0);
    let ads = spread_to_pixels(gun.spread_deg(&rifle, true), fov * rifle.ads_zoom, 800.0);
    assert!(first < 1.0, "first shot is pin-point: {first}");
    assert!(bloomed > first + 10.0);
    assert!(ads < bloomed);
}

#[test]
fn world_points_project_to_the_expected_screen_spots() {
    let camera = GlobalTransform::from(Transform::from_xyz(0.0, 1.6, 0.0));
    let fov = 70f32.to_radians();
    let screen = Vec2::new(1280.0, 800.0);
    let center = project_to_screen(&camera, fov, screen, Vec3::new(0.0, 1.6, -10.0)).unwrap();
    assert!(center.distance(screen / 2.0) < 0.01);
    // Straight up at the edge of the FOV → the top of the screen.
    let up = 10.0 * (fov / 2.0).tan();
    let top = project_to_screen(&camera, fov, screen, Vec3::new(0.0, 1.6 + up, -10.0)).unwrap();
    assert!(top.y.abs() < 0.01 && (top.x - 640.0).abs() < 0.01);
    // Right of center lands right of center.
    let right = project_to_screen(&camera, fov, screen, Vec3::new(2.0, 1.6, -10.0)).unwrap();
    assert!(right.x > 640.0);
    // Behind the camera: not drawn.
    assert!(project_to_screen(&camera, fov, screen, Vec3::new(0.0, 1.6, 5.0)).is_none());
    // A turned camera sees what's in front of it.
    let turned = GlobalTransform::from(
        Transform::from_xyz(0.0, 1.6, 0.0).with_rotation(Quat::from_rotation_y(90f32.to_radians())),
    );
    let ahead = project_to_screen(&turned, fov, screen, Vec3::new(-10.0, 1.6, 0.0)).unwrap();
    assert!(ahead.distance(screen / 2.0) < 0.01);
}

// ---------------------------------------------------------------------------
// Hit feedback styling
// ---------------------------------------------------------------------------

#[test]
fn damage_numbers_are_white_yellow_blue_or_wood() {
    let c = DamageTarget::Character;
    assert_eq!(NumberKind::of(c, false, 0.0), NumberKind::Body);
    assert_eq!(NumberKind::of(c, false, 28.0), NumberKind::Shield);
    // A headshot is always yellow, even into shield.
    assert_eq!(NumberKind::of(c, true, 42.0), NumberKind::Headshot);
    assert_eq!(NumberKind::of(c, true, 0.0), NumberKind::Headshot);
    assert_eq!(
        NumberKind::of(DamageTarget::Piece, false, 0.0),
        NumberKind::Structure
    );
    assert_eq!(NumberKind::Body.color(), palette::HIT_WHITE);
    assert_eq!(NumberKind::Headshot.color(), palette::HEADSHOT);
    assert_eq!(NumberKind::Shield.color(), palette::SHIELD);
    assert!(NumberKind::Headshot.size() > NumberKind::Body.size());
    assert!(NumberKind::Structure.size() < NumberKind::Body.size());
}

#[test]
fn damage_labels_are_whole_points_and_never_zero() {
    assert_eq!(damage_label(28.0), "28");
    assert_eq!(damage_label(41.99), "42");
    assert_eq!(damage_label(19.6), "20");
    assert_eq!(damage_label(0.2), "1");
}

#[test]
fn damage_numbers_pop_rise_and_fade() {
    let life = 0.85;
    let start = number_motion(0.0, life, 46.0);
    assert_eq!(start.rise, 0.0);
    assert_eq!(start.alpha, 1.0);
    assert!(start.scale > 1.2, "pops in large");
    let mut last = start.rise;
    for i in 1..=20 {
        let m = number_motion(life * i as f32 / 20.0, life, 46.0);
        assert!(m.rise >= last, "rises monotonically");
        last = m.rise;
    }
    let settled = number_motion(0.2, life, 46.0);
    assert_eq!(settled.scale, 1.0);
    assert_eq!(settled.alpha, 1.0, "fully visible for most of its life");
    let end = number_motion(life, life, 46.0);
    assert!(end.alpha.abs() < 1e-6 && (end.rise - 46.0).abs() < 1e-4);
}

#[test]
fn hitmarker_priority_is_kill_then_headshot_then_hit() {
    assert_eq!(MarkerKind::of(false, false), MarkerKind::Hit);
    assert_eq!(MarkerKind::of(false, true), MarkerKind::Headshot);
    assert_eq!(MarkerKind::of(true, true), MarkerKind::Kill);
    assert_eq!(MarkerKind::of(true, false), MarkerKind::Kill);
    assert!(MarkerKind::Kill > MarkerKind::Headshot && MarkerKind::Headshot > MarkerKind::Hit);
}

#[test]
fn bar_trail_holds_the_lost_chunk_then_drains() {
    let dt = 1.0 / 60.0;
    let (hold, rate) = (0.35, 1.2);
    let mut bar = TrailBar::new(1.0);
    bar.update(0.72, dt, hold, rate);
    assert_eq!(bar.shown, 1.0, "the chunk shows right away");
    for _ in 0..18 {
        bar.update(0.72, dt, hold, rate);
    }
    assert_eq!(bar.shown, 1.0, "held for ~0.3 s");
    for _ in 0..30 {
        bar.update(0.72, dt, hold, rate);
    }
    assert!(
        bar.shown < 1.0 && bar.shown >= 0.72,
        "draining: {}",
        bar.shown
    );
    for _ in 0..60 {
        bar.update(0.72, dt, hold, rate);
    }
    assert_eq!(bar.shown, 0.72, "settles on the real value");
    // More damage restarts the hold.
    bar.update(0.44, dt, hold, rate);
    bar.update(0.44, dt, hold, rate);
    assert_eq!(bar.shown, 0.72);
    // Healing snaps up.
    bar.update(1.0, dt, hold, rate);
    assert_eq!(bar.shown, 1.0);
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[test]
fn settings_debounce_fires_once_after_changes_stop() {
    let mut d = Debounce::default();
    assert!(!d.fire(10.0, 1.0), "nothing pending");
    d.touch(0.0);
    assert!(!d.fire(0.5, 1.0));
    d.touch(0.6); // still changing: the quiet period restarts
    assert!(!d.fire(1.4, 1.0));
    assert!(d.fire(1.6, 1.0), "1 s after the last change");
    assert!(!d.fire(5.0, 1.0), "fires only once");
    d.touch(6.0);
    assert!(d.take(), "a pending change can be flushed on exit");
    assert!(!d.is_pending());
}

#[test]
fn settings_edit_the_live_tuning_within_range() {
    let mut t = Tuning::default();
    for setting in Setting::ALL {
        let before = setting.get(&t);
        setting.set(&mut t, before);
        assert!(
            (setting.get(&t) - before).abs() < 1e-4,
            "{setting:?} round-trips"
        );
        assert!(!setting.display(&t).is_empty());
    }
    Setting::Fov.set(&mut t, 120.0);
    assert_eq!(t.look.fov_deg, 90.0);
    Setting::Fov.set(&mut t, 20.0);
    assert_eq!(t.look.fov_deg, 60.0);
    Setting::Fov.set_fraction(&mut t, 0.5);
    assert_eq!(t.look.fov_deg, 75.0);
    Setting::CameraShake.set(&mut t, 0.0);
    assert_eq!(
        t.feedback.camera_shake, 0.0,
        "shake goes all the way to zero"
    );
    Setting::Sensitivity.set(&mut t, 5.0);
    assert!((t.look.sensitivity - 0.005).abs() < 1e-7);
    Setting::Mute.set(&mut t, 1.0);
    assert!(t.audio.muted && t.audio.effective_master() == 0.0);
    Setting::Quality.set(&mut t, 1.0);
    assert_eq!(t.graphics.preset, QualityPreset::PluggedIn);
    Setting::WindowMode.set(&mut t, 1.0);
    assert!(!t.graphics.fullscreen);
    Setting::Bloom.set(&mut t, 0.0);
    assert!(!t.hud.show_bloom);
    for setting in Setting::ALL {
        if let SettingKind::Slider { min, max, .. } = setting.kind() {
            assert!(min < max, "{setting:?}");
        }
    }
}

#[test]
fn every_menu_setting_is_a_real_tuning_field_that_persists() {
    let dir = std::env::temp_dir().join(format!("pieced-hud-{}", std::process::id()));
    let path = dir.join("settings.json");
    let mut t = Tuning::default();
    Setting::Volume.set(&mut t, 0.35);
    Setting::AdsMultiplier.set(&mut t, 0.55);
    Setting::Acceleration.set(&mut t, 1.0);
    t.save(&path).unwrap();
    let loaded = Tuning::load_or_default(&path);
    assert_eq!(loaded, t);
    assert!((Setting::Volume.get(&loaded) - 0.35).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(dir);
}

// ---------------------------------------------------------------------------
// Sound bank
// ---------------------------------------------------------------------------

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn zero_crossing_rate(samples: &[f32]) -> f32 {
    let crossings = samples
        .windows(2)
        .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
        .count();
    crossings as f32 / samples.len().max(1) as f32
}

#[test]
fn every_sound_is_a_valid_short_wav() {
    for sfx in Sfx::ALL {
        let wav = sfx.wav();
        let info = WavInfo::parse(&wav).unwrap_or_else(|| panic!("{sfx:?}: invalid WAV"));
        assert_eq!(info.channels, 1, "{sfx:?}");
        assert_eq!(info.sample_rate, SAMPLE_RATE, "{sfx:?}");
        assert_eq!(info.bits_per_sample, 16, "{sfx:?}");
        assert_eq!(wav.len(), 44 + info.frames as usize * 2, "{sfx:?}");
        let seconds = info.seconds();
        assert!((0.05..=1.0).contains(&seconds), "{sfx:?}: {seconds} s");
        let samples = sfx.synthesize();
        assert_eq!(samples.len(), info.frames as usize);
        let p = peak(&samples);
        assert!((0.5..=1.0).contains(&p), "{sfx:?}: peak {p}");
        assert!(samples.iter().all(|s| s.is_finite()), "{sfx:?}");
        assert!(
            samples.last().unwrap().abs() < 1e-3,
            "{sfx:?} ends without a click"
        );
    }
}

#[test]
fn the_sound_bank_is_deterministic() {
    for sfx in Sfx::ALL {
        assert_eq!(sfx.wav(), sfx.wav(), "{sfx:?}");
    }
}

#[test]
fn wav_encoding_round_trips_its_header() {
    let wav = encode_wav(&[0.0, 1.0, -1.0, 0.5]);
    let info = WavInfo::parse(&wav).unwrap();
    assert_eq!(info.frames, 4);
    assert_eq!(&wav[44..46], &0i16.to_le_bytes());
    assert_eq!(&wav[46..48], &i16::MAX.to_le_bytes());
    // Truncated or mislabeled files are rejected.
    assert!(WavInfo::parse(&wav[..wav.len() - 1]).is_none());
    let mut bad = wav.clone();
    bad[8] = b'X';
    assert!(WavInfo::parse(&bad).is_none());
}

#[test]
fn the_hit_tick_is_crisp_and_distinct_from_the_gunshot() {
    let tick = Sfx::HitTick.synthesize();
    let shot = Sfx::RifleShot.synthesize();
    assert!(tick.len() * 3 < shot.len(), "the tick is much shorter");
    let (zt, zs) = (zero_crossing_rate(&tick), zero_crossing_rate(&shot));
    assert!(zt > 3.0 * zs, "the tick is much brighter: {zt} vs {zs}");
    // The pump is the bigger, longer boom.
    assert!(Sfx::PumpShot.synthesize().len() > shot.len());
}

#[test]
fn sound_categories_and_priorities_protect_hit_feedback() {
    for sfx in Sfx::ALL {
        let expected = match sfx.category() {
            SfxCategory::Hits => 3,
            SfxCategory::Movement => 1,
            _ => 2,
        };
        assert_eq!(sfx.priority(), expected, "{sfx:?}");
        assert!(sfx.base_volume() > 0.0 && sfx.base_volume() <= 1.0);
    }
    assert_eq!(Sfx::HitTick.category(), SfxCategory::Hits);
    assert_eq!(Sfx::Footstep.category(), SfxCategory::Movement);
}

#[test]
fn the_voice_cap_steals_the_oldest_least_important_sound() {
    // (id, priority, started)
    let voices = [(1, 2, 0.5), (2, 1, 0.9), (3, 1, 0.2), (4, 3, 0.1)];
    assert_eq!(choose_voice(&voices, 8, 1), VoiceDecision::Play);
    assert_eq!(choose_voice(&voices, 4, 3), VoiceDecision::Steal(3));
    assert_eq!(choose_voice(&voices, 4, 1), VoiceDecision::Steal(3));
    let important = [(1, 3, 0.5), (2, 3, 0.1)];
    assert_eq!(
        choose_voice(&important, 2, 1),
        VoiceDecision::Drop,
        "a footstep never cuts a hit sound"
    );
}

#[test]
fn master_volume_and_mute_combine() {
    let mut a = AudioTuning::default();
    assert!((a.effective_master() - 0.8).abs() < 1e-6);
    a.master_volume = 1.7;
    assert_eq!(a.effective_master(), 1.0);
    a.muted = true;
    assert_eq!(a.effective_master(), 0.0);
}
