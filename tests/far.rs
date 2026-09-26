//! The far view's motion (gate S4, docs/M2-SPEC.md → Testing): the galaxy turns
//! at one revolution per 10 minutes (and `skyrot=off` holds it), every ship
//! flies its loop and crosses the spawn view in 10–20 s, every island bobs
//! within its amplitude, the stained glass pulses, and the galaxy cubemap is
//! deterministic and generated within its load-time budget.
//!
//! All of it runs headless: `FarMotionPlugin` plus the far view's anchors from
//! `spawn_far_view`, stepped with a manual clock. No GPU, window or models.

use bevy::{core_pipeline::Skybox, prelude::*, time::TimeUpdateStrategy};
use pieced::{
    far::{
        Bob, FarLayout, FarMotionPlugin, GALAXY_PERIOD_S, GLASS_PERIOD_S, GalaxyParams, GalaxySky,
        GalaxySpin, GlassPulse, SPAWN_EYE, ShipFlight, SkyClock, galaxy::sample_galaxy,
        generate_galaxy, glass_panes, spawn_far_view,
    },
    look::{FarMaterial, LookSettings},
    perf_knobs::PerfKnobs,
    render::QualityPreset,
};
use std::{
    f32::consts::TAU,
    path::PathBuf,
    time::{Duration, Instant},
};

const STEP: f64 = 1.0 / 60.0;

/// A headless app with the motion systems and the far view's anchors.
fn far_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<FarMaterial>()
        .init_resource::<FarLayout>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            STEP,
        )))
        .add_plugins(FarMotionPlugin)
        .add_systems(Startup, spawn_far_view);
    let layout = app.world().resource::<FarLayout>().clone();
    app.insert_resource(GalaxySpin {
        axis: layout.galaxy,
        period_s: GALAXY_PERIOD_S,
    });
    app.update();
    app
}

fn clock(app: &App) -> f64 {
    app.world().resource::<SkyClock>().seconds
}

/// Steps until the sky clock reaches `seconds`, calling `each` after every frame.
fn run_until(app: &mut App, seconds: f64, mut each: impl FnMut(&mut App)) {
    while clock(app) < seconds - 1e-9 {
        app.update();
        each(app);
    }
}

fn sky_camera(app: &mut App) -> Entity {
    app.world_mut()
        .spawn((
            Skybox {
                image: None,
                brightness: 1000.0,
                rotation: Quat::IDENTITY,
            },
            GalaxySky,
        ))
        .id()
}

#[test]
fn after_ten_seconds_the_galaxy_has_turned_a_sixtieth() {
    let mut app = far_app();
    let camera = sky_camera(&mut app);
    run_until(&mut app, 10.0, |_| {});
    let t = clock(&app);
    assert!((t - 10.0).abs() <= STEP + 1e-9, "clock {t}");
    let rotation = app.world().get::<Skybox>(camera).unwrap().rotation;
    let (axis, angle) = rotation.to_axis_angle();
    let expected = (TAU as f64 * t / GALAXY_PERIOD_S) as f32;
    assert!(
        (angle - expected).abs() < 1e-4,
        "angle {angle} rad, expected {expected} rad (about 6° after 10 s)"
    );
    assert!((expected - TAU / 60.0).abs() < 2e-3);
    // It turns about the galaxy's own core, which therefore stays put.
    let spin = *app.world().resource::<GalaxySpin>();
    assert!(axis.dot(spin.axis) > 0.9999, "axis {axis}");
    assert!((rotation * spin.axis - spin.axis).length() < 1e-5);
}

#[test]
fn skyrot_off_holds_the_galaxy_still() {
    let mut app = far_app();
    app.insert_resource(pieced::look::resolve_look(
        QualityPreset::Battery,
        Some(&PerfKnobs::parse("skyrot=off")),
        false,
    ));
    assert!(!app.world().resource::<LookSettings>().sky_rotation);
    let camera = sky_camera(&mut app);
    run_until(&mut app, 10.0, |_| {});
    let rotation = app.world().get::<Skybox>(camera).unwrap().rotation;
    assert_eq!(rotation, Quat::IDENTITY);
}

