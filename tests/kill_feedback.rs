//! M4 chunk 1, hit feedback and kills (docs/M4-SPEC.md → Chunk 1, D105),
//! through the simulation seam: scripted `PlayerIntent` in (aim, fire), the
//! real combat, wave director, HUD, effects and audio queue headless, and what
//! a player would get out, counted on the kill's own frame:
//!
//! - the kill confirm: a `KillConfirmed`, the "X" marker, the kill chime, and
//!   a hitstop that holds `FreezableTime` for exactly two frames while the
//!   fixed step keeps running;
//! - the physical death: helmet, gauntlets and boots flying off, landing and
//!   gone within 2 s; a dented helmet after a headshot;
//! - score popups (Waves), multi-kill callouts, and pools that stay capped
//!   over five minutes of waves.
//!
//! The pure pieces (chunk physics, clatter cap, hit regions, the flinch, the
//! kill chain) are tested directly.

use avian3d::prelude::PhysicsPlugins;
use bevy::{
    mesh::MeshPlugin, prelude::*, scene::ScenePlugin, state::app::StatesPlugin,
    time::TimeUpdateStrategy,
};
use pieced::{
    app::SimPlugins,
    arena::visuals::{TargetFigure, TargetFigurePlugin},
    audio::{GameAudioPlugin, Sfx, kill_cues},
    combat::Downed,
    dummy::{Dummy, look_toward},
    fx::{
        armor::{ArmorClattered, ArmorPiece, ArmorPlugin, ArmorPool},
        chunks::{Chunk, ClatterGate},
        kills::{
            Callout, HitstopClock, KillChain, KillConfirmed, KillFeedbackPlugin, KillFeedbackStats,
            ticks_in,
        },
        spells::{SpellsPlugin, pool_counts},
    },
    grunt::Parked,
    hud::{
        HudPlugin,
        kills::{KillCallout, KillGlyph, POPUP_POOL, ScorePopup, stamp_motion, write_popup},
    },
    knight::{FLINCH_LEGS_BELOW, HitRegion, KnightAnim, KnightEvent, KnightInput, KnightPose},
    look::ToonMaterial,
    player::HEAD_CENTER,
    render::CurrentFov,
    rng::{Rng, SimRng},
    shared::{
        ActiveTool, AppState, DamageDealt, DamageTarget, Eliminated, EyeHeight, FreezableTime,
        GameCue, GameMode, Health, HitstopFrozen, LookAngles, PreviousFeet, SimTick, WeaponKind,
        tick_duration,
    },
    sim::Sim,
    tuning::Tuning,
    waves::{
        PoolGrunt, Run, RunPhase, ScoreAwarded, ScoreKind, SkipBreak,
        ui::{RunUi, WavesUiPlugin},
    },
};

const DT: f32 = 1.0 / 60.0;

// ---------------------------------------------------------------------------
// A headless game with the HUD, the effects' kill feedback and (optionally)
// the audio queue
// ---------------------------------------------------------------------------

/// Presentation time per frame, as `FreezableTime` reads it, and the tick.
#[derive(Resource, Default)]
struct Clock(Vec<(u64, f32)>);

fn record_clock(time: FreezableTime, tick: Res<SimTick>, mut clock: ResMut<Clock>) {
    clock.0.push((tick.0, time.delta_secs()));
}

fn game(seed: u64, mode: GameMode, audio: bool) -> Sim {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        StatesPlugin,
        AssetPlugin::default(),
        MeshPlugin,
        ScenePlugin,
        PhysicsPlugins::default(),
    ))
    .add_plugins(SimPlugins)
    .init_asset::<ToonMaterial>()
    .add_plugins(HudPlugin)
    .add_plugins((
        KillFeedbackPlugin,
        ArmorPlugin,
        TargetFigurePlugin,
        WavesUiPlugin,
    ))
    .init_resource::<ButtonInput<KeyCode>>()
    .init_resource::<CurrentFov>()
    .init_resource::<Clock>()
    .add_systems(Update, record_clock)
    .insert_resource(mode)
    .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()))
    .insert_resource(SimRng(Rng::new(seed)));
    if audio {
        app.init_asset::<AudioSource>().add_plugins(GameAudioPlugin);
    }
    app.finish();
    app.cleanup();
    SpellsPlugin::install(&mut app);
    {
        let mut tuning = app.world_mut().resource_mut::<Tuning>();
        tuning.dummy.stand_still = true;
    }
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Playing);
    app.update();
    let mut sim = Sim { app };
    // A player the knights can't take down.
    let p = sim.player();
    let mut health = Health::full(1.0e9, 100.0);
    health.shield = 100.0;
    *sim.world_mut().get_mut::<Health>(p).unwrap() = health;
    sim
}

fn single<C: Component>(sim: &mut Sim) -> Entity {
    let world = sim.world_mut();
    world
        .query_filtered::<Entity, With<C>>()
        .single(world)
        .unwrap()
}

fn place(sim: &mut Sim, who: Entity, feet: Vec3) {
    sim.world_mut()
        .get_mut::<Transform>(who)
        .unwrap()
        .translation = feet;
    sim.world_mut().get_mut::<PreviousFeet>(who).unwrap().0 = feet;
}

