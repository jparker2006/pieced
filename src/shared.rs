//! Shared vocabulary for every slice. Owned by the orchestrator: slices may add
//! fields or variants additively, but never rename or repurpose existing ones.

use avian3d::prelude::PhysicsLayer;
use bevy::prelude::*;

/// Top-level game flow. Gameplay fixed-step systems run only in [`AppState::Playing`].
#[derive(States, Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppState {
    #[default]
    Boot,
    Playing,
    Paused,
}

/// Collision layers. Movement collides with `World` and `Piece`; hitscan also hits
/// `Body` and `Head` hitboxes of characters.
#[derive(PhysicsLayer, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    #[default]
    World,
    Piece,
    Body,
    Head,
}

// ---------------------------------------------------------------------------
// Characters
// ---------------------------------------------------------------------------

/// Any combatant: the player, the training dummy, and (later) bots.
#[derive(Component, Debug, Default)]
pub struct Character;

/// The human-controlled character. Exactly one exists in the game.
#[derive(Component, Debug, Default)]
pub struct Player;

/// Marks a hitbox child entity and points back at its owning character.
#[derive(Component, Debug, Clone, Copy)]
pub struct Hitbox {
    pub owner: Entity,
    pub head: bool,
}

/// View direction. Yaw rotates about +Y (0 looks toward -Z); pitch is positive upward.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq)]
pub struct LookAngles {
    pub yaw: f32,
    pub pitch: f32,
}

impl LookAngles {
    pub const PITCH_LIMIT: f32 = 89.0_f32 * std::f32::consts::PI / 180.0;

    pub fn rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0)
    }

    /// Unit aim direction.
    pub fn forward(&self) -> Vec3 {
        self.rotation() * Vec3::NEG_Z
    }

    /// Flat (XZ) forward and right vectors for movement.
    pub fn flat_basis(&self) -> (Vec3, Vec3) {
        let yaw = Quat::from_rotation_y(self.yaw);
        (yaw * Vec3::NEG_Z, yaw * Vec3::X)
    }

    pub fn add(&mut self, delta_yaw: f32, delta_pitch: f32) {
        self.yaw = (self.yaw + delta_yaw).rem_euclid(std::f32::consts::TAU);
        self.pitch = (self.pitch + delta_pitch).clamp(-Self::PITCH_LIMIT, Self::PITCH_LIMIT);
    }
}

/// Eye height above the character's feet. Movement lowers it while crouching.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct EyeHeight(pub f32);

impl Default for EyeHeight {
    fn default() -> Self {
        Self(1.62)
    }
}

/// Feet position at the start of the latest fixed tick, for render interpolation.
/// The character's `Transform.translation` is its authoritative feet position.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct PreviousFeet(pub Vec3);

/// Eye position and aim direction of a character in the fixed simulation.
pub fn eye_ray(transform: &Transform, eye: &EyeHeight, look: &LookAngles) -> (Vec3, Dir3) {
    let origin = transform.translation + Vec3::Y * eye.0;
    (origin, Dir3::new(look.forward()).unwrap_or(Dir3::NEG_Z))
}

/// Health with a shield that absorbs damage first.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Health {
    pub hp: f32,
    pub shield: f32,
    pub max_hp: f32,
    pub max_shield: f32,
}

impl Default for Health {
    fn default() -> Self {
        Self::full(100.0, 100.0)
    }
}

/// How one damage application split between shield and health.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DamageSplit {
    pub to_shield: f32,
    pub to_hp: f32,
    pub shield_broke: bool,
    pub killed: bool,
}

impl Health {
    pub fn full(max_hp: f32, max_shield: f32) -> Self {
        Self {
            hp: max_hp,
            shield: max_shield,
            max_hp,
            max_shield,
        }
    }

    pub fn is_dead(&self) -> bool {
        self.hp <= 0.0
    }

    pub fn total(&self) -> f32 {
        self.hp + self.shield
    }

