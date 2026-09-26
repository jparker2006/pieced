//! The target-board gallery (docs/M2-SPEC.md → The target board and gallery;
//! gate S1) and its freeze hook, headless:
//!
//! - the table has the twelve views, one per target (T01-T12), well formed;
//! - a whole gallery run through the simulation seam captures every view, in
//!   order, on its moment: the shots do what the target shows (a body hit, a
//!   headshot, a shield break, the elimination), the fort's pieces all place and
//!   the ghost shows beside it, the pause menu is up, and the knight's chest sits
//!   where the target paints him, in view and not behind a piece;
//! - [`GalleryFreeze`] stops the knight's animation clock and eye timers, and the
//!   dummy's movement (strafing, mid-jump, respawn), and everything resumes when
//!   it lifts.
//!
//! The render of the same views is `tests/gallery_offscreen.rs` (needs a GPU).

use bevy::{prelude::*, time::TimeUpdateStrategy};
use pieced::{
    arena::visuals::{TargetFigure, animate_knights},
    building::{PieceSlot, initial_cover},
    combat::Downed,
    dummy::Dummy,
    knight::{EyeState, KnightAnim, KnightEvent, KnightInput},
    movement::Motor,
    scenario::gallery::{
        Act, Expect, GALLERY_FOV_DEG, GalleryPlugin, GalleryRunner, GalleryView, KnightMotion,
        Moment, STATION, VIEW_TIMEOUT, ViewRecord, galaxy_dir, screen_point, views,
    },
    shared::{
        ARENA_HALF, Character, DamageDealt, FreezableTime, GalleryFreeze, GameCue, Health,
        LookAngles, tick_duration,
    },
    sim::Sim,
    tuning::Tuning,
};

/// The target images in `docs/design/concepts/`, in board order.
const TARGETS: [&str; 12] = [
    "T01-spawn-vista",
    "T02-rifle-idle",
    "T03-rifle-bolt",
    "T04-pump-fan",
    "T05-knight-hit",
    "T06-headshot",
    "T07-shield-break",
    "T08-elimination",
    "T09-fort",
    "T10-station-up",
    "T11-island-edge",
    "T12-pause-menu",
];

/// How far (half-heights) the knight may sit from where the target paints him.
const FRAMING_TOLERANCE: f32 = 0.1;
/// The game's frame is 16:10: half-widths reach 1.6 half-heights.
const HALF_WIDTH: f32 = 1.6;

#[test]
fn the_table_has_one_view_per_target() {
    let table = views();
    let names: Vec<&str> = table.iter().map(|v| v.name).collect();
    assert_eq!(names, TARGETS);
    for (i, view) in table.iter().enumerate() {
        assert_eq!(view.id, format!("T{:02}", i + 1));
        assert!(view.name.starts_with(view.id));
        assert!(!view.title.is_empty());
    }
}

#[test]
fn every_view_is_well_formed() {
    let cover: Vec<PieceSlot> = initial_cover();
    let inside = |p: Vec3| p.x.abs() < ARENA_HALF - 0.5 && p.z.abs() < ARENA_HALF - 0.5;
    for view in views() {
        let id = view.id;
        assert!(
            inside(view.feet),
            "{id}: the player stands outside the arena"
        );
        // Scripts run in order; views that capture a shot fire one first.
        assert!(view.script.windows(2).all(|w| w[0].0 <= w[1].0), "{id}");
        let fires = view.script.iter().any(|(_, a)| *a == Act::Fire);
        match view.moment {
            Moment::AfterShot(n) => {
                assert!(fires && n >= 1, "{id}: captures a shot it never fires")
            }
            Moment::Settled(n) => {
                assert!(!fires && n < VIEW_TIMEOUT, "{id}: fires but captures blind")
            }
        }
        if let Some(knight) = view.knight {
            assert!(inside(knight.feet), "{id}: the knight stands outside");
            if let KnightMotion::Run { azimuth, lead } = knight.motion {
                let start = knight.feet - pieced::scenario::gallery::azimuth_dir(azimuth) * lead;
                assert!(inside(start), "{id}: the knight's run starts outside");
            }
            assert!(knight.hp > 0.0 && knight.shield >= 0.0);
            assert!(knight.screen.x.abs() < HALF_WIDTH && knight.screen.y.abs() < 1.0);
        }
        // The view's pieces stay clear of the arena's own cover.
        for slot in &view.pieces {
            assert!(slot.in_arena() && slot.level_ok(), "{id}: {slot:?}");
            assert!(
                cover.iter().all(|c| c.key() != slot.key()),
                "{id}: {slot:?} is initial cover"
            );
        }
    }
}

