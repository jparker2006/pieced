//! Milestone 4 chunk 0 budgets (docs/M4-SPEC.md → Chunk 0, item 7; D114).
//!
//! - **A full wave within the triangle, draw and particle budgets.** The
//!   headless game with every visual plugin, Waves at wave 6 with 8 knights
//!   landed, orbs in flight and a drop ship, and the levers on (`overdraw=cap`,
//!   `dynres=on`): the meshes that would draw are counted (entities, distinct
//!   mesh + material batches, triangles, glows), and every effect pool is at
//!   or under its cap. The budgets pin what M3 closed with plus chunk 1's
//!   additions, so a feature that adds draws shows up here first (the GPU
//!   regression hunt of play-test 1 found no new cost; these keep it so).
//! - **The levers allocate nothing per frame**: the overdraw cap over many
//!   glows, the dynamic-resolution controller and the knight animation LOD,
//!   under a counting allocator.
//! - **8 knights animate within the budget** (M4 chunk 4): with Bevy's
//!   `AnimationPlugin` (as in the game) the full wave's knights all play the
//!   authored clips through the one shared graph, the clips add no draws
//!   (the same pinned meshes, batches and triangles), and the knights' clip
//!   mixing allocates nothing per step.

use bevy::{mesh::Indices, prelude::*};
use pieced::{
    perf::{AnimLod, DynamicResolution, PerfTuning},
    shared::AppState,
    waves::Run,
};
use std::time::Duration;

// ---------------------------------------------------------------------------
// The full wave
// ---------------------------------------------------------------------------

fn full_wave_app(seed: u64) -> App {
    use avian3d::prelude::PhysicsPlugins;
    use bevy::{
        gltf::GltfPlugin, input::InputPlugin, time::TimeUpdateStrategy,
        world_serialization::WorldSerializationPlugin,
    };
    use pieced::{
        rng::{Rng, SimRng},
        shared::{GameMode, tick_duration},
    };
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        bevy::state::app::StatesPlugin,
        AssetPlugin::default(),
        bevy::mesh::MeshPlugin,
        GltfPlugin::default(),
        WorldSerializationPlugin,
        PhysicsPlugins::default(),
        InputPlugin,
        bevy::animation::AnimationPlugin,
    ))
    .init_asset::<Image>()
    .init_asset::<StandardMaterial>()
    .add_plugins(pieced::app::SimPlugins)
    .add_plugins((
        pieced::look::LookPlugin,
        pieced::models::ModelsPlugin,
        pieced::arena::visuals::ArenaVisualsPlugin,
        pieced::far::FarViewPlugin,
        pieced::building::BuildingVisualsPlugin,
        pieced::fx::FxPlugin,
        pieced::hud::HudPlugin,
        pieced::waves::ships_visuals::ShipsVisualsPlugin,
        pieced::perf::PerfPlugin,
        pieced::profile::FrameProfilePlugin,
    ))
    .init_resource::<pieced::render::CurrentFov>()
    .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()))
    .insert_resource(SimRng(Rng::new(seed)))
    .insert_resource(GameMode::Waves);
    {
        let mut tuning = app.world_mut().resource_mut::<pieced::tuning::Tuning>();
        tuning.perf.overdraw_cap = true;
        tuning.perf.dynres = true;
    }
    app.finish();
    app.cleanup();
    let mut loaded = false;
    for _ in 0..3000 {
        app.update();
        if app
            .world()
            .get_resource::<pieced::models::ModelLibrary>()
            .is_some_and(|m| m.is_ready())
            && app
                .world()
                .contains_resource::<pieced::building::visuals::PieceAssets>()
        {
            loaded = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(loaded, "the models never loaded");
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Playing);
    app.update();
    let player = app
        .world_mut()
        .query_filtered::<Entity, With<pieced::shared::Player>>()
        .single(app.world())
        .unwrap();
    let mut health = pieced::shared::Health::full(1.0e9, 100.0);
    health.shield = 100.0;
    *app.world_mut()
        .get_mut::<pieced::shared::Health>(player)
        .unwrap() = health;
    app
}

/// What would draw now: meshes whose own visibility and every ancestor's
/// isn't `Hidden`.
#[derive(Debug, Default)]
struct DrawCount {
    meshes: usize,
    batches: usize,
    triangles: usize,
    glows: usize,
    knights_triangles: usize,
}