fn aim_at(sim: &mut Sim, point: Vec3) {
    let player = sim.player();
    let eye = sim.feet(player) + Vec3::Y * sim.get::<EyeHeight>(player).0;
    let look = look_toward(point - eye);
    sim.set_look(player, look.yaw, look.pitch);
}

/// Pulls the trigger for one frame.
fn fire(sim: &mut Sim) {
    {
        let mut intent = sim.player_intent();
        intent.fire = true;
        intent.fire_pressed = true;
    }
    sim.tick();
    let mut intent = sim.player_intent();
    intent.fire = false;
    intent.fire_pressed = false;
}

/// Stands `target` 7 m in front of the player, at `hp` health and no shield.
fn stand_in_front(sim: &mut Sim, target: Entity, hp: f32) -> Vec3 {
    let player = sim.player();
    let feet = sim.feet(player) + Vec3::new(0.5, 0.0, -7.0);
    place(sim, target, feet);
    let mut health = sim.world_mut().get_mut::<Health>(target).unwrap();
    health.hp = hp;
    health.shield = 0.0;
    sim.player_intent().select = Some(ActiveTool::Weapon(WeaponKind::Rifle));
    sim.ticks(40);
    // Re-stand him (the dummy may have drifted a hair) and re-aim.
    place(sim, target, feet);
    feet
}

/// Kills `target` with a rifle shot (a headshot if `head`); returns its feet.
fn shoot_dead(sim: &mut Sim, target: Entity, head: bool) -> Vec3 {
    let feet = stand_in_front(sim, target, 1.0);
    let aim = if head {
        feet + Vec3::Y * HEAD_CENTER
    } else {
        feet + Vec3::Y * 1.0
    };
    aim_at(sim, aim);
    fire(sim);
    assert!(
        sim.world().get::<Downed>(target).is_some(),
        "the shot downed him"
    );
    feet
}

fn stats(sim: &Sim) -> KillFeedbackStats {
    sim.world().resource::<KillFeedbackStats>().clone()
}

fn armor(sim: &Sim) -> &ArmorPool {
    sim.world().resource::<ArmorPool>()
}

fn figure_anim(sim: &mut Sim, owner: Entity) -> KnightAnim {
    let world = sim.world_mut();
    world
        .query::<(&TargetFigure, &KnightAnim)>()
        .iter(world)
        .find(|(f, _)| f.owner == owner)
        .map(|(_, a)| a.clone())
        .expect("the knight's figure")
}

/// The face text of every active score popup.
fn popup_texts(sim: &mut Sim) -> Vec<String> {
    let world = sim.world_mut();
    let active: Vec<Entity> = world
        .query::<(Entity, &ScorePopup)>()
        .iter(world)
        .filter(|(_, p)| p.active)
        .map(|(e, _)| e)
        .collect();
    let mut out = Vec::new();
    for e in active {
        let children: Vec<Entity> = world.get::<Children>(e).unwrap().iter().collect();
        for c in children {
            if let (Some(g), Some(t)) = (world.get::<KillGlyph>(c), world.get::<Text>(c))
                && g.0 == 4
            {
                out.push(t.0.clone());
            }
        }
    }
    out.sort();
    out
}

fn callout_text(sim: &mut Sim) -> Option<String> {
    let world = sim.world_mut();
    let (e, callout) = world
        .query::<(Entity, &KillCallout)>()
        .iter(world)
        .map(|(e, c)| (e, c.clone()))
        .next()?;
    callout.callout?;
    let children: Vec<Entity> = world.get::<Children>(e).unwrap().iter().collect();
    children.into_iter().find_map(|c| {
        (world.get::<KillGlyph>(c)?.0 == 4).then(|| world.get::<Text>(c).unwrap().0.clone())
    })
}

// ---------------------------------------------------------------------------
// The kill frame
// ---------------------------------------------------------------------------

#[test]
fn a_kill_confirms_on_its_own_frame_x_chime_armor_poof_and_hitstop() {
    let mut sim = game(1, GameMode::Practice, true);
    let dummy = single::<Dummy>(&mut sim);
    sim.record::<KillConfirmed>();
    let before = stats(&sim);
    let solids = pool_counts(sim.world()).unwrap().solid.0;
    shoot_dead(&mut sim, dummy, false);

    // The kill frame.
    let kills = sim.recorded::<KillConfirmed>();
    assert_eq!(kills.len(), 1, "one kill confirmed");
    assert_eq!(kills[0].victim, dummy);
    assert!(!kills[0].headshot && kills[0].chain == 1);
    let after = stats(&sim);
    assert_eq!(after.kills - before.kills, 1);
    assert_eq!(
        after.markers_same_frame - before.markers_same_frame,
        1,
        "the X marker on the kill's frame"
    );
    assert_eq!(
        after.sounds_same_frame - before.sounds_same_frame,
        1,
        "the kill chime on the kill's frame"
    );
    assert_eq!(
        after.armor_same_frame - before.armor_same_frame,
        1,
        "his armor flies off on the kill's frame"
    );
    assert_eq!(armor(&sim).live(), 5, "helmet, two gauntlets, two boots");
    assert!(
        pool_counts(sim.world()).unwrap().solid.0 > solids,
        "the poof and the chips"
    );
    assert_eq!(after.hitstops - before.hitstops, 1);
    assert!(
        sim.world().resource::<HitstopFrozen>().0,
        "the next frame holds"
    );
    // The knight's figure has shed his armor.
    let anim = figure_anim(&mut sim, dummy);
    assert!(anim.armor_is_off() && anim.is_downed());
}