/// World position of each ship (ships fly in the station's frame).
fn ships(app: &mut App) -> Vec<(Entity, Vec3, Vec3)> {
    let frame = app.world().resource::<FarLayout>().station_frame();
    let world = app.world_mut();
    let mut q = world.query::<(Entity, &ShipFlight, &Transform)>();
    q.iter(world)
        .map(|(e, _, t)| (e, t.translation, frame.transform_point(t.translation)))
        .collect()
}

#[test]
fn every_ship_flies_at_least_five_metres_along_its_loop() {
    let mut app = far_app();
    let start = ships(&mut app);
    assert!(
        (3..=5).contains(&start.len()),
        "{} ships (3-5)",
        start.len()
    );
    let mut travelled = vec![0.0f32; start.len()];
    let mut farthest = vec![0.0f32; start.len()];
    let mut last: Vec<Vec3> = start.iter().map(|s| s.1).collect();
    let mut worst_off_loop = 0.0f32;
    run_until(&mut app, 10.0, |app| {
        for (i, (e, _, _)) in start.iter().enumerate() {
            let flight = app.world().get::<ShipFlight>(*e).unwrap();
            let p = app.world().get::<Transform>(*e).unwrap().translation;
            worst_off_loop = worst_off_loop.max(flight.path.distance_to(p));
            travelled[i] += p.distance(last[i]);
            farthest[i] = farthest[i].max(p.distance(start[i].1));
            last[i] = p;
        }
    });
    assert!(
        worst_off_loop < 0.5,
        "a ship left its loop by {worst_off_loop} m"
    );
    for (i, (e, _, _)) in start.iter().enumerate() {
        assert!(
            travelled[i] >= 5.0 && farthest[i] >= 5.0,
            "ship {e} flew {} m (at most {} m from its start) in 10 s",
            travelled[i],
            farthest[i]
        );
    }
}

#[test]
fn ships_cross_the_view_in_ten_to_twenty_seconds() {
    // Seen from the spawn eye facing -Z (70° vertical FOV, 16:10): level (T01),
    // or tilted 25° up at the station for the ships that fly between its
    // spires (T10). Crossing time = the view's width / the ship's mean angular
    // speed while it is in view.
    let half_v = 35f32.to_radians();
    let half_h = (half_v.tan() * 1.6).atan();
    let view_width_deg = 2.0 * half_h.to_degrees();
    let layout = FarLayout::default();
    let station_dir = (layout.station.position - SPAWN_EYE).normalize();
    let level = Quat::IDENTITY;
    let up = Transform::default()
        .looking_to(
            Vec3::new(station_dir.x, 0.0, station_dir.z).normalize()
                + Vec3::Y * 25f32.to_radians().tan(),
            Vec3::Y,
        )
        .rotation;
    let mut app = far_app();
    let frame = layout.station_frame();
    let entities: Vec<Entity> = ships(&mut app).iter().map(|s| s.0).collect();
    // Per view, per ship: (degrees swept while in view, seconds in view).
    let views = [("level", level), ("up at the station", up)];
    let mut swept = vec![vec![(0.0f32, 0.0f32); entities.len()]; views.len()];
    let mut last: Vec<Option<Vec3>> = vec![None; entities.len()];
    run_until(&mut app, 40.0, |app| {
        for (i, &e) in entities.iter().enumerate() {
            let local = app.world().get::<Transform>(e).unwrap().translation;
            let dir = (frame.transform_point(local) - SPAWN_EYE).normalize();
            for (v, (_, rotation)) in views.iter().enumerate() {
                let d = rotation.inverse() * dir;
                let visible = d.z < 0.0
                    && (d.x / -d.z).abs() < half_h.tan()
                    && (d.y / -d.z).abs() < half_v.tan();
                if let Some(prev) = last[i]
                    && visible
                {
                    swept[v][i].0 += prev.angle_between(dir).to_degrees();
                    swept[v][i].1 += STEP as f32;
                }
            }
            last[i] = Some(dir);
        }
    });
    let mut level_seen = 0;
    for i in 0..entities.len() {
        let (v, &(deg, secs)) = swept
            .iter()
            .map(|per_ship| &per_ship[i])
            .enumerate()
            .find(|(_, s)| s.1 >= 1.0)
            .unwrap_or_else(|| panic!("ship {i} never shows from the arena"));
        level_seen += usize::from(v == 0);
        let crossing = view_width_deg / (deg / secs);
        println!(
            "ship {i} ({} view): {:.1}°/s → crosses the view in {crossing:.1} s",
            views[v].0,
            deg / secs
        );
        assert!(
            (10.0..=20.0).contains(&crossing),
            "ship {i} would cross the view in {crossing:.1} s"
        );
    }
    assert!(
        level_seen >= 3,
        "at least three ships show from spawn ({level_seen})"
    );
}

