//! Slice B — building: targeting, placement rules, turbo, the rebuild lock,
//! damage, cracks, destruction and the initial cover, driven through
//! `PlayerIntent` in the headless simulation.

use avian3d::prelude::*;
use bevy::{ecs::system::RunSystemOnce, prelude::*};
use pieced::{
    building::{
        AimedPiece, BuildTarget, BuildTuning, InitialCover, Piece, PieceMap, PieceSlot, Placement,
        check_placement, clear_pieces, damage_piece, initial_cover, place_piece,
        ramp_surface_height, target_slot,
    },
    player::spawn_character,
    shared::{
        ARENA_CELLS, ActiveTool, DamageDealt, DamageTarget, Facing, GameCue, GridCell, Health,
        LEVEL_HEIGHT, Layer, LookAngles, MAX_LEVELS, PieceChange, PieceChanged, PieceHit,
        PieceKind, PreviousFeet,
    },
    sim::Sim,
};

const EYE: f32 = 1.62;

fn cell(x: i32, z: i32, level: i32) -> GridCell {
    GridCell::new(x, z, level)
}

fn center(x: i32, z: i32) -> Vec3 {
    cell(x, z, 0).base_center()
}

fn dir(yaw: f32, pitch: f32) -> Vec3 {
    LookAngles { yaw, pitch }.forward()
}

fn deg(d: f32) -> f32 {
    d.to_radians()
}

/// Pure targeting on an empty map, standing at `feet` looking (yaw, pitch).
fn target(kind: PieceKind, feet: Vec3, yaw: f32, pitch: f32) -> PieceSlot {
    target_on(&PieceMap::default(), kind, feet, yaw, pitch)
}

fn target_on(map: &PieceMap, kind: PieceKind, feet: Vec3, yaw: f32, pitch: f32) -> PieceSlot {
    target_slot(
        feet + Vec3::Y * EYE,
        dir(yaw, pitch),
        feet,
        kind,
        map,
        &BuildTuning::default(),
    )
}

fn ahead(c: GridCell, f: Facing) -> GridCell {
    let o = f.offset();
    GridCell::new(c.x + o.x, c.z + o.y, c.level)
}

/// A simulation with the initial cover removed.
fn empty_sim() -> Sim {
    let mut sim = Sim::new();
    clear_pieces(sim.world_mut());
    sim.tick();
    sim
}

fn put_player(sim: &mut Sim, feet: Vec3, yaw: f32, pitch: f32) {
    let p = sim.player();
    sim.world_mut().get_mut::<Transform>(p).unwrap().translation = feet;
    if let Some(mut prev) = sim.world_mut().get_mut::<PreviousFeet>(p) {
        prev.0 = feet;
    }
    sim.set_look(p, yaw, pitch);
}

fn select(sim: &mut Sim, kind: PieceKind) {
    sim.player_intent().select = Some(ActiveTool::Build(kind));
}

fn press(sim: &mut Sim) {
    sim.player_intent().fire_pressed = true;
    sim.tick();
}

fn piece_count(sim: &Sim) -> usize {
    sim.world().resource::<PieceMap>().len()
}

fn occupant(sim: &Sim, slot: PieceSlot) -> Option<Entity> {
    sim.world().resource::<PieceMap>().occupant(&slot)
}

fn player_target(sim: &mut Sim) -> Option<pieced::building::BuildCandidate> {
    let p = sim.player();
    sim.get::<BuildTarget>(p).candidate
}

fn rejections(sim: &Sim) -> usize {
    sim.recorded::<GameCue>()
        .iter()
        .filter(|c| matches!(c, GameCue::PlacementRejected { .. }))
        .count()
}

fn cast(sim: &mut Sim, origin: Vec3, direction: Dir3, max: f32) -> Option<(Entity, f32)> {
    sim.world_mut()
        .run_system_once(move |q: SpatialQuery| {
            q.cast_ray(
                origin,
                direction,
                max,
                true,
                &SpatialQueryFilter::from_mask(Layer::Piece),
            )
            .map(|h| (h.entity, h.distance))
        })
        .unwrap()
}

// ---------------------------------------------------------------------------
// Targeting (pure)
// ---------------------------------------------------------------------------

