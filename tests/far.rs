//! The far view's motion (gate S4, docs/M2-SPEC.md → Testing): the galaxy turns
//! at one revolution per 10 minutes (and `skyrot=off` holds it), every ship
//! flies its loop and crosses the spawn view in 10–20 s, every island bobs
//! within its amplitude, the stained glass pulses, and the galaxy cubemap is
//! deterministic and generated within its load-time budget.
//!
//! The castle and the magical sky (M3, docs/M3-SPEC.md → The castle and the
//! sky): shooting stars fire every 8–20 s and cross the sky, motes drift
//! inside their boxes, lanterns bob and stream up into the sky, the rune ring
//! turns about its tilted axis, and the galaxy shimmer and aurora vary over
//! their periods.
//!
//! All of it runs headless: `FarMotionPlugin` plus the far view's anchors from
//! `spawn_far_view`, stepped with a manual clock. No GPU, window or models. The
//! far layer's triangle and batch budget is checked from the model sidecars.

use bevy::{core_pipeline::Skybox, prelude::*, time::TimeUpdateStrategy};
use pieced::{
    far::{
        Aurora, Bob, Ember, FarLayout, FarMotionPlugin, GALAXY_PERIOD_S, GLASS_PERIOD_S,
        GalaxyLayer, GalaxyParams, GalaxySky, GalaxySpin, GlassPulse, Lantern, LanternPath, Mote,
        RuneRing, SHIMMER_PERIOD_S, SPAWN_EYE, ShipFlight, ShootingStar, ShootingStars, SkyClock,
        SkyGlow, SkyMagic, Twinkle, aurora_level,
        galaxy::sample_galaxy,
        galaxy_rotation, generate_galaxy, glass_panes,
        magic::{self, AURORA_BREATH_S, AURORA_SWAY_S, MAX_MOTES, create_magic_assets},
        shimmer_level, spawn_far_view, spawn_sky_magic,
    },
    look::{FarMaterial, LookSettings},
    models::Sidecar,
    perf_knobs::PerfKnobs,
    render::QualityPreset,
};
use std::{
    f32::consts::TAU,
    path::PathBuf,
    time::{Duration, Instant},
};

const STEP: f64 = 1.0 / 60.0;

/// A headless app with the motion systems, the far view's anchors and the
/// castle's magic (its materials too, so they can breathe; nothing is drawn).
fn far_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<FarMaterial>()
        .init_asset::<Mesh>()
        .init_resource::<FarLayout>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            STEP,
        )))
        .add_plugins(FarMotionPlugin)
        .add_systems(
            Startup,
            (spawn_far_view, spawn_sky_magic, create_magic_assets).chain(),
        );
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
    let mut slow_or_fast = Vec::new();
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
        if !(10.0..=20.0).contains(&crossing) {
            slow_or_fast.push(format!("ship {i}: {crossing:.1} s"));
        }
    }
    assert!(slow_or_fast.is_empty(), "outside 10-20 s: {slow_or_fast:?}");
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
    assert_eq!(islands.len(), layout.all_islands().count());
    assert!(islands.len() >= 40, "{} islands", islands.len());
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

