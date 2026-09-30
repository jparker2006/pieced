//! Building juice (M4 chunk 5, D109), presentation only:
//!
//! - **Assembly:** a placed piece's model builds itself in
//!   [`ASSEMBLE_SECONDS`]: a wall's bricks stack up course by course from the
//!   ground, a floor, ramp or cone's planks slap down from above with a
//!   squash. The piece's collider and full HP exist from its first tick
//!   (D45): only the model's child transform moves ([`assemble_pose`]).
//! - **Edit flips:** when an edit lands, each wall or floor tile it cuts
//!   flips out of the hole, and each tile it restores flips in, over
//!   [`FLIP_SECONDS`] ([`flip_pose`]). The mesh and the collider follow the
//!   edit at once; the flipping tiles are a small pool of slabs
//!   ([`FLIP_POOL`]) over it.
//! - **The ghost pulses:** its glow breathes at [`GHOST_PULSE_HZ`]
//!   ([`ghost_glow`]).
//!
//! Broken pieces' debris is [`super::debris`]; placement sounds' ±5% pitch
//! is `audio::pitch_spread` (chunk 3).
//!
//! **Cost.** Assembly moves the one visual child a piece already has for
//! 9 frames. The flip pool is [`FLIP_POOL`] entities made once, sharing the
//! pieces' toon material and two slab meshes (warmed behind the loading
//! screen); a flip moves at most 9 of them for 6 frames. The ghost's pulse
//! rewrites one material's glow per frame while the ghost shows.

use super::{
    PieceEdit,
    edit::{tile_count, tile_quad},
};
use crate::{
    look::{Outline, ToonMaterial, warmup::Warmup, with_outline_normals},
    palette::cartoon,
    shared::PieceKind,
    viewmodel::mesh::ModelBuilder,
};
use bevy::{light::NotShadowCaster, prelude::*};
use std::f32::consts::{FRAC_PI_2, PI};