#[test]
fn wall_snaps_to_the_front_edge_of_your_cell_for_each_facing() {
    let feet = center(5, 5);
    for f in Facing::ALL {
        assert_eq!(
            target(PieceKind::Wall, feet, f.yaw(), 0.0),
            PieceSlot::wall(cell(5, 5, 0), f),
            "{f:?}"
        );
        // Anything within 45° of a cardinal yaw snaps to it.
        assert_eq!(
            target(PieceKind::Wall, feet, f.yaw() + deg(35.0), deg(-10.0)).facing,
            f
        );
    }
}

#[test]
fn wall_moves_to_the_next_grid_line_when_pressed_up_to_your_own() {
    let c = center(5, 5);
    for f in Facing::ALL {
        let feet = c + f.vector() * (2.0 - 0.3);
        assert_eq!(
            target(PieceKind::Wall, feet, f.yaw(), 0.0),
            PieceSlot::wall(ahead(cell(5, 5, 0), f), f),
            "{f:?}"
        );
        // Still in your own cell with room to spare: your own edge.
        let feet = c + f.vector() * (2.0 - 0.6);
        assert_eq!(
            target(PieceKind::Wall, feet, f.yaw(), 0.0),
            PieceSlot::wall(cell(5, 5, 0), f)
        );
    }
}

#[test]
fn wall_level_follows_look_pitch() {
    let feet = center(5, 5);
    for f in Facing::ALL {
        let level = |pitch: f32| {
            target(PieceKind::Wall, feet, f.yaw(), deg(pitch))
                .cell
                .level
        };
        assert_eq!(level(-60.0), 0);
        assert_eq!(level(20.0), 0, "the top of a wall 2 m away is 34° up");
        assert_eq!(level(45.0), 1);
        assert_eq!(level(85.0), 1, "never more than one level up");
    }
    // Standing on a level-2 floor builds from level 2.
    let high = center(5, 5) + Vec3::Y * (2.0 * LEVEL_HEIGHT + 0.1);
    assert_eq!(target(PieceKind::Wall, high, 0.0, 0.0).cell.level, 2);
    assert_eq!(target(PieceKind::Wall, high, 0.0, deg(50.0)).cell.level, 3);
}

#[test]
fn floor_goes_ahead_at_your_feet_or_one_level_up_when_looking_up() {
    let own = cell(5, 5, 0);
    let feet = center(5, 5);
    for f in Facing::ALL {
        let t = |pitch: f32| target(PieceKind::Floor, feet, f.yaw(), deg(pitch));
        assert_eq!(t(0.0), PieceSlot::new(PieceKind::Floor, ahead(own, f), f));
        assert_eq!(t(-20.0).cell, ahead(own, f), "the ground in the next cell");
        assert_eq!(t(-60.0).cell, own, "the ground at your feet");
        assert_eq!(t(25.0).cell, cell(ahead(own, f).x, ahead(own, f).z, 1));
        assert_eq!(t(70.0).cell, cell(5, 5, 1), "a roof over your own cell");
    }
}

#[test]
fn ramp_goes_ahead_rising_away_or_at_your_feet_when_looking_down() {
    let own = cell(5, 5, 0);
    let feet = center(5, 5);
    for f in Facing::ALL {
        let t = |pitch: f32| target(PieceKind::Ramp, feet, f.yaw(), deg(pitch));
        assert_eq!(t(0.0), PieceSlot::ramp(ahead(own, f), f));
        assert_eq!(t(-60.0), PieceSlot::ramp(own, f));
        let up = t(25.0);
        assert_eq!(up.cell.level, 1, "pitch picks the level");
        assert_eq!(up.facing, f);
    }
}

