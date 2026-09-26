//! Pipeline warm-up. On macOS, Bevy compiles each new render pipeline
//! (material type × material key × mesh vertex layout × view) synchronously
//! the first time something using it is drawn, which drops a frame. So during
//! `AppState::Boot`, every module draws one of each variant it will need, in
//! front of the camera and behind an opaque loading overlay, and play starts
//! only once every item has been drawn for [`WARMUP_FRAMES`] frames.
//!
//! Any module can register items through the [`Warmup`] system param:
//!
//! ```ignore
//! fn warm_my_stuff(mut warmup: Warmup, assets: Res<MyAssets>) {
//!     warmup.add(assets.mesh.clone(), assets.material.clone());
//!     warmup.add_with(assets.mesh.clone(), assets.glow.clone(), RenderLayers::layer(1));
//! }
//! ```
//!
//! Adding an item holds the [`BootGate`] key [`WARMUP_GATE`] immediately (not
//! deferred), so a module may add items and release its own gate in the same
//! system (use [`Warmup::gate`]) without Boot slipping out first. The gate is
//! released when every item has been rendered [`WARMUP_FRAMES`] times, and the
//! items are despawned. Items added after Boot are processed the same way, but
//! drawn too small to see.

use crate::{app::BootGate, render::MainCamera, shared::AppState};
use bevy::{
    camera::visibility::NoFrustumCulling, ecs::system::SystemParam, light::NotShadowCaster,
    prelude::*,
};

/// The `BootGate` key held while warm-up items are pending.
pub const WARMUP_GATE: &str = "warmup";
/// Frames each item must be rendered before the gate opens.
pub const WARMUP_FRAMES: u32 = 3;
/// With nothing registered, the gate still waits this many frames for late
/// Startup registrations.
pub const MIN_WAIT_FRAMES: u32 = 2;
/// Never hold Boot longer than this (an item that can't be drawn).
pub const TIMEOUT_FRAMES: u32 = 240;
/// Item scale while hidden by the loading overlay...
const BOOT_SCALE: f32 = 0.05;
/// ...and after Boot, when nothing covers them.
const HIDDEN_SCALE: f32 = 1e-4;

/// A warm-up draw. Spawned through [`Warmup`]; despawned when done.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct WarmupItem {
    /// Frames this item has been drawn (visible in some view).
    pub rendered_frames: u32,
    slot: u32,
}

#[derive(Resource, Debug, Default)]
pub struct WarmupState {
    /// Frames waited in the current round.
    waited: u32,
    /// Items registered and not yet finished (some may not be spawned yet).
    expected: u32,
    next_slot: u32,
}

/// Registers warm-up items; see the module docs.
#[derive(SystemParam)]
pub struct Warmup<'w, 's> {
    commands: Commands<'w, 's>,
    gate: ResMut<'w, BootGate>,
    state: ResMut<'w, WarmupState>,
}

impl Warmup<'_, '_> {
    /// Draws `mesh` with `material` once (well, [`WARMUP_FRAMES`] times).
    pub fn add<M: Material>(&mut self, mesh: Handle<Mesh>, material: Handle<M>) -> Entity {
        self.add_with(mesh, material, ())
    }

    /// Like [`Warmup::add`], with extra components on the item: its
    /// `RenderLayers` (e.g. the viewmodel layer), an `Outline`, a `MeshTag`...
    /// Don't pass `Transform` or `Visibility`; the warm-up owns those.
    pub fn add_with<M: Material>(
        &mut self,
        mesh: Handle<Mesh>,
        material: Handle<M>,
        extra: impl Bundle,
    ) -> Entity {
        self.gate.hold(WARMUP_GATE);
        self.state.expected += 1;
        let slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.commands
            .spawn((
                Name::new("Warm-up item"),
                WarmupItem {
                    rendered_frames: 0,
                    slot,
                },
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_scale(Vec3::splat(BOOT_SCALE)),
                Visibility::Visible,
                NoFrustumCulling,
                NotShadowCaster,
                extra,
            ))
            .id()
    }