#[test]
fn hitstop_holds_presentation_exactly_two_frames_while_the_fixed_step_runs() {
    let mut sim = game(2, GameMode::Practice, false);
    let dummy = single::<Dummy>(&mut sim);
    assert_eq!(sim.world().resource::<Tuning>().feedback.hitstop_frames, 2);
    shoot_dead(&mut sim, dummy, true);
    sim.world_mut().resource_mut::<Clock>().0.clear();
    sim.ticks(5);
    let clock = sim.world().resource::<Clock>().0.clone();
    let deltas: Vec<f32> = clock.iter().map(|c| c.1).collect();
    assert_eq!(deltas[0], 0.0, "{deltas:?}");
    assert_eq!(deltas[1], 0.0, "{deltas:?}");
    assert!(
        deltas[2..].iter().all(|d| (*d - DT).abs() < 1e-4),
        "exactly two frames held: {deltas:?}"
    );
    // The fixed gameplay step never paused.
    for w in clock.windows(2) {
        assert_eq!(w[1].0, w[0].0 + 1, "one tick per frame: {clock:?}");
    }
    assert!(!sim.world().resource::<HitstopClock>().0.active());
    // Two kills in a row still hold only two frames after the last.
    let mut clock = HitstopClock::default();
    clock.0.trigger(2);
    clock.0.end_frame();
    clock.0.trigger(2);
    clock.0.end_frame();
    let mut held = 0;
    for _ in 0..6 {
        if clock.0.holding() {
            held += 1;
        }
        clock.0.end_frame();
    }
    assert_eq!(held, 2);
}

#[test]
fn a_headshot_dents_the_helmet_until_he_goes_down_and_it_pops_off_dented() {
    let mut sim = game(3, GameMode::Practice, false);
    let dummy = single::<Dummy>(&mut sim);
    // A headshot that doesn't kill.
    let feet = stand_in_front(&mut sim, dummy, 100.0);
    aim_at(&mut sim, feet + Vec3::Y * HEAD_CENTER);
    fire(&mut sim);
    sim.tick();
    let anim = figure_anim(&mut sim, dummy);
    assert!(anim.is_dented(), "the headshot dented his helmet");
    assert!(!anim.armor_is_off());
    sim.ticks(90);
    assert!(figure_anim(&mut sim, dummy).is_dented(), "and it stays");
    // Pure: it stays until respawn.
    let mut a = KnightAnim::new(5);
    a.event(KnightEvent::Hit {
        push: Vec3::Z,
        headshot: true,
    });
    let pose = a.step(DT, &KnightInput::default());
    assert!(pose.dented && pose.armor);
    let downed = KnightInput {
        downed: true,
        ..default()
    };
    let pose = a.step(DT, &downed);
    assert!(pose.dented && !pose.armor && !pose.hat_visible);
    let pose = a.step(DT, &KnightInput::default());
    assert!(!pose.dented && pose.armor, "respawned whole: {pose:?}");
}

// ---------------------------------------------------------------------------
// The physical death
// ---------------------------------------------------------------------------

#[test]
fn armor_arcs_out_lands_on_the_island_and_is_gone_within_two_seconds() {
    let mut sim = game(4, GameMode::Practice, false);
    let dummy = single::<Dummy>(&mut sim);
    sim.record::<ArmorClattered>();
    let feet = shoot_dead(&mut sim, dummy, false);
    let start: Vec<Vec3> = armor(&sim).positions().collect();
    assert_eq!(start.len(), 5);
    let helmet_start = start[0];
    let mut highest = helmet_start.y;
    let mut lowest = f32::MAX;
    let mut farthest: f32 = 0.0;
    let mut gone_at = None;
    for frame in 1..=150 {
        sim.tick();
        let now: Vec<Vec3> = armor(&sim).positions().collect();
        if now.is_empty() {
            gone_at.get_or_insert(frame);
            continue;
        }
        highest = highest.max(now[0].y);
        for p in &now {
            lowest = lowest.min(p.y);
            farthest = farthest.max((p.xz() - feet.xz()).length());
            assert!(p.y > feet.y - 0.01, "never through the island: {p}");
        }
        assert!(armor(&sim).clatters() <= 4, "at most four clatters");
    }
    let gone = gone_at.expect("every piece gone");
    assert!(
        gone as f32 * DT <= 2.0,
        "gone {:.2} s after the kill",
        gone as f32 * DT
    );
    assert!(
        highest > helmet_start.y + 0.4,
        "the helmet pops up ({highest:.2})"
    );
    assert!(
        lowest < feet.y + 0.2,
        "pieces land on the grass ({lowest:.2})"
    );
    assert!(farthest > 0.5, "pieces fly clear of him ({farthest:.2} m)");
    let clatters = sim.recorded::<ArmorClattered>();
    assert!(
        (1..=5).contains(&clatters.len()),
        "a clatter per first bounce: {}",
        clatters.len()
    );
}