/// The far layer's budget (performance first; M3 raised it for the castle
/// and the magical sky, docs/M3-SPEC.md): the whole far view stays within 90k
/// triangles, and however many islands and lanterns float, the opaque far
/// models draw as at most 18 batches, because every island instance shares
/// its model's meshes and the one island material, every lantern shares one
/// mesh and material (Bevy instances equal mesh and material pairs), and the
/// castle is a handful of merged parts. Every waterfall draws from one shared
/// strip mesh with one material. At most 64 halos glow.
#[test]
fn the_far_layer_stays_within_its_triangle_and_batch_budget() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/models");
    let side = |name: &str| {
        Sidecar::parse(&std::fs::read_to_string(dir.join(format!("{name}.json"))).unwrap()).unwrap()
    };
    let layout = FarLayout::default();
    let falls = |s: &Sidecar| {
        s.parts
            .iter()
            .filter(|(n, _)| n.starts_with("Waterfall"))
            .map(|(_, p)| p.triangles)
            .sum::<u32>()
    };
    let station = side("station");
    let planet = side("planet");
    let ship = side("ship");
    let ships: usize = layout.ships.iter().map(|s| s.starts.len()).sum();
    let mut triangles = station.triangles + planet.triangles + ship.triangles * ships as u32;
    let mut island_models = std::collections::BTreeSet::new();
    let mut waterfalls = station
        .parts
        .keys()
        .filter(|n| n.starts_with("Waterfall"))
        .count();
    // Additive halos: the castle's glows, aura and mists, the ships' engines,
    // the planet's atmosphere, and the mists of the islands inside MIST_RANGE.
    let mut halos = station
        .attach
        .keys()
        .filter(|n| n.starts_with("Glow") || n.starts_with("Mist") || n.starts_with("Aura"))
        .count()
        + ships
        + 1;
    for island in layout.all_islands() {
        let s = side(island.piece.model);
        island_models.insert(island.piece.model);
        triangles += s.triangles - if island.waterfall { 0 } else { falls(&s) };
        if island.waterfall {
            let n = s
                .parts
                .keys()
                .filter(|n| n.starts_with("Waterfall"))
                .count();
            waterfalls += n;
            if island.piece.position.xz().length() < pieced::far::MIST_RANGE {
                halos += n;
            }
        }
    }
    let horizon = pieced::far::horizon_mesh(layout.horizon_radius)
        .indices()
        .unwrap()
        .len() as u32
        / 3;
    triangles += horizon;
    // The castle's magic and the sky: ring, lanterns, embers, motes, twinkles,
    // shooting stars and aurora.
    let sky = SkyMagic::new(&layout);
    let magic_triangles = magic::magic_triangles(&sky);
    triangles += magic_triangles;
    // Opaque batches: each distinct (mesh, material) among the far models,
    // plus the lanterns (one instanced batch).
    let opaque = |s: &Sidecar| {
        s.parts
            .keys()
            .filter(|n| !n.starts_with("Waterfall") && n.as_str() != "Trail")
            .count()
    };
    let batches = opaque(&station)
        + opaque(&planet)
        + opaque(&ship)
        + island_models
            .iter()
            .map(|m| opaque(&side(m)))
            .sum::<usize>()
        + 1;
    println!(
        "far layer: {triangles} triangles ({} castle, {magic_triangles} magic with {} \
         lanterns), {} islands from {} models, {waterfalls} waterfalls (one mesh), {halos} \
         halos, {batches} opaque batches",
        station.triangles,
        sky.lanterns.len(),
        layout.all_islands().count(),
        island_models.len()
    );
    assert!(triangles <= 90_000, "{triangles} far triangles");
    assert!(batches <= 18, "{batches} opaque far batches");
    assert!(halos <= 64, "{halos} far halos");
    assert!(island_models.len() <= 5);
    assert!(sky.motes.len() <= MAX_MOTES);
}

// ---------------------------------------------------------------------------
// The castle's magic and the magical sky (M3)
// ---------------------------------------------------------------------------

fn entities_with<C: Component + Copy>(app: &mut App) -> Vec<(Entity, C)> {
    let world = app.world_mut();
    let mut q = world.query::<(Entity, &C)>();
    q.iter(world).map(|(e, c)| (e, *c)).collect()
}

fn translation(app: &App, e: Entity) -> Vec3 {
    app.world().get::<Transform>(e).unwrap().translation
}