#[test]
fn climbing_a_ramp_targets_the_next_level_ahead() {
    for f in Facing::ALL {
        // A ramp under the player, rising along the look direction.
        let own = cell(5, 5, 0);
        let mut sim = empty_sim();
        place_piece(sim.world_mut(), PieceSlot::ramp(own, f)).unwrap();
        let map = sim.world().resource::<PieceMap>().clone();
        let next = cell(ahead(own, f).x, ahead(own, f).z, 1);
        for along in [-1.8f32, 0.0, 1.9] {
            // `along` meters from the cell center toward the top of the ramp.
            let feet = center(5, 5) + f.vector() * along + Vec3::Y * ramp_surface_height(-along);
            assert_eq!(
                target_on(&map, PieceKind::Ramp, feet, f.yaw(), deg(5.0)),
                PieceSlot::ramp(next, f),
                "{f:?} at {along}"
            );
            assert_eq!(
                target_on(&map, PieceKind::Floor, feet, f.yaw(), 0.0).cell,
                next
            );
            // The wall in front sits on top of the ramp you're climbing.
            let wall = target_on(&map, PieceKind::Wall, feet, f.yaw(), 0.0);
            assert_eq!(wall.cell.level, 1, "{f:?} at {along}");
        }
    }
}

// ---------------------------------------------------------------------------
// Placement rules
// ---------------------------------------------------------------------------

#[test]
fn press_places_the_targeted_piece_with_a_static_collider() {
    let mut sim = empty_sim();
    sim.record::<PieceChanged>();
    put_player(&mut sim, center(4, 10), Facing::North.yaw(), 0.0);
    select(&mut sim, PieceKind::Wall);
    press(&mut sim);
    let slot = PieceSlot::wall(cell(4, 10, 0), Facing::North);
    let wall = occupant(&sim, slot).expect("wall placed");
    let piece = *sim.get::<Piece>(wall);
    assert_eq!(
        (piece.kind, piece.hp, piece.crack_stage),
        (PieceKind::Wall, 200.0, 0)
    );
    assert_eq!(*sim.get::<RigidBody>(wall), RigidBody::Static);
    let layers = *sim.get::<CollisionLayers>(wall);
    assert_eq!(layers.memberships, LayerMask::from(Layer::Piece));
    assert!(matches!(
        sim.recorded::<PieceChanged>().as_slice(),
        [PieceChanged {
            change: PieceChange::Placed,
            kind: PieceKind::Wall,
            ..
        }]
    ));
    // The collider is where the wall is: 2 m ahead of the player, 0.1 m thick.
    sim.tick();
    let eye = center(4, 10) + Vec3::Y * EYE;
    let (hit, distance) = cast(&mut sim, eye, Dir3::NEG_Z, 10.0).expect("ray hits the wall");
    assert_eq!(hit, wall);
    assert!((distance - 1.9).abs() < 0.01, "{distance}");
}

#[test]
fn occupied_slots_are_rejected_including_shared_edges() {
    let mut sim = empty_sim();
    sim.record::<GameCue>();
    put_player(&mut sim, center(4, 10), Facing::North.yaw(), 0.0);
    select(&mut sim, PieceKind::Wall);
    press(&mut sim);
    assert_eq!(piece_count(&sim), 1);
    assert_eq!(
        player_target(&mut sim).map(|c| c.placement),
        Some(Placement::Occupied)
    );
    press(&mut sim);
    assert_eq!(piece_count(&sim), 1);
    assert_eq!(rejections(&sim), 1, "a rejected press cues feedback");
    // The same edge seen from the neighbouring cell is the same slot.
    let map = sim.world().resource::<PieceMap>().clone();
    let tuning = BuildTuning::default();
    let other_side = PieceSlot::wall(cell(4, 9, 0), Facing::South);
    assert_eq!(
        check_placement(&other_side, &map, 0, &[], &tuning),
        Placement::Occupied
    );
    // Floors, ramps and walls in one cell use separate slots.
    for slot in [
        PieceSlot::floor(cell(4, 10, 0)),
        PieceSlot::ramp(cell(4, 10, 0), Facing::East),
    ] {
        assert!(place_piece(sim.world_mut(), slot).is_ok());
    }
    assert_eq!(
        place_piece(
            sim.world_mut(),
            PieceSlot::ramp(cell(4, 10, 0), Facing::West)
        ),
        Err(Placement::Occupied),
        "one ramp per cell, whatever its facing"
    );
}

