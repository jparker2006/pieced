//! Ship arrivals (docs/M3-SPEC.md → Ship arrivals, D82; Testing Decisions →
//! Ships) through the simulation seam: the drop ship's flight and hover, the
//! rune circle's lead, knights in the beam (unhittable, idle), landed knights
//! acting, the alive-or-beaming cap over a long run, landing distance from the
//! player, waves split across ships, the seeded schedule, and restarts.
//!
//! The player is made unkillable where a test runs long; most tests freeze
//! the brains ([`GalleryFreeze`]) so knights stand where they land.

use bevy::prelude::*;
use pieced::{
    combat::Downed,
    dummy::look_toward,
    grunt::Parked,
    shared::{
        ActiveTool, AppState, DamageDealt, DamageTarget, EyeHeight, GalleryFreeze, GameCue,
        GameMode, Health, PlayerIntent, PreviousFeet, WeaponKind,
    },
    sim::Sim,
    waves::{
        PoolGrunt, RestartRun, Run, RunPhase, RunSeed, SPAWN_MIN_PLAYER_DISTANCE, SkipBreak,
        ships::{
            Beaming, DROP_POINTS, FLIGHT_SECONDS, MAX_SHIPS, SeatState, Ships, Sortie,
            TELEGRAPH_LEAD, station_dock,
        },
    },
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn toughen(sim: &mut Sim) {
    let p = sim.player();
    let mut health = Health::full(1.0e9, 100.0);
    health.shield = 100.0;
    *sim.world_mut().get_mut::<Health>(p).unwrap() = health;
}

/// A Waves sim with an unkillable player; brains frozen if `freeze`.
fn waves(seed: u64, freeze: bool) -> Sim {
    let mut sim = Sim::waves(seed);
    if freeze {
        sim.world_mut().insert_resource(GalleryFreeze);
    }
    toughen(&mut sim);
    sim
}

/// A Waves sim whose run uses `run_seed`.
fn seeded(sim_seed: u64, run_seed: u64) -> Sim {
    let mut app = pieced::app::headless_app(sim_seed);
    app.insert_resource(GameMode::Waves)
        .insert_resource(RunSeed(run_seed));
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Playing);
    app.update();
    let mut sim = Sim { app };
    sim.world_mut().insert_resource(GalleryFreeze);
    toughen(&mut sim);
    sim
}

fn ships(sim: &Sim) -> Ships {
    sim.world().resource::<Ships>().clone()
}

fn run(sim: &Sim) -> Run {
    sim.world().resource::<Run>().clone()
}

fn sorties(sim: &Sim) -> Vec<Sortie> {
    ships(sim).slots.iter().flatten().copied().collect()
}

fn query<F: bevy::ecs::query::QueryFilter>(sim: &mut Sim) -> Vec<Entity> {
    let mut q = sim.world_mut().query_filtered::<Entity, F>();
    let mut v: Vec<Entity> = q.iter(sim.world()).collect();
    v.sort();
    v
}

fn in_play(sim: &mut Sim) -> Vec<Entity> {
    query::<(With<PoolGrunt>, Without<Parked>, Without<Downed>)>(sim)
}

fn beaming(sim: &mut Sim) -> Vec<Entity> {
    query::<With<Beaming>>(sim)
}

fn aboard(sim: &mut Sim) -> usize {
    let mut q = sim.world_mut().query::<&PoolGrunt>();
    q.iter(sim.world()).filter(|p| p.aboard).count()
}

/// Ticks until `done` holds, at most `seconds`.
fn tick_until(sim: &mut Sim, seconds: f32, mut done: impl FnMut(&mut Sim) -> bool) {
    for _ in 0..(seconds * 60.0) as u32 {
        if done(sim) {
            return;
        }
        sim.tick();
    }
    panic!("condition never held within {seconds} s");
}