    /// Applies damage, shield first. Dead characters take no further damage.
    pub fn apply(&mut self, amount: f32) -> DamageSplit {
        if self.is_dead() || amount <= 0.0 {
            return DamageSplit::default();
        }
        let had_shield = self.shield > 0.0;
        let to_shield = amount.min(self.shield);
        self.shield -= to_shield;
        let to_hp = (amount - to_shield).min(self.hp);
        self.hp -= to_hp;
        DamageSplit {
            to_shield,
            to_hp,
            shield_broke: had_shield && self.shield <= 0.0,
            killed: self.hp <= 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.hp = self.max_hp;
        self.shield = self.max_shield;
    }
}

// ---------------------------------------------------------------------------
// Intent: the only way gameplay is controlled
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WeaponKind {
    Rifle,
    Pump,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PieceKind {
    Wall,
    Floor,
    Ramp,
}

/// What the character holds: a gun, or a piece to build.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActiveTool {
    Weapon(WeaponKind),
    Build(PieceKind),
}

impl Default for ActiveTool {
    fn default() -> Self {
        Self::Weapon(WeaponKind::Rifle)
    }
}

impl ActiveTool {
    pub fn is_build(&self) -> bool {
        matches!(self, Self::Build(_))
    }
}

/// Whether the character is aiming down sights. Written by combat, read by the
/// input adapter (sensitivity) and presentation.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Ads(pub bool);

/// Per-character control input. Written by exactly one controller per character
/// (keyboard/trackpad adapter, scenario director, or a bot) and consumed by the
/// fixed-step gameplay systems.
///
/// Held fields describe the current state. `*_pressed` fields are rising edges
/// that stay latched until a fixed tick consumes them; they are cleared in
/// `FixedLast` so a press is never lost when a frame runs zero fixed ticks, and
/// never repeated when a frame runs two.
#[derive(Component, Debug, Default, Clone, PartialEq)]
pub struct PlayerIntent {
    /// x = strafe right, y = forward. Length ≤ 1.
    pub move_axis: Vec2,
    pub jump: bool,
    pub jump_pressed: bool,
    pub sprint: bool,
    pub crouch: bool,
    pub crouch_pressed: bool,
    /// Primary action held: fire the gun or keep placing pieces (turbo build).
    pub fire: bool,
    pub fire_pressed: bool,
    pub ads_toggle_pressed: bool,
    pub reload_pressed: bool,
    /// Requested tool change, latched like a press.
    pub select: Option<ActiveTool>,
    /// Look change in radians (yaw, pitch), already scaled by sensitivity.
    /// Applied every rendered frame, not in the fixed step.
    pub look_delta: Vec2,
}

impl PlayerIntent {
    pub fn clear_edges(&mut self) {
        self.jump_pressed = false;
        self.crouch_pressed = false;
        self.fire_pressed = false;
        self.ads_toggle_pressed = false;
        self.reload_pressed = false;
        self.select = None;
    }
}

// ---------------------------------------------------------------------------
// Simulation clock
// ---------------------------------------------------------------------------

/// Number of fixed gameplay ticks simulated so far (60 per second).
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SimTick(pub u64);

pub const TICK_HZ: f64 = 60.0;

pub fn tick_duration() -> std::time::Duration {
    // Same value for Time<Fixed> and the headless manual step, so each headless
    // update runs exactly one fixed tick.
    std::time::Duration::from_nanos(16_666_667)
}

pub const TICK_SECONDS: f32 = 1.0 / 60.0;

// ---------------------------------------------------------------------------
// World grid (meters)
// ---------------------------------------------------------------------------

/// Horizontal size of one build cell.
pub const CELL_SIZE: f32 = 4.0;
/// Height of one build level.
pub const LEVEL_HEIGHT: f32 = 3.0;
/// The arena is `ARENA_CELLS × ARENA_CELLS` cells, centered on the origin.
pub const ARENA_CELLS: i32 = 12;
/// Highest buildable level index is `MAX_LEVELS - 1`.
pub const MAX_LEVELS: i32 = 6;
/// Half the arena's side length.
pub const ARENA_HALF: f32 = CELL_SIZE * ARENA_CELLS as f32 / 2.0;

/// A build-grid cell: `x`, `z` in `0..ARENA_CELLS`, `level` in `0..MAX_LEVELS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GridCell {
    pub x: i32,
    pub z: i32,
    pub level: i32,
}

impl GridCell {
    pub const fn new(x: i32, z: i32, level: i32) -> Self {
        Self { x, z, level }
    }

    pub fn in_bounds(&self) -> bool {
        (0..ARENA_CELLS).contains(&self.x)
            && (0..ARENA_CELLS).contains(&self.z)
            && (0..MAX_LEVELS).contains(&self.level)
    }

    /// World-space minimum corner (x, base y, z).
    pub fn min_corner(&self) -> Vec3 {
        Vec3::new(
            -ARENA_HALF + self.x as f32 * CELL_SIZE,
            self.level as f32 * LEVEL_HEIGHT,
            -ARENA_HALF + self.z as f32 * CELL_SIZE,
        )
    }

    /// Center of the cell's floor at its base height.
    pub fn base_center(&self) -> Vec3 {
        self.min_corner() + Vec3::new(CELL_SIZE / 2.0, 0.0, CELL_SIZE / 2.0)
    }

    /// Cell containing a world point (level from height, floored).
    pub fn containing(point: Vec3) -> Self {
        Self {
            x: ((point.x + ARENA_HALF) / CELL_SIZE).floor() as i32,
            z: ((point.z + ARENA_HALF) / CELL_SIZE).floor() as i32,
            level: (point.y / LEVEL_HEIGHT).floor() as i32,
        }
    }
}

/// Cardinal direction on the grid. `North` is -Z (the default look direction).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Facing {
    North,
    East,
    South,
    West,
}

impl Facing {
    pub const ALL: [Facing; 4] = [Facing::North, Facing::East, Facing::South, Facing::West];