#[test]
fn chunks_bounce_settle_on_a_ledge_fall_through_the_void_and_shrink_away() {
    let ledge = |_: Vec3| Some(3.0);
    let mut c = Chunk {
        pos: Vec3::new(0.0, 5.0, 0.0),
        vel: Vec3::new(2.0, 3.0, 0.0),
        radius: 0.1,
        life: 1.5,
        ..default()
    };
    let mut first = 0;
    let mut t = 0.0;
    while t < 1.2 {
        let step = c.step(DT, ledge);
        assert!(step.alive);
        first += u32::from(step.first_bounce);
        assert!(
            c.pos.y >= 3.1 - 1e-4,
            "rests on the ledge's top: {}",
            c.pos.y
        );
        t += DT;
    }
    assert_eq!(first, 1, "the first bounce is reported once");
    assert!(
        c.bounces >= 2 && c.is_resting(),
        "bounced and settled: {c:?}"
    );
    assert!(c.shrink() < 1.0, "shrinking by the end");
    while c.step(DT, ledge).alive {}
    assert!(c.age >= c.life - 1e-4);
    // Over the void it just falls.
    let mut v = Chunk {
        pos: Vec3::new(0.0, 0.5, 0.0),
        ..default()
    };
    for _ in 0..60 {
        v.step(DT, |_| None);
    }
    assert!(v.pos.y < -5.0 && v.bounces == 0);
    // A long frame stays stable.
    let mut a = Chunk {
        pos: Vec3::Y,
        vel: Vec3::X,
        ..default()
    };
    a.step(0.2, |_| Some(0.0));
    assert!(a.pos.is_finite() && a.pos.y >= a.radius - 1e-4);
}

#[test]
fn clatter_is_capped_at_four_voices() {
    let mut gate = ClatterGate::new(4, 0.35);
    let started = (0..10).filter(|_| gate.try_start(1.0)).count();
    assert_eq!(started, 4);
    assert_eq!(gate.sounding(1.1), 4);
    assert!(!gate.try_start(1.2));
    assert!(
        gate.try_start(1.36),
        "a voice frees up once one has rung out"
    );
}

#[test]
fn flung_into_the_void_he_comes_apart_as_he_falls() {
    let mut sim = game(6, GameMode::Practice, false);
    let dummy = single::<Dummy>(&mut sim);
    let feet = sim.feet(dummy);
    sim.world_mut()
        .entity_mut(dummy)
        .insert(pieced::movement::VoidFall {
            by: None,
            tick: 0,
            from: feet,
            direction: Vec3::X,
        });
    sim.world_mut()
        .write_message(GameCue::VoidFall { who: dummy });
    sim.tick();
    assert_eq!(armor(&sim).live(), 5, "his armor comes apart as he flies");
    assert!(figure_anim(&mut sim, dummy).armor_is_off());
    // When the void elimination counts, it doesn't come off twice.
    let tick = sim.sim_tick();
    sim.world_mut().write_message(Eliminated {
        victim: dummy,
        by: None,
        position: feet - Vec3::Y * 4.0,
        tick,
    });
    sim.tick();
    assert_eq!(armor(&sim).live(), 5);
}

// ---------------------------------------------------------------------------
// Directional flinch
// ---------------------------------------------------------------------------

#[test]
fn hits_are_placed_by_region() {
    let head = Vec3::new(0.0, 1.6, -0.1);
    assert_eq!(HitRegion::classify(head, true), HitRegion::Head);
    assert_eq!(
        HitRegion::classify(Vec3::new(0.0, 1.1, -0.2), false),
        HitRegion::Chest
    );
    assert_eq!(
        HitRegion::classify(Vec3::new(-0.25, 1.1, -0.1), false),
        HitRegion::Left
    );
    assert_eq!(
        HitRegion::classify(Vec3::new(0.25, 1.1, -0.1), false),
        HitRegion::Right
    );
    assert_eq!(
        HitRegion::classify(Vec3::new(-0.1, FLINCH_LEGS_BELOW - 0.3, 0.0), false),
        HitRegion::Legs { left: true }
    );
}

/// Peak pose change from rest over `seconds` after a flinch at `region`.
fn flinch(region: HitRegion, seconds: f32) -> Vec<KnightPose> {
    let mut a = KnightAnim::new(9);
    let rest = KnightInput::default();
    for _ in 0..30 {
        a.step(DT, &rest);
    }
    let mut b = a.clone();
    // Shot from in front: pushed back (+Z in his frame).
    a.event(KnightEvent::Flinch {
        region,
        push: Vec3::Z,
    });
    (0..(seconds / DT) as usize)
        .map(|_| {
            let p = a.step(DT, &rest);
            let q = b.step(DT, &rest);
            // The difference the flinch makes, part by part.
            KnightPose {
                torso: q.torso.inverse() * p.torso,
                head: q.head.inverse() * p.head,
                arms: [
                    q.arms[0].inverse() * p.arms[0],
                    q.arms[1].inverse() * p.arms[1],
                ],
                legs: [
                    q.legs[0].inverse() * p.legs[0],
                    q.legs[1].inverse() * p.legs[1],
                ],
                ..p
            }
        })
        .collect()
}