#[test]
fn height_limit_and_arena_bounds_are_enforced() {
    let map = PieceMap::default();
    let tuning = BuildTuning::default();
    let check = |slot: PieceSlot| check_placement(&slot, &map, 0, &[], &tuning);
    assert_eq!(
        check(PieceSlot::floor(cell(3, 3, MAX_LEVELS - 1))),
        Placement::Valid
    );
    assert_eq!(
        check(PieceSlot::floor(cell(3, 3, MAX_LEVELS))),
        Placement::AboveHeightLimit
    );
    assert_eq!(
        check(PieceSlot::wall(cell(3, 3, MAX_LEVELS), Facing::North)),
        Placement::AboveHeightLimit
    );
    assert_eq!(
        check(PieceSlot::floor(cell(-1, 3, 0))),
        Placement::OutOfBounds
    );
    assert_eq!(
        check(PieceSlot::ramp(cell(3, ARENA_CELLS, 0), Facing::North)),
        Placement::OutOfBounds
    );
    // The arena's outer edges take walls; beyond them nothing does.
    assert_eq!(
        check(PieceSlot::wall(cell(0, 3, 0), Facing::West)),
        Placement::Valid
    );
    assert_eq!(
        check(PieceSlot::wall(cell(-1, 3, 0), Facing::West)),
        Placement::OutOfBounds
    );

    // Through intents: at the top level, looking up is rejected with a cue.
    let mut sim = empty_sim();
    sim.record::<GameCue>();
    let top = center(4, 10) + Vec3::Y * ((MAX_LEVELS - 1) as f32 * LEVEL_HEIGHT + 0.1);
    put_player(&mut sim, top, Facing::North.yaw(), deg(60.0));
    select(&mut sim, PieceKind::Floor);
    press(&mut sim);
    assert_eq!(
        player_target(&mut sim).map(|c| c.placement),
        Some(Placement::AboveHeightLimit)
    );
    assert_eq!(piece_count(&sim), 0);
    assert_eq!(rejections(&sim), 1);
    // At the arena edge, a floor ahead is out of bounds.
    put_player(&mut sim, center(0, 5), Facing::West.yaw(), 0.0);
    press(&mut sim);
    assert_eq!(
        player_target(&mut sim).map(|c| c.placement),
        Some(Placement::OutOfBounds)
    );
    assert_eq!(piece_count(&sim), 0);
}

#[test]
fn a_wall_that_would_overlap_a_character_is_rejected() {
    let mut sim = empty_sim();
    sim.record::<GameCue>();
    put_player(&mut sim, center(4, 10), Facing::North.yaw(), 0.0);
    // Someone standing on the north edge of the player's cell.
    let edge = center(4, 10) + Vec3::NEG_Z * 2.0;
    {
        let world = sim.world_mut();
        {
            let mut commands = world.commands();
            spawn_character(
                &mut commands,
                edge + Vec3::new(1.0, 0.0, 0.15),
                LookAngles::default(),
                Health::default(),
                (),
            );
        }
        world.flush();
    }
    select(&mut sim, PieceKind::Wall);
    press(&mut sim);
    assert_eq!(
        player_target(&mut sim).map(|c| c.placement),
        Some(Placement::BlocksCharacter)
    );
    assert_eq!(piece_count(&sim), 0);
    assert_eq!(rejections(&sim), 1);
    // Floors and ramps under characters are fine (movement lifts them out).
    let tuning = BuildTuning::default();
    let feet = [edge];
    let map = PieceMap::default();
    assert_eq!(
        check_placement(&PieceSlot::floor(cell(4, 9, 0)), &map, 0, &feet, &tuning),
        Placement::Valid
    );
    // Half a meter from the edge is clear of a wall.
    let clear = [edge + Vec3::Z * 0.5];
    assert_eq!(
        check_placement(
            &PieceSlot::wall(cell(4, 10, 0), Facing::North),
            &map,
            0,
            &clear,
            &tuning
        ),
        Placement::Valid
    );
}

