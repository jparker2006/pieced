//! The island's barrier (T11): a shimmering, translucent rune curtain on the
//! arena's boundary, where the invisible boundary walls stand. It is invisible
//! from afar, fades in within about [`REVEAL_END`] m of the player's eye, and
//! brightens where the player touches it. All of that happens in
//! `barrier.wgsl` from the camera position, so nothing is updated per frame
//! except which sides are drawn at all: a side further than [`REVEAL_END`]
//! from the camera is hidden, so the curtain costs nothing mid-arena.

use crate::{look::warmup::Warmup, palette::cartoon, render::MainCamera, shared::ARENA_HALF};
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
};

pub const BARRIER_SHADER_PATH: &str = "embedded://pieced/shaders/barrier.wgsl";
/// Height of the curtain (it fades out well before its top).
pub const BARRIER_HEIGHT: f32 = 9.0;
/// Fully shown within this distance of the eye (m)...
pub const REVEAL_FULL: f32 = 1.6;
/// ...and gone beyond this one.
pub const REVEAL_END: f32 = 4.5;

/// The curtain's look (alpha-blended; see `barrier.wgsl`).
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct BarrierMaterial {
    /// rgb: line, rune and glow colour (linear); w: the sheet's opacity.
    #[uniform(0)]
    pub color: Vec4,
    /// rgb: the sheet's colour (linear); w: rune strength.
    #[uniform(0)]
    pub rune_color: Vec4,
    /// x: fully shown within (m), y: gone beyond (m), z: height (m), w: speed.
    #[uniform(0)]
    pub reveal: Vec4,
    /// x: touch glow radius (m), y: touch strength, z: rune cell size (m).
    #[uniform(0)]
    pub touch: Vec4,
}

fn linear(color: Color, w: f32) -> Vec4 {
    let c = color.to_linear();
    Vec4::new(c.red, c.green, c.blue, w)
}

impl Default for BarrierMaterial {
    fn default() -> Self {
        Self {
            color: linear(cartoon::BARRIER_CYAN, 0.26),
            rune_color: linear(cartoon::GHOST_BLUE, 0.9),
            reveal: Vec4::new(REVEAL_FULL, REVEAL_END, BARRIER_HEIGHT, 1.0),
            touch: Vec4::new(0.9, 0.4, 0.8, 0.0),
        }
    }
}

impl Material for BarrierMaterial {
    fn fragment_shader() -> ShaderRef {
        BARRIER_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }
}