/// Seconds a placed piece takes to assemble.
pub const ASSEMBLE_SECONDS: f32 = 0.15;
/// Brick courses a wall stacks in.
pub const WALL_COURSES: u32 = 4;
/// How high above its place a plank piece starts (m).
pub const SLAP_HEIGHT: f32 = 0.55;
/// Seconds an edit tile takes to flip.
pub const FLIP_SECONDS: f32 = 0.1;
/// Flipping tiles at once (a wall has 9 tiles).
pub const FLIP_POOL: usize = 12;
/// The ghost's glow: its rate and its range (× its base glow).
pub const GHOST_PULSE_HZ: f32 = 1.0;
pub const GHOST_PULSE_LOW: f32 = 0.6;
pub const GHOST_PULSE_HIGH: f32 = 1.35;

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// A piece model's assembly `t` seconds after it was placed: its offset
/// from its resting place, its tilt, and its scale (about the model's
/// origin, which is on the ground for walls and ramps). Exactly at rest
/// from [`ASSEMBLE_SECONDS`] on.
pub fn assemble_pose(kind: PieceKind, t: f32) -> (Vec3, Quat, Vec3) {
    let x = t / ASSEMBLE_SECONDS;
    if x >= 1.0 {
        return (Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
    }
    let x = x.max(0.0);
    match kind {
        PieceKind::Wall => {
            // Courses land one after another over the first 85%; each
            // drops in fast. Then a tiny settle.
            let span = 0.85;
            let per = span / WALL_COURSES as f32;
            let mut height = 0.0;
            for c in 0..WALL_COURSES {
                let start = c as f32 * per * 0.8;
                height += smooth((x - start) / (per * 1.2));
            }
            let h = (height / WALL_COURSES as f32).max(0.02);
            let settle = if x > span {
                let k = (x - span) / (1.0 - span);
                0.05 * (k * PI).sin()
            } else {
                0.0
            };
            (
                Vec3::ZERO,
                Quat::IDENTITY,
                Vec3::new(1.0 + settle * 0.5, h * (1.0 - settle), 1.0),
            )
        }
        PieceKind::Floor | PieceKind::Ramp | PieceKind::Cone => {
            // Falls (accelerating) and slaps down at 60%, then squashes.
            let hit = 0.6;
            if x < hit {
                let f = x / hit;
                let drop = SLAP_HEIGHT * (1.0 - f * f);
                let tilt = Quat::from_rotation_x(0.22 * (1.0 - f));
                (Vec3::Y * drop, tilt, Vec3::ONE)
            } else {
                let k = (x - hit) / (1.0 - hit);
                let squash = 0.28 * (1.0 - k) * (k * PI * 1.5).cos().max(-0.3);
                (
                    Vec3::ZERO,
                    Quat::IDENTITY,
                    Vec3::new(1.0 + squash * 0.3, 1.0 - squash, 1.0 + squash * 0.3),
                )
            }
        }
    }
}

/// An edit tile's flip `t` seconds in: how far it has turned about its
/// width (radians) and its size (1 whole). A cut tile turns from flat to
/// edge-on and shrinks away; a restored one does the reverse. `None` when
/// done.
pub fn flip_pose(t: f32, restoring: bool) -> Option<(f32, f32)> {
    if !(0.0..FLIP_SECONDS).contains(&t) {
        return None;
    }
    let x = smooth(t / FLIP_SECONDS);
    let k = if restoring { 1.0 - x } else { x };
    Some((FRAC_PI_2 * k, 1.0 - 0.7 * k))
}

/// The ghost's glow multiplier `seconds` in: a 1 Hz breath.
pub fn ghost_glow(seconds: f32) -> f32 {
    let wave = 0.5 - 0.5 * (seconds * GHOST_PULSE_HZ * std::f32::consts::TAU).cos();
    GHOST_PULSE_LOW + (GHOST_PULSE_HIGH - GHOST_PULSE_LOW) * wave
}

/// The tiles an edit change flips: `(tile, restoring)` for each wall or
/// floor tile whose cut state changed. Ramps and cones reshape instead (no
/// tiles flip). Fixed size, no allocation.
pub fn flipped_tiles(
    kind: PieceKind,
    before: PieceEdit,
    after: PieceEdit,
) -> impl Iterator<Item = (u8, bool)> {
    let tiles = match kind {
        PieceKind::Wall | PieceKind::Floor => tile_count(kind),
        PieceKind::Ramp | PieceKind::Cone => 0,
    };
    (0..tiles)
        .filter(move |&t| before.has(t) != after.has(t))
        .map(move |t| (t, before.has(t)))
}

/// A tile slab's resting transform in the piece's frame: centred on the
/// tile, its x along the tile's width, its y along its height, z out of the
/// piece; scaled to the tile and `depth` thick.
pub fn tile_slab(kind: PieceKind, tile: u8, depth: f32) -> Transform {
    let q = tile_quad(kind, tile);
    let centre = (q[0] + q[1] + q[2] + q[3]) / 4.0;
    let u = q[1] - q[0];
    let v = q[3] - q[0];
    let x = u.normalize_or(Vec3::X);
    let z = u.cross(v).normalize_or(Vec3::Z);
    let y = z.cross(x);
    Transform {
        translation: centre,
        rotation: Quat::from_mat3(&Mat3::from_cols(x, y, z)),
        scale: Vec3::new(u.length() * 0.96, v.length() * 0.96, depth),
    }
}

/// A flip in the pool.
#[derive(Debug, Clone, Copy)]
struct Flip {
    /// The tile's resting transform (world).
    rest: Transform,
    restoring: bool,
    /// Brick (a wall's tile) or plank.
    brick: bool,
    age: f32,
}

/// The flipping tiles' pool (see the module docs).
#[derive(Resource)]
pub struct EditFlips {
    entities: Vec<Entity>,
    live: [Option<Flip>; FLIP_POOL],
    slabs: [Handle<Mesh>; 2],
    next: usize,
}

impl EditFlips {
    /// Tiles flipping now.
    pub fn live(&self) -> usize {
        self.live.iter().flatten().count()
    }

    /// Starts a tile's flip (the oldest one gives way when all are busy).
    pub fn start(&mut self, rest: Transform, restoring: bool, brick: bool) {
        let slot = self
            .live
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.next % FLIP_POOL);
        self.next = slot + 1;
        self.live[slot] = Some(Flip {
            rest,
            restoring,
            brick,
            age: 0.0,
        });
    }
}

/// A slab mesh: a unit chamfered box in `color`, with outline normals.
fn slab_mesh(color: Color) -> Mesh {
    let mut m = ModelBuilder::new();
    m.chamfer_box(Vec3::splat(-0.5), Vec3::splat(0.5), 0.06, color);
    with_outline_normals(m.build())
}

/// Makes the flip pool once the piece material exists (and warms it).
pub(super) fn spawn_flip_pool(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: &Handle<ToonMaterial>,
    warmup: &mut Warmup,
) {
    let slabs = [
        meshes.add(slab_mesh(cartoon::BRICK)),
        meshes.add(slab_mesh(crate::palette::WOOD)),
    ];
    let entities = (0..FLIP_POOL)
        .map(|_| {
            commands
                .spawn((
                    Name::new("Edit flip"),
                    Mesh3d(slabs[0].clone()),
                    MeshMaterial3d(material.clone()),
                    Outline::default(),
                    Transform::default(),
                    Visibility::Hidden,
                    NotShadowCaster,
                ))
                .id()
        })
        .collect();
    for slab in &slabs {
        warmup.add_with(slab.clone(), material.clone(), Outline::default());
    }
    commands.insert_resource(EditFlips {
        entities,
        live: [None; FLIP_POOL],
        slabs,
        next: 0,
    });
}

