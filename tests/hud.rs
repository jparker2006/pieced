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

// ---------------------------------------------------------------------------
// Milestone 2: the cartoon HUD
// ---------------------------------------------------------------------------

mod cartoon {
    use avian3d::prelude::PhysicsPlugins;
    use bevy::{input::InputPlugin, prelude::*, time::TimeUpdateStrategy};
    use pieced::{
        app::SimPlugins,
        hud::{
            DamageNumber, HitFeedbackStats, HudPlugin, INK_LAYERS, NUMBER_POOL, NumberGlyph,
            NumberKind, SQUASH_SECONDS, ammo_crystal_look, anchors, number_motion, squash_pop,
        },
        palette,
        render::CurrentFov,
        shared::{DamageDealt, DamageTarget, Player, tick_duration},
    };

    fn luminance(c: Color) -> f32 {
        let s = c.to_srgba();
        0.2126 * s.red + 0.7152 * s.green + 0.0722 * s.blue
    }

    #[test]
    fn damage_number_colors_map_to_their_types() {
        let c = DamageTarget::Character;
        // White on health, cyan on shield, gold on headshots (story 19).
        let body = NumberKind::of(c, false, 0.0).color().to_srgba();
        assert!(body.red > 0.95 && body.green > 0.95 && body.blue > 0.95);
        let shield = NumberKind::of(c, false, 20.0).color().to_srgba();
        assert!(
            shield.blue > 0.9 && shield.green > 0.6 && shield.red < 0.5,
            "shield is cyan: {shield:?}"
        );
        let head = NumberKind::of(c, true, 20.0).color().to_srgba();
        assert!(
            head.red > 0.9 && (0.6..0.9).contains(&head.green) && head.blue < 0.4,
            "headshot is gold: {head:?}"
        );
        assert_eq!(NumberKind::Shield.color(), palette::SHIELD);
        // Every number wears the same dark ink outline, thicker on bigger numbers.
        let kinds = [
            NumberKind::Body,
            NumberKind::Shield,
            NumberKind::Headshot,
            NumberKind::Structure,
        ];
        for k in kinds {
            assert_eq!(k.ink(), NumberKind::Body.ink());
            assert!(luminance(k.ink()) < 0.06, "{k:?} ink is dark");
            assert!(k.outline() >= 1.5 && k.outline() < k.size() * 0.15);
        }
        assert!(NumberKind::Headshot.outline() > NumberKind::Body.outline());
        // Structure damage stays muted next to character hits.
        assert!(NumberKind::Structure.size() < NumberKind::Body.size());
    }

    #[test]
    fn damage_numbers_squash_then_rise_and_fade() {
        // Wide and flat on the hit frame...
        let hit = squash_pop(0.0);
        assert!(hit.x > 1.2 && hit.y < 0.8, "squashed: {hit}");
        assert_eq!(number_motion(0.0, 0.85, 60.0).squash, hit);
        // ...then a tall stretch...
        let stretched = (1..16)
            .map(|i| squash_pop(i as f32 * SQUASH_SECONDS / 16.0))
            .any(|s| s.y > 1.05 && s.x < 0.95);
        assert!(stretched, "stretches after the squash");
        // ...settled well before the rise ends, then the fade.
        assert_eq!(squash_pop(SQUASH_SECONDS), Vec2::ONE);
        assert_eq!(number_motion(0.3, 0.85, 60.0).squash, Vec2::ONE);
        let late = number_motion(0.8, 0.85, 60.0);
        assert!(late.alpha < 0.2 && late.rise > 55.0);
    }

    #[test]
    fn the_ammo_crystal_dims_as_the_magazine_empties_and_flashes_on_reload() {
        let full = ammo_crystal_look(1.0);
        let half = ammo_crystal_look(0.625);
        let empty = ammo_crystal_look(0.25);
        assert_eq!(full, (1.0, 1.0));
        assert!(
            empty.0 < half.0 && half.0 < full.0,
            "dims with the magazine"
        );
        assert!(empty.0 >= 0.5, "still readable when empty: {empty:?}");
        let flash = ammo_crystal_look(1.3);
        assert!(
            flash.1 > 1.1 && flash.0 == 1.0,
            "swells in the reload flash"
        );
    }

    #[test]
    fn hud_anchors_are_milestone_ones() {
        assert_eq!(anchors::STATUS, (28.0, 26.0));
        assert_eq!(anchors::AMMO, (30.0, 20.0));
        assert_eq!(anchors::HOTBAR_BOTTOM, 20.0);
        assert_eq!(anchors::READOUT, (20.0, 18.0));
        assert_eq!(anchors::PERF, (16.0, 14.0));
        assert_eq!(anchors::PIECE, (34.0, 124.0));
    }