#[test]
fn shooting_stars_fire_every_8_to_20_seconds_and_cross_the_sky() {
    let mut app = far_app();
    let streaks = entities_with::<ShootingStar>(&mut app);
    assert!(
        (1..=4).contains(&streaks.len()),
        "a small pool: {}",
        streaks.len()
    );
    // Each pass seen: (first seen s, last seen s, first direction, last direction).
    let mut runs: Vec<(f64, f64, Vec3, Vec3)> = Vec::new();
    let mut showing: Option<Entity> = None;
    run_until(&mut app, 75.0, |app| {
        let t = clock(app);
        let visible: Vec<Entity> = streaks
            .iter()
            .map(|s| s.0)
            .filter(|&e| app.world().get::<Visibility>(e) != Some(&Visibility::Hidden))
            .collect();
        assert!(visible.len() <= 1, "one star at a time: {visible:?}");
        match visible.first() {
            Some(&e) => {
                let dir = translation(app, e).normalize();
                if showing == Some(e) {
                    let run = runs.last_mut().unwrap();
                    run.1 = t;
                    run.3 = dir;
                } else {
                    runs.push((t, t, dir, dir));
                }
                showing = Some(e);
            }
            None => showing = None,
        }
    });
    assert!(runs.len() >= 4, "{} shooting stars in 75 s", runs.len());
    for pair in runs.windows(2) {
        let gap = pair[1].0 - pair[0].0;
        assert!(
            (8.0 - 2.0 * STEP..=20.0 + 2.0 * STEP).contains(&gap),
            "{gap:.2} s between two shooting stars"
        );
    }
    for (i, &(t0, t1, d0, d1)) in runs.iter().enumerate() {
        let secs = t1 - t0;
        let swept = d0.angle_between(d1).to_degrees();
        println!("shooting star {i}: {secs:.2} s, {swept:.1}° across, from {d0}");
        assert!((0.9..=1.7).contains(&secs), "star {i} flew {secs} s");
        assert!(swept >= 12.0, "star {i} only crossed {swept}°");
        // Up in the sky, not under the horizon.
        assert!(d0.y > 0.3, "star {i} starts at {d0}");
    }
    // The schedule is seeded: the same passes every run.
    let a = ShootingStars::default().passes(10);
    assert_eq!(a, ShootingStars::default().passes(10));
    assert!((a[0].start_s - runs[0].0).abs() < 2.0 * STEP);
}

#[test]
fn motes_drift_inside_their_boxes() {
    let mut app = far_app();
    let motes = entities_with::<Mote>(&mut app);
    assert!(
        (16..=MAX_MOTES).contains(&motes.len()),
        "{} motes",
        motes.len()
    );
    let starts: Vec<Vec3> = motes.iter().map(|m| translation(&app, m.0)).collect();
    let mut farthest = vec![0.0f32; motes.len()];
    run_until(&mut app, 30.0, |app| {
        for (i, (e, mote)) in motes.iter().enumerate() {
            let p = translation(app, *e);
            let off = (p - mote.centre).abs();
            assert!(
                off.cmple(mote.extent + Vec3::splat(1e-3)).all(),
                "mote {i} left its box: {off} > {}",
                mote.extent
            );
            farthest[i] = farthest[i].max(p.distance(starts[i]));
        }
    });
    for (i, (_, mote)) in motes.iter().enumerate() {
        assert!(
            farthest[i] > 0.3 * mote.extent.min_element(),
            "mote {i} barely moved ({} m)",
            farthest[i]
        );
    }
    // Some round the arena, some out round the far islands.
    let near = motes.iter().filter(|m| m.1.centre.length() < 80.0).count();
    assert!(near >= 10 && motes.len() - near >= 10, "{near} near");
}