/// Downs `grunt` the way combat does (a killing hit from the player).
fn kill(sim: &mut Sim, grunt: Entity) {
    let player = sim.player();
    let tick = sim.sim_tick();
    let at = sim.feet(grunt);
    let hp = sim.get::<Health>(grunt).hp;
    sim.world_mut().get_mut::<Health>(grunt).unwrap().hp = 0.0;
    sim.world_mut().entity_mut(grunt).insert(Downed { tick });
    sim.world_mut().write_message(DamageDealt {
        source: Some(player),
        target: grunt,
        target_kind: DamageTarget::Character,
        amount: hp,
        to_shield: 0.0,
        headshot: false,
        shield_broke: false,
        killed: true,
        point: at + Vec3::Y,
        normal: Vec3::Z,
        tick,
    });
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

/// What one drop ship did: when it launched and where, how many it carried,
/// and its hover height.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Launch {
    tick: u64,
    point: usize,
    count: usize,
    hover: f32,
}

/// Every launch in the next `seconds` (knights downed 1 s after landing,
/// breaks skipped).
fn schedule(sim: &mut Sim, seconds: f32) -> Vec<Launch> {
    let mut launches: Vec<Launch> = Vec::new();
    let mut landed: Vec<(Entity, u64)> = Vec::new();
    for _ in 0..(seconds * 60.0) as u32 {
        for s in sorties(sim) {
            if !launches
                .iter()
                .any(|l| l.tick == s.launch_tick && l.point == s.point)
            {
                launches.push(Launch {
                    tick: s.launch_tick,
                    point: s.point,
                    count: s.count,
                    hover: s.hover_height,
                });
            }
        }
        let now = sim.sim_tick();
        let alive = in_play(sim);
        landed.retain(|(e, _)| alive.contains(e));
        for &g in &alive {
            if !landed.iter().any(|(e, _)| *e == g) {
                landed.push((g, now));
            }
        }
        let due: Vec<Entity> = landed
            .iter()
            .filter(|(_, at)| now >= at + 60)
            .map(|(e, _)| *e)
            .collect();
        for g in due {
            kill(sim, g);
        }
        if matches!(run(sim).phase, RunPhase::Break { .. }) {
            sim.world_mut().write_message(SkipBreak);
        }
        sim.tick();
    }
    launches
}

// ---------------------------------------------------------------------------
// The flight and the telegraph
// ---------------------------------------------------------------------------

#[test]
fn a_drop_ship_peels_off_the_station_and_hovers_after_about_four_seconds() {
    let mut sim = waves(1, true);
    tick_until(&mut sim, 5.0, |sim| ships(sim).live() > 0);
    let sortie = sorties(&sim)[0];
    let launched = sortie.launch_tick;
    let hover = sortie.hover_point();
    assert!(
        (12.0..=16.0).contains(&sortie.hover_height),
        "hovers {} m up",
        sortie.hover_height
    );
    let start = sortie.position(sortie.seconds(sim.sim_tick()));
    assert!(
        start.distance(station_dock()) < 80.0,
        "it starts at the station: {start}"
    );
    // Follow it until it's over its drop point.
    let mut arrived = None;
    for _ in 0..(8 * 60) {
        sim.tick();
        let s = sorties(&sim)
            .into_iter()
            .find(|s| s.launch_tick == launched)
            .expect("the ship is still flying");
        let at = s.position(s.seconds(sim.sim_tick()));
        if at.distance(hover) < 0.35 {
            arrived = Some(sim.sim_tick() - launched);
            break;
        }
    }
    let seconds = arrived.expect("it reached its hover point") as f32 / 60.0;
    assert!(
        (FLIGHT_SECONDS - 0.5..=FLIGHT_SECONDS + 0.5).contains(&seconds),
        "hovering after {seconds:.2} s"
    );
    // It hovers over a drop point just inside the edge.
    let point = DROP_POINTS[sortie.point].at;
    assert!(hover.xz().distance(point.xz()) < 1e-3);
}

