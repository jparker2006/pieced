//! The spell glow material: unlit, additive, vertex-coloured, with a
//! per-entity tint and intensity packed into its `MeshTag` (the same packing as
//! the look's halos, [`pack_halo`]). Every glowing spell shape (bolt heads and
//! ribbons, sparks, starbursts, shield glass, sparkles, the muzzle bursts)
//! shares **one** material asset, so they all batch, and fading one is a
//! component write instead of another material. Shapes carry their colours as
//! `COLOR_0` (a white-hot core fading to blue tips, say), and the vertex alpha
//! scales the glow. See `assets/shaders/spell.wgsl`.

use crate::look::pack_halo;
use bevy::{
    mesh::{MeshTag, MeshVertexBufferLayoutRef},
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};

pub const SPELL_SHADER_PATH: &str = "embedded://pieced/shaders/spell.wgsl";
/// Where the shader is registered in the embedded asset source.
pub const SPELL_SHADER_EMBEDDED: &str = "pieced/shaders/spell.wgsl";

/// See the module docs. One asset is shared by every glowing spell shape.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, PartialEq)]
pub struct SpellMaterial {
    /// rgb: global gain on every glow (1 = as authored). w unused.
    #[uniform(0)]
    pub gain: Vec4,
}

impl Default for SpellMaterial {
    fn default() -> Self {
        Self { gain: Vec4::ONE }
    }
}

impl Material for SpellMaterial {
    fn vertex_shader() -> ShaderRef {
        SPELL_SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        SPELL_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
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
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(1),
        ])?];
        // Flat cards and shells, seen from either side.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// The `MeshTag` for a glow: `tint` (sRGB, multiplies the vertex colours) at
/// `intensity` (0..[`crate::look::HALO_MAX_INTENSITY`], 1/32 steps).
pub fn glow_tag(tint: Color, intensity: f32) -> MeshTag {
    MeshTag(pack_halo(tint, intensity))
}