#[test]
fn lanterns_bob_and_stream_up_into_the_sky() {
    let mut app = far_app();
    let lanterns = entities_with::<Lantern>(&mut app);
    let swarm = lanterns
        .iter()
        .filter(|l| matches!(l.1.path, LanternPath::Swarm { .. }))
        .count();
    let streams: Vec<_> = lanterns
        .iter()
        .filter(|l| matches!(l.1.path, LanternPath::Stream { .. }))
        .collect();
    assert!(swarm >= 100, "{swarm} lanterns in the swarm");
    assert!(streams.len() >= 40, "{} lanterns streaming", streams.len());
    let mut y_range = vec![(f32::MAX, f32::MIN); lanterns.len()];
    let start: Vec<(Vec3, f32)> = lanterns
        .iter()
        .map(|l| {
            let t = app.world().get::<Transform>(l.0).unwrap();
            (t.translation, t.scale.x)
        })
        .collect();
    run_until(&mut app, 10.0, |app| {
        for (i, (e, _)) in lanterns.iter().enumerate() {
            let y = translation(app, *e).y;
            y_range[i] = (y_range[i].0.min(y), y_range[i].1.max(y));
        }
    });
    for (i, (e, lantern)) in lanterns.iter().enumerate() {
        let p = translation(&app, *e);
        match lantern.path {
            LanternPath::Swarm {
                home, drift, bob, ..
            } => {
                // Bobs through most of its range in 10 s, never strays from home.
                let span = y_range[i].1 - y_range[i].0;
                assert!(span > 1.5 * bob, "lantern {i} bobbed {span} m of ±{bob}");
                let off = p - home;
                assert!(off.x.abs() <= drift.x + 1e-3 && off.z.abs() <= drift.y + 1e-3);
                assert!(off.y.abs() <= bob + 1e-3);
            }
            LanternPath::Stream { from, rise, .. } => {
                // Rising (unless it reached the top and started again from
                // the bottom in these 10 s).
                let p0 = start[i].0;
                if p.y > p0.y {
                    assert!(p.y - p0.y > 10.0, "lantern {i} only rose {} m", p.y - p0.y);
                } else {
                    assert!(
                        p0.y > from.y + rise.y * 0.75 && p.y < from.y + rise.y * 0.25,
                        "lantern {i} sank from {} to {}",
                        p0.y,
                        p.y
                    );
                }
                assert!(p.y >= from.y - 1.0 && p.y <= from.y + rise.y + 1.0);
            }
        }
    }
    // The streams reach high into the sky over the castle.
    let top = streams
        .iter()
        .filter_map(|l| match l.1.path {
            LanternPath::Stream { from, rise, .. } => Some(from.y + rise.y),
            _ => None,
        })
        .fold(0.0f32, f32::max);
    assert!(top > 500.0, "streams top out at {top} m");
    // Gold embers drift up off the castle's glow too.
    let embers = entities_with::<Ember>(&mut app);
    assert!(!embers.is_empty());
    for (e, ember) in &embers {
        let y = translation(&app, *e).y;
        assert!(y >= ember.from.y - 1e-3 && y <= ember.from.y + ember.rise.y + 1e-3);
    }
}

#[test]
fn the_rune_ring_turns_about_its_tilted_axis() {
    let mut app = far_app();
    let rings = entities_with::<RuneRing>(&mut app);
    assert_eq!(rings.len(), 1, "one rune ring");
    let (e, ring) = rings[0];
    let r0 = app.world().get::<Transform>(e).unwrap().rotation;
    let t0 = clock(&app);
    run_until(&mut app, 10.0, |_| {});
    let t1 = clock(&app);
    let r1 = app.world().get::<Transform>(e).unwrap().rotation;
    let (axis, angle) = (r1 * r0.inverse()).to_axis_angle();
    let expected = TAU * ((t1 - t0) as f32) / ring.period_s;
    assert!(
        (angle - expected).abs() < 1e-3,
        "turned {angle} rad, expected {expected}"
    );
    assert!(axis.dot(ring.axis()).abs() > 0.9999, "about its own axis");
    // Slow and stately: a turn takes minutes, not seconds.
    assert!((60.0..=300.0).contains(&ring.period_s));
    // Tilted 20–40° off vertical, its front dipping toward the arena (-Z in
    // the station's frame), so from the arena below it opens as an ellipse
    // (C4) instead of lying edge-on.
    let tilt = ring.axis().angle_between(Vec3::Y).to_degrees();
    assert!((20.0..=40.0).contains(&tilt), "tilt {tilt}°");
    assert!(ring.axis().z < 0.0, "front dips toward the arena");
}