    /// The Boot gate, for a module that registers items and then releases its
    /// own hold in the same system.
    pub fn gate(&mut self) -> &mut BootGate {
        &mut self.gate
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarmupVerdict {
    Wait,
    Done,
    TimedOut,
}

/// The gate decision after `waited` frames, given how many items are
/// registered (`expected`) and each spawned item's rendered-frame count.
pub fn warmup_verdict(waited: u32, expected: usize, rendered: &[u32]) -> WarmupVerdict {
    if waited >= TIMEOUT_FRAMES {
        WarmupVerdict::TimedOut
    } else if rendered.len() < expected {
        // Some registrations haven't been spawned yet.
        WarmupVerdict::Wait
    } else if rendered.is_empty() {
        if waited >= MIN_WAIT_FRAMES {
            WarmupVerdict::Done
        } else {
            WarmupVerdict::Wait
        }
    } else if rendered.iter().all(|&frames| frames >= WARMUP_FRAMES) {
        WarmupVerdict::Done
    } else {
        WarmupVerdict::Wait
    }
}

/// Counts rendered frames (an item visible last frame was drawn last frame)
/// and opens the gate when every item is warm.
pub(crate) fn run_warmup(
    mut commands: Commands,
    mut gate: ResMut<BootGate>,
    mut state: ResMut<WarmupState>,
    mut items: Query<(Entity, &mut WarmupItem, &ViewVisibility)>,
) {
    if !gate.held().any(|key| key == WARMUP_GATE) {
        state.waited = 0;
        return;
    }
    state.waited += 1;
    let mut rendered = Vec::new();
    for (_, mut item, visibility) in &mut items {
        if visibility.get() {
            item.rendered_frames += 1;
        }
        rendered.push(item.rendered_frames);
    }
    let verdict = warmup_verdict(state.waited, state.expected as usize, &rendered);
    if verdict == WarmupVerdict::Wait {
        return;
    }
    if verdict == WarmupVerdict::TimedOut {
        let cold = rendered.iter().filter(|&&f| f < WARMUP_FRAMES).count();
        warn!(
            "pipeline warm-up timed out after {} frames: {cold} of {} items never drew",
            state.waited,
            rendered.len()
        );
    } else {
        info!(
            "pipeline warm-up: {} items drawn in {} frames",
            rendered.len(),
            state.waited
        );
    }
    for (entity, ..) in &items {
        commands.entity(entity).despawn();
    }
    state.expected = state.expected.saturating_sub(rendered.len() as u32);
    if verdict == WarmupVerdict::TimedOut {
        state.expected = 0;
    }
    state.waited = 0;
    state.next_slot = 0;
    gate.release(WARMUP_GATE);
}

/// Keeps items in a small grid just in front of the main camera.
pub(crate) fn place_warmup_items(
    state: Res<State<AppState>>,
    camera: Option<Single<&Transform, (With<MainCamera>, Without<WarmupItem>)>>,
    mut items: Query<(&WarmupItem, &mut Transform)>,
) {
    let Some(camera) = camera else {
        return;
    };
    let scale = if *state.get() == AppState::Boot {
        BOOT_SCALE
    } else {
        HIDDEN_SCALE
    };
    for (item, mut transform) in &mut items {
        let (col, row) = ((item.slot % 8) as f32, (item.slot / 8) as f32);
        let local = Vec3::new((col - 3.5) * 0.09, (row - 1.5) * 0.09, -1.2);
        *transform = Transform {
            translation: camera.transform_point(local),
            rotation: camera.rotation,
            scale: Vec3::splat(scale),
        };
    }
}

/// Covers the world while Boot lasts, so warm-up draws are never seen.
#[derive(Component, Debug)]
pub struct LoadingOverlay;

pub(crate) fn spawn_loading_overlay(mut commands: Commands) {
    commands.spawn((
        Name::new("Loading overlay"),
        LoadingOverlay,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        BackgroundColor(Color::srgb(0.1, 0.07, 0.2)),
        GlobalZIndex(1000),
    ));
}

pub(crate) fn remove_loading_overlay(
    mut commands: Commands,
    overlays: Query<Entity, With<LoadingOverlay>>,
) {
    for entity in &overlays {
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_waits_for_every_item_to_draw_three_times() {
        use WarmupVerdict::*;
        assert_eq!(warmup_verdict(1, 2, &[3, 2]), Wait);
        assert_eq!(warmup_verdict(5, 2, &[3, 3]), Done);
        assert_eq!(warmup_verdict(5, 3, &[3, 3]), Wait, "one still unspawned");
        assert_eq!(warmup_verdict(1, 1, &[4]), Done);
        // Nothing registered: open after a short grace period.
        assert_eq!(warmup_verdict(1, 0, &[]), Wait);
        assert_eq!(warmup_verdict(MIN_WAIT_FRAMES, 0, &[]), Done);
        // Never hang Boot.
        assert_eq!(warmup_verdict(TIMEOUT_FRAMES, 2, &[0, 0]), TimedOut);
    }

    fn warmup_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .init_state::<AppState>()
            .init_resource::<BootGate>()
            .init_resource::<WarmupState>()
            .add_systems(Update, run_warmup);
        app.world_mut().resource_mut::<BootGate>().hold(WARMUP_GATE);
        app
    }

    fn register(app: &mut App, n: usize) -> Vec<Entity> {
        let mut system = bevy::ecs::system::SystemState::<Warmup>::new(app.world_mut());
        let mut warmup = system.get_mut(app.world_mut()).unwrap();
        let items: Vec<Entity> = (0..n)
            .map(|_| warmup.add::<StandardMaterial>(Handle::default(), Handle::default()))
            .collect();
        system.apply(app.world_mut());
        items
    }

    fn draw(app: &mut App, entities: &[Entity]) {
        for &e in entities {
            app.world_mut()
                .entity_mut(e)
                .insert(ViewVisibility::VISIBLE);
        }
    }

    fn open(app: &App) -> bool {
        app.world().resource::<BootGate>().is_open()
    }

    #[test]
    fn gate_opens_after_every_item_rendered_three_frames() {
        let mut app = warmup_app();
        let items = register(&mut app, 2);
        // Frame 1: nothing drawn yet (the renderer hasn't seen them).
        app.update();
        assert!(!open(&app));
        // One item draws from now on, the other one frame later.
        draw(&mut app, &items[..1]);
        app.update();
        draw(&mut app, &items[1..]);
        app.update();
        app.update();
        assert!(!open(&app), "second item has only drawn twice");
        assert_eq!(
            app.world()
                .get::<WarmupItem>(items[1])
                .unwrap()
                .rendered_frames,
            2
        );
        app.update();
        assert!(open(&app));
        // Items are cleaned up.
        app.update();
        assert!(items.iter().all(|&e| app.world().get_entity(e).is_err()));
    }

    #[test]
    fn late_registration_holds_the_gate_again() {
        let mut app = warmup_app();
        let first = register(&mut app, 1);
        draw(&mut app, &first);
        for _ in 0..4 {
            app.update();
        }
        assert!(open(&app));
        // A module registers more (say, after its models load) and releases its
        // own hold in the same breath: the warm-up gate is already held again.
        {
            let mut system = bevy::ecs::system::SystemState::<Warmup>::new(app.world_mut());
            let mut warmup = system.get_mut(app.world_mut()).unwrap();
            warmup.gate().hold("models");
            warmup.add::<StandardMaterial>(Handle::default(), Handle::default());
            warmup.gate().release("models");
            assert!(!warmup.gate().is_open());
            system.apply(app.world_mut());
        }
        app.update();
        assert!(!open(&app));
        let late: Vec<Entity> = app
            .world_mut()
            .query_filtered::<Entity, With<WarmupItem>>()
            .iter(app.world())
            .collect();
        assert_eq!(late.len(), 1);
        draw(&mut app, &late);
        for _ in 0..3 {
            app.update();
        }
        assert!(open(&app));
    }

    #[test]
    fn nothing_registered_opens_after_a_grace_period() {
        let mut app = warmup_app();
        app.update();
        assert!(!open(&app));
        app.update();
        assert!(open(&app));
    }

    #[test]
    fn an_item_that_never_draws_cannot_hang_boot() {
        let mut app = warmup_app();
        let items = register(&mut app, 1);
        for _ in 0..TIMEOUT_FRAMES {
            app.update();
        }
        assert!(open(&app));
        app.update();
        assert!(app.world().get_entity(items[0]).is_err());
    }
}