#[test]
fn your_own_walls_never_trap_you() {
    let mut sim = empty_sim();
    let tuning = BuildTuning::default();
    let own_north = PieceSlot::wall(cell(4, 10, 0), Facing::North);
    select(&mut sim, PieceKind::Wall);
    // Walk up to the north edge of a cell in small steps, pressing each time.
    for step in 0..40 {
        let feet = center(4, 10) + Vec3::NEG_Z * (step as f32 * 0.05);
        put_player(&mut sim, feet, Facing::North.yaw(), 0.0);
        let before = piece_count(&sim);
        press(&mut sim);
        if piece_count(&sim) > before {
            let placed = player_target(&mut sim).unwrap().slot;
            let (min, max) = placed.aabb(&tuning);
            assert!(
                !pieced::building::capsule_overlaps_box(feet, &tuning, min, max),
                "wall {placed:?} overlaps the player at {feet}"
            );
        }
    }
    // Pressed up against your own wall, the ghost stays on it.
    assert_eq!(piece_count(&sim), 1);
    assert_eq!(
        player_target(&mut sim),
        Some(pieced::building::BuildCandidate {
            slot: own_north,
            placement: Placement::Occupied
        })
    );

    // Pressed up to an empty grid line, the wall goes on the next one.
    clear_pieces(sim.world_mut());
    let feet = center(4, 10) + Vec3::NEG_Z * 1.7;
    put_player(&mut sim, feet, Facing::North.yaw(), 0.0);
    press(&mut sim);
    let next = PieceSlot::wall(cell(4, 9, 0), Facing::North);
    assert!(occupant(&sim, next).is_some());
    assert!(occupant(&sim, own_north).is_none());
    let (min, max) = next.aabb(&tuning);
    assert!(!pieced::building::capsule_overlaps_box(
        feet, &tuning, min, max
    ));
}

// ---------------------------------------------------------------------------
// Rebuild lock and turbo
// ---------------------------------------------------------------------------

#[test]
fn destroyed_spot_is_locked_for_the_rebuild_window() {
    let mut sim = empty_sim();
    sim.record::<PieceChanged>();
    sim.record::<GameCue>();
    put_player(&mut sim, center(4, 10), Facing::North.yaw(), 0.0);
    select(&mut sim, PieceKind::Wall);
    press(&mut sim);
    let slot = PieceSlot::wall(cell(4, 10, 0), Facing::North);
    let wall = occupant(&sim, slot).unwrap();
    damage_piece(sim.world_mut(), wall, 1000.0);
    sim.tick();
    let destroyed = sim
        .recorded::<PieceChanged>()
        .iter()
        .find(|c| c.change == PieceChange::Destroyed)
        .map(|c| c.tick)
        .expect("destroyed");
    assert_eq!(occupant(&sim, slot), None, "slot freed");

    // 0.1 s after destruction: rejected.
    while sim.sim_tick() < destroyed + 5 {
        sim.tick();
    }
    press(&mut sim);
    assert_eq!(sim.sim_tick(), destroyed + 6);
    assert_eq!(occupant(&sim, slot), None);
    assert_eq!(
        player_target(&mut sim).map(|c| c.placement),
        Some(Placement::RebuildLocked)
    );
    assert_eq!(rejections(&sim), 1);

    // 0.2 s after destruction: allowed.
    while sim.sim_tick() < destroyed + 11 {
        sim.tick();
    }
    press(&mut sim);
    assert_eq!(sim.sim_tick(), destroyed + 12);
    assert!(occupant(&sim, slot).is_some(), "rebuilt after the lock");
}

#[test]
fn turbo_build_places_at_most_one_piece_per_interval_while_moving() {
    let mut sim = empty_sim();
    sim.record::<PieceChanged>();
    select(&mut sim, PieceKind::Wall);
    sim.player_intent().fire = true;
    let start = sim.sim_tick();
    // Walk east along a row (a new cell every 0.2 s) while sweeping the view
    // north/south and up/down, so a free target appears every 0.05 s.
    for i in 0..60u32 {
        let k = (i / 12) as i32;
        let phase = (i / 3) % 4;
        let within = (i % 12) as f32 / 12.0;
        let feet = center(1 + k, 10) + Vec3::X * (within * 0.5);
        let (facing, pitch) = [
            (Facing::North, 0.0),
            (Facing::South, 0.0),
            (Facing::North, 45.0),
            (Facing::South, 45.0),
        ][phase as usize];
        put_player(&mut sim, feet, facing.yaw(), deg(pitch));
        sim.tick();
    }
    let ticks: Vec<u64> = sim
        .recorded::<PieceChanged>()
        .iter()
        .filter(|c| c.change == PieceChange::Placed)
        .map(|c| c.tick - start)
        .collect();
    assert!(
        ticks.len() >= 15,
        "turbo capacity: {} placements in 1 s",
        ticks.len()
    );
    let interval = BuildTuning::default().turbo_ticks();
    assert_eq!(interval, 3);
    assert!(
        ticks.windows(2).all(|w| w[1] - w[0] >= interval),
        "at most one placement per 0.05 s: {ticks:?}"
    );

    // Releasing stops placing.
    sim.player_intent().fire = false;
    let before = piece_count(&sim);
    put_player(&mut sim, center(8, 10), Facing::North.yaw(), 0.0);
    sim.ticks(10);
    assert_eq!(piece_count(&sim), before);
}