#[test]
fn the_galaxy_shimmers_and_the_aurora_drifts_over_their_periods() {
    assert!((4.0..=8.0).contains(&SHIMMER_PERIOD_S));
    let mut app = far_app();
    let glow = app.world().resource::<SkyGlow>().clone();
    let twinkle_mat = glow.twinkle.expect("the twinkle material");
    let aurora_mat = glow.aurora.expect("the aurora material");
    let twinkles = entities_with::<Twinkle>(&mut app);
    let auroras = entities_with::<Aurora>(&mut app);
    assert!(twinkles.len() >= 12 && (2..=5).contains(&auroras.len()));
    let level = |app: &App, h: &Handle<FarMaterial>| {
        app.world()
            .resource::<Assets<FarMaterial>>()
            .get(h)
            .unwrap()
            .base_color
            .to_linear()
            .red
    };
    let mut shimmer = Vec::new();
    let mut scales = vec![(f32::MAX, f32::MIN); twinkles.len()];
    let end = clock(&app) + SHIMMER_PERIOD_S as f64;
    run_until(&mut app, end, |app| {
        shimmer.push(level(app, &twinkle_mat));
        for (i, (e, _)) in twinkles.iter().enumerate() {
            let s = app.world().get::<Transform>(*e).unwrap().scale.x;
            scales[i] = (scales[i].0.min(s), scales[i].1.max(s));
        }
    });
    let (lo, hi) = shimmer
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    assert!(hi - lo > 0.3 * hi, "the shimmer only varies {lo}..{hi}");
    let (first, last) = (shimmer[0], *shimmer.last().unwrap());
    assert!(
        (first - last).abs() < 0.02,
        "one full cycle: {first} vs {last}"
    );
    for (i, (lo, hi)) in scales.iter().enumerate() {
        assert!(*hi > 2.0 * lo, "twinkle {i} only swells {lo}..{hi}");
    }
    assert!((shimmer_level(0.0) - shimmer_level(SHIMMER_PERIOD_S as f64)).abs() < 1e-4);
    // The twinkles turn with the galaxy (they sit on its sky).
    let layer = entities_with::<GalaxyLayer>(&mut app)[0].0;
    let spin = *app.world().resource::<GalaxySpin>();
    let rotation = app.world().get::<Transform>(layer).unwrap().rotation;
    assert!(rotation.dot(galaxy_rotation(&spin, clock(&app))).abs() > 0.999_999);
    // The aurora sways and breathes.
    let start: Vec<Quat> = auroras
        .iter()
        .map(|a| app.world().get::<Transform>(a.0).unwrap().rotation)
        .collect();
    let mut swayed = vec![0.0f32; auroras.len()];
    let mut breath = (f32::MAX, f32::MIN);
    let end = clock(&app) + AURORA_SWAY_S as f64;
    run_until(&mut app, end, |app| {
        for (i, (e, _)) in auroras.iter().enumerate() {
            let r = app.world().get::<Transform>(*e).unwrap().rotation;
            swayed[i] = swayed[i].max(r.angle_between(start[i]).to_degrees());
        }
        let l = level(app, &aurora_mat);
        breath = (breath.0.min(l), breath.1.max(l));
    });
    for (i, s) in swayed.iter().enumerate() {
        assert!(*s > 3.0, "aurora {i} only swayed {s}°");
    }
    assert!(
        breath.1 - breath.0 > 0.3 * breath.1,
        "aurora breath {breath:?}"
    );
    assert!((aurora_level(0.0) - aurora_level(AURORA_BREATH_S as f64)).abs() < 1e-4);
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