#[test]
fn the_rune_circle_lights_at_least_two_seconds_before_the_first_landing() {
    let mut sim = waves(2, true);
    sim.record::<GameCue>();
    let mut checked = 0;
    // Circle-lit tick per sortie (by launch tick), and first landing.
    let mut lit: Vec<(u64, u64)> = Vec::new();
    for _ in 0..(20 * 60) {
        let now = sim.sim_tick();
        for s in sorties(&sim) {
            let t = s.seconds(now);
            if s.telegraph(t) > 0.0 && !lit.iter().any(|(l, _)| *l == s.launch_tick) {
                lit.push((s.launch_tick, now));
            }
            if s.seats[0].state == SeatState::Landed
                && let Some(&(_, at)) = lit.iter().find(|(l, _)| *l == s.launch_tick)
                && at != u64::MAX
            {
                let lead = (now - at) as f32 / 60.0;
                assert!(
                    lead >= TELEGRAPH_LEAD - 1e-3,
                    "circle only {lead:.2} s before the landing"
                );
                assert!(lead <= TELEGRAPH_LEAD + 0.1, "circle {lead:.2} s early");
                lit.iter_mut().find(|(l, _)| *l == s.launch_tick).unwrap().1 = u64::MAX;
                checked += 1;
            }
        }
        sim.tick();
    }
    assert!(checked >= 2, "checked {checked} landings");
}

// ---------------------------------------------------------------------------
// The beam
// ---------------------------------------------------------------------------

#[test]
fn knights_in_the_beam_cant_be_hit_and_dont_act() {
    let mut sim = waves(3, false);
    tick_until(&mut sim, 12.0, |sim| !beaming(sim).is_empty());
    let knight = beaming(&mut sim)[0];
    let player = sim.player();
    assert!(sim.world().get::<Parked>(knight).is_some());
    let hp = *sim.get::<Health>(knight);
    // Stand 8 m inward of the beam and fire the rifle straight at the knight.
    let at = sim.feet(knight);
    let inward = (-at).with_y(0.0).normalize();
    let stand = (at + inward * 8.0).with_y(0.0);
    place(&mut sim, player, stand);
    sim.player_intent().select = Some(ActiveTool::Weapon(WeaponKind::Rifle));
    sim.tick();
    sim.record::<DamageDealt>();
    let mut shots = 0;
    while sim.world().get::<Beaming>(knight).is_some() {
        assert_eq!(
            *sim.get::<PlayerIntent>(knight),
            PlayerIntent::default(),
            "a beaming knight holds no intent"
        );
        let chest = sim.feet(knight) + Vec3::Y * 1.0;
        place(&mut sim, player, stand);
        aim_at(&mut sim, chest);
        {
            let mut i = sim.player_intent();
            i.fire = true;
            i.fire_pressed = true;
        }
        sim.tick();
        shots += 1;
        assert!(shots < 120, "the beam lasts about a second");
    }
    sim.player_intent().fire = false;
    assert!(shots >= 20, "fired {shots} times at the beam");
    let hits = sim
        .recorded::<DamageDealt>()
        .iter()
        .filter(|d| d.target == knight)
        .count();
    assert_eq!(hits, 0, "no shot hit a knight in the beam");
    assert_eq!(*sim.get::<Health>(knight), hp, "unharmed");

    // Landed: in play, and it acts (its brain moves it or aims its wand).
    assert!(sim.world().get::<Parked>(knight).is_none());
    assert!(sim.world().get::<Downed>(knight).is_none());
    let landed_at = sim.feet(knight);
    place(&mut sim, player, Vec3::new(2.0, 0.0, 14.0));
    let mut acted = false;
    for _ in 0..(3 * 60) {
        sim.tick();
        if *sim.get::<PlayerIntent>(knight) != PlayerIntent::default() {
            acted = true;
        }
    }
    assert!(acted, "the landed knight acts");
    assert!(
        sim.feet(knight).distance(landed_at) > 0.5,
        "and moves off its landing spot"
    );
}

#[test]
fn a_wave_comes_on_several_ships() {
    for seed in [4, 5, 6] {
        let mut sim = waves(seed, true);
        let mut launches: Vec<u64> = Vec::new();
        let mut knights = 0;
        for _ in 0..(15 * 60) {
            for s in sorties(&sim) {
                if !launches.contains(&s.launch_tick) {
                    launches.push(s.launch_tick);
                    knights += s.count;
                }
            }
            sim.tick();
        }
        assert_eq!(knights, 3, "seed {seed}: wave 1's three knights");
        assert!(launches.len() >= 2, "seed {seed}: {} ships", launches.len());
    }
}