fn peak(poses: &[KnightPose], part: impl Fn(&KnightPose) -> Quat) -> f32 {
    poses
        .iter()
        .map(|p| part(p).angle_between(Quat::IDENTITY))
        .fold(0.0, f32::max)
}

#[test]
fn a_hit_knight_flinches_away_from_the_shot_by_region_and_settles() {
    let head = flinch(HitRegion::Head, 0.8);
    let chest = flinch(HitRegion::Chest, 0.8);
    let left = flinch(HitRegion::Left, 0.8);
    let right = flinch(HitRegion::Right, 0.8);
    let legs = flinch(HitRegion::Legs { left: false }, 0.8);
    // Head: the head snaps back, more than the torso.
    assert!(
        peak(&head, |p| p.head) > 0.25,
        "{}",
        peak(&head, |p| p.head)
    );
    assert!(peak(&head, |p| p.head) > 2.0 * peak(&head, |p| p.torso));
    // Chest: the torso is punched back.
    assert!(peak(&chest, |p| p.torso) > 0.2);
    // Sides: that arm flings back, the other stays; the torso twists opposite ways.
    assert!(peak(&left, |p| p.arms[0]) > 0.35 && peak(&left, |p| p.arms[1]) < 0.01);
    assert!(peak(&right, |p| p.arms[1]) > 0.35 && peak(&right, |p| p.arms[0]) < 0.01);
    let twist = |poses: &[KnightPose]| {
        poses
            .iter()
            .map(|p| p.torso.to_euler(EulerRot::YXZ).0)
            .fold(0.0f32, |a, b| if b.abs() > a.abs() { b } else { a })
    };
    assert!(
        twist(&left) * twist(&right) < 0.0,
        "left and right twist opposite ways: {} {}",
        twist(&left),
        twist(&right)
    );
    // Legs: the hit leg is kicked back.
    assert!(peak(&legs, |p| p.legs[1]) > 0.3 && peak(&legs, |p| p.legs[0]) < 0.01);
    // "Away from the shot": the head tips back, toward +Z at its top.
    let tipped = head
        .iter()
        .map(|p| p.head * Vec3::Y)
        .fold(0.0f32, |a, v| a.max(v.z));
    assert!(tipped > 0.2, "the head tips back from the shot: {tipped}");
    // And it settles within the clip.
    for poses in [&head, &chest, &left, &right, &legs] {
        let last = poses.last().unwrap();
        assert!(
            last.torso.angle_between(Quat::IDENTITY) < 0.02
                && last.head.angle_between(Quat::IDENTITY) < 0.02,
            "settled"
        );
    }
}

// ---------------------------------------------------------------------------
// Multi-kills and popups
// ---------------------------------------------------------------------------

#[test]
fn the_kill_chain_calls_out_double_to_rampage_and_a_gap_resets() {
    let window = ticks_in(1.5);
    assert_eq!(window, 90);
    let mut chain = KillChain::default();
    let seen: Vec<Option<Callout>> = [0u64, 60, 120, 200, 280, 360]
        .into_iter()
        .map(|t| Callout::for_chain(chain.kill(t, window)))
        .collect();
    assert_eq!(
        seen,
        [
            None,
            Some(Callout::Double),
            Some(Callout::Triple),
            Some(Callout::Quad),
            Some(Callout::Rampage),
            Some(Callout::Rampage)
        ]
    );
    assert_eq!(chain.kill(360 + window + 1, window), 1, "a gap resets");
    assert_eq!(Callout::Double.text(), "DOUBLE!");
    assert_eq!(Callout::Rampage.text(), "RAMPAGE!");
    // The sting's level follows the callout.
    let kill = |chain| KillConfirmed {
        victim: Entity::PLACEHOLDER,
        at: Vec3::ZERO,
        headshot: false,
        void: false,
        chain,
        tick: 0,
    };
    assert_eq!(kill_cues(&kill(1)), (Sfx::KillConfirm, None));
    assert_eq!(kill_cues(&kill(3)).1, Some((Sfx::MultiKill, 1)));
    assert_eq!(kill_cues(&kill(9)).1, Some((Sfx::MultiKill, 3)));
}