    /// The simulation plus the HUD, headless (no window, GPU or UI render).
    fn hud_app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            bevy::state::app::StatesPlugin,
            AssetPlugin::default(),
            bevy::mesh::MeshPlugin,
            bevy::scene::ScenePlugin,
            PhysicsPlugins::default(),
            InputPlugin,
        ))
        .add_plugins(SimPlugins)
        .add_plugins(HudPlugin)
        .init_resource::<CurrentFov>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()));
        app.finish();
        app.cleanup();
        app.update();
        app
    }

    fn count<C: Component>(app: &mut App) -> usize {
        let world = app.world_mut();
        world.query::<&C>().iter(world).count()
    }

    fn node_named(app: &mut App, name: &str) -> Node {
        let world = app.world_mut();
        world
            .query::<(&Name, &Node)>()
            .iter(world)
            .find(|(n, _)| n.as_str() == name)
            .map(|(_, node)| node.clone())
            .unwrap_or_else(|| panic!("no HUD node {name:?}"))
    }

    #[test]
    fn the_hud_keeps_milestone_ones_layout() {
        let mut app = hud_app();
        let status = node_named(&mut app, "Status");
        assert_eq!((status.left, status.bottom), (px(28), px(26)));
        let ammo = node_named(&mut app, "Ammo");
        assert_eq!((ammo.right, ammo.bottom), (px(30), px(20)));
        let hotbar = node_named(&mut app, "Hotbar");
        assert_eq!(
            (hotbar.left, hotbar.right, hotbar.bottom),
            (px(0), px(0), px(20))
        );
        assert_eq!(hotbar.align_items, AlignItems::Center);
        let readout = node_named(&mut app, "Combat readout");
        assert_eq!((readout.right, readout.top), (px(20), px(18)));
        let perf = node_named(&mut app, "Performance overlay");
        assert_eq!((perf.left, perf.top), (px(16), px(14)));
    }

    #[test]
    fn the_damage_number_pool_never_grows_under_a_30_hit_burst() {
        let mut app = hud_app();
        let texts = count::<Text>(&mut app);
        let images = count::<ImageNode>(&mut app);
        assert_eq!(count::<DamageNumber>(&mut app), NUMBER_POOL);
        assert_eq!(
            count::<NumberGlyph>(&mut app),
            NUMBER_POOL * (INK_LAYERS as usize + 1)
        );
        let player = {
            let world = app.world_mut();
            world
                .query_filtered::<Entity, With<Player>>()
                .single(world)
                .unwrap()
        };
        // 30 hits land on one tick; the last one is a headshot.
        for i in 0..30 {
            app.world_mut().write_message(DamageDealt {
                source: Some(player),
                target: player,
                target_kind: DamageTarget::Character,
                amount: 20.0 + i as f32,
                to_shield: 0.0,
                headshot: i == 29,
                shield_broke: false,
                killed: false,
                point: Vec3::new(0.0, 1.0, -10.0),
                normal: Vec3::Z,
                tick: 1,
            });
        }
        app.update();
        // Every number shows on the frame of its hit: the whole pool, recycled.
        let world = app.world_mut();
        let shown: Vec<_> = world
            .query::<(&DamageNumber, &Visibility)>()
            .iter(world)
            .filter(|(n, v)| n.active && **v != Visibility::Hidden)
            .map(|(n, _)| n.color)
            .collect();
        assert_eq!(shown.len(), NUMBER_POOL);
        assert!(shown.contains(&palette::HEADSHOT));
        assert_eq!(world.resource::<HitFeedbackStats>().hits, 30);
        // Nothing was spawned for them: no new text, images or numbers.
        assert_eq!(count::<DamageNumber>(&mut app), NUMBER_POOL);
        assert_eq!(count::<Text>(&mut app), texts);
        assert_eq!(count::<ImageNode>(&mut app), images);
        // The headshot's face is gold over ink copies.
        let world = app.world_mut();
        let mut gold_faces = 0;
        for (glyph, text, color) in world
            .query::<(&NumberGlyph, &Text, &TextColor)>()
            .iter(world)
        {
            if text.0 == "49" {
                if glyph.0 == INK_LAYERS {
                    assert_eq!(color.0, palette::HEADSHOT);
                    gold_faces += 1;
                } else {
                    assert_eq!(color.0, NumberKind::Headshot.ink());
                }
            }
        }
        assert_eq!(gold_faces, 1);
    }
}