/// Queues the flips for an edit of the piece at `frame` (its world frame).
pub(super) fn queue_flips(
    flips: &mut EditFlips,
    kind: PieceKind,
    frame: &Transform,
    before: PieceEdit,
    after: PieceEdit,
    depth: f32,
) {
    for (tile, restoring) in flipped_tiles(kind, before, after) {
        let rest = frame.mul_transform(tile_slab(kind, tile, depth));
        flips.start(rest, restoring, kind == PieceKind::Wall);
    }
}

/// Turns the flipping tiles about their width and hides the finished ones.
pub(super) fn animate_flips(
    time: Res<Time>,
    flips: Option<ResMut<EditFlips>>,
    mut tiles: Query<(&mut Transform, &mut Visibility, &mut Mesh3d), Without<super::Piece>>,
) {
    let Some(mut flips) = flips else { return };
    let dt = time.delta_secs();
    let flips = &mut *flips;
    for (i, slot) in flips.live.iter_mut().enumerate() {
        let Some(&entity) = flips.entities.get(i) else {
            continue;
        };
        let Ok((mut tf, mut vis, mut mesh)) = tiles.get_mut(entity) else {
            continue;
        };
        let Some(flip) = slot.as_mut() else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        match flip_pose(flip.age, flip.restoring) {
            Some((turn, size)) => {
                let slab = &flips.slabs[usize::from(!flip.brick)];
                if mesh.0 != *slab {
                    mesh.0 = slab.clone();
                }
                *tf = Transform {
                    translation: flip.rest.translation,
                    rotation: flip.rest.rotation * Quat::from_rotation_x(turn),
                    scale: flip.rest.scale * Vec3::new(size, size, 1.0),
                };
                vis.set_if_neq(Visibility::Inherited);
                flip.age += dt;
            }
            None => {
                vis.set_if_neq(Visibility::Hidden);
                *slot = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_assemble_in_0_15_s_and_rest_exactly() {
        for kind in [
            PieceKind::Wall,
            PieceKind::Floor,
            PieceKind::Ramp,
            PieceKind::Cone,
        ] {
            let rest = (Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
            assert_eq!(assemble_pose(kind, ASSEMBLE_SECONDS), rest);
            assert_eq!(assemble_pose(kind, 5.0), rest);
            assert_ne!(assemble_pose(kind, 0.0), rest, "{kind:?} starts apart");
        }
        // A wall grows from the ground in courses.
        let low = assemble_pose(PieceKind::Wall, 0.02).2.y;
        let mid = assemble_pose(PieceKind::Wall, 0.07).2.y;
        assert!(low < mid && mid < 1.0, "{low} {mid}");
        // Planks fall from above.
        let (drop, _, _) = assemble_pose(PieceKind::Floor, 0.0);
        assert!((drop.y - SLAP_HEIGHT).abs() < 1e-5);
    }

    #[test]
    fn edit_tiles_flip_in_0_1_s() {
        let (turn, size) = flip_pose(0.0, false).unwrap();
        assert!(
            turn.abs() < 1e-6 && (size - 1.0).abs() < 1e-6,
            "cut: starts flat"
        );
        let (turn, _) = flip_pose(0.0, true).unwrap();
        assert!((turn - FRAC_PI_2).abs() < 1e-5, "restored: starts edge-on");
        assert!(flip_pose(FLIP_SECONDS, false).is_none());
        // A window cut in a wall flips its tiles out; resetting flips them in.
        let window = PieceEdit::of(&[4]);
        let cut: Vec<_> = flipped_tiles(PieceKind::Wall, PieceEdit::FULL, window).collect();
        assert_eq!(cut, [(4, false)]);
        let back: Vec<_> = flipped_tiles(PieceKind::Wall, window, PieceEdit::FULL).collect();
        assert_eq!(back, [(4, true)]);
    }

    #[test]
    fn the_ghost_breathes_once_a_second() {
        assert!((ghost_glow(0.0) - GHOST_PULSE_LOW).abs() < 1e-5);
        assert!((ghost_glow(0.5) - GHOST_PULSE_HIGH).abs() < 1e-5);
        assert!((ghost_glow(1.0) - ghost_glow(0.0)).abs() < 1e-5);
    }
}
