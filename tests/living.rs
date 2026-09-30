//! A living world (M4 chunk 6, D123): the scenery's sway weights, the sway
//! moving nothing the simulation reads, and every per-frame motion and mix
//! step allocating nothing.
//!
//! The sway itself is a GPU vertex offset (`wind.wgsl`, mirrored by
//! `look::wind::sway_offset`); the clouds and birds are checked moving in
//! `tests/far.rs` and `arena::visuals::living`.

use avian3d::prelude::{Collider, RigidBody};
use bevy::prelude::*;
use pieced::{
    arena::visuals::{Island, living::CloudDrift, sway_weight},
    audio::{
        ambience::{AmbienceDirector, AmbienceInput},
        barks::{Bark, BarkGate},
        music::Screen,
    },
    far::{FarLayout, birds::flocks},
    look::wind::{gust_at, sway_offset},
    sim::Sim,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

/// Counts allocations on the thread that turned counting on.
struct Counting;

static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
thread_local! {
    static MINE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) && MINE.with(|m| m.get()) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn allocations_in(f: impl FnOnce()) -> usize {
    MINE.with(|m| m.set(true));
    ALLOCATIONS.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    f();
    COUNTING.store(false, Ordering::Relaxed);
    MINE.with(|m| m.set(false));
    ALLOCATIONS.load(Ordering::Relaxed)
}

#[test]
fn every_frame_of_motion_and_mix_allocates_nothing() {
    let layout = FarLayout::default();
    let birds = flocks(&layout);
    let clouds = [
        CloudDrift::Sea { phase: 0.4 },
        CloudDrift::Anchored { phase: 1.1 },
    ];
    let mut director = AmbienceDirector::default();
    let mut gate = BarkGate::default();
    let knight = Entity::from_raw_u32(7).unwrap();
    let n = allocations_in(|| {
        let mut sink = 0.0f32;
        for frame in 0..600 {
            let t = frame as f64 / 60.0;
            for b in &birds {
                sink += b.transform(t).translation.x;
            }
            for c in &clouds {
                sink += c.transform(t).translation.y;
            }
            let p = Vec3::new(frame as f32, 0.0, 3.0);
            sink += sway_offset(t as f32, p, 0.7, 0.07).x + gust_at(t as f32, p.xz());
            let g = director.step(
                1.0 / 60.0,
                &AmbienceInput {
                    screen: Screen::Playing,
                    fighting: frame % 200 < 100,
                    alive: 4,
                    gust: 0.5,
                    warning: frame % 97 == 0,
                    effects: 0.8,
                },
            );
            sink += g[0];
            let _ = gate.try_bark(t, knight, t - 2.0, Bark::Yelp, 0.0);
            if frame % 150 == 0 {
                gate.warning(t, |v| sink += v.end as f32);
            }
        }
        std::hint::black_box(sink);
    });
    assert_eq!(n, 0, "{n} allocations in 600 frames");
}

#[test]
fn the_scenery_carries_its_sway_weights_and_the_ground_never_sways() {
    let island = Island::generate();
    // The ground and cliffs: unit normals, weight 0.
    for n in island.ground.normals.iter().chain(&island.skirt.normals) {
        assert!(sway_weight(*n) < 1e-4);
    }
    // Every grass blade: rooted at its base, swaying at its tip.
    let mut tips = 0;
    for chunk in &island.tufts {
        for (p, n) in chunk.positions.chunks(3).zip(chunk.normals.chunks(3)) {
            let w = [sway_weight(n[0]), sway_weight(n[1]), sway_weight(n[2])];
            let top = (0..3).max_by(|a, b| p[*a][1].total_cmp(&p[*b][1])).unwrap();
            for (i, w) in w.iter().enumerate() {
                if i == top {
                    assert!(*w > 0.99, "a blade's tip sways");
                    tips += 1;
                } else {
                    assert!(*w < 1e-4, "a blade's root stays");
                }
            }
        }
    }
    assert!(tips > 1000);
    // Bushes: rooted low, swaying most at the crown.
    for chunk in &island.bushes {
        for (p, n) in chunk.positions.iter().zip(&chunk.normals) {
            let w = sway_weight(*n);
            assert!((0.0..=1.0).contains(&w));
            if p[1] < 0.05 {
                assert!(w < 0.05, "a bush's foot stays at {p:?}: {w}");
            }
        }
    }
    assert!(
        island
            .bushes
            .iter()
            .flat_map(|c| &c.normals)
            .filter(|n| sway_weight(**n) > 0.5)
            .count()
            > 1000,
        "bush crowns sway"
    );
}

#[test]
fn the_sway_moves_no_collider_or_hitbox() {
    // The sway lives in the shaders: over a stretch of a wave, every static
    // collider (the island, props, pieces standing) stays exactly where it was,
    // however hard the wind blows on screen.
    let mut sim = Sim::waves(5);
    let snapshot = |sim: &mut Sim| {
        let world = sim.world_mut();
        let mut q =
            world.query_filtered::<(Entity, &GlobalTransform, &RigidBody), With<Collider>>();
        let mut v: Vec<(Entity, Vec3)> = q
            .iter(world)
            .filter(|(_, _, b)| **b == RigidBody::Static)
            .map(|(e, t, _)| (e, t.translation()))
            .collect();
        v.sort_by_key(|x| x.0);
        v
    };
    let before = snapshot(&mut sim);
    assert!(!before.is_empty());
    sim.run_seconds(5.0);
    let after = snapshot(&mut sim);
    for (e, p) in &before {
        if let Some((_, q)) = after.iter().find(|(f, _)| f == e) {
            assert_eq!(p, q, "{e:?} moved");
        }
    }
}