#[test]
fn turbo_ramp_rush_keeps_building_ahead_while_running_up() {
    let mut sim = empty_sim();
    select(&mut sim, PieceKind::Ramp);
    sim.player_intent().fire = true;
    let x = 4;
    let speed = 7.5 / 60.0;
    let mut feet = center(x, 11);
    put_player(&mut sim, feet, Facing::North.yaw(), deg(8.0));
    for _ in 0..120 {
        // Advance north if the ground ahead exists (ramps only; flat start cell).
        let next_z = feet.z - speed;
        let next_cell = GridCell::containing(Vec3::new(feet.x, 0.0, next_z));
        let map = sim.world().resource::<PieceMap>();
        let ramp = (0..MAX_LEVELS).find_map(|l| map.ramp_at(cell(x, next_cell.z, l)).map(|_| l));
        let height = match ramp {
            Some(level) => {
                let local = next_z - center(x, next_cell.z).z;
                Some(level as f32 * LEVEL_HEIGHT + ramp_surface_height(local))
            }
            None if next_cell.z == 11 => Some(0.0),
            None => None,
        };
        if let Some(h) = height {
            feet = Vec3::new(feet.x, h, next_z);
        }
        put_player(&mut sim, feet, Facing::North.yaw(), deg(8.0));
        sim.tick();
    }
    let map = sim.world().resource::<PieceMap>();
    for (i, z) in (7..=10).rev().enumerate() {
        let level = i as i32;
        assert!(
            matches!(map.ramp_at(cell(x, z, level)), Some((_, Facing::North))),
            "ramp {i} of the rush at z={z}, level {level}"
        );
    }
    assert!(feet.y > 2.0 * LEVEL_HEIGHT, "climbed to {}", feet.y);
}

#[test]
fn builder_pro_places_on_the_piece_key() {
    let mut sim = empty_sim();
    put_player(&mut sim, center(4, 10), Facing::North.yaw(), 0.0);
    select(&mut sim, PieceKind::Wall);
    sim.tick();
    assert_eq!(piece_count(&sim), 0, "off by default: the key only selects");
    sim.tuning_mut().building.builder_pro = true;
    select(&mut sim, PieceKind::Wall);
    sim.tick();
    assert_eq!(piece_count(&sim), 1);
}

#[test]
fn a_1x1_box_completes_within_one_second_of_intents() {
    let mut sim = empty_sim();
    sim.record::<PieceChanged>();
    let own = cell(4, 10, 0);
    put_player(&mut sim, center(4, 10), Facing::North.yaw(), 0.0);
    let start = sim.sim_tick();
    // Q + click: north wall.
    select(&mut sim, PieceKind::Wall);
    press(&mut sim);
    // Swipe 90° right (8 ticks ≈ 0.13 s), click; three times.
    for _ in 0..3 {
        for _ in 0..8 {
            sim.player_intent().look_delta = Vec2::new(-std::f32::consts::FRAC_PI_2 / 8.0, 0.0);
            sim.tick();
        }
        press(&mut sim);
    }
    // E, swipe down at your feet, click: a ramp inside the box.
    select(&mut sim, PieceKind::Ramp);
    for _ in 0..8 {
        sim.player_intent().look_delta = Vec2::new(0.0, deg(-60.0) / 8.0);
        sim.tick();
    }
    press(&mut sim);
    let elapsed = (sim.sim_tick() - start) as f32 / 60.0;
    assert!(elapsed <= 1.0, "box took {elapsed} s");
    for f in Facing::ALL {
        assert!(
            occupant(&sim, PieceSlot::wall(own, f)).is_some(),
            "{f:?} wall"
        );
    }
    let map = sim.world().resource::<PieceMap>();
    assert!(map.ramp_at(own).is_some(), "ramp inside the box");
    assert_eq!(map.len(), 5);
    assert_eq!(
        sim.recorded::<PieceChanged>()
            .iter()
            .filter(|c| c.change == PieceChange::Placed)
            .count(),
        5
    );
}