// ---------------------------------------------------------------------------
// The cap, distance, and pacing over a long run
// ---------------------------------------------------------------------------

#[test]
fn never_more_than_eight_alive_or_coming_down_over_many_waves() {
    let mut sim = waves(7, true);
    sim.record::<GameCue>();
    let player = sim.player();
    let cap = sim
        .world()
        .resource::<pieced::tuning::Tuning>()
        .waves
        .max_alive as usize;
    let pool: Vec<Entity> = query::<With<PoolGrunt>>(&mut sim);
    let mut landed: Vec<(Entity, u64)> = Vec::new();
    let mut landings = 0;
    let mut most = 0;
    for _ in 0..(4 * 60 * 60) {
        let now = sim.sim_tick();
        let alive = in_play(&mut sim);
        let coming = aboard(&mut sim);
        let in_beam = beaming(&mut sim).len();
        assert!(in_beam <= coming, "every beaming knight is still aboard");
        assert!(
            alive.len() + coming <= cap,
            "{} alive + {coming} aboard or in a beam (cap {cap})",
            alive.len()
        );
        most = most.max(alive.len() + coming);
        let s = ships(&sim);
        assert!(s.live() <= MAX_SHIPS);
        let r = run(&sim);
        assert_eq!(r.aboard as usize, coming, "the director counts them");
        assert_eq!(s.unlanded(), coming);
        // Landings, from the cue: always well away from the player.
        let feet = sim.feet(player);
        for cue in sim.recorded::<GameCue>() {
            if let GameCue::Respawned { who } = cue
                && pool.contains(&who)
            {
                landings += 1;
                let d = sim.feet(who).xz().distance(feet.xz());
                assert!(
                    d >= SPAWN_MIN_PLAYER_DISTANCE,
                    "landed {d:.1} m from the player"
                );
            }
        }
        sim.clear_recorded::<GameCue>();
        // Down each knight 1.5 s after it lands.
        landed.retain(|(e, _)| alive.contains(e));
        for &g in &alive {
            if !landed.iter().any(|(e, _)| *e == g) {
                landed.push((g, now));
            }
        }
        let due: Vec<Entity> = landed
            .iter()
            .filter(|(_, at)| now >= at + 90)
            .map(|(e, _)| *e)
            .collect();
        for g in due {
            kill(&mut sim, g);
        }
        if matches!(r.phase, RunPhase::Break { .. }) {
            sim.world_mut().write_message(SkipBreak);
        }
        sim.tick();
    }
    let r = run(&sim);
    assert!(r.wave >= 6, "reached wave {}", r.wave);
    assert_eq!(
        landings,
        r.eliminations + r.alive,
        "every knight landed once"
    );
    assert_eq!(most, cap, "the island fills to the cap");
}

#[test]
fn a_moving_player_still_sees_knights_land_at_least_twelve_metres_away() {
    // The player strafes back and forth across the middle of the island.
    let mut sim = waves(8, true);
    sim.record::<GameCue>();
    let player = sim.player();
    let pool: Vec<Entity> = query::<With<PoolGrunt>>(&mut sim);
    let mut landings = 0;
    for tick in 0..(40 * 60) {
        sim.player_intent().move_axis = if (tick / 180) % 2 == 0 {
            Vec2::X
        } else {
            Vec2::NEG_X
        };
        sim.tick();
        let feet = sim.feet(player);
        for cue in sim.recorded::<GameCue>() {
            if let GameCue::Respawned { who } = cue
                && pool.contains(&who)
            {
                landings += 1;
                let d = sim.feet(who).xz().distance(feet.xz());
                assert!(d >= SPAWN_MIN_PLAYER_DISTANCE, "landed {d:.1} m away");
            }
        }
        sim.clear_recorded::<GameCue>();
        for g in in_play(&mut sim) {
            kill(&mut sim, g);
        }
        if matches!(run(&sim).phase, RunPhase::Break { .. }) {
            sim.world_mut().write_message(SkipBreak);
        }
    }
    assert!(landings >= 8, "{landings} landings");
}

