//! Headless simulation harness for tests: the real gameplay plugins, no window,
//! exactly one fixed 60 Hz tick per [`Sim::tick`], seeded randomness.

use crate::{
    shared::{AppState, LookAngles, Player, PlayerIntent, SimTick},
    tuning::Tuning,
};
use bevy::{ecs::message::Message, prelude::*};

pub struct Sim {
    pub app: App,
}

#[derive(Resource)]
struct Recorded<M: Message + Clone>(Vec<M>);

fn record_messages<M: Message + Clone>(
    mut reader: MessageReader<M>,
    mut store: ResMut<Recorded<M>>,
) {
    store.0.extend(reader.read().cloned());
}

impl Default for Sim {
    fn default() -> Self {
        Self::new()
    }
}

impl Sim {
    pub fn new() -> Self {
        Self::with_seed(1)
    }

    /// Builds the simulation, enters `Playing` and runs the first update.
    pub fn with_seed(seed: u64) -> Self {
        let mut app = crate::app::headless_app(seed);
        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::Playing);
        app.update();
        Self { app }
    }

    pub fn world(&self) -> &World {
        self.app.world()
    }

    pub fn world_mut(&mut self) -> &mut World {
        self.app.world_mut()
    }

    /// Advances exactly one fixed tick (1/60 s).
    pub fn tick(&mut self) {
        self.app.update();
    }

    pub fn ticks(&mut self, n: u32) {
        for _ in 0..n {
            self.tick();
        }
    }

    /// Advances by whole ticks covering `seconds`.
    pub fn run_seconds(&mut self, seconds: f32) {
        self.ticks((seconds * 60.0).round() as u32);
    }

    pub fn sim_tick(&self) -> u64 {
        self.world().resource::<SimTick>().0
    }

    pub fn player(&mut self) -> Entity {
        self.world_mut()
            .query_filtered::<Entity, With<Player>>()
            .single(self.world())
            .expect("exactly one player")
    }

    pub fn intent(&mut self, entity: Entity) -> Mut<'_, PlayerIntent> {
        self.world_mut()
            .get_mut::<PlayerIntent>(entity)
            .expect("character has an intent")
    }

    pub fn player_intent(&mut self) -> Mut<'_, PlayerIntent> {
        let player = self.player();
        self.intent(player)
    }

    pub fn set_look(&mut self, entity: Entity, yaw: f32, pitch: f32) {
        let mut look = self
            .world_mut()
            .get_mut::<LookAngles>(entity)
            .expect("character has look angles");
        look.yaw = yaw;
        look.pitch = pitch;
    }

    pub fn feet(&self, entity: Entity) -> Vec3 {
        self.get::<Transform>(entity).translation
    }

    pub fn get<T: Component>(&self, entity: Entity) -> &T {
        self.world()
            .get::<T>(entity)
            .unwrap_or_else(|| panic!("missing {}", std::any::type_name::<T>()))
    }

    pub fn tuning_mut(&mut self) -> Mut<'_, Tuning> {
        self.world_mut().resource_mut::<Tuning>()
    }

    /// Starts recording every message of type `M` (read back with [`Sim::recorded`]).
    pub fn record<M: Message + Clone>(&mut self) {
        if !self.world().contains_resource::<Recorded<M>>() {
            self.app
                .insert_resource(Recorded::<M>(Vec::new()))
                .add_systems(Last, record_messages::<M>);
        }
    }

    pub fn recorded<M: Message + Clone>(&self) -> Vec<M> {
        self.world()
            .get_resource::<Recorded<M>>()
            .map(|r| r.0.clone())
            .unwrap_or_default()
    }

    pub fn clear_recorded<M: Message + Clone>(&mut self) {
        if let Some(mut r) = self.world_mut().get_resource_mut::<Recorded<M>>() {
            r.0.clear();
        }
    }
}
