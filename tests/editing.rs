//! Gate S10 (docs/M2-SPEC.md, Amendment A): Fortnite-style editing (D44) and
//! the cone, driven through `PlayerIntent` in the headless simulation.
//!
//! - Every valid edit shape of every piece kind changes collision exactly as its
//!   shape says (rays through each tile), and a reset restores the full piece.
//! - You walk through a door and an arch, shoot through a window, and stand on
//!   a half floor; a window still blocks you.
//! - Invalid selections are refused ("bwomp") and change nothing.
//! - An edit keeps the piece's HP fraction; the edited piece keeps its slot.
//! - The cone places, shares its cell with a floor and a ramp, blocks movement
//!   and shots, cracks and breaks.

use avian3d::prelude::*;
use bevy::{ecs::system::RunSystemOnce, prelude::*};
use pieced::{
    building::{
        BuildTarget, CONE_HEIGHT, EditMode, EditShape, EditTarget, Piece, PieceEdit, PieceMap,
        PieceSlot, Placement, check_placement, clear_pieces, damage_piece,
        edit::{self, shape_of, tile_count, tile_quad, valid_edits},
        edit_piece, place_piece,
    },
    shared::{
        ActiveTool, EyeHeight, Facing, GameCue, GridCell, LEVEL_HEIGHT, Layer, PieceChange,
        PieceChanged, PieceKind, PreviousFeet, ShotFired,
    },
    sim::Sim,
};

fn cell(x: i32, z: i32, level: i32) -> GridCell {
    GridCell::new(x, z, level)
}

