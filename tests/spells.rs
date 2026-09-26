//! Slice E (Milestone 2): spells through the simulation seam. Scripted
//! `PlayerIntent` in, the real combat, HUD and spell systems headless, and
//! what a player would see out: the impact, hitmarker and damage number on
//! the hit frame and the bolt on its hit point within 2 frames (gate S6), ten
//! pump sparks along the pellet paths, the dropped hat, and fixed pools.

use avian3d::prelude::PhysicsPlugins;
use bevy::{
    mesh::MeshPlugin, prelude::*, scene::ScenePlugin, state::app::StatesPlugin,
    time::TimeUpdateStrategy,
};
use pieced::{
    app::SimPlugins,
    combat::Downed,
    dummy::{Dummy, look_toward},
    fx::{
        hat::HatProp,
        material::SpellMaterial,
        sim::{
            BOLT_ARRIVAL_FRAMES, HatBody, RIFLE_SCREEN_FLIGHT, apparent_size, axial_billboard,
            bolt_progress, orbit_offset, pellet_flight, rifle_flight, screen_to_path,
        },
        spells::{
            BOLT_POOL, BoltHead, DIZZY_TIME, HALO_POOL, HEAD_FLASH_LIFE, HEAD_FLASH_SIZE,
            ImpactKind, RIFLE_HEAD_ANGLE, RIFLE_HEAD_SIZE, RIFLE_TRAIL_SPARKLES, SpellAssets,
            SpellGlow, SpellHalo, SpellSolid, SpellTiming, SpellsPlugin, pool_counts,
        },
    },
    hud::{HitFeedbackStats, HudPlugin},
    look::ToonMaterial,
    player::HEAD_CENTER,
    render::CurrentFov,
    rng::{Rng, SimRng},
    shared::{
        ActiveTool, AppState, EyeHeight, GalleryFreeze, Health, LookAngles, Player, ShotFired,
        WeaponKind, tick_duration,
    },
    tuning::Tuning,
};

const RIFLE: ActiveTool = ActiveTool::Weapon(WeaponKind::Rifle);
const PUMP: ActiveTool = ActiveTool::Weapon(WeaponKind::Pump);

// ---------------------------------------------------------------------------
// A headless game with the HUD and the spells
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
struct Shots(Vec<ShotFired>);

fn record_shots(mut reader: MessageReader<ShotFired>, mut shots: ResMut<Shots>) {
    shots.0.extend(reader.read().cloned());
}

struct Game {
    app: App,
    player: Entity,
    dummy: Entity,
}