// ---------------------------------------------------------------------------
// Damage, cracks, destruction
// ---------------------------------------------------------------------------

#[test]
fn damage_reaches_each_crack_stage_then_destroys() {
    let mut sim = empty_sim();
    sim.record::<PieceChanged>();
    sim.record::<DamageDealt>();
    let wall = place_piece(
        sim.world_mut(),
        PieceSlot::wall(cell(4, 10, 0), Facing::North),
    )
    .unwrap();
    let mut stages = Vec::new();
    for hit in 1..=8 {
        damage_piece(sim.world_mut(), wall, 28.0);
        sim.tick();
        if hit < 8 {
            let piece = *sim.get::<Piece>(wall);
            assert!((piece.hp - (200.0 - 28.0 * hit as f32)).abs() < 1e-3);
            stages.push(piece.crack_stage);
        }
    }
    // 172, 144 intact; 116 (58%) cracked; 88; 60 (30%) badly cracked; 32; 4.
    assert_eq!(stages, vec![0, 0, 1, 1, 2, 2, 2]);
    let changes: Vec<PieceChange> = sim
        .recorded::<PieceChanged>()
        .iter()
        .map(|c| c.change)
        .collect();
    assert_eq!(
        changes,
        vec![
            PieceChange::Cracked(1),
            PieceChange::Cracked(2),
            PieceChange::Destroyed
        ]
    );
    let dealt = sim.recorded::<DamageDealt>();
    assert_eq!(dealt.len(), 8);
    assert!(
        dealt
            .iter()
            .all(|d| d.target == wall && d.target_kind == DamageTarget::Piece)
    );
    assert_eq!(
        dealt.last().map(|d| (d.amount, d.killed)),
        Some((4.0, true))
    );
    assert!(sim.world().get_entity(wall).is_err(), "despawned");

    // Floors and ramps have 170 HP.
    for slot in [
        PieceSlot::floor(cell(6, 10, 0)),
        PieceSlot::ramp(cell(7, 10, 0), Facing::East),
    ] {
        let e = place_piece(sim.world_mut(), slot).unwrap();
        assert_eq!(sim.get::<Piece>(e).max_hp, 170.0);
        damage_piece(sim.world_mut(), e, 60.0);
        sim.tick();
        assert_eq!(sim.get::<Piece>(e).crack_stage, 1, "110/170 = 65%");
        damage_piece(sim.world_mut(), e, 55.0);
        sim.tick();
        assert_eq!(sim.get::<Piece>(e).crack_stage, 2, "55/170 = 32%");
    }
}

#[test]
fn pellets_in_one_tick_land_as_one_damage_event() {
    let mut sim = empty_sim();
    sim.record::<DamageDealt>();
    let wall = place_piece(
        sim.world_mut(),
        PieceSlot::wall(cell(4, 10, 0), Facing::North),
    )
    .unwrap();
    let shooter = sim.player();
    for _ in 0..10 {
        sim.world_mut().write_message(PieceHit {
            piece: wall,
            amount: 10.0,
            source: Some(shooter),
            point: Vec3::ZERO,
            normal: Vec3::Z,
        });
    }
    sim.tick();
    assert_eq!(sim.get::<Piece>(wall).hp, 100.0);
    let dealt = sim.recorded::<DamageDealt>();
    assert_eq!(dealt.len(), 1);
    assert_eq!((dealt[0].amount, dealt[0].source), (100.0, Some(shooter)));
}

