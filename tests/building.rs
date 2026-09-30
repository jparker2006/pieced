//! Slice B — building: targeting, placement rules, turbo, the rebuild lock,
//! damage, cracks, destruction and the initial cover, driven through
//! `PlayerIntent` in the headless simulation.

use avian3d::prelude::*;
use bevy::{ecs::system::RunSystemOnce, prelude::*};
use pieced::{
    building::{
        AimedPiece, BuildTarget, BuildTuning, Gait, InitialCover, Piece, PieceMap, PieceSlot,
        Placement, build_target, check_placement, clear_pieces, damage_piece, initial_cover,
        place_piece, ramp_surface_height, target_slot,
    },
    player::spawn_character,
    shared::{
        ARENA_CELLS, ActiveTool, CELL_SIZE, DamageDealt, DamageTarget, Facing, GameCue, GridCell,
        Health, LEVEL_HEIGHT, Layer, LookAngles, MAX_LEVELS, PieceChange, PieceChanged, PieceHit,
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
        false,
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
fn walls_go_on_your_own_cells_edge_wherever_you_stand() {
    // Fortnite-style: pressed right up to the edge, the wall still goes on your
    // own cell's edge (building it pushes you back into the cell), so a box
    // always forms around you.
    let c = center(5, 5);
    for f in Facing::ALL {
        for from_edge in [1.9f32, 0.6, 0.3, 0.05] {
            let feet = c + f.vector() * (2.0 - from_edge);
            assert_eq!(
                target(PieceKind::Wall, feet, f.yaw(), 0.0),
                PieceSlot::wall(cell(5, 5, 0), f),
                "{f:?}, {from_edge} m from the edge"
            );
        }
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
            // Looking down, it's still the next ramp: never the cell above the
            // ramp you're on (it would cap it and wedge you under it).
            assert_eq!(
                target_on(&map, PieceKind::Ramp, feet, f.yaw(), deg(-70.0)),
                PieceSlot::ramp(next, f),
                "{f:?} at {along}, looking down"
            );
            // The wall in front: under your own ramp's top while you aim below it
            // (low on the ramp), otherwise at the high end of the next ramp.
            // Never on the edge you step across onto the next ramp.
            let wall = target_on(&map, PieceKind::Wall, feet, f.yaw(), 0.0);
            let expected = if along < -1.0 {
                PieceSlot::wall(own, f)
            } else {
                PieceSlot::wall(next, f)
            };
            assert_eq!(wall, expected, "{f:?} at {along}");
            assert_ne!(wall, PieceSlot::wall(cell(5, 5, 1), f));
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

    // Pressed right up to an empty grid line, the wall still goes on your own
    // edge, and you're pushed back into your cell, clear of it.
    clear_pieces(sim.world_mut());
    for from_edge in [0.3f32, 0.05] {
        clear_pieces(sim.world_mut());
        let feet = center(4, 10) + Vec3::NEG_Z * (2.0 - from_edge);
        put_player(&mut sim, feet, Facing::North.yaw(), 0.0);
        press(&mut sim);
        assert!(occupant(&sim, own_north).is_some(), "{from_edge} m");
        assert_eq!(piece_count(&sim), 1);
        sim.ticks(3);
        let p = sim.player();
        let now = sim.feet(p);
        let inner_face = cell(4, 10, 0).min_corner().z + tuning.wall_thickness / 2.0;
        assert!(
            now.z - inner_face >= 0.35 - 0.005,
            "{from_edge} m from the edge: still inside the wall at {now}"
        );
        assert_eq!(GridCell::containing(now).z, 10, "pushed back into the cell");
    }
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

// ---------------------------------------------------------------------------
// Ramp rushing, driven through intents with real movement
// ---------------------------------------------------------------------------

/// One scripted ramp rush: select the ramp, then hold forward and build.
#[derive(Debug, Clone, Copy)]
struct Rush {
    start: Vec3,
    /// Start on a floor at this level (built first), rather than the ground.
    floor: Option<i32>,
    /// Look pitch in degrees (the yaw is north).
    pitch: f32,
    sprint: bool,
    /// Jump on the first tick.
    jump: bool,
    /// Switch between the ramp and the wall every this many ticks (0: ramp only).
    wall_every: u32,
    /// Sweep the aim this many degrees to the right and back, every
    /// `sweep_every` ticks, still running north (0: look straight ahead).
    sweep: f32,
    sweep_every: u32,
    ticks: u32,
}

impl Default for Rush {
    fn default() -> Self {
        Self {
            start: center(6, 11),
            floor: None,
            pitch: 0.0,
            sprint: true,
            jump: false,
            wall_every: 0,
            sweep: 0.0,
            sweep_every: 4,
            ticks: 540,
        }
    }
}

/// What a rush did, tick by tick.
struct RushLog {
    /// Every placement: tick index, slot, and the builder's feet then.
    placed: Vec<(usize, PieceSlot, Vec3)>,
    feet: Vec<Vec3>,
    grounded: Vec<bool>,
    falling: Vec<bool>,
    ghost: Vec<Option<pieced::building::BuildCandidate>>,
}

impl RushLog {
    fn ramps(&self) -> Vec<PieceSlot> {
        self.placed
            .iter()
            .map(|p| p.1)
            .filter(|s| s.kind == PieceKind::Ramp)
            .collect()
    }

    /// Horizontal speed over tick `i`.
    fn speed(&self, i: usize) -> f32 {
        let (a, b) = (self.feet[i - 1], self.feet[i]);
        Vec2::new(b.x - a.x, b.z - a.z).length() * 60.0
    }

    /// First tick at which the feet reach `y`.
    fn reaches(&self, y: f32) -> Option<usize> {
        self.feet.iter().position(|f| f.y >= y)
    }

    /// First tick at which the feet are held at the arena's north edge.
    fn at_north_edge(&self) -> Option<usize> {
        let bound = pieced::arena::ArenaLayout::default().bounds_min.y;
        self.feet.iter().position(|f| f.z <= bound + 1e-3)
    }
}

fn run_rush(r: Rush) -> RushLog {
    let mut sim = empty_sim();
    sim.record::<PieceChanged>();
    let mut start = r.start;
    if let Some(level) = r.floor {
        let c = GridCell::containing(start);
        place_piece(sim.world_mut(), PieceSlot::floor(cell(c.x, c.z, level))).unwrap();
        start.y = level as f32 * LEVEL_HEIGHT + 0.15;
    }
    put_player(&mut sim, start, Facing::North.yaw(), deg(r.pitch));
    sim.ticks(10);
    let p = sim.player();
    select(&mut sim, PieceKind::Ramp);
    {
        let mut i = sim.player_intent();
        i.move_axis = Vec2::Y;
        i.sprint = r.sprint;
        i.fire = true;
        i.fire_pressed = true;
        i.jump = r.jump;
        i.jump_pressed = r.jump;
    }
    let mut log = RushLog {
        placed: Vec::new(),
        feet: Vec::new(),
        grounded: Vec::new(),
        falling: Vec::new(),
        ghost: Vec::new(),
    };
    let mut seen = 0;
    for n in 0..r.ticks {
        if r.wall_every > 0 && n > 0 && n % r.wall_every == 0 {
            let kind = if (n / r.wall_every) % 2 == 1 {
                PieceKind::Wall
            } else {
                PieceKind::Ramp
            };
            select(&mut sim, kind);
        }
        if r.sweep != 0.0 && n % r.sweep_every == 0 {
            // Aim right (or straight), keep running north (W plus a strafe).
            let yaw = if (n / r.sweep_every) % 2 == 1 {
                -deg(r.sweep)
            } else {
                0.0
            };
            sim.set_look(p, Facing::North.yaw() + yaw, deg(r.pitch));
            sim.player_intent().move_axis = Vec2::new(yaw.sin(), yaw.cos());
        }
        sim.tick();
        sim.player_intent().jump = false;
        let motor = sim.get::<pieced::movement::Motor>(p).clone();
        let feet = sim.feet(p);
        log.feet.push(feet);
        log.grounded.push(motor.grounded);
        log.falling.push(!motor.grounded && motor.velocity.y < 0.0);
        log.ghost.push(player_target(&mut sim));
        let changes = sim.recorded::<PieceChanged>();
        for c in &changes[seen..] {
            if c.change == PieceChange::Placed {
                let slot = sim.get::<Piece>(c.entity).slot();
                log.placed.push((n as usize, slot, feet));
            }
        }
        seen = changes.len();
    }
    log
}

/// The straight chain a rush from cell (`x`, `z`) at `level` builds: ramp k in
/// the k-th cell north, k levels up, rising north, until the arena's edge or
/// the height limit stops it.
fn chain_from(x: i32, z: i32, level: i32) -> Vec<PieceSlot> {
    (0..)
        .map(|k| (z - 1 - k, level + k))
        .take_while(|&(z, level)| z >= 0 && level < MAX_LEVELS)
        .map(|(z, level)| PieceSlot::ramp(cell(x, z, level), Facing::North))
        .collect()
}

/// The chain from the ground at the default start, cell (6, 11).
fn chain() -> Vec<PieceSlot> {
    chain_from(6, 11, 0)
}

/// A checked rush.
struct Climb {
    log: RushLog,
    /// Tick the climb ends: the top of the chain, or held at the arena's edge.
    end: usize,
    seconds: f32,
    /// Slowest horizontal speed on the way, over the target speed.
    slowest: f32,
}

/// Checks one rush: `chain` built in order (with nothing but `extra` ramps
/// beside it), each ramp early, and a climb that never dips, bumps or slows.
fn assert_endless_ramp(name: &str, r: Rush, chain: &[PieceSlot], extra: &[PieceSlot]) -> Climb {
    let log = run_rush(r);
    let target = if r.sprint { 7.5 } else { 5.5 };
    let linked: Vec<PieceSlot> = log
        .ramps()
        .into_iter()
        .filter(|s| chain.contains(s))
        .collect();
    assert_eq!(linked, chain, "{name}: chain");
    // Every next ramp is placed while the builder is still well short of it
    // (as they step onto the one before).
    for (tick, slot, feet) in log.placed.iter().filter(|p| chain[1..].contains(&p.1)) {
        let low_edge = slot.cell.min_corner().z + CELL_SIZE;
        assert!(
            feet.z - low_edge >= 2.5,
            "{name}: {slot:?} placed late (tick {tick}, feet {feet})"
        );
    }
    let last = chain.last().unwrap();
    let top_y = (last.cell.level + 1) as f32 * LEVEL_HEIGHT;
    let end = [log.reaches(top_y - 0.05), log.at_north_edge()]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or_else(|| panic!("{name}: never got up the chain: {:?}", log.feet.last()));
    assert!(
        log.feet[end].y >= top_y - LEVEL_HEIGHT * 0.2,
        "{name}: short of the top at {}",
        log.feet[end]
    );
    let full = (1..end)
        .find(|&i| log.speed(i) >= target - 1e-3)
        .expect("reaches full speed");
    let first = chain[0].cell;
    let on_chain = (0..end)
        .find(|&i| {
            let c = GridCell::containing(log.feet[i]);
            (c.x, c.z) == (first.x, first.z) && log.grounded[i]
        })
        .expect("lands on the first ramp");
    let mut slowest = f32::MAX;
    for i in full..=end {
        // The tick that runs into the arena's edge is cut short by it.
        if i < end {
            slowest = slowest.min(log.speed(i) / target);
            assert!(
                log.speed(i) >= 0.97 * target,
                "{name}: slowed to {:.2} m/s at {}",
                log.speed(i),
                log.feet[i]
            );
        }
        if i > on_chain {
            assert!(log.grounded[i], "{name}: left the ramp at {}", log.feet[i]);
            assert!(
                log.feet[i].y >= log.feet[i - 1].y - 1e-3,
                "{name}: dipped at {}",
                log.feet[i]
            );
            assert!(
                log.feet[i].y - log.feet[i - 1].y < 0.15,
                "{name}: bumped up at {}",
                log.feet[i]
            );
        }
    }
    // No ramp lands behind, beside or above the chain on the way up.
    for (tick, slot, _) in &log.placed {
        if *tick <= end && slot.kind == PieceKind::Ramp {
            assert!(
                chain.contains(slot) || extra.contains(slot),
                "{name}: stray {slot:?} at tick {tick}"
            );
        }
    }
    Climb {
        seconds: end as f32 / 60.0,
        slowest,
        end,
        log,
    }
}

#[test]
fn ramp_rush_runs_up_an_endless_ramp_without_slowing() {
    // Gate S10: at least 10 consecutive ramps, each a level higher, never
    // slower than 90% of run speed (checked: 97% of the run or sprint speed),
    // from a standing start and a sprint, looking level, 15° down and 15° up.
    let chain = chain();
    assert!(chain.len() >= 10, "the arena fits {} ramps", chain.len());
    for (name, pitch, sprint, jump) in [
        ("level", 0.0, false, false),
        ("level sprint", 0.0, true, false),
        ("15 down", -15.0, false, false),
        ("15 down sprint", -15.0, true, false),
        ("15 up", 15.0, false, false),
        ("15 up sprint", 15.0, true, false),
        ("35 down sprint", -35.0, true, false),
        ("45 up sprint", 45.0, true, false),
        ("jump sprint", 0.0, true, true),
    ] {
        let r = Rush {
            pitch,
            sprint,
            jump,
            ..default()
        };
        let climb = assert_endless_ramp(name, r, &chain, &[]);
        println!(
            "{name:>15}: {} ramps, {:.1} m up in {:.2} s, slowest {:.0}% of {}",
            chain.len(),
            climb.log.feet[climb.end].y,
            climb.seconds,
            climb.slowest * 100.0,
            if sprint { "sprint" } else { "run" }
        );
    }
}

#[test]
fn ramp_rush_stops_cleanly_at_the_arena_edge_and_the_height_limit() {
    for sprint in [true, false] {
        let target = if sprint { 7.5 } else { 5.5 };

        // From the ground, the rush runs the arena's length. At its edge the
        // next ramp would leave the arena: the ghost shows it red, nothing is
        // placed, and you stand at the top of the last ramp, held by the edge.
        let chain = chain();
        let climb = assert_endless_ramp(
            "to the edge",
            Rush {
                sprint,
                ..default()
            },
            &chain,
            &[],
        );
        let log = &climb.log;
        let ghost = log.ghost.last().unwrap().expect("ghost");
        assert_eq!(ghost.placement, Placement::OutOfBounds);
        assert_eq!(
            ghost.slot,
            PieceSlot::ramp(cell(6, -1, chain.len() as i32), Facing::North)
        );
        assert_eq!(log.placed.len(), chain.len(), "nothing stray");
        for i in climb.end + 1..log.feet.len() {
            assert!(
                log.grounded[i] && log.speed(i) < 1e-3,
                "held at {}",
                log.feet[i]
            );
        }

        // From a floor six levels up, the rush meets the height limit first.
        let high = chain_from(6, 11, 6);
        assert_eq!(high.last().unwrap().cell.level, MAX_LEVELS - 1);
        let r = Rush {
            sprint,
            floor: Some(6),
            ticks: 600,
            ..default()
        };
        let climb = assert_endless_ramp("to the height limit", r, &high, &[]);
        let log = &climb.log;
        let top = climb.end;
        // On the top ramp the next one would break the height limit: the ghost
        // shows it red and nothing is placed.
        let ghost = log.ghost[top].expect("ghost");
        assert_eq!(ghost.placement, Placement::AboveHeightLimit);
        assert_eq!(
            ghost.slot,
            PieceSlot::ramp(cell(6, 4, MAX_LEVELS), Facing::North)
        );
        assert!(log.placed.iter().all(|p| p.1.cell.level < MAX_LEVELS));
        // Running off the top: nothing is built in the air on the way down.
        let fall: Vec<usize> = (top..log.feet.len()).filter(|&i| log.falling[i]).collect();
        assert!(fall.len() > 60, "falls off the top");
        for &i in &fall {
            assert!(log.placed.iter().all(|p| p.0 != i), "placed while falling");
        }
        assert!(
            fall.iter()
                .any(|&i| log.ghost[i].map(|g| g.placement) == Some(Placement::Falling))
        );
        let landed = *fall.last().unwrap() + 1;
        assert!(
            log.grounded[landed] && log.feet[landed].y < 0.05,
            "lands on the ground"
        );
        assert!(
            log.speed(landed + 1) >= 0.97 * target,
            "landing keeps the run speed"
        );
        // Landed, the rush starts again from the ground and runs up to the
        // arena's edge.
        let restart: Vec<PieceSlot> = log.ramps()[high.len()..].to_vec();
        let first = restart.first().expect("the rush restarts").cell;
        assert_eq!(restart, chain_from(6, first.z + 1, 0));
        assert_eq!(restart.last().unwrap().cell.z, 0, "up to the arena's edge");
        let end = *log.feet.last().unwrap();
        assert!(
            log.at_north_edge().is_some(),
            "held by the arena's edge at {end}"
        );
        assert!(
            *log.grounded.last().unwrap(),
            "standing on the last ramp at {end}"
        );
        assert_eq!(
            log.placed.len(),
            high.len() + restart.len(),
            "nothing stray"
        );
    }
}

#[test]
fn ramp_rush_with_walls_in_front_never_blocks_the_climb() {
    // Ramp + wall: flick between the two while holding forward and build. Each
    // wall shields the next ramp's high end and never blocks the way onto it.
    let chain = chain();
    for (name, pitch, sprint) in [("sprint", 0.0, true), ("run, 15 down", -15.0, false)] {
        let r = Rush {
            pitch,
            sprint,
            wall_every: 4,
            ..default()
        };
        let climb = assert_endless_ramp(name, r, &chain, &[]);
        let log = &climb.log;
        let walls: Vec<PieceSlot> = log
            .placed
            .iter()
            .filter(|p| p.0 <= climb.end && p.1.kind == PieceKind::Wall)
            .map(|p| p.1)
            .collect();
        let shields: Vec<PieceSlot> = chain
            .iter()
            .map(|r| PieceSlot::wall(r.cell, r.facing))
            .collect();
        assert_eq!(walls, shields, "{name}");
        println!(
            "{name:>13}: {} ramps + {} walls, {:.1} m up in {:.2} s, slowest {:.0}%",
            chain.len(),
            walls.len(),
            log.feet[climb.end].y,
            climb.seconds,
            climb.slowest * 100.0
        );
    }
}

#[test]
fn a_double_ramp_rush_builds_two_ramps_side_by_side() {
    // Sweep the aim across to the tile on the right and back while rushing (and
    // keep running straight): every row gets a second ramp beside the first,
    // level with it.
    let chain = chain();
    let right = chain_from(7, 11, 0);
    let r = Rush {
        sweep: 35.0,
        ..default()
    };
    let climb = assert_endless_ramp("double", r, &chain, &right);
    let doubled = right
        .iter()
        .filter(|s| climb.log.ramps().contains(s))
        .count();
    assert!(
        doubled >= chain.len() - 1,
        "{doubled} of {} rows doubled",
        chain.len()
    );
    println!(
        "double ramp: {} + {doubled} ramps in {:.2} s, slowest {:.0}%",
        chain.len(),
        climb.seconds,
        climb.slowest * 100.0
    );
}

#[test]
fn ramp_rush_targeting_ignores_the_look_pitch() {
    let rush = |map: &PieceMap, feet: Vec3, yaw: f32, pitch: f32| {
        target_slot(
            feet + Vec3::Y * EYE,
            dir(yaw, deg(pitch)),
            feet,
            PieceKind::Ramp,
            true,
            map,
            &BuildTuning::default(),
        )
    };
    let own = cell(5, 5, 0);
    for f in Facing::ALL {
        // On the ground: the cell ahead at ground level, looking up or down,
        // from anywhere in the cell. Only a steep look down builds under you.
        let empty = PieceMap::default();
        for along in [-1.9f32, 0.0, 1.9] {
            let feet = center(5, 5) + f.vector() * along;
            for pitch in (-10..=17).map(|k| k as f32 * 5.0) {
                let expected = if pitch <= -55.0 { own } else { ahead(own, f) };
                assert_eq!(
                    rush(&empty, feet, f.yaw(), pitch),
                    PieceSlot::ramp(expected, f),
                    "{f:?} at {along}, pitch {pitch}"
                );
            }
        }
        // Up against a wall of yours, the ramp goes under you, rising to it.
        let mut sim = empty_sim();
        place_piece(sim.world_mut(), PieceSlot::wall(own, f)).unwrap();
        let walled = sim.world().resource::<PieceMap>().clone();
        assert_eq!(
            rush(&walled, center(5, 5), f.yaw(), 0.0),
            PieceSlot::ramp(own, f)
        );

        // On a ramp: the next link, whatever the pitch, looking a little to
        // either side too.
        let mut sim = empty_sim();
        place_piece(sim.world_mut(), PieceSlot::ramp(own, f)).unwrap();
        let map = sim.world().resource::<PieceMap>().clone();
        let next = PieceSlot::ramp(cell(ahead(own, f).x, ahead(own, f).z, 1), f);
        for along in [-1.9f32, 0.0, 1.9] {
            let feet = center(5, 5) + f.vector() * along + Vec3::Y * ramp_surface_height(-along);
            for pitch in (-17..=17).map(|k| k as f32 * 5.0) {
                for turn in [-15.0f32, 0.0, 15.0] {
                    assert_eq!(
                        rush(&map, feet, f.yaw() + deg(turn), pitch),
                        next,
                        "{f:?} at {along}, pitch {pitch}, turned {turn}"
                    );
                }
            }
        }
        // Aiming at the tile beside the next one builds there, level with it
        // (double ramps): 40° to the right or left from the middle of the ramp.
        let mid = center(5, 5) + Vec3::Y * 1.5;
        let right = f.vector().cross(Vec3::Y);
        for (turn, side) in [(-40.0f32, right), (40.0, -right)] {
            let o = Facing::from_direction(side).offset();
            let beside = cell(next.cell.x + o.x, next.cell.z + o.y, 1);
            assert_eq!(
                rush(&map, mid, f.yaw() + deg(turn), 0.0),
                PieceSlot::ramp(beside, f),
                "{f:?} turned {turn}"
            );
        }
        // Turned 90° at the top: a steep look down builds in your own column one
        // level up, facing the new way (90s); a level look starts a new chain.
        let top = center(5, 5) + f.vector() * 1.8 + Vec3::Y * 2.9;
        let side = Facing::ALL[(Facing::ALL.iter().position(|&g| g == f).unwrap() + 1) % 4];
        assert_eq!(
            rush(&map, top, side.yaw(), -60.0),
            PieceSlot::ramp(cell(5, 5, 1), side)
        );
        assert_eq!(
            rush(&map, top, side.yaw(), 0.0),
            PieceSlot::ramp(cell(ahead(own, side).x, ahead(own, side).z, 1), side)
        );
    }
}

#[test]
fn running_up_near_a_ramps_side_edge_still_counts_as_on_it() {
    // Feet a hair across the ramp's side edge (the body still on the ramp):
    // building continues the chain, never caps the ramp you're on.
    let own = cell(5, 5, 0);
    for f in Facing::ALL {
        let mut sim = empty_sim();
        place_piece(sim.world_mut(), PieceSlot::ramp(own, f)).unwrap();
        let map = sim.world().resource::<PieceMap>().clone();
        let next = PieceSlot::ramp(cell(ahead(own, f).x, ahead(own, f).z, 1), f);
        let side = f.vector().cross(Vec3::Y);
        for along in [-1.0f32, 0.5, 1.5] {
            let feet = center(5, 5)
                + f.vector() * along
                + side * (CELL_SIZE / 2.0 + 0.05)
                + Vec3::Y * (ramp_surface_height(-along) + 0.09);
            for (pitch, advancing) in [(8.0, false), (8.0, true), (-20.0, false), (20.0, true)] {
                let slot = target_slot(
                    feet + Vec3::Y * EYE,
                    dir(f.yaw(), deg(pitch)),
                    feet,
                    PieceKind::Ramp,
                    advancing,
                    &map,
                    &BuildTuning::default(),
                );
                assert_ne!(
                    slot.cell,
                    cell(5, 5, 1),
                    "{f:?} at {along}: capped the ramp"
                );
                assert_eq!(
                    slot, next,
                    "{f:?} at {along}, pitch {pitch}, advancing {advancing}"
                );
            }
        }
    }
}

#[test]
fn floors_and_ramps_go_where_the_aim_lands_around_you() {
    // Fortnite reach: the tile under where the aim ray lands, among the 3×3
    // tiles around yours (diagonals included), at your level looking down or
    // one up looking up; looking about level (landing out of reach) builds
    // ahead. Facing north throughout; offsets are (east, south) tiles.
    let near = BuildTuning::default();
    let far = BuildTuning {
        reach_tiles: 2,
        ..default()
    };
    let own = cell(5, 5, 0);
    let at = |dx: i32, dz: i32, level: i32| cell(own.x + dx, own.z + dz, level);
    let aim = |tuning: &BuildTuning,
               kind: PieceKind,
               offset: Vec2,
               yaw: f32,
               pitch: f32,
               advancing: bool| {
        let feet = center(5, 5) + Vec3::new(offset.x, 0.0, offset.y);
        target_slot(
            feet + Vec3::Y * EYE,
            dir(deg(yaw), deg(pitch)),
            feet,
            kind,
            advancing,
            &PieceMap::default(),
            tuning,
        )
    };
    for kind in [PieceKind::Floor, PieceKind::Ramp] {
        let cases: [(&BuildTuning, Vec2, f32, f32, GridCell); 7] = [
            // Straight ahead, down onto the next tile.
            (&near, Vec2::ZERO, 0.0, -30.0, at(0, -1, 0)),
            // Diagonally ahead-right (yaw -44° still faces north).
            (&near, Vec2::ZERO, -44.0, -25.0, at(1, -1, 0)),
            // Standing in the south-east of the cell, down onto the tile to the right.
            (&near, Vec2::new(1.5, 1.0), -30.0, -35.0, at(1, 0, 0)),
            // Looking up at the tile diagonally ahead-left: one level up.
            (&near, Vec2::ZERO, 44.0, 20.0, at(-1, -1, 1)),
            // Landing two tiles out: out of reach, so the cell ahead...
            (&near, Vec2::ZERO, -44.0, -8.0, at(0, -1, 0)),
            // ...unless reach is 2.
            (&far, Vec2::ZERO, -44.0, -8.0, at(2, -2, 0)),
            // Near level, the cell ahead.
            (&near, Vec2::ZERO, -44.0, 0.0, at(0, -1, 0)),
        ];
        for (tuning, offset, yaw, pitch, expected) in cases {
            assert_eq!(
                aim(tuning, kind, offset, yaw, pitch, false),
                PieceSlot::new(kind, expected, Facing::North),
                "{kind:?} from {offset} at yaw {yaw}, pitch {pitch}"
            );
        }
    }
    // Moving forward, a floor's landing point is pushed ahead: a look that
    // lands at your feet standing lands in the next tile running.
    assert_eq!(
        aim(&near, PieceKind::Floor, Vec2::ZERO, 0.0, -45.0, false).cell,
        own
    );
    assert_eq!(
        aim(&near, PieceKind::Floor, Vec2::ZERO, 0.0, -45.0, true).cell,
        at(0, -1, 0)
    );
}

#[test]
fn falling_rushes_wait_for_your_feet() {
    let tuning = BuildTuning::default();
    let empty = PieceMap::default();
    let ghost = |map: &PieceMap, feet: Vec3, gait: Gait| {
        build_target(
            feet + Vec3::Y * EYE,
            dir(0.0, 0.0),
            feet,
            PieceKind::Ramp,
            gait,
            map,
            0,
            &[],
            &tuning,
        )
    };
    // Falling past empty cells: the chain waits (it would pass overhead).
    let air = center(5, 5) + Vec3::Y * 10.0;
    assert_eq!(
        ghost(&empty, air, Gait::Falling).placement,
        Placement::Falling
    );
    assert_eq!(
        ghost(&empty, air, Gait::Advancing).placement,
        Placement::Valid
    );
    // Dropping onto a ramp: its next link can still go down.
    let mut sim = empty_sim();
    place_piece(
        sim.world_mut(),
        PieceSlot::ramp(cell(5, 5, 0), Facing::North),
    )
    .unwrap();
    let map = sim.world().resource::<PieceMap>().clone();
    let over = center(5, 5) + Vec3::Y * 2.0;
    let c = ghost(&map, over, Gait::Falling);
    assert_eq!(c.slot, PieceSlot::ramp(cell(5, 4, 1), Facing::North));
    assert_eq!(c.placement, Placement::Valid);
}

#[test]
fn a_ramp_built_on_your_own_cell_lifts_you_onto_it() {
    // Standing anywhere in the cell, a ramp at your feet lifts you onto its slope
    // (you used to be left buried inside it, unable to move).
    for along in [0.0f32, 1.6] {
        let mut sim = empty_sim();
        let feet = center(4, 10) + Vec3::NEG_Z * along;
        put_player(&mut sim, feet, Facing::North.yaw(), deg(-70.0));
        sim.ticks(3);
        select(&mut sim, PieceKind::Ramp);
        press(&mut sim);
        let own = PieceSlot::ramp(cell(4, 10, 0), Facing::North);
        assert!(occupant(&sim, own).is_some());
        sim.ticks(3);
        let p = sim.player();
        let lifted = sim.feet(p);
        let surface = ramp_surface_height(-along);
        assert!(
            lifted.y >= surface - 0.02 && lifted.y < surface + 0.2,
            "at {along}: feet {lifted}, slope {surface}"
        );
        assert!(sim.get::<pieced::movement::Motor>(p).grounded);
        // And you can walk on up it.
        sim.set_look(p, Facing::North.yaw(), 0.0);
        sim.player_intent().move_axis = Vec2::Y;
        sim.ticks(12);
        let later = sim.feet(p);
        assert!(
            later.z < lifted.z - 0.5,
            "walks on from {lifted} to {later}"
        );
    }
}

/// Swipes the view by (yaw, pitch) degrees over `ticks` ticks.
fn swipe(sim: &mut Sim, yaw: f32, pitch: f32, ticks: u32) {
    for _ in 0..ticks {
        sim.player_intent().look_delta = Vec2::new(deg(yaw), deg(pitch)) / ticks as f32;
        sim.tick();
    }
}

/// After a box: the builder stands inside `own`, clear of all four walls and
/// not buried in the ramp.
fn assert_boxed_in(sim: &mut Sim, own: GridCell, name: &str) {
    let tuning = BuildTuning::default();
    for f in Facing::ALL {
        assert!(
            occupant(sim, PieceSlot::wall(own, f)).is_some(),
            "{name}: {f:?} wall"
        );
    }
    let p = sim.player();
    let feet = sim.feet(p);
    let min = own.min_corner();
    let clear = 0.35 + tuning.wall_thickness / 2.0 - 0.005;
    assert!(
        feet.x - min.x >= clear
            && min.x + CELL_SIZE - feet.x >= clear
            && feet.z - min.z >= clear
            && min.z + CELL_SIZE - feet.z >= clear,
        "{name}: not clear of the walls at {feet}"
    );
    assert!(
        sim.get::<pieced::movement::Motor>(p).grounded,
        "{name}: grounded"
    );
}

#[test]
fn a_1x1_box_forms_around_you_from_anywhere_in_the_cell() {
    let own = cell(4, 10, 0);
    let spots = [
        ("centre", Vec2::ZERO),
        ("north-east corner", Vec2::new(1.75, -1.75)),
        ("north-west corner", Vec2::new(-1.75, -1.75)),
        ("south-east corner", Vec2::new(1.75, 1.75)),
        ("south-west corner", Vec2::new(-1.75, 1.75)),
    ];
    for (name, offset) in spots {
        // Clicks: wall, swipe 90° right, wall (×3), then a ramp at your feet.
        let mut sim = empty_sim();
        put_player(
            &mut sim,
            center(4, 10) + Vec3::new(offset.x, 0.0, offset.y),
            Facing::North.yaw(),
            0.0,
        );
        sim.ticks(3);
        let start = sim.sim_tick();
        select(&mut sim, PieceKind::Wall);
        press(&mut sim);
        for _ in 0..3 {
            swipe(&mut sim, -90.0, 0.0, 8);
            press(&mut sim);
        }
        select(&mut sim, PieceKind::Ramp);
        swipe(&mut sim, 0.0, -60.0, 8);
        press(&mut sim);
        let elapsed = (sim.sim_tick() - start) as f32 / 60.0;
        assert!(elapsed <= 1.0, "{name}: box took {elapsed} s");
        let map = sim.world().resource::<PieceMap>();
        assert!(map.ramp_at(own).is_some(), "{name}: ramp inside");
        assert_eq!(map.len(), 5, "{name}");
        sim.ticks(5);
        assert_boxed_in(&mut sim, own, name);

        // Turbo: hold the click and sweep a full turn; every edge gets its wall.
        let mut sim = empty_sim();
        put_player(
            &mut sim,
            center(4, 10) + Vec3::new(offset.x, 0.0, offset.y),
            Facing::North.yaw(),
            0.0,
        );
        sim.ticks(3);
        select(&mut sim, PieceKind::Wall);
        sim.player_intent().fire = true;
        sim.player_intent().fire_pressed = true;
        swipe(&mut sim, -360.0, 0.0, 30);
        sim.player_intent().fire = false;
        sim.ticks(5);
        assert_eq!(piece_count(&sim), 4, "{name}: turbo box");
        let p = sim.player();
        let feet = sim.feet(p);
        assert_eq!(
            GridCell::containing(feet),
            own,
            "{name}: inside the turbo box"
        );
    }
}

#[test]
fn nineties_stack_a_ramp_tower_in_your_own_column() {
    // Fortnite's 90s: at the top of each ramp, turn 90° right, wall, look down
    // and jump as you build the next ramp in your own column one level up (it
    // lifts you onto it), climb it, repeat.
    let mut sim = empty_sim();
    let (x, z) = (4, 10);
    put_player(&mut sim, center(x, z), Facing::North.yaw(), 0.0);
    sim.ticks(3);
    let p = sim.player();
    let start = sim.sim_tick();
    let facings = [Facing::North, Facing::East, Facing::South, Facing::West];
    for (level, &f) in facings.iter().enumerate() {
        let level = level as i32;
        select(&mut sim, PieceKind::Wall);
        press(&mut sim);
        assert!(
            occupant(&sim, PieceSlot::wall(cell(x, z, level), f)).is_some(),
            "wall {level}"
        );
        select(&mut sim, PieceKind::Ramp);
        swipe(&mut sim, 0.0, -60.0, 3);
        {
            let mut i = sim.player_intent();
            i.jump = true;
            i.jump_pressed = true;
        }
        press(&mut sim);
        sim.player_intent().jump = false;
        assert!(
            occupant(&sim, PieceSlot::ramp(cell(x, z, level), f)).is_some(),
            "ramp {level}"
        );
        swipe(&mut sim, 0.0, 60.0, 3);
        // Climb to the top.
        {
            let mut i = sim.player_intent();
            i.move_axis = Vec2::Y;
            i.sprint = true;
        }
        let top = (level + 1) as f32 * LEVEL_HEIGHT - 0.15;
        for _ in 0..90 {
            if sim.feet(p).y >= top {
                break;
            }
            sim.tick();
        }
        sim.player_intent().move_axis = Vec2::ZERO;
        let feet = sim.feet(p);
        assert!(feet.y >= top, "climbed ramp {level}: {feet}");
        assert_eq!(
            (GridCell::containing(feet).x, GridCell::containing(feet).z),
            (x, z)
        );
        swipe(&mut sim, -90.0, 0.0, 6);
    }
    let elapsed = (sim.sim_tick() - start) as f32 / 60.0;
    let feet = sim.feet(p);
    println!("90s: 4 levels ({:.1} m) in {elapsed:.2} s", feet.y);
    assert!(elapsed <= 3.5, "four levels took {elapsed} s");
    assert!(feet.y >= 4.0 * LEVEL_HEIGHT - 0.2);
    assert_eq!(piece_count(&sim), 8, "4 ramps and 4 walls, nothing stray");
}

// ---------------------------------------------------------------------------
// M4 chunk 5 (D109): building juice is presentation only
// ---------------------------------------------------------------------------

#[test]
fn an_assembling_piece_blocks_a_shot_and_a_player_on_its_first_tick() {
    use pieced::{
        building::juice::{ASSEMBLE_SECONDS, assemble_pose},
        shared::{ShotFired, TICK_SECONDS},
    };
    let mut sim = empty_sim();
    sim.record::<ShotFired>();
    let feet = center(4, 10);
    put_player(&mut sim, feet, Facing::North.yaw(), 0.0);
    sim.ticks(30);
    // A wall goes up in front of the player: one tick later its model is
    // still assembling...
    let slot = PieceSlot::wall(cell(4, 10, 0), Facing::North);
    let wall = place_piece(sim.world_mut(), slot).unwrap();
    sim.tick();
    const { assert!(TICK_SECONDS < ASSEMBLE_SECONDS) };
    let still = (Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
    assert_ne!(assemble_pose(PieceKind::Wall, TICK_SECONDS), still);
    // ...but it has full HP and stops a shot now.
    let piece = *sim.get::<Piece>(wall);
    assert_eq!(piece.hp, piece.max_hp, "full HP from the first tick");
    sim.clear_recorded::<ShotFired>();
    sim.player_intent().fire_pressed = true;
    sim.tick();
    let shots = sim.recorded::<ShotFired>();
    assert_eq!(shots.len(), 1);
    assert_eq!(shots[0].traces[0].hit, Some(wall), "the shot hits the wall");
    assert!(sim.get::<Piece>(wall).hp < piece.max_hp);

    // A floor's planks slap down; a player walking into a fresh wall stops.
    let mut sim = empty_sim();
    put_player(&mut sim, feet, Facing::North.yaw(), 0.0);
    sim.tick();
    let wall = place_piece(sim.world_mut(), slot).unwrap();
    sim.tick();
    let p = sim.player();
    let start = sim.feet(p);
    let wall_z = slot.center().z;
    for _ in 0..90 {
        sim.player_intent().move_axis = Vec2::new(0.0, 1.0);
        sim.tick();
        let z = sim.feet(p).z;
        assert!(z > wall_z + 0.3, "the wall holds the player back: {z}");
    }
    assert!(start.z - sim.feet(p).z > 1.0, "the player walked up to it");
    assert!(sim.world().get::<Piece>(wall).is_some());
}

/// `app::headless_app` with `extra` plugins added before it is finished.
fn headless_with(seed: u64, extra: impl FnOnce(&mut App)) -> App {
    use bevy::time::TimeUpdateStrategy;
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        bevy::state::app::StatesPlugin,
        AssetPlugin::default(),
        bevy::mesh::MeshPlugin,
        bevy::scene::ScenePlugin,
        avian3d::prelude::PhysicsPlugins::default(),
    ))
    .add_plugins(pieced::app::SimPlugins)
    .insert_resource(TimeUpdateStrategy::ManualDuration(
        pieced::shared::tick_duration(),
    ))
    .insert_resource(pieced::rng::SimRng(pieced::rng::Rng::new(seed)));
    extra(&mut app);
    app.finish();
    app.cleanup();
    app
}
/// A headless sim with the building debris (the client's, meshless).
fn debris_sim() -> Sim {
    let mut app = headless_with(3, |app| {
        app.add_plugins(pieced::building::debris::BuildingDebrisPlugin);
    });
    app.world_mut()
        .resource_mut::<NextState<pieced::shared::AppState>>()
        .set(pieced::shared::AppState::Playing);
    app.update();
    let mut sim = Sim { app };
    clear_pieces(sim.world_mut());
    sim.tick();
    sim
}

#[test]
fn broken_pieces_burst_into_at_most_48_chunks_that_settle_and_go_within_2_s() {
    use pieced::building::debris::{BuildingDebris, DEBRIS_CAP, DEBRIS_SECONDS};
    let mut sim = debris_sim();
    // Ten walls break at once.
    let walls: Vec<Entity> = (0..10)
        .map(|i| {
            let slot = PieceSlot::wall(cell(1 + i % 5, 3 + 2 * (i / 5), 0), Facing::North);
            place_piece(sim.world_mut(), slot).unwrap()
        })
        .collect();
    sim.tick();
    for &w in &walls {
        damage_piece(sim.world_mut(), w, 1.0e6);
    }
    let mut most = 0;
    let mut rested = false;
    let mut last_alive = 0.0;
    for frame in 0..(3.0 * 60.0) as u32 {
        sim.tick();
        let debris = sim.world().resource::<BuildingDebris>();
        most = most.max(debris.live());
        assert!(debris.live() <= DEBRIS_CAP, "{} chunks", debris.live());
        rested |= debris.chunks().any(|(_, resting)| resting);
        if debris.live() > 0 {
            last_alive = frame as f32 / 60.0;
        }
    }
    let debris = sim.world().resource::<BuildingDebris>();
    assert_eq!(debris.bursts, 10);
    assert_eq!(most, DEBRIS_CAP, "the cap fills, the oldest give way");
    assert!(rested, "chunks settle on the ground before they go");
    assert!(last_alive < DEBRIS_SECONDS, "all gone by {last_alive} s");
    assert_eq!(debris.live(), 0);
    assert_eq!(debris.capacity(), DEBRIS_CAP, "a fixed pool");
}