impl Game {
    /// The simulation plus the real HUD (hitmarkers, damage numbers) and the
    /// spells, one fixed tick per rendered frame. `tweak` edits the tuning
    /// before the pools are sized.
    fn new(seed: u64, tweak: impl FnOnce(&mut Tuning)) -> Self {
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
        .add_plugins(HudPlugin)
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<CurrentFov>()
        .init_resource::<Shots>()
        .add_systems(Last, record_shots)
        .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()))
        .insert_resource(SimRng(Rng::new(seed)));
        app.finish();
        app.cleanup();
        {
            let mut tuning = app.world_mut().resource_mut::<Tuning>();
            tuning.dummy.stand_still = true;
            tuning.hud.damage_numbers = true;
            tweak(&mut tuning);
        }
        SpellsPlugin::install(&mut app);
        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::Playing);
        app.update();
        let player = single::<Player>(&mut app);
        let dummy = single::<Dummy>(&mut app);
        let mut game = Self { app, player, dummy };
        // 8 m in front of the player's spawn, in the clear.
        game.place_dummy(Vec3::new(2.0, 0.0, 6.0), 100.0, 100.0);
        game.frames(10);
        game
    }

    fn world(&self) -> &World {
        self.app.world()
    }

    fn frame(&mut self) {
        self.app.update();
    }

    fn frames(&mut self, n: u32) {
        for _ in 0..n {
            self.frame();
        }
    }

    fn feet(&self, e: Entity) -> Vec3 {
        self.world().get::<Transform>(e).unwrap().translation
    }

    fn place_dummy(&mut self, feet: Vec3, hp: f32, shield: f32) {
        let mut e = self.app.world_mut().entity_mut(self.dummy);
        e.get_mut::<Transform>().unwrap().translation = feet;
        let mut h = e.get_mut::<Health>().unwrap();
        h.hp = hp;
        h.shield = shield;
    }

    fn aim_at(&mut self, point: Vec3) {
        let eye = self.feet(self.player)
            + Vec3::Y * self.world().get::<EyeHeight>(self.player).unwrap().0;
        let look = look_toward(point - eye);
        let mut angles = self
            .app
            .world_mut()
            .get_mut::<LookAngles>(self.player)
            .unwrap();
        angles.yaw = look.yaw;
        angles.pitch = look.pitch;
    }

    fn equip(&mut self, tool: ActiveTool) {
        self.intent(|i| i.select = Some(tool));
        self.frames(40);
    }

    fn intent(&mut self, f: impl FnOnce(&mut pieced::shared::PlayerIntent)) {
        let mut intent = self
            .app
            .world_mut()
            .get_mut::<pieced::shared::PlayerIntent>(self.player)
            .unwrap();
        f(&mut intent);
    }

    /// Presses fire and runs the frame the shot lands on.
    fn fire(&mut self) {
        self.intent(|i| {
            i.fire = true;
            i.fire_pressed = true;
        });
        self.frame();
        self.intent(|i| i.fire = false);
    }

    fn timing(&self) -> &SpellTiming {
        self.world().resource::<SpellTiming>()
    }

    fn hud(&self) -> HitFeedbackStats {
        self.world().resource::<HitFeedbackStats>().clone()
    }

    fn shots(&self) -> Vec<ShotFired> {
        self.world().resource::<Shots>().0.clone()
    }

    fn clear_shots(&mut self) {
        self.app.world_mut().resource_mut::<Shots>().0.clear();
    }

    /// Visible pooled glows within `r` of `p`.
    fn glows_near(&mut self, p: Vec3, r: f32) -> usize {
        let world = self.app.world_mut();
        world
            .query_filtered::<(&Transform, &Visibility), Or<(With<SpellGlow>, With<SpellHalo>)>>()
            .iter(world)
            .filter(|(t, v)| **v == Visibility::Visible && t.translation.distance(p) < r)
            .count()
    }

    fn visible_heads(&mut self) -> Vec<(BoltHead, Vec3)> {
        let world = self.app.world_mut();
        world
            .query::<(&BoltHead, &Transform, &Visibility)>()
            .iter(world)
            .filter(|(_, _, v)| **v == Visibility::Visible)
            .map(|(b, t, _)| (*b, t.translation))
            .collect()
    }

    fn hats(&mut self) -> Vec<(HatProp, Transform, Visibility)> {
        let world = self.app.world_mut();
        world
            .query::<(&HatProp, &Transform, &Visibility)>()
            .iter(world)
            .map(|(h, t, v)| (*h, *t, *v))
            .collect()
    }

    fn entity_counts(&mut self) -> [usize; 4] {
        let world = self.app.world_mut();
        [
            world.query::<&SpellGlow>().iter(world).count(),
            world.query::<&SpellSolid>().iter(world).count(),
            world.query::<&SpellHalo>().iter(world).count(),
            world.query::<&HatProp>().iter(world).count(),
        ]
    }

    fn asset_counts(&self) -> [usize; 5] {
        fn count<A: Asset>(w: &World) -> usize {
            w.get_resource::<Assets<A>>().map_or(0, |a| a.len())
        }
        let w = self.world();
        [
            count::<Mesh>(w),
            count::<SpellMaterial>(w),
            count::<ToonMaterial>(w),
            count::<StandardMaterial>(w),
            count::<Image>(w),
        ]
    }
}

fn single<C: Component>(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<C>>()
        .single(world)
        .unwrap()
}

fn chest(feet: Vec3) -> Vec3 {
    feet + Vec3::Y * 1.05
}