/// How much of the curtain shows at `distance` m from the eye (the shader's
/// fade, on the CPU).
pub fn barrier_reveal(distance: f32) -> f32 {
    let t = ((distance - REVEAL_FULL) / (REVEAL_END - REVEAL_FULL)).clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

/// One side of the curtain; `outward` points out of the arena.
#[derive(Component, Debug, Clone, Copy)]
pub struct BarrierSide {
    pub outward: Vec3,
}

impl BarrierSide {
    /// Horizontal distance from a point to this side's plane.
    pub fn distance(&self, point: Vec3) -> f32 {
        (ARENA_HALF - point.dot(self.outward)).abs()
    }
}

/// One side's quad: the full arena edge, from just below the ground to
/// [`BARRIER_HEIGHT`], facing into the arena, in the frame of the north side
/// (outward -Z). Position and normal only.
pub fn side_mesh() -> Mesh {
    let (h, top, bottom) = (ARENA_HALF, BARRIER_HEIGHT, -0.3);
    let z = -ARENA_HALF;
    let positions = vec![[-h, bottom, z], [h, bottom, z], [h, top, z], [-h, top, z]];
    let normals = vec![[0.0, 0.0, 1.0]; 4];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    // Counter-clockwise seen from inside the arena (+Z of the north side).
    .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
}

/// Spawns the four sides (hidden until the camera comes near) and warms the
/// curtain's pipeline behind the loading screen.
pub(super) fn spawn_barrier(
    mut commands: Commands,
    mut warmup: Warmup,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<BarrierMaterial>>,
) {
    let mesh = meshes.add(side_mesh());
    let material = materials.add(BarrierMaterial::default());
    warmup.add(mesh.clone(), material.clone());
    for (k, outward) in [Vec3::NEG_Z, Vec3::X, Vec3::Z, Vec3::NEG_X]
        .into_iter()
        .enumerate()
    {
        commands.spawn((
            Name::new("Island barrier"),
            BarrierSide { outward },
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_rotation(Quat::from_rotation_y(
                -std::f32::consts::FRAC_PI_2 * k as f32,
            )),
            Visibility::Hidden,
        ));
    }
}

/// Draws only the sides the camera is close enough to see.
pub(super) fn show_barrier_near_camera(
    camera: Option<Single<&GlobalTransform, With<MainCamera>>>,
    mut sides: Query<(&BarrierSide, &mut Visibility)>,
) {
    let eye = camera.map(|c| c.translation());
    for (side, mut visibility) in &mut sides {
        let near = eye.is_some_and(|eye| side.distance(eye) < REVEAL_END + 0.25);
        visibility.set_if_neq(if near {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curtain_shows_only_near_the_eye() {
        assert_eq!(barrier_reveal(0.5), 1.0);
        assert_eq!(barrier_reveal(REVEAL_FULL), 1.0);
        let mid = barrier_reveal(3.0);
        assert!(mid > 0.1 && mid < 0.9, "{mid}");
        assert_eq!(barrier_reveal(REVEAL_END), 0.0);
        assert_eq!(barrier_reveal(20.0), 0.0);
    }

    #[test]
    fn the_sides_stand_on_the_arena_edge_facing_in() {
        let mesh = side_mesh();
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions");
        };
        for k in 0..4 {
            let rot = Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2 * k as f32);
            let outward = rot * Vec3::NEG_Z;
            let side = BarrierSide { outward };
            for v in p {
                let v = rot * Vec3::from_array(*v);
                assert!(side.distance(v) < 1e-4, "on the edge: {v}");
                assert!((-0.31..=BARRIER_HEIGHT + 1e-4).contains(&v.y), "{v}");
            }
            // The quad's front faces into the arena.
            let [a, b, c] = [p[0], p[1], p[2]].map(|v| rot * Vec3::from_array(v));
            assert!((b - a).cross(c - a).dot(-outward) > 0.0);
        }
    }

    #[test]
    fn only_nearby_sides_are_drawn() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_systems(Update, show_barrier_near_camera);
        let camera = app
            .world_mut()
            .spawn((MainCamera, GlobalTransform::from_xyz(0.0, 1.6, 0.0)))
            .id();
        let sides: Vec<Entity> = [Vec3::NEG_Z, Vec3::X, Vec3::Z, Vec3::NEG_X]
            .into_iter()
            .map(|outward| {
                app.world_mut()
                    .spawn((BarrierSide { outward }, Visibility::Inherited))
                    .id()
            })
            .collect();
        app.update();
        let shown = |app: &App| -> Vec<bool> {
            sides
                .iter()
                .map(|&e| app.world().get::<Visibility>(e) != Some(&Visibility::Hidden))
                .collect()
        };
        assert_eq!(shown(&app), [false; 4], "mid-arena: no curtain");
        // At the east edge, only the east side.
        *app.world_mut().get_mut::<GlobalTransform>(camera).unwrap() =
            GlobalTransform::from_xyz(ARENA_HALF - 1.0, 1.6, 3.0);
        app.update();
        assert_eq!(shown(&app), [false, true, false, false]);
        // In the north-west corner, two sides.
        *app.world_mut().get_mut::<GlobalTransform>(camera).unwrap() =
            GlobalTransform::from_xyz(-ARENA_HALF + 2.0, 1.6, -ARENA_HALF + 2.0);
        app.update();
        assert_eq!(shown(&app), [true, false, false, true]);
    }
}
