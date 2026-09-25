//! Stylized sky: a dome that rides on the main camera, shading a zenith → horizon
//! gradient, a warm glow and an anti-aliased sun disc. At and below the horizon it
//! returns exactly the distance-fog color (read from the same view bindings the
//! fog uses), so distant terrain melts into the sky without a seam.

use super::geo::Geo;
use bevy::{
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};

pub const SKY_SHADER_PATH: &str = "embedded://pieced/shaders/sky.wgsl";
/// Dome radius: inside the camera's far plane, outside all scenery that matters.
pub const SKY_RADIUS: f32 = 1000.0;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct SkyMaterial {
    #[uniform(0)]
    pub zenith: LinearRgba,
    #[uniform(0)]
    pub mid: LinearRgba,
    #[uniform(0)]
    pub horizon: LinearRgba,
    #[uniform(0)]
    pub sun: LinearRgba,
    /// x: cosine of the sun disc's angular radius, y: glow exponent,
    /// z: glow strength, w: gradient exponent (smaller = more blue overhead).
    #[uniform(0)]
    pub params: Vec4,
}

impl Material for SkyMaterial {
    fn fragment_shader() -> ShaderRef {
        SKY_SHADER_PATH.into()
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // The sky never occludes anything, so it needn't write depth.
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

/// Inward-facing UV sphere (triangles wind counter-clockwise seen from inside).
pub fn dome(radius: f32, sectors: usize, stacks: usize) -> Geo {
    let mut geo = Geo::default();
    let point = |i: usize, j: usize| {
        // Exact poles, so the pole triangles collapse cleanly and are skipped.
        if j == 0 || j == stacks {
            return Vec3::Y * if j == 0 { radius } else { -radius };
        }
        let theta = std::f32::consts::TAU * i as f32 / sectors as f32;
        let phi = std::f32::consts::PI * j as f32 / stacks as f32;
        Vec3::new(phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin()) * radius
    };
    for j in 0..stacks {
        for i in 0..sectors {
            let (a, b, c, d) = (
                point(i, j),
                point(i + 1, j),
                point(i + 1, j + 1),
                point(i, j + 1),
            );
            // a→b→c winds outward here; reverse it to face the center.
            geo.tri(a, c, b, [1.0; 4]);
            geo.tri(a, d, c, [1.0; 4]);
        }
    }
    geo
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dome_faces_inward() {
        let geo = dome(10.0, 16, 8);
        assert!(geo.tri_count() > 200);
        for (i, n) in geo.normals.chunks(3).enumerate() {
            let p = Vec3::from_array(geo.positions[i * 3]);
            assert!(p.dot(Vec3::from_array(n[0])) < 0.0, "face {i} points out");
        }
    }
}