/// Plays the whole gallery headless and returns the capture order and records.
fn run_gallery() -> (Vec<&'static str>, Vec<ViewRecord>, Vec<GalleryView>) {
    let mut sim = Sim::new();
    let table = views();
    let mut runner = GalleryRunner::new(table.clone(), 10);
    let mut captured = Vec::new();
    for _ in 0..20_000 {
        let running = runner.frame(sim.world_mut());
        captured.extend(runner.take_captures());
        if !running {
            break;
        }
        sim.tick();
    }
    assert!(runner.is_done(), "the gallery never finished");
    (captured, runner.records().to_vec(), table)
}

/// Does the segment `a → b` pass through the box `min..max`?
fn segment_hits_box(a: Vec3, b: Vec3, min: Vec3, max: Vec3) -> bool {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for axis in 0..3 {
        if d[axis].abs() < 1e-6 {
            if a[axis] < min[axis] || a[axis] > max[axis] {
                return false;
            }
            continue;
        }
        let (mut near, mut far) = (
            (min[axis] - a[axis]) / d[axis],
            (max[axis] - a[axis]) / d[axis],
        );
        if near > far {
            std::mem::swap(&mut near, &mut far);
        }
        t0 = t0.max(near);
        t1 = t1.min(far);
        if t0 > t1 {
            return false;
        }
    }
    true
}

#[test]
fn a_gallery_run_captures_every_view_on_its_moment() {
    let (captured, records, table) = run_gallery();
    assert_eq!(captured, TARGETS, "captured out of order or missing");
    assert_eq!(records.len(), 12);
    let fov = GALLERY_FOV_DEG.to_radians();
    let building = Tuning::default().building;
    for (view, record) in table.iter().zip(&records) {
        let id = view.id;
        assert!(!record.missed, "{id}: never reached its moment: {record:?}");
        assert!(
            record.rejected.is_empty(),
            "{id}: pieces rejected: {:?}",
            record.rejected
        );
        let (shots, hits) = (record.shots, record.hits);
        match view.expect {
            Expect::Still | Expect::Ghost(_) | Expect::Paused => {
                assert_eq!(shots, 0, "{id}: fired")
            }
            Expect::Hit => assert!(
                hits >= 1
                    && record.kills == 0
                    && record.headshots == 0
                    && record.shield_breaks == 0,
                "{id}: wanted a plain body hit: {record:?}"
            ),
            Expect::Headshot => assert!(
                record.headshots >= 1 && record.kills == 0,
                "{id}: wanted a headshot: {record:?}"
            ),
            Expect::ShieldBreak => assert!(
                record.shield_breaks >= 1 && record.kills == 0,
                "{id}: wanted a shield break: {record:?}"
            ),
            Expect::Kill => assert_eq!(record.kills, 1, "{id}: wanted the elimination"),
        }
        if let Expect::Ghost(slot) = view.expect {
            assert_eq!(record.ghost, Some((slot, true)), "{id}: the ghost");
        }
        assert_eq!(record.paused, view.expect == Expect::Paused, "{id}: paused");

        // Composition: the knight's chest where the target paints him, or out
        // of view.
        let camera = record.camera.expect("camera at the moment");
        let feet = Vec3::from_array(record.knight_feet.expect("the knight exists"));
        let chest = feet + Vec3::Y;
        let on_screen =
            screen_point(&camera, fov, chest).filter(|s| s.x.abs() < HALF_WIDTH && s.y.abs() < 1.0);
        match view.knight {
            Some(knight) => {
                let at = on_screen.unwrap_or_else(|| panic!("{id}: the knight is out of view"));
                assert!(
                    at.distance(knight.screen) < FRAMING_TOLERANCE,
                    "{id}: the knight shows at {at}, the target at {}",
                    knight.screen
                );
                // Nothing built stands between the eye and his chest.
                let mut pieces = initial_cover();
                pieces.extend(view.pieces.iter().copied());
                for slot in pieces {
                    let (min, max) = slot.aabb(&building);
                    assert!(
                        !segment_hits_box(camera.translation, chest, min, max),
                        "{id}: {slot:?} hides the knight"
                    );
                }
            }
            None => assert!(on_screen.is_none(), "{id}: the parked knight is in view"),
        }
    }

    // The far view where the targets show it.
    let record = |id: &str| records.iter().find(|r| r.id == id).unwrap();
    let at = |id: &str, point: Vec3| screen_point(&record(id).camera.unwrap(), fov, point);
    let far = |id: &str, dir: Vec3| {
        let camera = record(id).camera.unwrap();
        screen_point(&camera, fov, camera.translation + dir * 1000.0)
    };
    // T01: station upper right, galaxy upper left.
    let station = at("T01", STATION).unwrap();
    assert!(
        station.x > 0.5 && station.y > 0.2,
        "T01 station at {station}"
    );
    let galaxy = far("T01", galaxy_dir()).unwrap();
    assert!(galaxy.x < -0.3 && galaxy.y > 0.5, "T01 galaxy at {galaxy}");
    // T02 station right, T03 station left, T10 station ahead and up.
    assert!(at("T02", STATION).unwrap().x > 0.5);
    assert!(at("T03", STATION).unwrap().x < -0.4);
    let t10 = at("T10", STATION).unwrap();
    assert!(t10.length() < 0.4, "T10 station at {t10}");
    assert!(
        record("T10").camera.unwrap().forward().y > 0.2,
        "T10 looks up"
    );
}