/// Distance from `p` to the segment `a`–`b`.
fn off_segment(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

// ---------------------------------------------------------------------------
// Gate S6: same-frame feedback and bolt timing
// ---------------------------------------------------------------------------

/// Fires one rifle shot at `aim` and checks the hit's frame: the impact
/// effect, the hitmarker and the damage number all show on the frame the hit
/// registers, and the bolt's head lands on the hit point within 2 frames.
fn check_hit_frame(game: &mut Game, aim: Vec3, kind: ImpactKind) {
    game.aim_at(aim);
    let before = game.timing().hits.len();
    let hud_before = game.hud();
    game.clear_shots();
    game.fire();

    let timing = game.timing().clone();
    let frame = timing.frame;
    assert_eq!(timing.hits.len(), before + 1, "one hit logged");
    let hit = timing.hits.last().unwrap();
    assert_eq!(hit.kind, kind);
    assert!(hit.hit_this_frame, "the hit registered during this frame");
    assert_eq!(hit.frame, frame);
    assert_eq!(hit.impact_frame, Some(frame), "impact on the hit frame");
    assert_eq!(hit.marker_frame, Some(frame), "hitmarker on the hit frame");
    assert_eq!(
        hit.number_frame,
        Some(frame),
        "damage number on the hit frame"
    );
    let hud = game.hud();
    assert_eq!(hud.markers_same_frame, hud_before.markers_same_frame + 1);
    assert_eq!(hud.numbers_same_frame, hud_before.numbers_same_frame + 1);

    // The impact is on screen now, at the hit point.
    let shot = game.shots().pop().expect("the shot");
    let end = shot.traces[0].end;
    assert!(
        game.glows_near(end, 0.8) >= 2,
        "impact glows at the hit point on the hit frame"
    );

    // The bolt: part-way on the hit frame, on the hit point by frame 2.
    let bolt = timing.bolts.last().unwrap().clone();
    assert_eq!(bolt.fired_frame, frame);
    let heads = game.visible_heads();
    assert_eq!(heads.len(), 1);
    let (head, at) = heads[0];
    assert_eq!(head.end, end);
    assert!(at.distance(end) > 0.5, "still flying on the hit frame");
    let mut arrived = None;
    for k in 1..=BOLT_ARRIVAL_FRAMES {
        game.frame();
        let heads = game.visible_heads();
        if heads.iter().any(|(_, p)| p.distance(end) < 1e-3) {
            arrived = Some(k);
            break;
        }
    }
    assert!(
        arrived.is_some(),
        "the head sat on the hit point within 2 frames"
    );
    let logged = game.timing().bolts.last().unwrap().clone();
    assert_eq!(
        logged.arrived_frame,
        Some(frame + u64::from(arrived.unwrap()))
    );
    game.frames(3);
    assert!(
        game.visible_heads().is_empty(),
        "the head is gone after it lands"
    );
}

#[test]
fn impact_hitmarker_and_number_show_on_the_hit_frame_and_the_bolt_lands_within_two_frames() {
    let mut game = Game::new(61, |_| {});
    let feet = game.feet(game.dummy);

    // Into the shield (a shimmer), a headshot (gold), a shield break, a body hit.
    check_hit_frame(&mut game, chest(feet), ImpactKind::Shield);
    game.frames(40);
    check_hit_frame(&mut game, feet + Vec3::Y * HEAD_CENTER, ImpactKind::Head);
    game.frames(40);
    game.place_dummy(feet, 100.0, 10.0);
    check_hit_frame(&mut game, chest(feet), ImpactKind::ShieldBreak);
    game.frames(40);
    check_hit_frame(&mut game, chest(feet), ImpactKind::Body);

    let timing = game.timing();
    assert_eq!(timing.character_hits, 4);
    assert_eq!(timing.impacts_same_frame, 4);
    assert_eq!(timing.markers_same_frame, 4);
    assert_eq!(timing.numbers_same_frame, 4);
    assert_eq!(timing.bolts_fired, 4);
    assert_eq!(timing.bolts_within_two_frames, 4);
    // The native fx_check run writes the same verdict into its summary.
    let verdict = pieced::scenario::fx_check::s6_verdict(timing);
    assert_eq!(verdict["pass"], true, "{verdict}");
    assert_eq!(
        verdict["hits_with_impact_marker_and_number_on_the_hit_frame"],
        4
    );
}

/// A headshot (T06) flashes a big solid gold starburst in front of the helmet
/// on the hit frame; it is still full size 0.2 s on, while the hat is up at
/// the top of its pop, and then it shrinks away.
#[test]
fn a_headshot_flashes_a_big_solid_gold_starburst_that_holds_while_the_hat_pops() {
    let mut game = Game::new(63, |_| {});
    let feet = game.feet(game.dummy);
    game.place_dummy(feet, 100.0, 0.0);
    game.frames(2);
    game.aim_at(feet + Vec3::Y * HEAD_CENTER);
    game.clear_shots();
    game.fire();
    assert_eq!(game.timing().hits.last().unwrap().kind, ImpactKind::Head);
    let end = game.shots().pop().expect("the shot").traces[0].end;
    let flash = game.world().resource::<SpellAssets>().flash.clone();
    let flashes = |game: &mut Game| -> Vec<Transform> {
        let world = game.app.world_mut();
        world
            .query_filtered::<(&Transform, &Visibility, &MeshMaterial3d<ToonMaterial>), With<SpellSolid>>()
            .iter(world)
            .filter(|(_, v, m)| **v == Visibility::Visible && m.0 == flash)
            .map(|(t, ..)| *t)
            .collect()
    };
    let now = flashes(&mut game);
    assert_eq!(now.len(), 1, "one gold flash on the hit frame");
    let eye = game.feet(game.player) + Vec3::Y * 1.62;
    let at = now[0].translation;
    assert!(at.distance(end) < 0.5, "at the hit: {at} vs {end}");
    assert!(
        at.distance(eye) < end.distance(eye) - 0.2,
        "in front of the helmet"
    );
    game.frames(12);
    let held = flashes(&mut game);
    assert_eq!(held.len(), 1);
    let size = held[0].scale.x;
    assert!(
        size >= HEAD_FLASH_SIZE * 0.99,
        "full size 0.2 s on: {size:.3} m"
    );
    game.frames((HEAD_FLASH_LIFE * 60.0) as u32);
    assert!(
        flashes(&mut game).is_empty(),
        "and gone once its life is up"
    );
}

#[test]
fn a_bolt_that_kills_still_lands_within_two_frames_through_the_hitstop() {
    let mut game = Game::new(62, |_| {});
    let feet = game.feet(game.dummy);
    game.place_dummy(feet, 20.0, 0.0);
    game.aim_at(chest(feet));
    game.fire();
    assert!(game.world().get::<Downed>(game.dummy).is_some(), "killed");
    let fired = game.timing().bolts.last().unwrap().fired_frame;
    game.frames(BOLT_ARRIVAL_FRAMES);
    let bolt = game.timing().bolts.last().unwrap().clone();
    assert!(
        bolt.arrived_frame
            .is_some_and(|f| f - fired <= u64::from(BOLT_ARRIVAL_FRAMES)),
        "{bolt:?}"
    );
}

#[test]
fn misses_fizzle_where_the_bolt_lands() {
    let mut game = Game::new(63, |_| {});
    game.place_dummy(Vec3::new(-18.0, 0.0, 18.0), 100.0, 100.0);
    // Down at the grass a few metres ahead.
    let eye = game.feet(game.player) + Vec3::Y * 1.62;
    let ground = (eye + Vec3::new(1.0, -1.62, -5.0)).with_y(0.0);
    game.aim_at(ground);
    game.clear_shots();
    game.fire();
    let shot = game.shots().pop().unwrap();
    let end = shot.traces[0].end;
    assert!(end.y.abs() < 0.05, "landed on the ground at {end}");
    assert!(game.glows_near(end, 0.6) >= 2, "a fizzle where it landed");
    let world = game.app.world_mut();
    let puffs = world
        .query_filtered::<(&Transform, &Visibility), With<SpellSolid>>()
        .iter(world)
        .filter(|(t, v)| **v == Visibility::Visible && t.translation.distance(end) < 0.6)
        .count();
    assert!(puffs >= 1, "a little puff too");
}

#[test]
fn the_freeze_holds_a_bolt_in_flight() {
    let mut game = Game::new(64, |_| {});
    let feet = game.feet(game.dummy);
    game.aim_at(chest(feet));
    game.fire();
    game.frame();
    let mid = game.visible_heads()[0].1;
    game.app.world_mut().insert_resource(GalleryFreeze);
    game.frames(10);
    let held = game.visible_heads();
    assert_eq!(held.len(), 1, "still drawn while frozen");
    assert!(held[0].1.distance(mid) < 1e-4, "held where it was");
    game.app.world_mut().remove_resource::<GalleryFreeze>();
    game.frame();
    let end = game.visible_heads()[0].0.end;
    assert!(
        game.visible_heads()[0].1.distance(end) < 1e-3,
        "lands once let go"
    );
}

/// T03 paints the rifle bolt as a big glowing starburst trailing a dense
/// sparkle ribbon: the head is at least [`RIFLE_HEAD_ANGLE`] across wherever
/// it flies, and the path is strewn with [`RIFLE_TRAIL_SPARKLES`] sparkles.
#[test]
fn a_rifle_bolt_is_a_big_starburst_with_a_dense_sparkle_trail() {
    let mut game = Game::new(66, |_| {});
    let feet = game.feet(game.dummy);
    game.aim_at(chest(feet));
    game.clear_shots();
    game.fire();
    let shot = game.shots().pop().expect("the shot");
    let (origin, end) = (shot.origin, shot.traces[0].end);
    let world = game.app.world_mut();
    let heads: Vec<(Vec3, f32)> = world
        .query::<(&BoltHead, &Transform, &Visibility)>()
        .iter(world)
        .filter(|(_, _, v)| **v == Visibility::Visible)
        .map(|(_, t, _)| (t.translation, t.scale.x))
        .collect();
    assert_eq!(heads.len(), 1, "one head in flight");
    let (at, size) = heads[0];
    let want = RIFLE_HEAD_SIZE.max(at.distance(origin) * RIFLE_HEAD_ANGLE);
    assert!(
        size >= want * 0.99 && RIFLE_HEAD_ANGLE >= 0.14,
        "head {size:.3} m across at {:.1} m (want {want:.3})",
        at.distance(origin)
    );
    // By the frame after, every trail sparkle is out along the path (the
    // impact at its end not counted).
    game.frames(2);
    let world = game.app.world_mut();
    let trail = world
        .query_filtered::<(&Transform, &Visibility), With<SpellGlow>>()
        .iter(world)
        .filter(|(t, v)| {
            **v == Visibility::Visible
                && off_segment(t.translation, origin, end) < 0.5
                && t.translation.distance(end) > 1.5
        })
        .count();
    assert!(
        trail >= RIFLE_TRAIL_SPARKLES && RIFLE_TRAIL_SPARKLES >= 10,
        "{trail} sparkles along the trail"
    );
}

// ---------------------------------------------------------------------------
// Pump sparks
// ---------------------------------------------------------------------------

#[test]
fn the_pump_sends_ten_sparks_along_the_real_pellet_paths() {
    let mut game = Game::new(65, |_| {});
    game.equip(PUMP);
    let feet = game.feet(game.dummy);
    game.place_dummy(feet, 100.0, 100.0);
    game.aim_at(chest(feet) + Vec3::X * 0.4);
    game.clear_shots();
    game.fire();
    let shot = game
        .shots()
        .into_iter()
        .find(|s| s.weapon == WeaponKind::Pump)
        .expect("a pump shot");
    assert_eq!(shot.traces.len(), 10);
    let heads = game.visible_heads();
    let pellets: Vec<_> = heads
        .iter()
        .filter(|(b, _)| b.weapon == Some(WeaponKind::Pump))
        .collect();
    assert_eq!(pellets.len(), 10, "one spark per pellet");
    // Every spark leaves the gun (headless, with no viewmodel, just ahead of
    // the eye on the aim) and flies to its own pellet's end, along its path.
    let muzzle = shot.origin + (shot.traces[0].end - shot.origin).normalize() * 0.5;
    for trace in &shot.traces {
        let (_, at) = pellets
            .iter()
            .find(|(b, _)| b.end == trace.end)
            .expect("a spark for this pellet");
        assert!(
            off_segment(*at, muzzle, trace.end) < 1e-3,
            "spark off its path"
        );
        assert!(at.distance(muzzle) > 0.3, "out of the gun already");
        let pellet = (trace.end - shot.origin).normalize();
        let spark = (trace.end - *at).normalize();
        assert!(
            pellet.angle_between(spark).to_degrees() < 1.5,
            "flies along the pellet's direction"
        );
    }
    // Where each pellet lands, a small violet-gold burst.
    for trace in shot.traces.iter().filter(|t| t.hit.is_some()) {
        assert!(game.glows_near(trace.end, 0.5) >= 1);
    }
    // All ten land on their pellets' ends within 2 frames.
    game.frames(BOLT_ARRIVAL_FRAMES);
    let heads = game.visible_heads();
    for trace in &shot.traces {
        assert!(
            heads
                .iter()
                .any(|(b, p)| b.end == trace.end && p.distance(trace.end) < 1e-3),
            "pellet spark landed"
        );
    }
}

// ---------------------------------------------------------------------------
// The elimination hat
// ---------------------------------------------------------------------------

#[test]
fn the_hat_drops_on_elimination_settles_within_two_seconds_and_goes_on_respawn() {
    let mut game = Game::new(66, |t| t.dummy.respawn_delay = 5.0);
    assert!(game.hats().iter().all(|(_, _, v)| *v == Visibility::Hidden));
    let feet = game.feet(game.dummy);
    game.place_dummy(feet, 20.0, 0.0);
    game.aim_at(chest(feet));
    game.fire();
    assert!(game.world().get::<Downed>(game.dummy).is_some());
    let dummy = game.dummy;
    let shown: Vec<_> = game
        .hats()
        .into_iter()
        .filter(|(_, _, v)| *v == Visibility::Visible)
        .collect();
    assert_eq!(shown.len(), 1, "one hat drops on the elimination frame");
    let (hat, start, _) = shown[0];
    assert_eq!(hat.victim, Some(dummy));
    assert!(
        (start.translation.y - (feet.y + 1.72)).abs() < 0.1,
        "it starts on his head: {}",
        start.translation
    );

    let mut settled_at = None;
    let mut spun = 0.0f32;
    let mut last_yaw = start.rotation;
    for frame in 1..=180 {
        game.frame();
        let (hat, t, v) = game
            .hats()
            .into_iter()
            .find(|(h, ..)| h.victim == Some(dummy))
            .unwrap();
        assert_eq!(v, Visibility::Visible);
        spun += t.rotation.angle_between(last_yaw);
        last_yaw = t.rotation;
        if hat.settled && settled_at.is_none() {
            settled_at = Some(frame);
            assert!(
                (t.translation.y - feet.y) < 0.1 && t.translation.y >= feet.y,
                "rests on the grass: {}",
                t.translation
            );
        }
    }
    let settled_at = settled_at.expect("settled");
    assert!(settled_at <= 120, "settled after {settled_at} frames");
    assert!(spun > 3.0, "it spun ({spun} rad)");

    // It stays until he's back, then goes.
    game.frames(60);
    assert!(game.world().get::<Downed>(dummy).is_some());
    assert!(
        game.hats()
            .iter()
            .any(|(h, _, v)| h.victim == Some(dummy) && *v == Visibility::Visible)
    );
    for _ in 0..180 {
        game.frame();
        if game.world().get::<Downed>(dummy).is_none() {
            break;
        }
    }
    assert!(game.world().get::<Downed>(dummy).is_none(), "respawned");
    game.frame();
    assert!(
        game.hats()
            .iter()
            .all(|(h, _, v)| *v == Visibility::Hidden && h.victim.is_none()),
        "the hat is gone"
    );
}

// ---------------------------------------------------------------------------
// Pools
// ---------------------------------------------------------------------------

#[test]
fn pools_hold_their_caps_through_a_30_shot_burst_and_nothing_new_is_made() {
    let cap = 16;
    let mut game = Game::new(67, |t| t.feedback.max_particles = cap);
    let counts = pool_counts(game.world()).unwrap();
    assert_eq!(
        counts.glow.1, cap as usize,
        "the particle cap sizes the glow pool"
    );
    assert_eq!(counts.solid.1, cap as usize / 2);
    assert_eq!(counts.bolts.1, BOLT_POOL);
    assert_eq!(counts.halos.1, HALO_POOL);
    let entities = game.entity_counts();
    let assets = game.asset_counts();

    let feet = game.feet(game.dummy);
    game.aim_at(chest(feet));
    game.intent(|i| i.fire = true);
    let mut shots = 0;
    let mut peak = pool_counts(game.world()).unwrap();
    for _ in 0..(6 * 60) {
        game.frame();
        shots = game
            .shots()
            .iter()
            .filter(|s| s.shooter == game.player)
            .count();
        let c = pool_counts(game.world()).unwrap();
        for (live, total) in [c.glow, c.solid, c.bolts, c.halos, c.hats] {
            assert!(live <= total, "{c:?}");
        }
        peak.glow.0 = peak.glow.0.max(c.glow.0);
        peak.solid.0 = peak.solid.0.max(c.solid.0);
        peak.halos.0 = peak.halos.0.max(c.halos.0);
        if shots >= 30 {
            break;
        }
        // Keep him standing (and his shield up) so every shot is a hit.
        game.place_dummy(feet, 100.0, 100.0);
    }
    game.intent(|i| i.fire = false);
    assert!(shots >= 30, "fired {shots}");
    assert_eq!(
        peak.glow.0, cap as usize,
        "the burst filled the capped pool"
    );

    // Pump blasts and a kill on top.
    game.equip(PUMP);
    for _ in 0..3 {
        game.fire();
        game.frames(60);
    }
    game.equip(RIFLE);
    game.place_dummy(feet, 20.0, 0.0);
    game.fire();
    game.frames(30);

    assert_eq!(
        game.entity_counts(),
        entities,
        "no effect entity was spawned"
    );
    assert_eq!(
        game.asset_counts(),
        assets,
        "no mesh, material or image was made"
    );
}

// ---------------------------------------------------------------------------
// Pure spell math
// ---------------------------------------------------------------------------

#[test]
fn bolts_fly_by_frame_and_land_on_frame_two() {
    let early = [0.3, 0.62];
    assert_eq!(bolt_progress(0, early), 0.3);
    assert_eq!(bolt_progress(1, early), 0.62);
    for f in BOLT_ARRIVAL_FRAMES..6 {
        assert_eq!(bolt_progress(f, early), 1.0);
    }
    // Pump sparks fan out in front of the gun, whatever the pellet's range.
    for len in [2.0, 5.0, 40.0] {
        let [a, b] = pellet_flight(len);
        assert!(a > 0.0 && a <= b && b < 1.0);
        assert!(
            a * len <= 1.2 + 1e-4 && b * len <= 3.2 + 1e-4,
            "{len}: {a} {b}"
        );
    }
}

#[test]
fn a_rifle_bolt_is_laid_out_across_the_screen_not_down_its_path() {
    // A shot from a muzzle 0.55 m ahead of the eye (0.25 m right, 0.2 m down)
    // to a knight 20 m out: where does each frame's head appear on screen?
    let eye = Vec3::ZERO;
    let start = Vec3::new(0.25, -0.2, -0.55);
    let end = Vec3::new(-1.5, 0.0, -20.0);
    let screen = |p: Vec3| Vec2::new(p.x, p.y) / -p.z;
    let (a, b) = (screen(start), screen(end));
    let [u0, u1] = rifle_flight(-(start - eye).z, -(end - eye).z);
    for (u, s) in [(u0, RIFLE_SCREEN_FLIGHT[0]), (u1, RIFLE_SCREEN_FLIGHT[1])] {
        let at = screen(start.lerp(end, u));
        let across = (at - a).dot(b - a) / (b - a).length_squared();
        assert!((across - s).abs() < 1e-3, "{across} vs {s}");
    }
    assert!(
        u0 < u1 && u1 < 0.1,
        "only a little way down a long path: {u1}"
    );
    assert_eq!(screen_to_path(0.0, 0.5, 20.0), 0.0);
    assert!((screen_to_path(1.0, 0.5, 20.0) - 1.0).abs() < 1e-6);
}

#[test]
fn streaks_turn_about_their_length_to_face_the_camera() {
    for (dir, to_camera) in [
        (Vec3::NEG_Z, Vec3::Y),
        (Vec3::new(1.0, 0.2, -0.3), Vec3::new(0.1, 0.3, 1.0)),
        (Vec3::X, Vec3::X),
    ] {
        let q = axial_billboard(dir, to_camera.normalize());
        assert!((q * Vec3::Z).angle_between(dir.normalize()) < 1e-4);
        let face = q * Vec3::Y;
        assert!(face.dot(dir.normalize()).abs() < 1e-4);
        let side =
            to_camera.normalize() - dir.normalize() * to_camera.normalize().dot(dir.normalize());
        if side.length() > 1e-3 {
            assert!(face.dot(side.normalize()) > 0.999, "faces the camera");
        }
    }
    assert_eq!(apparent_size(0.3, 2.0, 0.02), 0.3);
    assert!((apparent_size(0.3, 30.0, 0.02) - 0.6).abs() < 1e-6);
}

#[test]
fn dizzy_stars_circle_evenly_for_their_second() {
    let n = 3;
    for t in [0.0, 0.3, DIZZY_TIME] {
        let pts: Vec<Vec3> = (0..n).map(|i| orbit_offset(t, i, n, 0.25, 5.5)).collect();
        for p in &pts {
            assert!(p.xz().length() <= 0.25 + 1e-4 && p.y.abs() <= 0.03 + 1e-4);
        }
        let a = pts[0].xz().normalize();
        let b = pts[1].xz().normalize();
        assert!(a.angle_to(b).abs() > 0.5, "spread out");
    }
    assert!(orbit_offset(0.0, 0, n, 0.25, 5.5).distance(orbit_offset(0.2, 0, n, 0.25, 5.5)) > 0.1);
}

#[test]
fn a_dropped_hat_lands_by_the_poof_and_settles_askew_within_two_seconds() {
    let ground = 0.0;
    let mut hat = HatBody::launch(
        Vec3::new(0.0, 1.72, 0.0),
        Vec3::new(0.3, 1.2, 1.25),
        0.0,
        11.0,
        ground,
        0.045,
    );
    let dt = 1.0 / 60.0;
    let mut t = 0.0;
    let mut landed_at = None;
    let mut settled_at = None;
    while t < 3.0 {
        hat.step(dt);
        t += dt;
        assert!(hat.pos.y >= ground + 0.045 - 1e-4, "sank into the grass");
        if hat.landed && landed_at.is_none() {
            landed_at = Some(t);
        }
        if hat.settled && settled_at.is_none() {
            settled_at = Some(t);
        }
    }
    let landed = landed_at.unwrap();
    // T08 is captured about 0.45 s after the shot, through a 2-frame hitstop.
    assert!(landed <= 0.45 - 2.0 * dt, "landed at {landed}");
    let settled = settled_at.unwrap();
    assert!(settled <= 2.0, "settled at {settled}");
    assert!(hat.settled && hat.spin == 0.0 && hat.vel == Vec3::ZERO);
    assert!(hat.tilt.length() > 0.05, "lies a little askew");
    // Deterministic, and a long frame doesn't blow it up.
    let mut a = HatBody::launch(Vec3::Y, Vec3::Y, 0.0, 5.0, 0.0, 0.04);
    let mut b = a;
    a.step(0.5);
    for _ in 0..30 {
        b.step(0.5 / 30.0);
    }
    assert!(a.pos.distance(b.pos) < 0.05 && a.pos.is_finite());
}