// ---------------------------------------------------------------------------
// The seed and restarts
// ---------------------------------------------------------------------------

#[test]
fn the_same_seed_flies_the_same_ships() {
    let a = schedule(&mut seeded(1, 777), 40.0);
    let b = schedule(&mut seeded(2, 777), 40.0);
    assert!(a.len() >= 6, "{} launches", a.len());
    assert_eq!(a, b, "same run seed, same ships");
    let c = schedule(&mut seeded(1, 778), 40.0);
    assert_ne!(a, c, "another seed flies differently");
    // Drop points and loads vary within a run.
    assert!(a.iter().any(|l| l.point != a[0].point));
    assert!(a.iter().any(|l| l.count != a[0].count));
}

fn entities(sim: &mut Sim) -> usize {
    sim.world_mut().query::<Entity>().iter(sim.world()).count()
}

#[test]
fn restarting_with_ships_in_the_air_leaks_nothing() {
    let mut sim = waves(9, true);
    let baseline = entities(&mut sim);
    let pool = query::<With<PoolGrunt>>(&mut sim);
    for cycle in 0..10 {
        // Restart at a different moment each time: mid-flight, mid-beam, after.
        let seconds = 2.0 + cycle as f32 * 0.7;
        sim.run_seconds(seconds);
        sim.world_mut().write_message(RestartRun);
        sim.tick();
        assert_eq!(ships(&sim).live(), 0, "cycle {cycle}: ships gone");
        assert!(beaming(&mut sim).is_empty(), "cycle {cycle}: no beams");
        assert_eq!(aboard(&mut sim), 0);
        let r = run(&sim);
        assert_eq!((r.aboard, r.alive, r.remaining), (0, 0, 3));
        for &g in &pool {
            assert!(sim.world().get::<Parked>(g).is_some(), "{g} parked");
        }
        assert_eq!(entities(&mut sim), baseline, "cycle {cycle}: entity count");
    }
    // And the next run still lands its wave.
    tick_until(&mut sim, 20.0, |sim| in_play(sim).len() == 3);
}

#[test]
fn the_run_ending_sends_the_ships_home_empty() {
    let mut sim = Sim::waves(10);
    sim.world_mut().insert_resource(GalleryFreeze);
    tick_until(&mut sim, 12.0, |sim| !beaming(sim).is_empty());
    let player = sim.player();
    sim.world_mut()
        .get_mut::<Health>(player)
        .unwrap()
        .apply(1.0e12);
    sim.tick();
    assert!(run(&sim).is_ended());
    assert!(beaming(&mut sim).is_empty(), "nobody left in a beam");
    assert_eq!(aboard(&mut sim), 0);
    assert!(sorties(&sim).iter().all(|s| s.cancelled));
    let alive = in_play(&mut sim).len();
    sim.run_seconds(12.0);
    assert!(
        in_play(&mut sim).len() <= alive,
        "nobody lands after the end"
    );
    assert_eq!(ships(&sim).live(), 0, "and the ships fly home");
}

// ---------------------------------------------------------------------------
// The model
// ---------------------------------------------------------------------------

#[test]
fn the_beam_leaves_the_models_hull_crystal_within_budget() {
    let side = pieced::models::Sidecar::parse(include_str!("../assets/models/dropship.json"))
        .expect("the dropship sidecar parses");
    let beam = side.attach("Beam").expect("a Beam attach point").position();
    assert!(
        beam.distance(Vec3::NEG_Y * pieced::waves::ships::CRYSTAL_DROP) < 0.01,
        "the model's Beam point {beam} is CRYSTAL_DROP below its origin"
    );
    assert!(side.part("Crystal").is_some(), "a Crystal part to glow");
    assert!(side.triangles <= 2000, "{} triangles", side.triangles);
}
