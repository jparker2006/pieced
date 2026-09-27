//! Client-only: the edit grid over the piece the player is editing (D44), a
//! clear cartoon overlay in the ghost's glowing blue.
//!
//! Each tile is a faint glowing pane with a bright border, lifted just off the
//! piece's edit surface: both faces of a wall, the top and underside of a
//! floor, a ramp's slope, and a cone's pyramid plus its base (seen from inside
//! a box). The tile under the crosshair brightens, selected tiles turn solid
//! (Fortnite shows them "opaque rather than clear"), and a selection that isn't
//! a valid shape turns red, so a "bwomp" is visible before you release. Removed
//! tiles keep their pane, so you can select them again.
//!
//! The tile meshes and the eight materials are made once at startup; the grid
//! only moves and swaps handles.

use super::{
    EditMode, EditSession, PieceKind,
    edit::{self, tile_count, tile_quad},
    mesh::{KINDS, MeshBuilder, kind_index},
};
use crate::{
    look::ToonMaterial,
    palette::cartoon,
    shared::Player,
    tuning::Tuning,
};
use bevy::{light::NotShadowCaster, prelude::*};

pub(super) fn build(app: &mut App) {
    app.add_systems(Startup, create_edit_grid)
        .add_systems(Update, update_edit_grid);
}

/// How far the panes float off the piece (m).
const LIFT: f32 = 0.05;
/// Panes are inset this much from their tile's edges (m).
const INSET: f32 = 0.05;
const BORDER: f32 = 0.028;
const FILL_ALPHA: f32 = 1.0;
/// Where the brick wall model's faces are, off its middle plane (m).
const WALL_FACE: f32 = 0.16;

/// A tile's look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TileLook {
    Idle,
    Hovered,
    Selected,
    /// Selected while the selection isn't a valid shape.
    Invalid,
}

impl TileLook {
    const ALL: [TileLook; 4] = [
        TileLook::Idle,
        TileLook::Hovered,
        TileLook::Selected,
        TileLook::Invalid,
    ];

    fn index(self) -> usize {
        self as usize
    }
}

/// What tile `tile` of the open grid shows.
pub fn tile_look(session: &EditSession, tile: u8) -> TileLook {
    let selection = session.selection();
    if selection.has(tile) {
        if session.drag.is_some() && !session.selection_valid() {
            TileLook::Invalid
        } else {
            TileLook::Selected
        }
    } else if session.hovered == Some(tile) {
        TileLook::Hovered
    } else {
        TileLook::Idle
    }
}

#[derive(Resource)]
struct EditGridAssets {
    /// Per kind (in `kind_index` order), per tile: the pane and its border.
    fills: [Vec<Handle<Mesh>>; 4],
    borders: [Vec<Handle<Mesh>>; 4],
    /// Per [`TileLook`].
    fill_materials: [Handle<ToonMaterial>; 4],
    border_materials: [Handle<ToonMaterial>; 4],
}

#[derive(Component)]
struct EditGridRoot;

/// One tile's pane (`border` false) or border, on the grid root.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditGridTile {
    pub tile: u8,
    pub border: bool,
}