fn count_draws(app: &mut App) -> DrawCount {
    use std::collections::HashSet;
    let world = app.world_mut();
    let mut hidden: HashSet<Entity> = HashSet::new();
    for (e, v) in world.query::<(Entity, &Visibility)>().iter(world) {
        if *v == Visibility::Hidden {
            hidden.insert(e);
        }
    }
    let parents: Vec<(Entity, Entity)> = world
        .query::<(Entity, &ChildOf)>()
        .iter(world)
        .map(|(e, c)| (e, c.parent()))
        .collect();
    let parent_of = |e: Entity| parents.iter().find(|(c, _)| *c == e).map(|(_, p)| *p);
    let shown = |mut e: Entity| {
        for _ in 0..64 {
            if hidden.contains(&e) {
                return false;
            }
            match parent_of(e) {
                Some(p) => e = p,
                None => return true,
            }
        }
        true
    };
    let figures: HashSet<Entity> = world
        .query_filtered::<Entity, With<pieced::arena::visuals::TargetFigure>>()
        .iter(world)
        .collect();
    let in_figure = |mut e: Entity| {
        for _ in 0..64 {
            if figures.contains(&e) {
                return true;
            }
            match parent_of(e) {
                Some(p) => e = p,
                None => return false,
            }
        }
        false
    };
    let mut out = DrawCount::default();
    let mut batches = HashSet::new();
    let mut rows = Vec::new();
    for (e, mesh, material, glow) in world
        .query::<(
            Entity,
            &Mesh3d,
            Option<&MeshMaterial3d<pieced::look::ToonMaterial>>,
            Has<pieced::look::HaloSprite>,
        )>()
        .iter(world)
    {
        rows.push((e, mesh.0.clone(), material.map(|m| m.0.id()), glow));
    }
    let meshes = world.resource::<Assets<Mesh>>();
    for (e, mesh, material, glow) in rows {
        if !shown(e) {
            continue;
        }
        out.meshes += 1;
        out.glows += usize::from(glow);
        batches.insert((mesh.id(), material));
        let tris = meshes.get(&mesh).map_or(0, |m| match m.indices() {
            Some(Indices::U16(i)) => i.len() / 3,
            Some(Indices::U32(i)) => i.len() / 3,
            None => m.count_vertices() / 3,
        });
        out.triangles += tris;
        if in_figure(e) {
            out.knights_triangles += tris;
        }
    }
    out.batches = batches.len();
    out
}

/// A full wave is in play within M3's budgets plus chunk 1's additions, and
/// every effect pool stays within its cap.
#[test]
fn a_full_wave_stays_within_the_draw_triangle_and_particle_budgets() {
    let mut app = full_wave_app(7);
    {
        let mut run = app.world_mut().resource_mut::<Run>();
        run.wave = 6;
        run.remaining = 14;
    }
    let mut frames = 0;
    let mut most = DrawCount::default();
    let mut orbs_seen = 0;
    let mut ship_seen = false;
    let mut clip_driven = 0;
    while frames < 60 * 90 {
        app.update();
        frames += 1;
        if frames % 30 != 0 {
            continue;
        }
        let alive = app.world().resource::<Run>().alive;
        let orbs = app
            .world()
            .resource::<pieced::profile::LastCounters>()
            .0
            .orbs;
        orbs_seen = orbs_seen.max(orbs);
        ship_seen |= app
            .world()
            .get_resource::<pieced::waves::ships::Ships>()
            .is_some_and(|s| s.live() > 0);
        let draws = count_draws(&mut app);
        if alive >= 8 && draws.triangles > most.triangles {
            most = draws;
        }
        if alive >= 8 {
            clip_driven = clip_driven.max(knights_on_clips(&mut app));
        }
        // Every effect pool within its cap, all along.
        let tuning = app.world().resource::<pieced::tuning::Tuning>().clone();
        let (particles, debris) = app.world().resource::<pieced::fx::FxPools>().live();
        assert!(particles <= tuning.feedback.max_particles as usize);
        assert!(debris <= tuning.feedback.max_debris as usize);
        assert!(orbs as usize <= pieced::orb::ORB_POOL);
    }
    println!(
        "full wave: {most:?}, orbs seen {orbs_seen}, ship {ship_seen}, \
         {clip_driven} knights on the clips"
    );
    assert!(most.meshes > 0, "8 knights never landed together");
    assert!(
        clip_driven >= 8,
        "8 knights animate on the authored clips ({clip_driven})"
    );
    assert!(ship_seen, "no drop ship");
    // The knight budget (M4 chunk 4: ≤ 12k triangles each, wand included, and
    // outline hulls, which draw every knight mesh twice).
    assert!(
        most.knights_triangles <= 8 * 2 * KNIGHT_TRIANGLES,
        "knights: {} triangles",
        most.knights_triangles
    );
    // The whole scene, far layer included (≤ 90k of it): pinned just above
    // what chunk 1 closed with.
    assert!(most.triangles <= SCENE_TRIANGLES, "{most:?}");
    assert!(most.meshes <= SCENE_MESHES, "{most:?}");
    assert!(most.batches <= SCENE_BATCHES, "{most:?}");
    assert!(most.glows <= SCENE_GLOWS, "{most:?}");
}