// ---------------------------------------------------------------------------
// The freeze hook
// ---------------------------------------------------------------------------

fn dummy(sim: &mut Sim) -> Entity {
    sim.world_mut()
        .query_filtered::<Entity, With<Dummy>>()
        .single(sim.world())
        .expect("one dummy")
}

#[test]
fn freeze_stops_the_strafing_dummy_and_lifting_it_lets_him_go() {
    let mut sim = Sim::new();
    let dummy = dummy(&mut sim);
    sim.tuning_mut().dummy.jump_chance_per_sec = 0.0;
    sim.run_seconds(1.0);
    let before = sim.feet(dummy);
    sim.run_seconds(0.3);
    assert!(sim.feet(dummy).distance(before) > 0.5, "the dummy strafes");

    sim.world_mut().insert_resource(GalleryFreeze);
    let held = sim.feet(dummy);
    for _ in 0..120 {
        sim.tick();
        assert_eq!(sim.feet(dummy), held, "moved while frozen");
    }
    sim.world_mut().remove_resource::<GalleryFreeze>();
    sim.run_seconds(0.5);
    assert!(sim.feet(dummy).distance(held) > 0.5, "didn't resume");
}

#[test]
fn freeze_holds_the_dummy_mid_jump() {
    let mut sim = Sim::new();
    let dummy = dummy(&mut sim);
    sim.tuning_mut().dummy.jump_chance_per_sec = 1000.0;
    let mut airborne = false;
    for _ in 0..240 {
        sim.tick();
        if sim.feet(dummy).y > 0.4 {
            airborne = true;
            break;
        }
    }
    assert!(airborne, "the dummy never jumped");
    sim.world_mut().insert_resource(GalleryFreeze);
    let held = sim.feet(dummy);
    sim.run_seconds(1.0);
    assert_eq!(sim.feet(dummy), held, "fell while frozen");
    sim.tuning_mut().dummy.jump_chance_per_sec = 0.0;
    sim.world_mut().remove_resource::<GalleryFreeze>();
    sim.run_seconds(1.5);
    assert!(sim.feet(dummy).y < 0.05, "never landed after the freeze");
    assert!(sim.get::<Motor>(dummy).grounded);
}

#[test]
fn freeze_holds_a_downed_dummy_until_it_lifts() {
    let mut sim = Sim::new();
    let dummy = dummy(&mut sim);
    let tick = sim.sim_tick();
    sim.world_mut().entity_mut(dummy).insert(Downed { tick });
    sim.world_mut().insert_resource(GalleryFreeze);
    let held = sim.feet(dummy);
    let delay = sim.world().resource::<Tuning>().dummy.respawn_delay;
    sim.run_seconds(delay + 1.0);
    assert!(
        sim.world().get::<Downed>(dummy).is_some(),
        "respawned while frozen"
    );
    assert_eq!(sim.feet(dummy), held);
    sim.world_mut().remove_resource::<GalleryFreeze>();
    sim.run_seconds(0.2);
    assert!(
        sim.world().get::<Downed>(dummy).is_none(),
        "no respawn after thaw"
    );
}