    pub fn vector(self) -> Vec3 {
        match self {
            Facing::North => Vec3::NEG_Z,
            Facing::East => Vec3::X,
            Facing::South => Vec3::Z,
            Facing::West => Vec3::NEG_X,
        }
    }

    pub fn offset(self) -> IVec2 {
        match self {
            Facing::North => IVec2::new(0, -1),
            Facing::East => IVec2::new(1, 0),
            Facing::South => IVec2::new(0, 1),
            Facing::West => IVec2::new(-1, 0),
        }
    }

    pub fn opposite(self) -> Self {
        match self {
            Facing::North => Facing::South,
            Facing::East => Facing::West,
            Facing::South => Facing::North,
            Facing::West => Facing::East,
        }
    }

    /// Nearest cardinal direction to a horizontal direction.
    pub fn from_direction(dir: Vec3) -> Self {
        if dir.x.abs() > dir.z.abs() {
            if dir.x > 0.0 {
                Facing::East
            } else {
                Facing::West
            }
        } else if dir.z > 0.0 {
            Facing::South
        } else {
            Facing::North
        }
    }

    /// Yaw (radians) that looks along this facing.
    pub fn yaw(self) -> f32 {
        use std::f32::consts::{FRAC_PI_2, PI};
        match self {
            Facing::North => 0.0,
            Facing::West => FRAC_PI_2,
            Facing::South => PI,
            Facing::East => -FRAC_PI_2 + std::f32::consts::TAU,
        }
    }
}

// ---------------------------------------------------------------------------
// Gameplay messages (fixed step → presentation)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageTarget {
    Character,
    Piece,
}

/// Emitted after damage has been applied to a character or a piece.
#[derive(Message, Debug, Clone)]
pub struct DamageDealt {
    pub source: Option<Entity>,
    pub target: Entity,
    pub target_kind: DamageTarget,
    /// Total damage applied (shield + health, or structure damage).
    pub amount: f32,
    pub to_shield: f32,
    pub headshot: bool,
    pub shield_broke: bool,
    pub killed: bool,
    pub point: Vec3,
    pub normal: Vec3,
    pub tick: u64,
}

/// One traced bullet or pellet of a shot.
#[derive(Debug, Clone, Copy)]
pub struct ShotTrace {
    pub end: Vec3,
    pub normal: Vec3,
    pub hit: Option<Entity>,
}

/// Emitted when a gun fires (for viewmodel, tracers, audio and stats).
#[derive(Message, Debug, Clone)]
pub struct ShotFired {
    pub shooter: Entity,
    pub weapon: WeaponKind,
    pub origin: Vec3,
    pub traces: Vec<ShotTrace>,
    pub tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceChange {
    Placed,
    /// Crack stage reached: 1 at ≤66% HP, 2 at ≤33% HP.
    Cracked(u8),
    Destroyed,
}

/// Emitted by building whenever a piece is placed, cracks further, or breaks.
#[derive(Message, Debug, Clone)]
pub struct PieceChanged {
    pub entity: Entity,
    pub kind: PieceKind,
    pub change: PieceChange,
    /// World-space center of the piece.
    pub center: Vec3,
    pub tick: u64,
}

/// Emitted when a character's health reaches zero.
#[derive(Message, Debug, Clone)]
pub struct Eliminated {
    pub victim: Entity,
    pub by: Option<Entity>,
    pub position: Vec3,
    pub tick: u64,
}

/// Emitted for gameplay-driven sounds and effects that aren't shots, damage or pieces.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub enum GameCue {
    Jump { who: Entity },
    Land { who: Entity, speed: f32 },
    Footstep { who: Entity },
    SlideStart { who: Entity },
    ReloadStart { who: Entity, weapon: WeaponKind },
    ReloadShell { who: Entity },
    ReloadDone { who: Entity, weapon: WeaponKind },
    WeaponSwitch { who: Entity, tool: ActiveTool },
    AdsChanged { who: Entity, ads: bool },
    PlacementRejected { who: Entity },
    Respawned { who: Entity },
}

// ---------------------------------------------------------------------------
// Fixed-step ordering
// ---------------------------------------------------------------------------

/// Ordering of gameplay inside `FixedUpdate` (chained in this order, only while
/// [`AppState::Playing`]). Put each system in its slice's set.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimSet {
    /// Controllers that write intents (dummy pattern, later bots).
    Control,
    /// Tool selection from intents.
    Tool,
    Movement,
    Building,
    Combat,
    /// Deaths, respawns, bookkeeping after combat.
    Resolve,
}

/// Request from combat to damage a building piece. Building applies it in
/// [`SimSet::Resolve`] and then emits [`DamageDealt`] and [`PieceChanged`].
/// Piece colliders live on the piece entity itself, so a ray hit's entity is `piece`.
#[derive(Message, Debug, Clone)]
pub struct PieceHit {
    pub piece: Entity,
    pub amount: f32,
    pub source: Option<Entity>,
    pub point: Vec3,
    pub normal: Vec3,
}