/// Pinned budgets for a full wave (headless count: every mesh not hidden,
/// frustum culling aside). Measured at M4 chunk 1's close (seed 7): 1,437
/// meshes in 153 mesh + material batches, 488k triangles (knights 118k with
/// their outline hulls), 66 glows. These leave ~10% headroom (glows more:
/// spell bursts come and go). A feature that needs more must pay for it or
/// raise these with GPU numbers to back it (docs/M4-SPEC.md, D99).
///
/// M4 art pass: the detailed knight (≤ 11k triangles each with every eye
/// state, about 10.7k shown) and the round-puffed trees (≤ 3k) raised the
/// triangles to 632k (knights 172k) at 1,372 meshes in 150 batches, with the
/// offscreen full-wave GPU timer unchanged (world pass 4.15/5.57 ms mean/p95
/// against 4.20/5.42 before; `tests/wave_cost_offscreen.rs`). Triangles are
/// pinned 5% above that so art can't grow them silently; the mesh and batch
/// counts (the draws) are pinned tighter than before: art may not add draws.
/// Round 2 added three wand swirl sparks per knight: halo sprites, which
/// share the one halo batch and collapse to nothing while dark (1,509 meshes
/// in 153 batches, 624k triangles; world pass 4.06/5.19 ms offscreen).
const SCENE_TRIANGLES: usize = 665_000;
/// M4 chunk 4 raised the knight to ≤ 12k (the chibi knight is ~9.4k with
/// every eye state).
const KNIGHT_TRIANGLES: usize = 12_000;

/// Knights playing the authored clips, every one through the one shared
/// graph with all of its clips on its player.
fn knights_on_clips(app: &mut App) -> usize {
    use pieced::knight::{CLIP_COUNT, KnightAnim, KnightGraph};
    let Some(graph) = app
        .world()
        .get_resource::<KnightGraph>()
        .map(|g| g.graph.clone())
    else {
        return 0;
    };
    let world = app.world_mut();
    for (player, handle) in world
        .query::<(&AnimationPlayer, &AnimationGraphHandle)>()
        .iter(world)
    {
        assert_eq!(handle.0, graph, "one shared graph");
        assert_eq!(player.playing_animations().count(), CLIP_COUNT);
    }
    world
        .query::<&KnightAnim>()
        .iter(world)
        .filter(|a| a.clips_enabled() && !a.is_downed())
        .count()
}
const SCENE_MESHES: usize = 1_560;
const SCENE_BATCHES: usize = 160;
const SCENE_GLOWS: usize = 120;

// ---------------------------------------------------------------------------
// The levers allocate nothing per frame
// ---------------------------------------------------------------------------

struct CountingAlloc;

static ALLOCATIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