/// The layers a tile is drawn on: its quad (piece-local), lifted along each
/// outward normal. Cone tiles fold over the hip, so each half gets its own
/// face's normal.
fn tile_layers(kind: PieceKind, tile: u8, tuning: &Tuning) -> Vec<(Vec<Vec3>, Vec3)> {
    let q = tile_quad(kind, tile);
    let inset = |pts: &[Vec3]| -> Vec<Vec3> {
        let c = pts.iter().copied().sum::<Vec3>() / pts.len() as f32;
        pts.iter()
            .map(|p| *p + (c - *p).normalize_or_zero() * INSET)
            .collect()
    };
    let lifted = |pts: &[Vec3], n: Vec3, by: f32| -> (Vec<Vec3>, Vec3) {
        (inset(pts).iter().map(|p| *p + n * by).collect(), n)
    };
    let b = &tuning.building;
    match kind {
        // The brick faces stand 0.15 m off the wall's middle plane.
        PieceKind::Wall => vec![
            lifted(&q, Vec3::Z, WALL_FACE + LIFT),
            lifted(&q, Vec3::NEG_Z, WALL_FACE + LIFT),
        ],
        PieceKind::Floor => vec![
            lifted(&q, Vec3::Y, b.floor_thickness / 2.0 + LIFT),
            lifted(&q, Vec3::NEG_Y, b.floor_thickness / 2.0 + LIFT),
        ],
        PieceKind::Ramp => {
            let n = Vec3::new(0.0, crate::shared::CELL_SIZE, crate::shared::LEVEL_HEIGHT)
                .normalize();
            vec![lifted(&q, n, 0.08 + LIFT)]
        }
        PieceKind::Cone => {
            // q = [ground corner, edge midpoint (x = 0), apex, edge midpoint (z = 0)].
            let face = |a: Vec3, b: Vec3, c: Vec3| {
                let n = (b - a).cross(c - a).normalize_or_zero();
                if n.y < 0.0 { -n } else { n }
            };
            let (n1, n2) = (face(q[0], q[1], q[2]), face(q[0], q[2], q[3]));
            vec![
                lifted(&[q[0], q[1], q[2]], n1, 0.1 + LIFT),
                lifted(&[q[0], q[2], q[3]], n2, 0.1 + LIFT),
                // The base, facing down.
                lifted(
                    &[q[0], q[1], Vec3::ZERO, q[3]],
                    Vec3::NEG_Y,
                    LIFT,
                ),
            ]
        }
    }
}

fn tile_meshes(kind: PieceKind, tile: u8, tuning: &Tuning) -> (Mesh, Mesh) {
    let mut fill = MeshBuilder::default();
    let mut border = MeshBuilder::default();
    for (pts, n) in tile_layers(kind, tile, tuning) {
        let c = pts.iter().copied().sum::<Vec3>() / pts.len() as f32;
        fill.poly(&pts, c - n, FILL_ALPHA);
        for k in 0..pts.len() {
            border.bar(pts[k], pts[(k + 1) % pts.len()], BORDER, 1.0);
        }
    }
    (fill.build(), border.build())
}

fn material(color: Color, alpha: f32, glow: f32) -> ToonMaterial {
    ToonMaterial::new(color.with_alpha(alpha))
        .with_emissive(color, glow)
        .with_alpha(AlphaMode::Blend)
}

fn create_edit_grid(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ToonMaterial>>,
    tuning: Res<Tuning>,
) {
    let mut fills: [Vec<Handle<Mesh>>; 4] = Default::default();
    let mut borders: [Vec<Handle<Mesh>>; 4] = Default::default();
    for kind in KINDS {
        for tile in 0..tile_count(kind) {
            let (f, b) = tile_meshes(kind, tile, &tuning);
            fills[kind_index(kind)].push(meshes.add(f));
            borders[kind_index(kind)].push(meshes.add(b));
        }
    }
    let blue = cartoon::GHOST_BLUE;
    let white = Color::srgb(0.85, 0.97, 1.0);
    let red = cartoon::GHOST_RED;
    let fill_materials = TileLook::ALL.map(|look| {
        materials.add(match look {
            TileLook::Idle => material(blue, 0.12, 0.5),
            TileLook::Hovered => material(white, 0.3, 0.7),
            TileLook::Selected => material(blue, 0.72, 0.9),
            TileLook::Invalid => material(red, 0.72, 0.9),
        })
    });
    let border_materials = TileLook::ALL.map(|look| {
        materials.add(match look {
            TileLook::Idle => material(blue, 0.85, 0.8),
            TileLook::Hovered => material(white, 1.0, 1.0),
            TileLook::Selected => material(white, 1.0, 1.0),
            TileLook::Invalid => material(red, 1.0, 1.0),
        })
    });
    let root = commands
        .spawn((
            Name::new("Edit grid"),
            EditGridRoot,
            Transform::default(),
            Visibility::Hidden,
        ))
        .id();
    let most = KINDS.iter().map(|k| tile_count(*k)).max().unwrap_or(9);
    for tile in 0..most {
        for border in [false, true] {
            let list = if border { &borders } else { &fills };
            commands.spawn((
                Name::new("Edit grid tile"),
                EditGridTile { tile, border },
                Mesh3d(list[0][tile as usize].clone()),
                MeshMaterial3d(fill_materials[0].clone()),
                Transform::default(),
                Visibility::Hidden,
                NotShadowCaster,
                ChildOf(root),
            ));
        }
    }
    commands.insert_resource(EditGridAssets {
        fills,
        borders,
        fill_materials,
        border_materials,
    });
}