#[test]
fn chained_kills_pop_callouts_near_the_crosshair() {
    let mut sim = game(7, GameMode::Practice, false);
    let dummy = single::<Dummy>(&mut sim);
    let player = sim.player();
    let at = sim.feet(dummy);
    let eliminate = |sim: &mut Sim| {
        let tick = sim.sim_tick();
        sim.world_mut().write_message(Eliminated {
            victim: dummy,
            by: Some(player),
            position: at,
            tick,
        });
        sim.tick();
    };
    let mut callouts = Vec::new();
    for _ in 0..5 {
        eliminate(&mut sim);
        callouts.push(callout_text(&mut sim));
        sim.ticks(40);
    }
    assert_eq!(
        callouts,
        [
            None,
            Some("DOUBLE!".to_string()),
            Some("TRIPLE!".to_string()),
            Some("QUAD!".to_string()),
            Some("RAMPAGE!".to_string()),
        ]
    );
    // It fades on its own; after a pause the chain starts over.
    sim.ticks(120);
    assert_eq!(callout_text(&mut sim), None);
    eliminate(&mut sim);
    assert_eq!(callout_text(&mut sim), None, "a new chain");
    assert_eq!(stats(&sim).callouts, 4);
}

#[test]
fn popup_text_and_motion() {
    let mut s = String::with_capacity(32);
    write_popup(&mut s, ScoreKind::Kill, 100);
    assert_eq!(s, "+100");
    write_popup(&mut s, ScoreKind::Headshot, 50);
    assert_eq!(s, "+50 HEADSHOT");
    write_popup(&mut s, ScoreKind::Void, 150);
    assert_eq!(s, "+150 VOID");
    let (rise0, a0, s0) = stamp_motion(0.0, 0.8, 0.45);
    let (rise1, a1, s1) = stamp_motion(0.4, 0.8, 0.45);
    let (_, a2, _) = stamp_motion(0.8, 0.8, 0.45);
    assert!(rise0 == 0.0 && rise1 > 0.5 && s0 > 1.3 && (s1 - 1.0).abs() < 1e-4);
    assert!(a0 == 1.0 && a1 == 1.0 && a2 == 0.0);
}

/// A Waves game standing still (knights hold where they land) with a knight
/// in play.
fn waves_with_a_knight(seed: u64) -> (Sim, Entity) {
    let mut sim = game(seed, GameMode::Waves, false);
    for _ in 0..3600 {
        sim.tick();
        let world = sim.world_mut();
        let knight = world
            .query_filtered::<Entity, (With<PoolGrunt>, Without<Parked>, Without<Downed>)>()
            .iter(world)
            .next();
        if let Some(k) = knight {
            sim.world_mut()
                .insert_resource(pieced::shared::GalleryFreeze);
            return (sim, k);
        }
    }
    panic!("no knight landed");
}

#[test]
fn a_headshot_kill_pops_plus_100_and_plus_50_headshot_on_its_frame() {
    let (mut sim, knight) = waves_with_a_knight(8);
    sim.record::<ScoreAwarded>();
    let before = stats(&sim).popups_same_frame;
    shoot_dead(&mut sim, knight, true);
    let awards = sim.recorded::<ScoreAwarded>();
    let kinds: Vec<ScoreKind> = awards.iter().map(|a| a.kind).collect();
    assert_eq!(kinds, [ScoreKind::Kill, ScoreKind::Headshot]);
    assert_eq!(popup_texts(&mut sim), ["+100", "+50 HEADSHOT"]);
    assert_eq!(stats(&sim).popups_same_frame - before, 2);
    // Stacked: the bonus a line under the kill.
    let world = sim.world_mut();
    let mut lines: Vec<(ScoreKind, u8)> = world
        .query::<&ScorePopup>()
        .iter(world)
        .filter(|p| p.active)
        .map(|p| (p.kind.unwrap(), p.line))
        .collect();
    lines.sort_by_key(|l| l.1);
    assert_eq!(lines, [(ScoreKind::Kill, 0), (ScoreKind::Headshot, 1)]);
    // Gone after 0.8 s (the freeze holds effect clocks; let it go).
    sim.world_mut()
        .remove_resource::<pieced::shared::GalleryFreeze>();
    sim.ticks(60);
    assert!(popup_texts(&mut sim).is_empty(), "faded out");
}

#[test]
fn practice_has_no_score_popups() {
    let mut sim = game(9, GameMode::Practice, false);
    let dummy = single::<Dummy>(&mut sim);
    shoot_dead(&mut sim, dummy, true);
    assert!(popup_texts(&mut sim).is_empty());
}

// ---------------------------------------------------------------------------
// Pools over five minutes of waves
// ---------------------------------------------------------------------------

fn count<C: Component>(sim: &mut Sim) -> usize {
    let world = sim.world_mut();
    world.query_filtered::<(), With<C>>().iter(world).count()
}

/// Downs every knight in play the way combat does (a killing `DamageDealt`
/// from the player, `Downed`, an `Eliminated`), headshots on every other
/// kill counted by `kills`. Returns how many went down.
fn down_every_knight(sim: &mut Sim, kills: &mut u32) -> u32 {
    let player = sim.player();
    let in_play: Vec<(Entity, Vec3)> = {
        let world = sim.world_mut();
        world
            .query_filtered::<(Entity, &Transform), (With<PoolGrunt>, Without<Parked>, Without<Downed>)>()
            .iter(world)
            .map(|(e, t)| (e, t.translation))
            .collect()
    };
    let now = sim.sim_tick();
    let n = in_play.len() as u32;
    for (knight, at) in in_play {
        sim.world_mut().get_mut::<Health>(knight).unwrap().hp = 0.0;
        sim.world_mut()
            .entity_mut(knight)
            .insert(Downed { tick: now });
        let headshot = kills.is_multiple_of(2);
        sim.world_mut().write_message(DamageDealt {
            source: Some(player),
            target: knight,
            target_kind: DamageTarget::Character,
            amount: 50.0,
            to_shield: 0.0,
            headshot,
            shield_broke: false,
            killed: true,
            point: at + Vec3::Y * if headshot { 1.6 } else { 1.0 },
            normal: Vec3::Z,
            tick: now,
        });
        sim.world_mut().write_message(Eliminated {
            victim: knight,
            by: Some(player),
            position: at,
            tick: now,
        });
        *kills += 1;
    }
    n
}

