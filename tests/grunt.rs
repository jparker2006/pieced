//! The grunt's brain through the simulation seam (docs/M3-SPEC.md → The
//! grunt, Fairness): grunts spawned into a headless Waves sim, the player
//! placed or scripted through its intent, and only what a player would notice
//! checked: where grunts stand and walk, when they hold `fire` and what at,
//! how many shoot at once, and that they never build or edit.
//!
//! The wand (slice B) turns a held `fire` into a wind-up and an orb; these
//! tests only need the held intent, so they pass with or without it.

use avian3d::prelude::SpatialQuery;
use bevy::{ecs::system::SystemState, prelude::*};
use pieced::{
    building::{Piece, PieceMap, PieceSlot, clear_pieces, place_piece},
    grunt::{
        AttackTokens, Grunt, GruntBrain, GruntMode, GruntNavStats, GruntStats, Parked, ShotTarget,
        brain::sight, spawn_grunt,
    },
    shared::{
        ActiveTool, EyeHeight, Facing, GridCell, LookAngles, PlayerIntent, PreviousFeet,
        WeaponKind,
    },
    sim::Sim,
    tuning::Tuning,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn spawn(sim: &mut Sim, feet: Vec3, wave: u32) -> Entity {
    let stats = GruntStats::for_wave(wave, &sim.world().resource::<Tuning>().grunt);
    let entity = {
        let mut commands = sim.world_mut().commands();
        spawn_grunt(&mut commands, feet, LookAngles::default(), stats)
    };
    sim.world_mut().flush();
    entity
}

fn put_player(sim: &mut Sim, feet: Vec3) {
    let p = sim.player();
    let world = sim.world_mut();
    world.get_mut::<Transform>(p).unwrap().translation = feet;
    world.get_mut::<PreviousFeet>(p).unwrap().0 = feet;
}

/// The centre of a grid cell's base.
fn cell(x: i32, z: i32, level: i32) -> Vec3 {
    GridCell::new(x, z, level).base_center()
}

fn place(sim: &mut Sim, slot: PieceSlot) -> Entity {
    place_piece(sim.world_mut(), slot).expect("valid placement")
}

fn intent(sim: &Sim, e: Entity) -> PlayerIntent {
    sim.get::<PlayerIntent>(e).clone()
}

fn brain(sim: &Sim, e: Entity) -> GruntBrain {
    sim.get::<GruntBrain>(e).clone()
}

fn flat_distance(a: Vec3, b: Vec3) -> f32 {
    a.xz().distance(b.xz())
}

/// A grunt never builds or edits: no tool change, no edit or reset press,
/// and it keeps its (wand-carrying) weapon slot.
fn assert_never_builds(sim: &Sim, e: Entity) {
    let i = intent(sim, e);
    assert!(i.select.is_none(), "a grunt never picks a tool");
    assert!(!i.edit_pressed && !i.reset_pressed, "a grunt never edits");
    assert!(!i.ads_held);
    assert_eq!(
        *sim.get::<ActiveTool>(e),
        ActiveTool::Weapon(WeaponKind::Rifle)
    );
}

/// Line-of-sight probe from a grunt's eye to the player's head and chest,
/// exactly as a fairness check sees it.
struct Eyes {
    state: SystemState<(SpatialQuery<'static, 'static>, Query<'static, 'static, (), With<Piece>>)>,
}

impl Eyes {
    fn new(sim: &mut Sim) -> Self {
        Self {
            state: SystemState::new(sim.world_mut()),
        }
    }

    /// (player visible, the piece blocking the line if hidden).
    fn look(&mut self, sim: &mut Sim, grunt: Entity) -> (bool, Option<Entity>) {
        let player = sim.player();
        let pf = sim.feet(player);
        let pe = sim.get::<EyeHeight>(player).0;
        let gf = sim.feet(grunt);
        let ge = sim.get::<EyeHeight>(grunt).0;
        let (spatial, pieces) = self.state.get(sim.world()).expect("spatial query");
        let s = sight(
            &spatial,
            gf + Vec3::Y * ge,
            pf + Vec3::Y * pe,
            pf + Vec3::Y * (pe * 0.7),
            |e| pieces.contains(e),
        );
        (s.visible, s.blocker.map(|b| b.0))
    }

    /// The piece (if any) the grunt's look ray hits first.
    fn aimed_piece(&mut self, sim: &mut Sim, grunt: Entity) -> Option<Entity> {
        let gf = sim.feet(grunt);
        let eye = gf + Vec3::Y * sim.get::<EyeHeight>(grunt).0;
        let dir = sim.get::<LookAngles>(grunt).forward();
        let (spatial, pieces) = self.state.get(sim.world()).expect("spatial query");
        let filter = avian3d::prelude::SpatialQueryFilter::from_mask([
            pieced::shared::Layer::World,
            pieced::shared::Layer::Piece,
        ]);
        spatial
            .cast_ray(eye, Dir3::new(dir).ok()?, 60.0, true, &filter)
            .map(|h| h.entity)
            .filter(|e| pieces.contains(*e))
    }
}

/// A walled box around the ground cell (x, z).
fn box_in(sim: &mut Sim, x: i32, z: i32) -> Vec<Entity> {
    Facing::ALL
        .iter()
        .map(|f| place(sim, PieceSlot::wall(GridCell::new(x, z, 0), *f)))
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn a_grunt_approaches_to_mid_range_then_strafes_and_shoots() {
    let mut sim = Sim::waves(1);
    clear_pieces(sim.world_mut());
    put_player(&mut sim, Vec3::ZERO);
    let g = spawn(&mut sim, Vec3::new(-20.0, 0.0, -21.0), 1);
    let mut fired = false;
    for _ in 0..(12 * 60) {
        sim.tick();
        fired |= intent(&sim, g).fire;
        assert_never_builds(&sim, g);
    }
    let d = flat_distance(sim.feet(g), Vec3::ZERO);
    assert!((8.5..=19.5).contains(&d), "stands 10–18 m off: {d}");
    assert_eq!(brain(&sim, g).mode(), GruntMode::Strafe);
    let spot = brain(&sim, g).spot().expect("a spot").pos;
    let spot_d = flat_distance(spot, Vec3::ZERO);
    assert!((10.0..=18.0).contains(&spot_d), "spot in the band: {spot_d}");
    // Strafing: it keeps moving around the spot, within the band.
    let mut travelled = 0.0;
    let mut last = sim.feet(g);
    for _ in 0..(3 * 60) {
        sim.tick();
        fired |= intent(&sim, g).fire;
        let now = sim.feet(g);
        travelled += flat_distance(now, last);
        last = now;
        let d = flat_distance(now, Vec3::ZERO);
        assert!((8.5..=19.5).contains(&d), "stays in the band: {d}");
        assert!(flat_distance(now, spot) <= 2.2, "strafes around its spot");
    }
    assert!(travelled > 3.0, "strafes, not stands: {travelled} m in 3 s");
    assert!(fired, "a grunt in the open shoots");
}

#[test]
fn eight_grunts_spread_out_around_the_player() {
    let mut sim = Sim::waves(3);
    clear_pieces(sim.world_mut());
    put_player(&mut sim, Vec3::ZERO);
    let starts = [
        Vec3::new(-22.0, 0.0, -22.0),
        Vec3::new(0.0, 0.0, -22.5),
        Vec3::new(22.0, 0.0, -22.0),
        Vec3::new(22.5, 0.0, 0.0),
        Vec3::new(22.0, 0.0, 22.0),
        Vec3::new(0.0, 0.0, 22.5),
        Vec3::new(-22.0, 0.0, 22.0),
        Vec3::new(-22.5, 0.0, 0.0),
    ];
    let grunts: Vec<Entity> = starts.iter().map(|&p| spawn(&mut sim, p, 1)).collect();
    let n = grunts.len();
    let mut close = vec![vec![0u32; n]; n];
    let mut worst = 0;
    for _ in 0..(40 * 60) {
        sim.tick();
        let feet: Vec<Vec3> = grunts.iter().map(|&g| sim.feet(g)).collect();
        for i in 0..n {
            for j in (i + 1)..n {
                if flat_distance(feet[i], feet[j]) < 3.0 {
                    close[i][j] += 1;
                    worst = worst.max(close[i][j]);
                } else {
                    close[i][j] = 0;
                }
            }
        }
    }
    assert!(worst <= 120, "two grunts stood within 3 m for {worst} ticks");
    // And they've all settled into the band.
    for &g in &grunts {
        let d = flat_distance(sim.feet(g), Vec3::ZERO);
        assert!((8.5..=19.5).contains(&d), "in the band: {d}");
        assert_never_builds(&sim, g);
    }
    let stats = sim.world().resource::<GruntNavStats>().clone();
    assert!(stats.max_decisions_per_tick <= 2, "{stats:?}");
}

#[test]
fn a_grunt_climbs_a_ramp_to_a_player_on_a_floor_one_level_up() {
    let mut sim = Sim::waves(5);
    clear_pieces(sim.world_mut());
    // A two-cell platform one level up, with a ramp rising north onto it.
    place(&mut sim, PieceSlot::floor(GridCell::new(6, 5, 1)));
    place(&mut sim, PieceSlot::floor(GridCell::new(6, 6, 1)));
    let ramp = place(&mut sim, PieceSlot::ramp(GridCell::new(6, 7, 0), Facing::North));
    let top = cell(6, 5, 1) + Vec3::Y * 0.2;
    put_player(&mut sim, top);
    let g = spawn(&mut sim, Vec3::new(2.0, 0.0, 21.0), 1);
    let ramp_cell = GridCell::new(6, 7, 0);
    let mut on_ramp = false;
    let mut highest = 0.0f32;
    for _ in 0..(20 * 60) {
        sim.tick();
        let f = sim.feet(g);
        let c = GridCell::containing(f);
        on_ramp |= c.x == ramp_cell.x && c.z == ramp_cell.z && f.y > 0.5;
        highest = highest.max(f.y);
        assert_never_builds(&sim, g);
    }
    assert!(sim.world().get::<Piece>(ramp).is_some());
    assert!(on_ramp, "walked up the ramp");
    assert!(highest > 2.5, "reached the player's level: {highest}");
    let f = sim.feet(g);
    assert!(f.y > 2.5, "and stays up there: {f}");
    assert!(flat_distance(f, top) >= 2.0, "off the player's tile");
}

#[test]
fn a_boxed_in_player_gets_their_walls_shot() {
    let mut sim = Sim::waves(7);
    clear_pieces(sim.world_mut());
    let walls = box_in(&mut sim, 6, 6);
    let centre = cell(6, 6, 0);
    put_player(&mut sim, centre);
    let g = spawn(&mut sim, centre + Vec3::new(0.0, 0.0, -16.0), 1);
    let mut eyes = Eyes::new(&mut sim);
    let mut aimed_at_wall = 0;
    for _ in 0..(10 * 60) {
        sim.tick();
        let (visible, blocker) = eyes.look(&mut sim, g);
        assert!(!visible, "the box hides the player");
        let i = intent(&sim, g);
        if i.fire {
            assert!(blocker.is_some_and(|b| walls.contains(&b)));
            assert!(matches!(
                brain(&sim, g).shot_target(),
                Some(ShotTarget::Piece { .. })
            ));
            assert_eq!(brain(&sim, g).mode(), GruntMode::ShootPiece);
            if eyes.aimed_piece(&mut sim, g).is_some_and(|p| walls.contains(&p)) {
                aimed_at_wall += 1;
            }
        }
        assert_never_builds(&sim, g);
    }
    assert!(
        aimed_at_wall >= 20,
        "holds fire aimed at the wall: {aimed_at_wall} ticks"
    );
}

#[test]
fn the_first_shot_waits_for_the_reaction_delay_after_line_of_sight() {
    // In the open: sight is gained the moment it lands.
    let mut sim = Sim::waves(11);
    clear_pieces(sim.world_mut());
    put_player(&mut sim, Vec3::ZERO);
    let reaction = GruntStats::for_wave(1, &sim.world().resource::<Tuning>().grunt).reaction;
    let min_ticks = (reaction * 60.0).round() as u64;
    let spawned = sim.sim_tick();
    let g = spawn(&mut sim, Vec3::new(0.0, 0.0, -14.0), 1);
    let mut first = None;
    for _ in 0..(3 * 60) {
        sim.tick();
        if intent(&sim, g).fire {
            first = Some(sim.sim_tick());
            break;
        }
    }
    let first = first.expect("it fires");
    assert!(
        first - spawned > min_ticks,
        "fired {} ticks after sight; reaction is {min_ticks}",
        first - spawned
    );

    // Boxed in, then the box vanishes: the player shot still waits.
    let mut sim = Sim::waves(12);
    clear_pieces(sim.world_mut());
    let centre = cell(6, 6, 0);
    box_in(&mut sim, 6, 6);
    put_player(&mut sim, centre);
    let g = spawn(&mut sim, centre + Vec3::new(0.0, 0.0, -14.0), 1);
    sim.run_seconds(6.0);
    clear_pieces(sim.world_mut());
    let opened = sim.sim_tick();
    let mut first = None;
    for _ in 0..(3 * 60) {
        sim.tick();
        if intent(&sim, g).fire && brain(&sim, g).shot_target() == Some(ShotTarget::Player) {
            first = Some(sim.sim_tick());
            break;
        }
    }
    let first = first.expect("it fires at the player");
    assert!(
        first - opened > min_ticks,
        "fired {} ticks after the box fell; reaction is {min_ticks}",
        first - opened
    );
}

/// The player strafes left and right, facing north, and sometimes steps
/// behind cover: a stand-in for a real fight.
fn script_player(sim: &mut Sim, t: u32) {
    let dir = if (t / 90) % 2 == 0 { 1.0 } else { -1.0 };
    let forward = if (t / 400) % 3 == 1 { 0.6 } else { 0.0 };
    let mut i = sim.player_intent();
    i.move_axis = Vec2::new(dir, forward).clamp_length_max(1.0);
}

#[test]
fn grunts_never_hold_fire_without_line_of_sight_and_never_build() {
    let mut sim = Sim::waves(21);
    // The initial cover stays; add a few more walls near the player.
    place(&mut sim, PieceSlot::wall(GridCell::new(5, 7, 0), Facing::North));
    place(&mut sim, PieceSlot::wall(GridCell::new(7, 7, 0), Facing::North));
    place(&mut sim, PieceSlot::wall(GridCell::new(6, 4, 0), Facing::South));
    put_player(&mut sim, cell(6, 6, 0));
    let grunts: Vec<Entity> = [
        Vec3::new(-20.0, 0.0, -20.0),
        Vec3::new(20.0, 0.0, -20.0),
        Vec3::new(20.0, 0.0, 20.0),
        Vec3::new(-20.0, 0.0, 20.0),
    ]
    .iter()
    .map(|&p| spawn(&mut sim, p, 5))
    .collect();
    let mut eyes = Eyes::new(&mut sim);
    let mut prev_visible = vec![false; grunts.len()];
    let mut player_shots = 0;
    let mut piece_shots = 0;
    for t in 0..(60 * 60) {
        script_player(&mut sim, t);
        sim.tick();
        for (k, &g) in grunts.iter().enumerate() {
            let (visible, blocker) = eyes.look(&mut sim, g);
            let i = intent(&sim, g);
            if i.fire {
                match brain(&sim, g).shot_target() {
                    Some(ShotTarget::Player) => {
                        assert!(
                            visible || prev_visible[k],
                            "grunt {k} holds fire at an unseen player at tick {t}"
                        );
                        player_shots += 1;
                    }
                    Some(ShotTarget::Piece { .. }) => {
                        assert!(
                            blocker.is_some() || visible || prev_visible[k],
                            "grunt {k} shoots a piece it can't see at tick {t}"
                        );
                        piece_shots += 1;
                    }
                    None => panic!("fire held with no target"),
                }
            }
            prev_visible[k] = visible;
            assert_never_builds(&sim, g);
        }
    }
    assert!(player_shots > 100, "they did fight: {player_shots}");
    let _ = piece_shots;
}

#[test]
fn at_most_three_grunts_hold_attack_tokens() {
    let mut sim = Sim::waves(31);
    clear_pieces(sim.world_mut());
    put_player(&mut sim, Vec3::ZERO);
    let grunts: Vec<Entity> = (0..8)
        .map(|i| {
            let a = i as f32 / 8.0 * std::f32::consts::TAU;
            spawn(&mut sim, Vec3::new(a.cos() * 21.0, 0.0, a.sin() * 21.0), 10)
        })
        .collect();
    let mut peak = 0;
    let mut shots = 0;
    let mut was_firing = vec![false; grunts.len()];
    for t in 0..(60 * 60) {
        script_player(&mut sim, t);
        sim.tick();
        let holders = sim.world().resource::<AttackTokens>().holders().len();
        let firing: Vec<bool> = grunts.iter().map(|&g| intent(&sim, g).fire).collect();
        let n_firing = firing.iter().filter(|f| **f).count();
        assert!(holders <= 3, "{holders} token holders at tick {t}");
        assert!(n_firing <= 3, "{n_firing} grunts winding up at tick {t}");
        for (k, &f) in firing.iter().enumerate() {
            if f && !was_firing[k] {
                shots += 1;
            }
        }
        was_firing = firing;
        peak = peak.max(n_firing);
    }
    assert_eq!(peak, 3, "the cap is reached with eight grunts in sight");
    assert!(shots > 60, "and tokens keep changing hands: {shots} shots");
}

#[test]
fn no_grunt_gets_stuck_over_two_minutes_among_pieces() {
    let mut sim = Sim::waves(41);
    // The initial cover, plus a walled platform with a ramp, a wall run and a box.
    place(&mut sim, PieceSlot::floor(GridCell::new(8, 2, 1)));
    place(&mut sim, PieceSlot::floor(GridCell::new(9, 2, 1)));
    place(&mut sim, PieceSlot::ramp(GridCell::new(8, 3, 0), Facing::North));
    for x in 4..8 {
        place(&mut sim, PieceSlot::wall(GridCell::new(x, 9, 0), Facing::South));
    }
    box_in(&mut sim, 1, 9);
    let spots = [
        cell(6, 6, 0),
        cell(8, 2, 1) + Vec3::Y * 0.2,
        cell(3, 8, 0),
        cell(3, 2, 1) + Vec3::Y * 0.2,
        cell(9, 9, 0),
        cell(5, 3, 0),
    ];
    put_player(&mut sim, spots[0]);
    let grunts: Vec<Entity> = [
        Vec3::new(-22.0, 0.0, -22.0),
        Vec3::new(22.0, 0.0, -22.0),
        Vec3::new(22.0, 0.0, 22.0),
        Vec3::new(-22.0, 0.0, 22.0),
        Vec3::new(0.0, 0.0, 22.5),
        Vec3::new(0.0, 0.0, -22.5),
    ]
    .iter()
    .map(|&p| spawn(&mut sim, p, 8))
    .collect();
    let pieces_before = sim.world().resource::<PieceMap>().len();
    // Independent check: while travelling, 0.5 m of ground every 3 s.
    let mut anchors: Vec<(Vec3, u32)> = grunts.iter().map(|&g| (sim.feet(g), 0)).collect();
    for t in 0..(120 * 60u32) {
        if t % (10 * 60) == 0 {
            put_player(&mut sim, spots[(t / 600) as usize % spots.len()]);
        }
        sim.tick();
        let stuck = sim.world().resource::<GruntNavStats>().stuck_events;
        for (k, &g) in grunts.iter().enumerate() {
            let b = brain(&sim, g);
            let f = sim.feet(g);
            let far = b.spot().is_some_and(|s| s.pos.distance(f) > 1.5);
            if !(b.is_travelling() && far) || anchors[k].0.distance(f) >= 0.5 {
                anchors[k] = (f, t);
            }
            assert!(
                stuck == 0 && t - anchors[k].1 <= 3 * 60 - 10,
                "grunt {k} stalled at tick {t} ({stuck} stuck events): {f}, mode {:?}, spot {:?}, \
                 path {:?}, intent {:?}",
                b.mode(),
                b.spot(),
                b.remaining_path(),
                intent(&sim, g)
            );
            assert_never_builds(&sim, g);
        }
    }
    let stats = sim.world().resource::<GruntNavStats>().clone();
    assert_eq!(stats.stuck_events, 0, "{stats:?}");
    assert!(stats.max_decisions_per_tick <= 2, "{stats:?}");
    assert!(stats.max_expansions <= 2000, "{stats:?}");
    assert!(stats.plans > 20, "they re-plan as the player moves: {stats:?}");
    assert_eq!(
        sim.world().resource::<PieceMap>().len(),
        pieces_before,
        "grunts build nothing"
    );
}

fn approach_speed(wave: u32) -> (f32, f32) {
    let mut sim = Sim::waves(51);
    clear_pieces(sim.world_mut());
    put_player(&mut sim, Vec3::new(0.0, 0.0, 22.0));
    let g = spawn(&mut sim, Vec3::new(0.0, 0.0, -22.0), wave);
    sim.run_seconds(1.0);
    let a = sim.feet(g);
    sim.run_seconds(1.5);
    let b = sim.feet(g);
    let stats = *sim.get::<GruntStats>(g);
    (flat_distance(a, b) / 1.5, stats.speed)
}

#[test]
fn grunts_move_at_their_wave_speed_and_never_as_fast_as_a_sprint() {
    let (w1, s1) = approach_speed(1);
    assert!((s1 - 4.5).abs() < 1e-4);
    assert!((w1 - s1).abs() < 0.3, "wave 1 walks {w1} m/s, wants {s1}");
    let (w25, s25) = approach_speed(25);
    assert!(s25 > 5.5, "wave 25 is faster than a run: {s25}");
    assert!((w25 - s25).abs() < 0.3, "wave 25 moves {w25} m/s, wants {s25}");
    assert!(w25 < 7.5, "never a player's sprint");
}

#[test]
fn a_parked_grunt_idles_and_a_reset_one_comes_back() {
    let mut sim = Sim::waves(61);
    clear_pieces(sim.world_mut());
    put_player(&mut sim, Vec3::ZERO);
    let g = spawn(&mut sim, Vec3::new(0.0, 0.0, -20.0), 1);
    sim.run_seconds(3.0);
    assert!(sim.world().get::<Grunt>(g).is_some());
    sim.world_mut().entity_mut(g).insert(Parked);
    sim.ticks(2);
    let parked_at = sim.feet(g);
    for _ in 0..120 {
        sim.tick();
        assert_eq!(intent(&sim, g), PlayerIntent::default());
        assert!(!sim.world().resource::<AttackTokens>().holders().contains(&g));
    }
    assert!(flat_distance(sim.feet(g), parked_at) < 0.1);
    sim.world_mut().entity_mut(g).remove::<Parked>();
    sim.world_mut().get_mut::<GruntBrain>(g).unwrap().reset();
    assert!(brain(&sim, g).spot().is_none());
    sim.run_seconds(3.0);
    assert!(brain(&sim, g).spot().is_some(), "picks a spot again");
    assert!(flat_distance(sim.feet(g), parked_at) > 1.0, "and moves");
}