thread_local! {
    static ARMED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

// SAFETY: forwards every call to the system allocator unchanged.
unsafe impl std::alloc::GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        if ARMED.with(std::cell::Cell::get) {
            ALLOCATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        unsafe { std::alloc::System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, size: usize) -> *mut u8 {
        if ARMED.with(std::cell::Cell::get) {
            ALLOCATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        unsafe { std::alloc::System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

fn allocations_in(f: impl FnOnce()) -> u64 {
    let before = ALLOCATIONS.load(std::sync::atomic::Ordering::Relaxed);
    ARMED.with(|a| a.set(true));
    f();
    ARMED.with(|a| a.set(false));
    ALLOCATIONS.load(std::sync::atomic::Ordering::Relaxed) - before
}

/// The overdraw cap over 200 glows (well past the cap, so glows fade every
/// frame and some come back), single-threaded so it runs on this thread.
fn overdraw_app(capped: bool) -> App {
    use bevy::{
        ecs::schedule::{Schedules, SingleThreadedExecutor},
        mesh::MeshTag,
    };
    use pieced::look::{Halo, HaloSprite, LookSettings};
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<pieced::tuning::Tuning>()
        .init_resource::<LookSettings>()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f64(1.0 / 60.0),
        ))
        .add_systems(Update, pieced::perf::cap_overdraw);
    app.world_mut()
        .resource_mut::<pieced::tuning::Tuning>()
        .perf
        .overdraw_cap = capped;
    app.world_mut().spawn((
        pieced::render::MainCamera,
        Projection::Perspective(PerspectiveProjection::default()),
        GlobalTransform::IDENTITY,
    ));
    for i in 0..200 {
        let at = Vec3::new(
            (i % 20) as f32 - 10.0,
            (i / 20) as f32,
            -8.0 - (i % 7) as f32,
        );
        let owner = app
            .world_mut()
            .spawn((
                Halo::new(Color::WHITE, 2.0, 1.0),
                InheritedVisibility::VISIBLE,
            ))
            .id();
        app.world_mut().spawn((
            HaloSprite { owner },
            GlobalTransform::from(Transform::from_translation(at).with_scale(Vec3::splat(2.0))),
            MeshTag(0),
            Visibility::Inherited,
        ));
    }
    app.finish();
    app.cleanup();
    for (_, schedule) in app.world_mut().resource_mut::<Schedules>().iter_mut() {
        schedule.set_executor(SingleThreadedExecutor::new());
    }
    app
}

#[test]
fn the_levers_allocate_nothing_per_frame() {
    // The overdraw cap: the same frames with and without it.
    let mut plain = overdraw_app(false);
    let mut capped = overdraw_app(true);
    for _ in 0..30 {
        plain.update();
        capped.update();
    }
    let hidden = {
        let world = capped.world_mut();
        world
            .query::<&Visibility>()
            .iter(world)
            .filter(|v| **v == Visibility::Hidden)
            .count()
    };
    assert!(hidden > 50, "the cap faded glows out ({hidden} hidden)");
    let base = allocations_in(|| {
        for _ in 0..300 {
            plain.update();
        }
    });
    let with = allocations_in(|| {
        for _ in 0..300 {
            capped.update();
        }
    });
    assert!(
        with <= base,
        "the overdraw cap allocated {} times in 300 frames",
        with.saturating_sub(base)
    );

    // Dynamic resolution and the knight LOD: pure steps.
    let perf = PerfTuning::default();
    let mut dynres = DynamicResolution::default();
    let mut lod = AnimLod::default();
    let figures: Vec<Entity> = (0..8)
        .map(|i| Entity::from_raw_u32(i + 1).unwrap())
        .collect();
    for f in &figures {
        let _ = lod.step(*f, 1.0 / 60.0, 30.0, false, &perf);
    }
    let direct = allocations_in(|| {
        for i in 0..6_000u32 {
            let t = f64::from(i) / 60.0;
            let gpu = if (i / 600) % 2 == 0 { 15.0 } else { 7.0 };
            std::hint::black_box(dynres.observe(t, gpu, &perf));
            for (k, f) in figures.iter().enumerate() {
                std::hint::black_box(lod.step(*f, 1.0 / 60.0, 5.0 * k as f32, false, &perf));
            }
        }
    });
    assert_eq!(direct, 0);
    assert!(dynres.scale >= perf.dynres_min && dynres.scale <= perf.dynres_max);

    // 8 knights' animation with the clips (M4 chunk 4): stepping, mixing the
    // clips, flinching and dying allocate nothing.
    use pieced::knight::{HitRegion, KnightAnim, KnightEvent, KnightInput};
    let mut knights: Vec<KnightAnim> = (0..8)
        .map(|k| {
            let mut a = KnightAnim::new(k);
            a.set_clips(true);
            a
        })
        .collect();
    let stepped = allocations_in(|| {
        for i in 0..3_000u32 {
            for (k, anim) in knights.iter_mut().enumerate() {
                if (i + k as u32).is_multiple_of(97) {
                    anim.event(KnightEvent::Flinch {
                        region: HitRegion::Chest,
                        push: Vec3::Z,
                    });
                }
                let input = KnightInput {
                    velocity: Vec3::new((i as f32 * 0.01).sin() * 4.0, 0.0, -3.0),
                    windup: ((i / 60) % 3 == 0).then_some((i % 60) as f32 / 60.0),
                    downed: (i / 400) % 5 == 4,
                    ..Default::default()
                };
                std::hint::black_box(anim.step(1.0 / 60.0, &input));
                std::hint::black_box(anim.clip_mix());
            }
        }
    });
    assert_eq!(stepped, 0, "the knights' clip mixing allocated");
}