#[test]
fn clearing_a_wave_pops_its_bonus_under_the_hud_score() {
    let mut sim = game(11, GameMode::Waves, false);
    let mut kills = 0;
    for _ in 0..3600 {
        down_every_knight(&mut sim, &mut kills);
        sim.tick();
        if matches!(sim.world().resource::<Run>().phase, RunPhase::Break { .. }) {
            break;
        }
    }
    let bonus = |sim: &mut Sim| {
        let world = sim.world_mut();
        world
            .query::<(&RunUi, &Text, &Visibility)>()
            .iter(world)
            .find(|(p, _, _)| **p == RunUi::ScoreBonus)
            .map(|(_, t, v)| (t.0.clone(), *v != Visibility::Hidden))
            .unwrap()
    };
    assert_eq!(bonus(&mut sim), ("+250".to_string(), true), "wave 1: +250");
    sim.ticks(120);
    assert!(!bonus(&mut sim).1, "and it fades");
}

#[test]
fn pools_stay_capped_and_nothing_is_spawned_per_kill_over_five_minutes_of_waves() {
    let mut sim = game(10, GameMode::Waves, false);
    let fixed = (
        count::<ArmorPiece>(&mut sim),
        count::<ScorePopup>(&mut sim),
        count::<KillGlyph>(&mut sim),
        count::<KillCallout>(&mut sim),
    );
    let armor_cap = armor(&sim).capacity();
    let mut baseline = None;
    let mut most = 0;
    let mut kills = 0u32;
    for tick in 0..(5 * 60 * 60) {
        down_every_knight(&mut sim, &mut kills);
        if matches!(sim.world().resource::<Run>().phase, RunPhase::Break { .. }) {
            sim.world_mut().write_message(SkipBreak);
        }
        sim.tick();
        assert!(armor(&sim).live() <= armor_cap);
        let popups = {
            let world = sim.world_mut();
            world
                .query::<&ScorePopup>()
                .iter(world)
                .filter(|p| p.active)
                .count()
        };
        assert!(popups <= POPUP_POOL);
        let spells = pool_counts(sim.world()).unwrap();
        for (live, total) in [spells.glow, spells.solid, spells.bolts, spells.halos] {
            assert!(live <= total);
        }
        let entities = sim.world().entities().len();
        if tick == 60 * 60 {
            baseline = Some(entities);
        }
        if baseline.is_some() {
            most = most.max(entities);
        }
    }
    assert!(kills > 100, "{kills} kills in five minutes");
    let run = sim.world().resource::<Run>().clone();
    assert!(run.wave >= 5, "reached wave {}", run.wave);
    assert_eq!(
        (
            count::<ArmorPiece>(&mut sim),
            count::<ScorePopup>(&mut sim),
            count::<KillGlyph>(&mut sim),
            count::<KillCallout>(&mut sim),
        ),
        fixed,
        "the pools never grow"
    );
    let baseline = baseline.unwrap();
    assert!(
        most <= baseline + 16,
        "entities stay flat after the first minute: {baseline} -> {most}"
    );
    assert!(stats(&sim).kills >= kills - 8);
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[test]
fn a_saved_settings_file_never_freezes_the_kill_feel() {
    let dir = std::env::temp_dir().join(format!("pieced-kills-{}", std::process::id()));
    let path = dir.join("settings.json");
    let mut t = Tuning::default();
    t.feedback.hitstop_frames = 9;
    t.feedback.camera_shake = 0.0;
    t.kills.chain_seconds = 9.0;
    t.save(&path).unwrap();
    let loaded = Tuning::load_or_default(&path);
    assert_eq!(loaded.feedback.hitstop_frames, 2, "a designer number");
    assert_eq!(loaded.feedback.camera_shake, 0.0, "a menu setting survives");
    assert_eq!(loaded.kills, Default::default(), "never persisted");
    let _ = std::fs::remove_dir_all(dir);
}

// ---------------------------------------------------------------------------
// The rigged knight: the dented helmet and his armor coming off
// ---------------------------------------------------------------------------

mod figure {
    use super::*;
    use bevy::{gltf::GltfPlugin, world_serialization::WorldSerializationPlugin};
    use pieced::{
        app::BootGate,
        fx::armor::ArmorMeshes,
        knight::{KnightArmorRig, KnightRig},
        look::{ModelDressed, warmup::WarmupState},
        models::{ModelSpawned, ModelsPlugin},
        movement::Motor,
        shared::Character,
    };
    use std::time::Duration;

    /// Stands in for `look`, which dresses models (it needs a renderer).
    fn dress(mut spawned: MessageReader<ModelSpawned>, mut dressed: MessageWriter<ModelDressed>) {
        for m in spawned.read() {
            dressed.write(ModelDressed {
                root: m.root,
                name: m.name.clone(),
            });
        }
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            TransformPlugin,
            MeshPlugin,
            GltfPlugin::default(),
            WorldSerializationPlugin,
            StatesPlugin,
            PhysicsPlugins::default(),
            ModelsPlugin,
            TargetFigurePlugin,
            ArmorPlugin,
        ))
        .init_state::<AppState>()
        .init_resource::<Tuning>()
        .init_asset::<ToonMaterial>()
        .init_resource::<BootGate>()
        .init_resource::<WarmupState>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            DT,
        )))
        .add_systems(Update, dress);
        app.finish();
        app.cleanup();
        app
    }

    fn shown(app: &App, e: Entity) -> bool {
        app.world().get::<Visibility>(e) != Some(&Visibility::Hidden)
    }

    fn hit(app: &mut App, owner: Entity, headshot: bool, killed: bool) {
        let at = app.world().get::<Transform>(owner).unwrap().translation;
        app.world_mut().write_message(DamageDealt {
            source: None,
            target: owner,
            target_kind: DamageTarget::Character,
            amount: 50.0,
            to_shield: 0.0,
            headshot,
            shield_broke: false,
            killed,
            point: at + Vec3::Y * if headshot { 1.65 } else { 1.0 },
            normal: Vec3::Z,
            tick: 1,
        });
    }

    #[test]
    fn a_headshot_swaps_in_the_dented_helmet_and_going_down_flings_the_real_armor() {
        let mut app = app();
        let owner = app
            .world_mut()
            .spawn((
                Character,
                Transform::from_xyz(2.0, 0.0, -5.0),
                {
                    let mut motor = Motor::default();
                    motor.grounded = true;
                    motor
                },
                LookAngles::default(),
                Health::default(),
            ))
            .id();
        let mut rigged = None;
        for _ in 0..3000 {
            app.update();
            let world = app.world_mut();
            rigged = world
                .query::<(&TargetFigure, &KnightRig, &KnightArmorRig)>()
                .iter(world)
                .find(|(f, _, a)| f.owner == owner && a.dent.is_some())
                .map(|(_, r, a)| (r.clone(), a.clone()));
            if rigged.is_some() && world.contains_resource::<ArmorMeshes>() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let (rig, armor) = rigged.expect("the knight's armor was rigged with the dent");
        app.update();
        let dent = armor.dent.unwrap();
        let helmet = armor.shown[0];
        assert!(shown(&app, helmet) && !shown(&app, dent), "a whole helmet");
        // The dent sits exactly where the helmet does.
        assert_eq!(
            app.world().get::<ChildOf>(dent).map(ChildOf::parent),
            Some(armor.nodes[0])
        );
        let open_eye = rig.eyes[0][0];

        // A headshot: the dented helmet, and his eyes still there.
        hit(&mut app, owner, true, false);
        for _ in 0..3 {
            app.update();
        }
        assert!(shown(&app, dent) && !shown(&app, helmet), "dented");
        assert!(
            shown(&app, armor.nodes[0]),
            "the helmet's node (and his eyes under it) stays"
        );
        for _ in 0..60 {
            app.update();
        }
        assert!(
            shown(&app, open_eye),
            "his eyes are back to open, in the helmet"
        );
        assert!(shown(&app, dent), "the dent stays");

        // Going down: the pieces leave him and fly as his own meshes.
        let meshes = app.world().resource::<ArmorMeshes>().clone();
        let tick = 5;
        app.world_mut().entity_mut(owner).insert(Downed { tick });
        app.world_mut().write_message(Eliminated {
            victim: owner,
            by: None,
            position: Vec3::new(2.0, 0.0, -5.0),
            tick,
        });
        app.update();
        for &e in &armor.shown {
            assert!(!shown(&app, e), "the part came off him");
        }
        assert!(!shown(&app, dent));
        assert_eq!(app.world().resource::<ArmorPool>().live(), 5);
        let world = app.world_mut();
        let flying: Vec<Handle<Mesh>> = world
            .query_filtered::<(&Mesh3d, &Visibility), With<ArmorPiece>>()
            .iter(world)
            .filter(|(_, v)| **v == Visibility::Visible)
            .map(|(m, _)| m.0.clone())
            .collect();
        assert_eq!(flying.len(), 5);
        assert!(
            flying.contains(meshes.dent.as_ref().unwrap()),
            "the helmet flies off dented"
        );
        for part in &meshes.parts[1..] {
            assert!(flying.contains(&part.0), "each gauntlet and boot flies");
        }

        // Respawned: whole again.
        app.world_mut().entity_mut(owner).remove::<Downed>();
        for _ in 0..3 {
            app.update();
        }
        for &e in &armor.shown {
            assert!(shown(&app, e), "armor back on");
        }
        assert!(!shown(&app, dent), "a fresh helmet");
    }
}