#[test]
fn every_island_bobs_within_its_amplitude() {
    let mut app = far_app();
    let islands: Vec<(Entity, Bob)> = {
        let world = app.world_mut();
        let mut q = world.query::<(Entity, &Bob)>();
        q.iter(world).map(|(e, b)| (e, *b)).collect()
    };
    let layout = app.world().resource::<FarLayout>().clone();
    assert_eq!(islands.len(), layout.islands.len());
    let mut range: Vec<(f32, f32)> = vec![(f32::MAX, f32::MIN); islands.len()];
    run_until(&mut app, 12.0, |app| {
        for (i, (e, bob)) in islands.iter().enumerate() {
            let t = app.world().get::<Transform>(*e).unwrap().translation;
            let dy = t.y - bob.base.y;
            assert!(
                dy.abs() <= bob.amplitude + 1e-3,
                "island {i} strayed {dy} m (amplitude {})",
                bob.amplitude
            );
            assert_eq!(Vec2::new(t.x, t.z), Vec2::new(bob.base.x, bob.base.z));
            range[i] = (range[i].0.min(dy), range[i].1.max(dy));
        }
    });
    for (i, (_, bob)) in islands.iter().enumerate() {
        let span = range[i].1 - range[i].0;
        assert!(
            span > 1.8 * bob.amplitude,
            "island {i} only moved {span} m (amplitude {})",
            bob.amplitude
        );
    }
}

#[test]
fn the_stained_glass_pulses_over_its_period() {
    assert!((4.0..=6.0).contains(&GLASS_PERIOD_S));
    let mut app = far_app();
    let panes = {
        let mut far = app.world_mut().resource_mut::<Assets<FarMaterial>>();
        glass_panes(&mut far)
    };
    app.world_mut().resource_mut::<GlassPulse>().panes = panes.clone();
    let mut samples: Vec<Vec<(f32, f32)>> = vec![Vec::new(); panes.len()];
    let end = clock(&app) + GLASS_PERIOD_S as f64;
    run_until(&mut app, end, |app| {
        let far = app.world().resource::<Assets<FarMaterial>>();
        for (i, pane) in panes.iter().enumerate() {
            let m = far.get(&pane.material).unwrap();
            samples[i].push((m.base_color.to_linear().red, m.emissive_strength));
        }
    });
    for (i, s) in samples.iter().enumerate() {
        let (lo, hi) = s.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &(_, e)| {
            (lo.min(e), hi.max(e))
        });
        let (blo, bhi) = s.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &(b, _)| {
            (lo.min(b), hi.max(b))
        });
        assert!(hi - lo > 0.05, "pane {i} emissive only varies {lo}..{hi}");
        assert!(
            bhi - blo > 0.2,
            "pane {i} brightness only varies {blo}..{bhi}"
        );
        // One full period: it comes back round (first and last samples agree).
        let (first, last) = (s.first().unwrap().1, s.last().unwrap().1);
        assert!((first - last).abs() < 0.02, "pane {i}: {first} vs {last}");
    }
    // The sails don't all pulse together.
    let peak = |s: &Vec<(f32, f32)>| {
        s.iter()
            .enumerate()
            .max_by(|a, b| a.1.1.total_cmp(&b.1.1))
            .unwrap()
            .0
    };
    assert_ne!(peak(&samples[0]), peak(&samples[1]));
}

