//! The solid arena props (docs/M2-SPEC.md → Scope rule, D29), through the
//! simulation seam: rocks and stumps are static World-layer colliders at fixed
//! spots that block movement and shots, can be jumped onto (stumps), are
//! ignored by building, and keep clear of the spawns, the initial cover and
//! the dummy's strafing.

use avian3d::{collision::collider::contact_query::intersection_test, prelude::*};
use bevy::prelude::*;
use pieced::{
    arena::{
        ARENA_PROPS, ArenaLayout, ArenaProp, PROP_CLEARANCE, PropKind, PropPlacement,
        in_dummy_strafe_zone,
    },
    building::{Piece, PieceSlot, initial_cover},
    dummy::{Dummy, look_toward},
    models::{EMBEDDED_MODELS, Sidecar},
    movement::Motor,
    shared::{
        ARENA_HALF, ActiveTool, DamageDealt, EyeHeight, Facing, GridCell, Layer, PieceKind,
        PlayerIntent, ShotFired,
    },
    sim::Sim,
};

/// The collision capsule's radius (movement tuning).
const RADIUS: f32 = 0.35;

fn prop(kind: PropKind) -> PropPlacement {
    *ARENA_PROPS
        .iter()
        .find(|p| p.kind == kind)
        .expect("a prop of that kind")
}

fn prop_entity(sim: &mut Sim, placement: PropPlacement) -> Entity {
    sim.world_mut()
        .query::<(Entity, &ArenaProp)>()
        .iter(sim.world())
        .find(|(_, p)| p.0 == placement)
        .map(|(e, _)| e)
        .expect("the prop's collider")
}

fn dummy(sim: &mut Sim) -> Entity {
    sim.world_mut()
        .query_filtered::<Entity, With<Dummy>>()
        .single(sim.world())
        .expect("one dummy")
}

fn put(sim: &mut Sim, who: Entity, feet: Vec3, facing: Facing) {
    sim.world_mut().get_mut::<Transform>(who).unwrap().translation = feet;
    sim.set_look(who, facing.yaw(), 0.0);
    *sim.intent(who) = PlayerIntent::default();
    sim.ticks(10);
}

/// Distance between the XZ footprint circle of a prop and a box.
fn gap_to_box(prop: &PropPlacement, min: Vec3, max: Vec3) -> f32 {
    let c = prop.position.xz();
    let q = c.clamp(min.xz(), max.xz());
    c.distance(q) - prop.kind.footprint_radius()
}

#[test]
fn six_to_eight_props_are_static_world_colliders() {
    let mut sim = Sim::new();
    assert!((6..=8).contains(&ARENA_PROPS.len()));
    assert!(ARENA_PROPS.iter().filter(|p| p.kind.is_rock()).count() >= 3);
    assert!(ARENA_PROPS.iter().any(|p| p.kind == PropKind::StumpA));
    let props: Vec<(RigidBody, CollisionLayers, Transform, ArenaProp)> = sim
        .world_mut()
        .query::<(&RigidBody, &CollisionLayers, &Transform, &ArenaProp)>()
        .iter(sim.world())
        .map(|(b, l, t, p)| (*b, *l, *t, *p))
        .collect();
    assert_eq!(props.len(), ARENA_PROPS.len());
    for (body, layers, transform, prop) in props {
        assert_eq!(body, RigidBody::Static);
        assert_eq!(layers, CollisionLayers::new(Layer::World, LayerMask::ALL));
        assert_eq!(transform, prop.0.transform(), "at its listed spot");
        // Rocks are crouch cover (1.0–1.4 m), stumps jumpable (≤ 0.7 m).
        let h = prop.0.kind.height();
        if prop.0.kind.is_rock() {
            assert!((1.0..=1.4).contains(&h));
        } else {
            assert!(h <= 0.7);
        }
    }
}

#[test]
fn spawns_cover_and_the_dummys_strafe_zone_keep_clear_of_props() {
    let layout = ArenaLayout::default();
    let tuning = pieced::tuning::Tuning::default().building;
    for p in &ARENA_PROPS {
        for spawn in [layout.player_spawn, layout.dummy_spawn] {
            let gap = p.footprint_distance(spawn.xz());
            assert!(gap >= PROP_CLEARANCE, "{p:?} is {gap:.2} m from a spawn");
        }
        for slot in initial_cover() {
            let (min, max) = slot.aabb(&tuning);
            let gap = gap_to_box(p, min, max);
            assert!(gap >= PROP_CLEARANCE, "{p:?} is {gap:.2} m from cover {slot:?}");
        }
        // No part of it (nor its bounding circle) is where the dummy strafes.
        let r = p.kind.footprint_radius();
        let ring = (0..24).map(|k| {
            let a = std::f32::consts::TAU * k as f32 / 24.0;
            p.position.xz() + Vec2::new(a.cos(), a.sin()) * r
        });
        for q in p.world_points().iter().map(|v| v.xz()).chain(ring) {
            assert!(
                !in_dummy_strafe_zone(&layout, q),
                "{p:?} reaches into the dummy's strafe zone at {q}"
            );
            assert!(
                q.x.abs() < ARENA_HALF - 0.3 && q.y.abs() < ARENA_HALF - 0.3,
                "{p:?} leaves the arena at {q}"
            );
        }
    }
    // The zone is where the dummy actually goes: around its own spawn.
    assert!(in_dummy_strafe_zone(&layout, layout.dummy_spawn.xz()));
    assert!(in_dummy_strafe_zone(&layout, Vec2::new(-15.0, -6.0)));
    assert!(!in_dummy_strafe_zone(&layout, layout.player_spawn.xz()));
}