#[test]
fn freeze_stops_the_knights_clock_springs_and_eyes() {
    let mut anim = KnightAnim::new(7);
    let run = KnightInput {
        velocity: Vec3::new(0.0, 0.0, -5.5),
        grounded: true,
        downed: false,
    };
    for _ in 0..40 {
        anim.step(1.0 / 60.0, &run);
    }
    anim.event(KnightEvent::Hit {
        push: Vec3::Z,
        headshot: true,
    });
    let hit = anim.step(1.0 / 60.0, &run);
    assert_eq!(hit.eyes, EyeState::Wide);

    anim.set_frozen(true);
    let time = anim.time;
    let held = anim.step(1.0 / 60.0, &run);
    // Ten seconds frozen: past the wide-eye time and several blink periods,
    // mid-stride, mid-wobble and mid-hat-bounce.
    for _ in 0..600 {
        let pose = anim.step(1.0 / 60.0, &run);
        assert_eq!(pose, held, "the pose moved while frozen");
    }
    assert_eq!(anim.time, time, "the clock ran while frozen");

    anim.set_frozen(false);
    let mut eyes = Vec::new();
    for _ in 0..30 {
        eyes.push(anim.step(1.0 / 60.0, &run).eyes);
    }
    assert!(anim.time > time + 0.4);
    assert_ne!(eyes.last(), Some(&EyeState::Wide), "wide eyes never ended");
}

/// A knight figure animated by the real `animate_knights`, its clock frozen by
/// the gallery's plugin.
#[test]
fn freeze_stops_the_knight_figure_in_the_app() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, GalleryPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()))
        .add_message::<DamageDealt>()
        .add_message::<GameCue>()
        .add_systems(PostUpdate, animate_knights);
    let owner = app
        .world_mut()
        .spawn((
            Character,
            Transform::default(),
            Health::default(),
            LookAngles::default(),
            Motor::default(),
        ))
        .id();
    {
        // Running sideways, on the ground.
        let mut motor = app.world_mut().get_mut::<Motor>(owner).unwrap();
        motor.velocity = Vec3::new(3.0, 0.0, 0.0);
        motor.grounded = true;
    }
    let figure = app
        .world_mut()
        .spawn((
            TargetFigure { owner },
            KnightAnim::new(3),
            Transform::default(),
            Visibility::default(),
        ))
        .id();
    let time = |app: &App| app.world().get::<KnightAnim>(figure).unwrap().time;
    for _ in 0..20 {
        app.update();
    }
    let running = time(&app);
    assert!(running > 0.2, "the figure animates: {running}");

    app.world_mut().insert_resource(GalleryFreeze);
    app.update();
    let held = time(&app);
    for _ in 0..60 {
        app.update();
    }
    assert_eq!(time(&app), held, "the knight's clock ran while frozen");
    assert!(app.world().get::<KnightAnim>(figure).unwrap().is_frozen());

    app.world_mut().remove_resource::<GalleryFreeze>();
    for _ in 0..10 {
        app.update();
    }
    assert!(time(&app) > held + 0.1, "the clock never restarted");
}

#[derive(Resource, Default)]
struct Advanced(f32);

fn advance(time: FreezableTime, mut advanced: ResMut<Advanced>) {
    advanced.0 += time.delta_secs();
}

#[test]
fn freezable_time_reads_zero_while_frozen() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()))
        .init_resource::<Advanced>()
        .add_systems(Update, advance);
    for _ in 0..10 {
        app.update();
    }
    let before = app.world().resource::<Advanced>().0;
    assert!(before > 0.1);
    app.world_mut().insert_resource(GalleryFreeze);
    for _ in 0..10 {
        app.update();
    }
    assert_eq!(app.world().resource::<Advanced>().0, before);
    app.world_mut().remove_resource::<GalleryFreeze>();
    app.update();
    assert!(app.world().resource::<Advanced>().0 > before);
}