fn center(x: i32, z: i32) -> Vec3 {
    cell(x, z, 0).base_center()
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

fn eye(sim: &mut Sim) -> Vec3 {
    let p = sim.player();
    sim.feet(p) + Vec3::Y * sim.get::<EyeHeight>(p).0
}

/// Turns the player's view onto a world point.
fn aim_at(sim: &mut Sim, point: Vec3) {
    let d = (point - eye(sim)).normalize();
    let p = sim.player();
    sim.set_look(p, (-d.x).atan2(-d.z), d.y.asin());
}

/// The world-space centre of an edit tile (on the piece's edit surface),
/// nudged by `offset` in the tile's plane (x, then y or z).
fn tile_point(slot: &PieceSlot, tile: u8) -> Vec3 {
    let q = tile_quad(slot.kind, tile);
    let local = q.iter().copied().sum::<Vec3>() / 4.0;
    slot.transform().transform_point(local)
}

fn cast(sim: &mut Sim, origin: Vec3, direction: Vec3, max: f32) -> Option<(Entity, f32)> {
    let direction = Dir3::new(direction).unwrap();
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

fn editing(sim: &mut Sim) -> Option<Entity> {
    let p = sim.player();
    sim.get::<EditMode>(p).session.map(|s| s.piece)
}

fn cues(sim: &Sim, f: impl Fn(&GameCue) -> bool) -> usize {
    sim.recorded::<GameCue>().iter().filter(|c| f(c)).count()
}

fn press_edit(sim: &mut Sim) {
    sim.player_intent().edit_pressed = true;
    sim.tick();
}

/// Press on the current tile, drag across `then` (aiming at each for a tick),
/// release.
fn drag(sim: &mut Sim, slot: &PieceSlot, tiles: &[u8]) {
    aim_at(sim, tile_point(slot, tiles[0]));
    {
        let mut i = sim.player_intent();
        i.fire = true;
        i.fire_pressed = true;
    }
    sim.tick();
    for &t in &tiles[1..] {
        aim_at(sim, tile_point(slot, t));
        sim.tick();
    }
    sim.player_intent().fire = false;
    sim.tick();
}

fn edit_of(sim: &Sim, piece: Entity) -> PieceEdit {
    *sim.get::<PieceEdit>(piece)
}

// ---------------------------------------------------------------------------
// Collision follows every shape
// ---------------------------------------------------------------------------

/// Sample points (piece-local) across a tile, and whether a ray through each
/// should hit the piece under `edit`. Points within 8 cm of a triangle's cut
/// are skipped.
fn samples(kind: PieceKind, edit: PieceEdit) -> Vec<(Vec3, bool)> {
    let mut out = Vec::new();
    for tile in 0..tile_count(kind) {
        let q = tile_quad(kind, tile);
        let c = q.iter().copied().sum::<Vec3>() / 4.0;
        let (du, dv) = match kind {
            PieceKind::Wall => (Vec3::X * 0.35, Vec3::Y * 0.25),
            _ => (Vec3::X * 0.5, Vec3::Z * 0.5),
        };
        for (su, sv) in [(0.0, 0.0), (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let p = c + du * su + dv * sv;
            let expect = match kind {
                PieceKind::Wall => match edit::wall_triangle(edit) {
                    Some(tri) => {
                        let q = p.truncate();
                        let side = |a: Vec2, b: Vec2| (b - a).perp_dot(q - a);
                        let s = [
                            side(tri[0], tri[1]),
                            side(tri[1], tri[2]),
                            side(tri[2], tri[0]),
                        ];
                        let inside = s.iter().all(|v| *v >= 0.0) || s.iter().all(|v| *v <= 0.0);
                        // Distance to the hypotenuse (the edge that isn't axis aligned).
                        let near_cut = [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])]
                            .iter()
                            .filter(|(a, b)| a.x != b.x && a.y != b.y)
                            .any(|(a, b)| side(*a, *b).abs() / (*b - *a).length() < 0.08);
                        if near_cut {
                            continue;
                        }
                        inside
                    }
                    None => !edit.has(tile),
                },
                PieceKind::Floor => !edit.has(tile),
                // Ramps keep their path; cones keep every quarter (checked by height).
                PieceKind::Ramp => !edit.is_edited() || edit.has(tile),
                PieceKind::Cone => true,
            };
            out.push((p, expect));
        }
    }
    out
}

/// Casts a ray through a piece-local sample point along the piece's edit
/// normal (walls: from the owner's side; others: from above). Returns the hit
/// height above the piece's base, if it hit this piece.
fn probe(sim: &mut Sim, slot: &PieceSlot, piece: Entity, local: Vec3) -> Option<f32> {
    let xf = slot.transform();
    let (from, dir) = match slot.kind {
        PieceKind::Wall => (local + Vec3::Z * 2.0, Vec3::NEG_Z),
        _ => (Vec3::new(local.x, 5.0, local.z), Vec3::NEG_Y),
    };
    let origin = xf.transform_point(from);
    let dir = xf.rotation * dir;
    let (hit, d) = cast(sim, origin, dir, 8.0)?;
    (hit == piece).then(|| (origin + dir * d).y - slot.cell.level as f32 * LEVEL_HEIGHT)
}

fn slot_for(kind: PieceKind) -> PieceSlot {
    let c = cell(5, 5, 0);
    match kind {
        PieceKind::Wall => PieceSlot::wall(c, Facing::East),
        PieceKind::Floor => PieceSlot::floor(cell(5, 5, 1)),
        PieceKind::Ramp => PieceSlot::ramp(c, Facing::South),
        PieceKind::Cone => PieceSlot::cone(c),
    }
}

#[test]
fn every_valid_edit_changes_collision_as_its_shape_and_reset_restores_it() {
    let mut sim = empty_sim();
    // Keep the player well away from the probes.
    put_player(&mut sim, center(1, 10), 0.0, 0.0);
    let mut checked = 0;
    for kind in [
        PieceKind::Wall,
        PieceKind::Floor,
        PieceKind::Ramp,
        PieceKind::Cone,
    ] {
        // One piece at a time, so no probe meets another.
        clear_pieces(sim.world_mut());
        let slot = slot_for(kind);
        let piece = place_piece(sim.world_mut(), slot).unwrap();
        sim.tick();
        let full: Vec<Option<f32>> = samples(kind, PieceEdit::FULL)
            .iter()
            .map(|(p, _)| probe(&mut sim, &slot, piece, *p))
            .collect();
        assert!(full.iter().all(Option::is_some), "{kind:?}: the full piece");
        for e in valid_edits(kind) {
            let shape = shape_of(kind, e).unwrap();
            assert!(edit_piece(sim.world_mut(), piece, e), "{kind:?} {e:?}");
            sim.tick();
            for (p, expect) in samples(kind, e) {
                let hit = probe(&mut sim, &slot, piece, p);
                assert_eq!(
                    hit.is_some(),
                    expect,
                    "{kind:?} {shape:?} {e:?}: a ray through {p} (local)"
                );
                // Heights: a half ramp rises along its path; a cone's raised
                // corners stand above the pyramid, the others at or below it.
                if let Some(h) = hit {
                    match kind {
                        PieceKind::Ramp => {
                            let pts = edit::half_ramp_points(e).unwrap();
                            let top = (pts[4].xz() + pts[5].xz()) / 2.0;
                            let mid = pts[..4].iter().map(|q| q.xz()).sum::<Vec2>() / 4.0;
                            let rise = (top - mid).normalize();
                            let s1 = top.dot(rise);
                            let expected = (LEVEL_HEIGHT * (p.xz().dot(rise) - (s1 - CELL)) / CELL)
                                .clamp(0.0, LEVEL_HEIGHT);
                            assert!(
                                (h - expected).abs() < 0.1,
                                "{e:?} at {p}: {h}, expected {expected}"
                            );
                        }
                        PieceKind::Cone => {
                            let tile = (p.x >= 0.0) as u8 + 2 * (p.z >= 0.0) as u8;
                            let at_center = (p.x.abs() - 1.0).abs() < 0.01
                                && (p.z.abs() - 1.0).abs() < 0.01;
                            if at_center {
                                if e.has(tile) {
                                    assert!(h > 0.95, "{shape:?} {e:?} raised {tile}: {h}");
                                } else {
                                    assert!(h < 0.77, "{shape:?} {e:?} low {tile}: {h}");
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            // The piece map knows the edit; reset restores every ray.
            let entry = sim
                .world()
                .resource::<PieceMap>()
                .get(slot.key())
                .unwrap();
            assert_eq!((entry.entity, entry.edit), (piece, e));
            assert!(edit_piece(sim.world_mut(), piece, PieceEdit::FULL));
            sim.tick();
            let again: Vec<Option<f32>> = samples(kind, PieceEdit::FULL)
                .iter()
                .map(|(p, _)| probe(&mut sim, &slot, piece, *p))
                .collect();
            for (a, b) in again.iter().zip(&full) {
                assert!(
                    (a.unwrap() - b.unwrap()).abs() < 1e-3,
                    "{kind:?} reset after {shape:?}"
                );
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 22 + 14 + 8 + 14);
}

const CELL: f32 = pieced::shared::CELL_SIZE;

#[test]
fn half_ramps_rise_one_level_along_their_path() {
    let mut sim = empty_sim();
    put_player(&mut sim, center(1, 10), 0.0, 0.0);
    let slot = PieceSlot::ramp(cell(5, 5, 0), Facing::North);
    let ramp = place_piece(sim.world_mut(), slot).unwrap();
    // The left half (walking up), rising the way the ramp does: tiles 2 -> 0.
    assert!(edit_piece(sim.world_mut(), ramp, PieceEdit::path(2, 0)));
    sim.tick();
    let h = |sim: &mut Sim, x: f32, z: f32| probe(sim, &slot, ramp, Vec3::new(x, 0.0, z));
    let (foot, top) = (h(&mut sim, -1.0, 1.8).unwrap(), h(&mut sim, -1.0, -1.8).unwrap());
    assert!(foot < 0.3 && top > 2.7, "{foot} .. {top}");
    assert_eq!(h(&mut sim, 1.0, 0.0), None, "the right half is gone");
    // Turned: the low half (tiles 3 -> 2) rises toward local -X.
    assert!(edit_piece(sim.world_mut(), ramp, PieceEdit::path(3, 2)));
    sim.tick();
    let (foot, top) = (h(&mut sim, 1.8, 1.0).unwrap(), h(&mut sim, -1.8, 1.0).unwrap());
    assert!(foot < 0.3 && top > 2.7, "{foot} .. {top}");
    assert_eq!(h(&mut sim, 0.0, -1.0), None, "the high half is gone");
}

// ---------------------------------------------------------------------------
// Walking, shooting and standing on edits
// ---------------------------------------------------------------------------

/// Walks the player north for `seconds` from the centre of cell (4, 10) and
/// returns how far north of the wall line (the cell's north edge) it got.
fn walk_north(sim: &mut Sim, seconds: f32) -> f32 {
    put_player(sim, center(4, 10), Facing::North.yaw(), 0.0);
    sim.ticks(5);
    sim.player_intent().move_axis = Vec2::Y;
    sim.run_seconds(seconds);
    sim.player_intent().move_axis = Vec2::ZERO;
    let p = sim.player();
    let edge = cell(4, 10, 0).min_corner().z;
    edge - sim.feet(p).z
}

#[test]
fn you_walk_through_a_door_and_an_arch_but_not_a_window() {
    let mut sim = empty_sim();
    let slot = PieceSlot::wall(cell(4, 10, 0), Facing::North);
    let wall = place_piece(sim.world_mut(), slot).unwrap();
    sim.tick();
    assert!(walk_north(&mut sim, 1.5) < 0.0, "the full wall blocks");
    for (tiles, passes) in [
        (&[4, 7][..], true),
        (&[4, 6, 7, 8][..], true),
        (&[4][..], false),
        (&[3, 4, 5][..], false),
    ] {
        assert!(edit_piece(sim.world_mut(), wall, PieceEdit::of(tiles)));
        sim.tick();
        let past = walk_north(&mut sim, 1.5);
        assert_eq!(past > 1.0, passes, "{tiles:?}: {past} m past the wall");
    }
}

#[test]
fn you_shoot_through_a_window() {
    let mut sim = empty_sim();
    sim.record::<ShotFired>();
    let slot = PieceSlot::wall(cell(4, 10, 0), Facing::North);
    let wall = place_piece(sim.world_mut(), slot).unwrap();
    put_player(&mut sim, center(4, 10), Facing::North.yaw(), 0.0);
    sim.ticks(40);
    let window = tile_point(&slot, 4);
    let shoot = |sim: &mut Sim| -> Option<Entity> {
        sim.clear_recorded::<ShotFired>();
        aim_at(sim, window);
        sim.player_intent().fire_pressed = true;
        sim.tick();
        sim.ticks(20);
        let shots = sim.recorded::<ShotFired>();
        assert_eq!(shots.len(), 1, "one shot");
        shots[0].traces[0].hit
    };
    assert_eq!(shoot(&mut sim), Some(wall), "the full wall stops the shot");
    assert!(edit_piece(sim.world_mut(), wall, PieceEdit::of(&[4])));
    sim.tick();
    assert_ne!(shoot(&mut sim), Some(wall), "the shot goes through the window");
    // Beside the window the wall still stops shots.
    sim.clear_recorded::<ShotFired>();
    aim_at(&mut sim, tile_point(&slot, 3));
    sim.player_intent().fire_pressed = true;
    sim.tick();
    assert_eq!(sim.recorded::<ShotFired>()[0].traces[0].hit, Some(wall));
}

#[test]
fn you_stand_on_a_half_floor_and_fall_through_its_gap() {
    let mut sim = empty_sim();
    let slot = PieceSlot::floor(cell(4, 10, 1));
    let floor = place_piece(sim.world_mut(), slot).unwrap();
    // Remove the north half (tiles 0 and 1).
    assert!(edit_piece(sim.world_mut(), floor, PieceEdit::of(&[0, 1])));
    sim.tick();
    let c = center(4, 10);
    let p = sim.player();
    put_player(&mut sim, c + Vec3::new(0.0, 3.6, 1.0), 0.0, 0.0);
    sim.ticks(60);
    let y = sim.feet(p).y;
    assert!((y - 3.1).abs() < 0.1, "standing on the kept half: {y}");
    put_player(&mut sim, c + Vec3::new(0.0, 3.6, -1.0), 0.0, 0.0);
    sim.ticks(90);
    let y = sim.feet(p).y;
    assert!(y < 0.2, "fell through the removed half: {y}");
}

// ---------------------------------------------------------------------------
// The edit flow: G, click or drag, release; R resets
// ---------------------------------------------------------------------------

fn wall_in_front(sim: &mut Sim) -> (PieceSlot, Entity) {
    let slot = PieceSlot::wall(cell(4, 10, 0), Facing::North);
    let wall = place_piece(sim.world_mut(), slot).unwrap();
    put_player(sim, center(4, 10), Facing::North.yaw(), 0.0);
    sim.ticks(3);
    (slot, wall)
}

#[test]
fn g_drag_release_makes_a_door_at_once_and_leaves_edit_mode() {
    let mut sim = empty_sim();
    sim.record::<GameCue>();
    sim.record::<ShotFired>();
    let (slot, wall) = wall_in_front(&mut sim);
    aim_at(&mut sim, tile_point(&slot, 4));
    press_edit(&mut sim);
    assert_eq!(editing(&mut sim), Some(wall), "G on the wall opens its grid");
    let p = sim.player();
    assert_eq!(sim.get::<EditMode>(p).session.unwrap().hovered, Some(4));
    // Press on 4, drag down to 7: selected but not applied while held.
    aim_at(&mut sim, tile_point(&slot, 4));
    {
        let mut i = sim.player_intent();
        i.fire = true;
        i.fire_pressed = true;
    }
    sim.tick();
    aim_at(&mut sim, tile_point(&slot, 7));
    sim.tick();
    let session = sim.get::<EditMode>(p).session.unwrap();
    assert_eq!(session.selection(), PieceEdit::of(&[4, 7]));
    assert!(session.selection_valid());
    assert_eq!(edit_of(&sim, wall), PieceEdit::FULL, "nothing yet");
    // Release: the door is there on this very tick.
    sim.player_intent().fire = false;
    sim.tick();
    assert_eq!(edit_of(&sim, wall), PieceEdit::of(&[4, 7]));
    assert_eq!(
        shape_of(PieceKind::Wall, edit_of(&sim, wall)),
        Some(EditShape::Door)
    );
    assert_eq!(editing(&mut sim), None, "release confirms and leaves");
    assert_eq!(
        cues(&sim, |c| matches!(c, GameCue::PieceEdited { piece, .. } if *piece == wall)),
        1
    );
    // Clicking tiles never fired the rifle.
    assert!(sim.recorded::<ShotFired>().is_empty(), "no shots while editing");
    // The collider already has the doorway.
    sim.tick();
    let door = tile_point(&slot, 7);
    assert_eq!(cast(&mut sim, door + Vec3::Z * 2.0, Vec3::NEG_Z, 4.0), None);
}

#[test]
fn a_single_click_makes_a_window_and_g_or_a_tool_leaves() {
    let mut sim = empty_sim();
    let (slot, wall) = wall_in_front(&mut sim);
    aim_at(&mut sim, tile_point(&slot, 4));
    press_edit(&mut sim);
    drag(&mut sim, &slot, &[4]);
    assert_eq!(edit_of(&sim, wall), PieceEdit::of(&[4]));
    // G opens and G closes without changing anything.
    press_edit(&mut sim);
    assert_eq!(editing(&mut sim), Some(wall));
    press_edit(&mut sim);
    assert_eq!(editing(&mut sim), None);
    // Picking a tool leaves edit mode too.
    press_edit(&mut sim);
    sim.player_intent().select = Some(ActiveTool::Build(PieceKind::Ramp));
    sim.tick();
    assert_eq!(editing(&mut sim), None);
    assert_eq!(edit_of(&sim, wall), PieceEdit::of(&[4]));
}

#[test]
fn invalid_selections_are_refused_with_a_bwomp() {
    let mut sim = empty_sim();
    sim.record::<GameCue>();
    let (slot, wall) = wall_in_front(&mut sim);
    let bwomps = |sim: &Sim| cues(sim, |c| matches!(c, GameCue::EditRejected { .. }));
    aim_at(&mut sim, tile_point(&slot, 0));
    press_edit(&mut sim);
    // A diagonal: 0, 4, 8.
    drag(&mut sim, &slot, &[0, 4, 8]);
    assert_eq!(bwomps(&sim), 1);
    assert_eq!(edit_of(&sim, wall), PieceEdit::FULL, "nothing changed");
    let p = sim.player();
    let session = sim.get::<EditMode>(p).session.expect("the grid stays open");
    assert_eq!(session.drag, None);
    assert_eq!(session.selection(), PieceEdit::FULL);
    // The collider is untouched.
    let centre = tile_point(&slot, 4);
    assert_eq!(
        cast(&mut sim, centre + Vec3::Z * 2.0, Vec3::NEG_Z, 4.0).map(|h| h.0),
        Some(wall)
    );
    // A lone bottom tile isn't a shape either.
    drag(&mut sim, &slot, &[7]);
    assert_eq!(bwomps(&sim), 2);
    assert_eq!(edit_of(&sim, wall), PieceEdit::FULL);
    // Then a valid one still works.
    drag(&mut sim, &slot, &[1]);
    assert_eq!(edit_of(&sim, wall), PieceEdit::of(&[1]));

    // A ramp: one tile isn't a stair.
    clear_pieces(sim.world_mut());
    let ramp_slot = PieceSlot::ramp(cell(4, 9, 0), Facing::North);
    let ramp = place_piece(sim.world_mut(), ramp_slot).unwrap();
    put_player(&mut sim, center(4, 10), Facing::North.yaw(), 0.0);
    sim.ticks(3);
    aim_at(&mut sim, tile_point(&ramp_slot, 2));
    press_edit(&mut sim);
    assert_eq!(editing(&mut sim), Some(ramp));
    drag(&mut sim, &ramp_slot, &[2]);
    assert_eq!(bwomps(&sim), 3);
    // A two-tile path up its left side: a half ramp.
    drag(&mut sim, &ramp_slot, &[2, 0]);
    assert_eq!(edit_of(&sim, ramp), PieceEdit::path(2, 0));
}

#[test]
fn r_resets_in_edit_mode_and_on_an_edited_piece_without_entering_it() {
    let mut sim = empty_sim();
    let (slot, wall) = wall_in_front(&mut sim);
    let hp_before = sim.get::<Piece>(wall).hp;
    assert!(edit_piece(sim.world_mut(), wall, PieceEdit::of(&[4, 7])));
    sim.ticks(2);
    // Out of edit mode: aim through the doorway itself and reset.
    aim_at(&mut sim, tile_point(&slot, 7));
    sim.tick();
    let p = sim.player();
    let target = sim.get::<EditTarget>(p).0.expect("the door is targeted");
    assert!(target.entity == wall && target.edited);
    sim.player_intent().reset_pressed = true;
    sim.tick();
    assert_eq!(edit_of(&sim, wall), PieceEdit::FULL);
    assert_eq!(editing(&mut sim), None);
    // In edit mode: R resets and leaves.
    assert!(edit_piece(sim.world_mut(), wall, PieceEdit::of(&[0, 1, 2])));
    aim_at(&mut sim, tile_point(&slot, 4));
    press_edit(&mut sim);
    assert_eq!(editing(&mut sim), Some(wall));
    sim.player_intent().reset_pressed = true;
    sim.tick();
    assert_eq!(edit_of(&sim, wall), PieceEdit::FULL);
    assert_eq!(editing(&mut sim), None);
    sim.tick();
    let top = tile_point(&slot, 1);
    assert_eq!(
        cast(&mut sim, top + Vec3::Z * 2.0, Vec3::NEG_Z, 4.0).map(|h| h.0),
        Some(wall),
        "the full wall is back"
    );
    assert_eq!(sim.get::<Piece>(wall).hp, hp_before);
    // R on an unedited piece does nothing (the adapter sends a reload then).
    sim.player_intent().reset_pressed = true;
    sim.tick();
    assert_eq!(edit_of(&sim, wall), PieceEdit::FULL);
}

#[test]
fn an_edit_keeps_the_hp_fraction_and_the_slot() {
    let mut sim = empty_sim();
    let (slot, wall) = wall_in_front(&mut sim);
    damage_piece(sim.world_mut(), wall, 80.0);
    sim.tick();
    let before = *sim.get::<Piece>(wall);
    assert_eq!((before.hp, before.crack_stage), (120.0, 1));
    aim_at(&mut sim, tile_point(&slot, 4));
    press_edit(&mut sim);
    drag(&mut sim, &slot, &[4, 7]);
    let after = *sim.get::<Piece>(wall);
    assert_eq!(edit_of(&sim, wall), PieceEdit::of(&[4, 7]));
    assert_eq!(after.hp_fraction(), before.hp_fraction());
    assert_eq!((after.hp, after.max_hp, after.crack_stage), (120.0, 200.0, 1));
    // An edited piece still owns its slot: nothing can be placed there.
    let map = sim.world().resource::<PieceMap>().clone();
    assert_eq!(
        check_placement(&slot, &map, sim.sim_tick(), &[], &Default::default()),
        Placement::Occupied
    );
    sim.player_intent().select = Some(ActiveTool::Build(PieceKind::Wall));
    sim.tick();
    let p = sim.player();
    put_player(&mut sim, center(4, 10), Facing::North.yaw(), 0.0);
    sim.tick();
    let candidate = sim.get::<BuildTarget>(p).candidate.unwrap();
    assert_eq!(candidate.slot, slot);
    assert_eq!(candidate.placement, Placement::Occupied);
    // It still takes damage and breaks like any piece.
    damage_piece(sim.world_mut(), wall, 500.0);
    sim.ticks(2);
    assert!(sim.world().get_entity(wall).is_err());
    assert!(sim.world().resource::<PieceMap>().get(slot.key()).is_none());
}

#[test]
fn edit_mode_needs_a_piece_in_reach() {
    let mut sim = empty_sim();
    let far = PieceSlot::wall(cell(4, 7, 0), Facing::North);
    place_piece(sim.world_mut(), far).unwrap();
    put_player(&mut sim, center(4, 10), Facing::North.yaw(), 0.0);
    sim.ticks(2);
    aim_at(&mut sim, tile_point(&far, 4));
    press_edit(&mut sim);
    assert_eq!(editing(&mut sim), None, "about 10 m away: out of reach");
    let p = sim.player();
    assert_eq!(sim.get::<EditTarget>(p).0, None);
}

// ---------------------------------------------------------------------------
// The cone
// ---------------------------------------------------------------------------

#[test]
fn the_cone_places_on_v_and_shares_a_cell_with_a_floor_and_a_ramp() {
    let mut sim = empty_sim();
    sim.record::<PieceChanged>();
    put_player(&mut sim, center(4, 10), Facing::North.yaw(), 0.0);
    sim.player_intent().select = Some(ActiveTool::Build(PieceKind::Cone));
    sim.tick();
    let p = sim.player();
    assert_eq!(*sim.get::<ActiveTool>(p), ActiveTool::Build(PieceKind::Cone));
    let candidate = sim.get::<BuildTarget>(p).candidate.expect("a cone ghost");
    assert_eq!(candidate.slot.kind, PieceKind::Cone);
    sim.player_intent().fire_pressed = true;
    sim.tick();
    let placed = sim
        .world()
        .resource::<PieceMap>()
        .cone_at(candidate.slot.cell)
        .expect("placed");
    let piece = *sim.get::<Piece>(placed);
    assert_eq!((piece.kind, piece.hp, piece.max_hp), (PieceKind::Cone, 170.0, 170.0));
    assert!(sim.recorded::<PieceChanged>().iter().any(|c| c.change
        == PieceChange::Placed
        && c.kind == PieceKind::Cone));
    // A floor, a ramp and a cone in one cell.
    let c = cell(7, 7, 1);
    for slot in [
        PieceSlot::floor(c),
        PieceSlot::ramp(c, Facing::East),
        PieceSlot::cone(c),
    ] {
        place_piece(sim.world_mut(), slot).unwrap_or_else(|e| panic!("{slot:?}: {e:?}"));
    }
    assert!(matches!(
        place_piece(sim.world_mut(), PieceSlot::cone(c)),
        Err(Placement::Occupied)
    ));
}

#[test]
fn the_cone_caps_a_box_from_inside() {
    // Looking steeply up inside a box puts the cone on top of it.
    let mut sim = empty_sim();
    put_player(&mut sim, center(4, 10), Facing::North.yaw(), 70f32.to_radians());
    sim.player_intent().select = Some(ActiveTool::Build(PieceKind::Cone));
    sim.tick();
    let p = sim.player();
    let candidate = sim.get::<BuildTarget>(p).candidate.unwrap();
    assert_eq!(candidate.slot, PieceSlot::cone(cell(4, 10, 1)));
}

#[test]
fn the_cone_blocks_shots_and_movement() {
    let mut sim = empty_sim();
    let slot = PieceSlot::cone(cell(4, 9, 0));
    let cone = place_piece(sim.world_mut(), slot).unwrap();
    sim.tick();
    let base = slot.cell.base_center();
    // Low shots hit it, shots over its peak don't; from above it's there too.
    let from = base + Vec3::new(0.0, 0.5, 4.0);
    assert_eq!(cast(&mut sim, from, Vec3::NEG_Z, 8.0).map(|h| h.0), Some(cone));
    let over = base + Vec3::new(0.0, CONE_HEIGHT + 0.1, 4.0);
    assert_eq!(cast(&mut sim, over, Vec3::NEG_Z, 8.0), None);
    let (hit, d) = cast(&mut sim, base + Vec3::Y * 4.0, Vec3::NEG_Y, 8.0).unwrap();
    assert_eq!(hit, cone);
    assert!((4.0 - d - CONE_HEIGHT).abs() < 0.02, "the peak is 1.5 m up");
    // Walking into it from the south: it's solid, so you end up on its slope.
    let p = sim.player();
    put_player(&mut sim, center(4, 10), Facing::North.yaw(), 0.0);
    sim.ticks(5);
    sim.player_intent().move_axis = Vec2::Y;
    let mut highest = 0.0f32;
    for _ in 0..50 {
        sim.tick();
        let f = sim.feet(p);
        if (f.z - base.z).abs() < 1.5 {
            highest = highest.max(f.y);
        }
    }
    assert!(highest > 0.4, "walked up onto the cone: {highest}");
    // Standing on it: dropped onto the side, you rest on its surface.
    sim.player_intent().move_axis = Vec2::ZERO;
    put_player(&mut sim, base + Vec3::new(1.0, 3.0, 0.0), 0.0, 0.0);
    sim.ticks(2);
    let mut lowest = f32::MAX;
    for _ in 0..20 {
        sim.tick();
        lowest = lowest.min(sim.feet(p).y);
    }
    assert!(lowest > 0.2, "held up by the cone: {lowest}");
}

#[test]
fn the_cone_cracks_and_breaks() {
    let mut sim = empty_sim();
    sim.record::<PieceChanged>();
    let slot = PieceSlot::cone(cell(4, 9, 0));
    let cone = place_piece(sim.world_mut(), slot).unwrap();
    damage_piece(sim.world_mut(), cone, 60.0);
    sim.tick();
    assert_eq!(sim.get::<Piece>(cone).crack_stage, 1, "110/170 = 65%");
    damage_piece(sim.world_mut(), cone, 55.0);
    sim.tick();
    assert_eq!(sim.get::<Piece>(cone).crack_stage, 2, "55/170 = 32%");
    damage_piece(sim.world_mut(), cone, 60.0);
    sim.ticks(2);
    assert!(sim.world().get_entity(cone).is_err());
    let changes: Vec<PieceChange> = sim
        .recorded::<PieceChanged>()
        .iter()
        .filter(|c| c.kind == PieceKind::Cone)
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
    let base = slot.cell.base_center();
    assert_eq!(
        cast(&mut sim, base + Vec3::new(0.0, 0.5, 4.0), Vec3::NEG_Z, 8.0),
        None,
        "its collision is gone"
    );
    // The spot is briefly locked, like any piece.
    assert!(matches!(
        place_piece(sim.world_mut(), slot),
        Err(Placement::RebuildLocked)
    ));
}

#[test]
fn an_edited_cone_opens_where_its_corners_rise() {
    // A roof-ramp slope on a cone: the raised side is high enough to walk
    // under at its edge, the low side stays low.
    let mut sim = empty_sim();
    put_player(&mut sim, center(1, 10), 0.0, 0.0);
    let slot = PieceSlot::cone(cell(4, 9, 0));
    let cone = place_piece(sim.world_mut(), slot).unwrap();
    let edit = PieceEdit::of(&[0, 1]);
    assert_eq!(shape_of(PieceKind::Cone, edit), Some(EditShape::ConeSlope));
    assert!(edit_piece(sim.world_mut(), cone, edit));
    sim.tick();
    let top = |sim: &mut Sim, z: f32| probe(sim, &slot, cone, Vec3::new(0.0, 0.0, z)).unwrap();
    let (high, low) = (top(&mut sim, -1.9), top(&mut sim, 1.9));
    assert!(high > 1.4 && low < 0.1, "{high} .. {low}");
    // Every raised set of 4 is refused; any 1 to 3 is a cone variant.
    assert!(!edit_piece(sim.world_mut(), cone, PieceEdit::of(&[0, 1, 2, 3])));
    assert_eq!(edit_of(&sim, cone), edit);
}