#[test]
fn destroying_a_piece_removes_its_collision() {
    let mut sim = empty_sim();
    let slot = PieceSlot::wall(cell(4, 10, 0), Facing::North);
    let wall = place_piece(sim.world_mut(), slot).unwrap();
    sim.tick();
    let origin = slot.center() + Vec3::Z * 3.0;
    assert_eq!(
        cast(&mut sim, origin, Dir3::NEG_Z, 10.0).map(|h| h.0),
        Some(wall)
    );
    damage_piece(sim.world_mut(), wall, 500.0);
    sim.ticks(2);
    assert_eq!(
        cast(&mut sim, origin, Dir3::NEG_Z, 10.0),
        None,
        "ray passes"
    );
    assert_eq!(occupant(&sim, slot), None);
}

#[test]
fn the_crosshair_reports_the_aimed_piece_and_its_hp() {
    let mut sim = empty_sim();
    put_player(&mut sim, center(4, 10), Facing::North.yaw(), 0.0);
    let wall = place_piece(
        sim.world_mut(),
        PieceSlot::wall(cell(4, 10, 0), Facing::North),
    )
    .unwrap();
    sim.ticks(2);
    let p = sim.player();
    let aimed = sim.get::<AimedPiece>(p).0.expect("aiming at the wall");
    assert_eq!((aimed.entity, aimed.hp, aimed.max_hp), (wall, 200.0, 200.0));
    damage_piece(sim.world_mut(), wall, 50.0);
    sim.tick();
    assert_eq!(sim.get::<AimedPiece>(p).0.map(|a| a.hp), Some(150.0));
    sim.set_look(p, Facing::South.yaw(), 0.0);
    sim.tick();
    assert_eq!(sim.get::<AimedPiece>(p).0, None);
}

#[test]
fn build_target_follows_the_tool() {
    let mut sim = empty_sim();
    put_player(&mut sim, center(4, 10), Facing::East.yaw(), 0.0);
    sim.tick();
    assert_eq!(player_target(&mut sim), None, "guns out: no ghost");
    select(&mut sim, PieceKind::Ramp);
    sim.tick();
    let candidate = player_target(&mut sim).expect("build mode shows a ghost");
    assert_eq!(
        candidate.slot,
        PieceSlot::ramp(cell(5, 10, 0), Facing::East)
    );
    assert!(candidate.is_valid());
    sim.player_intent().select = Some(ActiveTool::default());
    sim.tick();
    assert_eq!(player_target(&mut sim), None);
}

// ---------------------------------------------------------------------------
// Initial cover
// ---------------------------------------------------------------------------

#[test]
fn initial_cover_is_placed_clear_of_the_spawns_and_the_line_between() {
    let mut sim = Sim::new();
    sim.tick();
    let cover = initial_cover();
    let map = sim.world().resource::<PieceMap>().clone();
    assert_eq!(map.len(), cover.len());
    let kinds = |k: PieceKind| cover.iter().filter(|s| s.kind == k).count();
    assert!(
        kinds(PieceKind::Wall) >= 4 && kinds(PieceKind::Ramp) >= 2 && kinds(PieceKind::Floor) >= 1
    );
    let tuning = BuildTuning::default();
    let player_spawn = Vec3::new(2.0, 0.0, 14.0);
    let dummy_spawn = Vec3::new(2.0, 0.0, -6.0);
    for slot in &cover {
        let e = map.occupant(slot).expect("placed");
        assert!(sim.world().get::<InitialCover>(e).is_some());
        assert!(sim.world().get::<Collider>(e).is_some());
        let (min, max) = slot.aabb(&tuning);
        for spawn in [player_spawn, dummy_spawn] {
            let nearest = spawn.clamp(min, max);
            let flat = |v: Vec3| Vec2::new(v.x, v.z);
            assert!(
                flat(nearest).distance(flat(spawn)) > 6.0,
                "{slot:?} crowds the spawn at {spawn}"
            );
        }
        assert!(
            max.x < player_spawn.x - 2.0 || min.x > player_spawn.x + 2.0,
            "{slot:?} blocks the line between the spawns"
        );
    }
    // The dummy is in plain view from the player spawn.
    let eye = player_spawn + Vec3::Y * EYE;
    let chest = dummy_spawn + Vec3::Y;
    let to = Dir3::new(chest - eye).unwrap();
    assert_eq!(cast(&mut sim, eye, to, eye.distance(chest)), None);
}