#[test]
fn the_galaxy_is_deterministic_and_generates_within_budget() {
    let small = GalaxyParams {
        face: 96,
        ..default()
    };
    let a = generate_galaxy(&small);
    let b = generate_galaxy(&small);
    assert_eq!(a.len(), 96 * 96 * 4 * 6);
    assert!(a == b, "same seed, same sky");
    let other = generate_galaxy(&GalaxyParams {
        seed: small.seed ^ 1,
        ..small.clone()
    });
    assert!(a != other, "the seed matters");

    // The real size. The budget is about 300 ms in a release build on the M4;
    // tests run at opt-level 1 on a machine that may be busy, so the bound here
    // is generous and the real number is printed.
    let params = GalaxyParams::default();
    let start = Instant::now();
    let full = generate_galaxy(&params);
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    println!(
        "galaxy {}² × 6 generated in {ms:.0} ms (debug-profile test build)",
        params.face
    );
    assert_eq!(full.len(), (params.face * params.face * 4 * 6) as usize);
    assert!(ms < 6000.0, "galaxy generation took {ms:.0} ms");
    // The core is where the layout says, upper left of the spawn view.
    let core = sample_galaxy(&full, params.face, params.center);
    assert!(
        core.iter().map(|&c| c as u32).sum::<u32>() > 600,
        "{core:?}"
    );
}

/// Writes the galaxy as an equirectangular panorama and a pinhole spawn view,
/// for review with an image viewer.
///
/// ```sh
/// PIECED_LOOK_OUT=/some/dir cargo test --locked --test far -- --ignored --nocapture
/// ```
#[test]
#[ignore = "writes review images"]
fn review_the_galaxy() {
    let out = std::env::var("PIECED_LOOK_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("pieced-look"));
    std::fs::create_dir_all(&out).unwrap();
    let params = GalaxyParams::default();
    let data = generate_galaxy(&params);
    let save = |name: &str, w: u32, h: u32, dir: &dyn Fn(u32, u32) -> Vec3| {
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let c = sample_galaxy(&data, params.face, dir(x, y));
                px.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        let image = Image::new(
            bevy::render::render_resource::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            px,
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            bevy::asset::RenderAssetUsages::MAIN_WORLD,
        );
        let path = out.join(name);
        image.try_into_dynamic().unwrap().save(&path).unwrap();
        println!("wrote {}", path.display());
    };
    save("galaxy-panorama.png", 1536, 768, &|x, y| {
        let az = (x as f32 + 0.5) / 1536.0 * TAU - std::f32::consts::PI;
        let el = std::f32::consts::FRAC_PI_2 - (y as f32 + 0.5) / 768.0 * std::f32::consts::PI;
        Vec3::new(az.sin() * el.cos(), el.sin(), -az.cos() * el.cos())
    });
    let (w, h) = (1472u32, 920u32);
    let tan_v = 35f32.to_radians().tan();
    save("galaxy-spawn-view.png", w, h, &|x, y| {
        let sx = ((x as f32 + 0.5) / w as f32 * 2.0 - 1.0) * tan_v * w as f32 / h as f32;
        let sy = (1.0 - (y as f32 + 0.5) / h as f32 * 2.0) * tan_v;
        Vec3::new(sx, sy, -1.0).normalize()
    });
}