#[test]
fn prop_colliders_match_their_models() {
    for kind in PropKind::ALL {
        let model = EMBEDDED_MODELS
            .iter()
            .find(|m| m.name == kind.model())
            .expect("the prop's model is embedded");
        let side = Sidecar::parse(model.sidecar).unwrap();
        let points: Vec<Vec3> = kind.hulls().into_iter().flatten().collect();
        let lo = points.iter().copied().fold(Vec3::MAX, Vec3::min);
        let hi = points.iter().copied().fold(Vec3::MIN, Vec3::max);
        let (mlo, mhi) = (side.bounds.min(), side.bounds.max());
        assert!(lo.y.abs() < 0.01, "{kind:?} stands on the ground");
        assert!((hi.y - mhi.y).abs() < 0.02, "{kind:?} height {} vs {}", hi.y, mhi.y);
        assert!((hi.y - kind.height()).abs() < 0.01);
        for (a, b) in [(lo.x, mlo.x), (lo.z, mlo.z), (hi.x, mhi.x), (hi.z, mhi.z)] {
            assert!((a - b).abs() < 0.15, "{kind:?} footprint {lo}..{hi} vs model {mlo}..{mhi}");
        }
    }
}

#[test]
fn the_player_collides_with_a_rock() {
    let rock = prop(PropKind::RockA);
    let run = |sim: &mut Sim| -> Vec<Vec3> {
        let player = sim.player();
        put(sim, player, rock.position + Vec3::Z * 4.0, Facing::North);
        sim.player_intent().move_axis = Vec2::Y;
        (0..90)
            .map(|_| {
                sim.tick();
                sim.feet(player)
            })
            .collect()
    };
    let mut sim = Sim::new();
    let path = run(&mut sim);
    let end = *path.last().unwrap();
    assert!(
        end.z > rock.position.z + 0.6,
        "running at the rock stops in front of it: {end}"
    );
    let rock_collider = rock.kind.collider();
    let capsule = Collider::capsule(RADIUS - 0.02, 1.8 - 2.0 * RADIUS);
    for feet in &path {
        let hit = intersection_test(
            &rock_collider,
            rock.position,
            rock.transform().rotation,
            &capsule,
            *feet + Vec3::Y * 0.9,
            Quat::IDENTITY,
        )
        .unwrap();
        assert!(!hit, "the player ran into the rock at {feet}");
    }
    // Without the rock the same run carries on straight through its spot.
    let mut open = Sim::new();
    let entity = prop_entity(&mut open, rock);
    open.world_mut().despawn(entity);
    let path = run(&mut open);
    assert!(path.last().unwrap().z < rock.position.z - 2.0);
}

#[test]
fn the_player_can_jump_onto_a_stump() {
    let stump = prop(PropKind::StumpA);
    let mut sim = Sim::new();
    let player = sim.player();
    // Walking into it, the stump blocks (it's taller than a step).
    put(&mut sim, player, stump.position + Vec3::Z * 2.5, Facing::North);
    sim.player_intent().move_axis = Vec2::Y;
    sim.ticks(60);
    let blocked = sim.feet(player);
    assert!(blocked.y < 0.05 && blocked.z > stump.position.z + 0.5, "{blocked}");
    // A running jump lands on top: run at it, jump about 3.3 m out (where the
    // arc comes back down to the stump's height), stop once landed.
    put(&mut sim, player, stump.position + Vec3::Z * 6.0, Facing::North);
    sim.player_intent().move_axis = Vec2::Y;
    let mut jumped = false;
    for _ in 0..120 {
        sim.tick();
        let feet = sim.feet(player);
        if !jumped && feet.z <= stump.position.z + 3.3 {
            let mut intent = sim.player_intent();
            intent.jump = true;
            intent.jump_pressed = true;
            jumped = true;
        } else if jumped && sim.get::<Motor>(player).grounded && feet.y > 0.3 {
            let mut intent = sim.player_intent();
            intent.move_axis = Vec2::ZERO;
            intent.jump = false;
            break;
        }
    }
    assert!(jumped);
    sim.ticks(60);
    let feet = sim.feet(player);
    assert!(
        (feet.y - stump.kind.height()).abs() < 0.05,
        "standing on the stump: {feet}"
    );
    assert!(sim.get::<Motor>(player).grounded);
    assert!(feet.xz().distance(stump.position.xz()) < 0.5);
}