fn update_edit_grid(
    assets: Option<Res<EditGridAssets>>,
    player: Option<Single<&EditMode, With<Player>>>,
    root: Option<Single<(&mut Transform, &mut Visibility), With<EditGridRoot>>>,
    mut tiles: Query<
        (
            &EditGridTile,
            &mut Mesh3d,
            &mut MeshMaterial3d<ToonMaterial>,
            &mut Visibility,
        ),
        Without<EditGridRoot>,
    >,
) {
    let (Some(assets), Some(root)) = (assets, root) else {
        return;
    };
    let (mut root_xf, mut root_vis) = root.into_inner();
    let Some(session) = player.and_then(|p| p.session) else {
        root_vis.set_if_neq(Visibility::Hidden);
        return;
    };
    root_vis.set_if_neq(Visibility::Inherited);
    root_xf.set_if_neq(session.slot.transform());
    let kind = session.slot.kind;
    let k = kind_index(kind);
    for (tile, mut mesh, mut material, mut vis) in &mut tiles {
        if tile.tile >= edit::tile_count(kind) {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        vis.set_if_neq(Visibility::Inherited);
        let list = if tile.border {
            &assets.borders
        } else {
            &assets.fills
        };
        let wanted = &list[k][tile.tile as usize];
        if mesh.0 != *wanted {
            mesh.0 = wanted.clone();
        }
        let look = tile_look(&session, tile.tile).index();
        let wanted = if tile.border {
            &assets.border_materials[look]
        } else {
            &assets.fill_materials[look]
        };
        if material.0 != *wanted {
            material.0 = wanted.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::building::{EditDrag, PieceEdit, PieceSlot};
    use crate::shared::{Facing, GridCell};

    #[test]
    fn tiles_brighten_under_the_crosshair_and_redden_when_invalid() {
        let slot = PieceSlot::wall(GridCell::new(5, 5, 0), Facing::North);
        let mut s = EditSession {
            piece: Entity::PLACEHOLDER,
            slot,
            base: PieceEdit::FULL,
            hovered: Some(4),
            drag: None,
        };
        assert_eq!(tile_look(&s, 4), TileLook::Hovered);
        assert_eq!(tile_look(&s, 0), TileLook::Idle);
        // Dragging 4 then 7: a door, valid.
        s.drag = Some(EditDrag {
            paint: true,
            tiles: PieceEdit::of(&[4, 7]).tiles,
            path: [4, 7, 0, 0],
            len: 2,
        });
        assert_eq!(tile_look(&s, 7), TileLook::Selected);
        // 4 and 8 aren't a shape: red until released.
        s.drag = Some(EditDrag {
            paint: true,
            tiles: PieceEdit::of(&[4, 8]).tiles,
            path: [4, 8, 0, 0],
            len: 2,
        });
        assert_eq!(tile_look(&s, 8), TileLook::Invalid);
    }

    #[test]
    fn every_tile_has_a_pane_on_each_side_it_is_seen_from() {
        let tuning = Tuning::default();
        for kind in KINDS {
            for tile in 0..tile_count(kind) {
                let layers = tile_layers(kind, tile, &tuning);
                let expected = match kind {
                    PieceKind::Wall | PieceKind::Floor => 2,
                    PieceKind::Ramp => 1,
                    PieceKind::Cone => 3,
                };
                assert_eq!(layers.len(), expected, "{kind:?} {tile}");
                for (pts, n) in layers {
                    assert!(pts.len() >= 3 && (n.length() - 1.0).abs() < 1e-4);
                }
            }
        }
    }
}