#[test]
fn a_wall_places_through_a_rock() {
    let rock = prop(PropKind::RockA);
    let mut sim = Sim::new();
    let player = sim.player();
    // The rock sits on the grid line between two cells; stand in the cell east
    // of it and build a wall on that cell's west edge.
    let cell = GridCell::containing(rock.position + Vec3::X * 0.5);
    assert_eq!(cell.min_corner().x, rock.position.x, "the rock is on a grid line");
    put(&mut sim, player, cell.base_center(), Facing::West);
    sim.player_intent().select = Some(ActiveTool::Build(PieceKind::Wall));
    sim.ticks(20);
    {
        let mut intent = sim.player_intent();
        intent.fire = true;
        intent.fire_pressed = true;
    }
    sim.tick();
    sim.player_intent().fire = false;
    sim.ticks(2);
    let slot = PieceSlot::wall(GridCell::new(cell.x, cell.z, 0), Facing::West);
    let wall = sim
        .world_mut()
        .query::<(Entity, &Piece)>()
        .iter(sim.world())
        .find(|(_, p)| p.slot() == slot)
        .map(|(e, _)| e)
        .expect("the wall was placed through the rock");
    // It really does pass through the rock.
    let tuning = pieced::tuning::Tuning::default().building;
    let wall_shape = slot.collider(&tuning);
    let t = slot.transform();
    let overlap = intersection_test(
        &rock.kind.collider(),
        rock.position,
        rock.transform().rotation,
        &wall_shape,
        t.translation,
        t.rotation,
    )
    .unwrap();
    assert!(overlap, "the wall intersects the rock");
    assert!(sim.world().get::<Piece>(wall).is_some());
}

#[test]
fn a_rock_blocks_a_rifle_shot() {
    let rock = prop(PropKind::RockA);
    let fire_at_dummy = |sim: &mut Sim| -> (Vec<ShotFired>, Vec<DamageDealt>, Entity) {
        sim.tuning_mut().dummy.stand_still = true;
        {
            let mut t = sim.tuning_mut();
            t.combat.rifle.base_spread_deg = 0.0;
            t.combat.rifle.bloom_per_shot_deg = 0.0;
        }
        sim.record::<ShotFired>();
        sim.record::<DamageDealt>();
        let player = sim.player();
        let target = dummy(sim);
        // The dummy stands just behind the rock, the player 3.5 m in front.
        put(sim, target, rock.position - Vec3::Z * 3.5, Facing::South);
        put(sim, player, rock.position + Vec3::Z * 3.5, Facing::North);
        let eye = sim.feet(player) + Vec3::Y * sim.get::<EyeHeight>(player).0;
        let aim = look_toward(sim.feet(target) + Vec3::Y * 0.3 - eye);
        sim.set_look(player, aim.yaw, aim.pitch);
        sim.clear_recorded::<ShotFired>();
        sim.clear_recorded::<DamageDealt>();
        {
            let mut intent = sim.player_intent();
            intent.fire = true;
            intent.fire_pressed = true;
        }
        sim.tick();
        sim.player_intent().fire = false;
        sim.ticks(2);
        (sim.recorded(), sim.recorded(), target)
    };

    let mut sim = Sim::new();
    let rock_entity = prop_entity(&mut sim, rock);
    let (shots, damage, target) = fire_at_dummy(&mut sim);
    let shot = shots.first().expect("the rifle fired");
    assert_eq!(shot.traces[0].hit, Some(rock_entity), "the bolt hits the rock");
    assert!(
        damage.iter().all(|d| d.target != target),
        "nothing reaches the dummy behind the rock"
    );

    // The same shot with the rock gone hits the dummy.
    let mut open = Sim::new();
    let entity = prop_entity(&mut open, rock);
    open.world_mut().despawn(entity);
    let (_, damage, target) = fire_at_dummy(&mut open);
    assert!(damage.iter().any(|d| d.target == target), "an open shot lands");
}

#[test]
fn over_five_seeded_minutes_the_dummy_never_stalls_on_a_prop() {
    for seed in [1, 2] {
        let mut sim = Sim::with_seed(seed);
        let d = dummy(&mut sim);
        let mut closest = f32::MAX;
        let mut travelled = 0.0;
        let mut last = sim.feet(d);
        for _ in 0..(60 * 60 * 5) {
            sim.tick();
            let feet = sim.feet(d);
            travelled += feet.xz().distance(last.xz());
            last = feet;
            for p in &ARENA_PROPS {
                closest = closest.min(p.footprint_distance(feet.xz()) - RADIUS);
            }
        }
        assert!(travelled > 300.0, "seed {seed}: the dummy strafed {travelled:.0} m");
        assert!(
            closest > 0.5,
            "seed {seed}: the dummy came within {closest:.2} m of touching a prop"
        );
    }
}
