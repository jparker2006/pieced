//! Waterfalls: a solid, bright water body with scrolling highlights for the
//! `Waterfall*` strips of the far models (`assets/shaders/waterfall.wgsl`).
//!
//! Each strip is a unit mesh (x -0.5..0.5 across, y from 0 at the lip down to
//! -1) scaled to size by its glTF node, so the shader reads strip coordinates
//! from the vertex position and the strip's width and length from the model
//! matrix. The highlights scroll with the shader's global time: no per-frame
//! uploads. The water is nearly opaque, feathers at its edges, dissolves toward
//! the bottom (into the mist halo) and hazes like the far layer.

use crate::look::FarHaze;
use bevy::{
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};

pub const WATERFALL_SHADER_PATH: &str = "embedded://pieced/shaders/waterfall.wgsl";

/// Scrolling waterfall water (premultiplied alpha).
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, PartialEq)]
pub struct WaterfallMaterial {
    /// Linear rgb of the water body; alpha is its opacity.
    #[uniform(0)]
    pub body: LinearRgba,
    /// Linear rgb of the scrolling highlights.
    #[uniform(0)]
    pub highlight: LinearRgba,
    /// x: fall speed (m/s), y: highlight band length (m), z: stripe width
    /// across the fall (m), w: pattern seed.
    #[uniform(0)]
    pub flow: Vec4,
    /// x: haze start (m), y: haze density (per m), z: how much haze this
    /// material takes (0..1). Kept in step with [`FarHaze`] by
    /// [`sync_waterfall_haze`].
    #[uniform(0)]
    pub haze: Vec4,
    /// Linear rgb of the far haze (synced like `haze`).
    #[uniform(0)]
    pub haze_color: LinearRgba,
}

impl WaterfallMaterial {
    pub fn new(body: Color, highlight: Color, haze: &FarHaze, haze_amount: f32) -> Self {
        let mut body = body.to_linear();
        body.alpha = 0.94;
        Self {
            body,
            highlight: highlight.to_linear(),
            flow: Vec4::new(20.0, 16.0, 3.0, 0.0),
            haze: Vec4::new(haze.start, haze.density, haze_amount, 0.0),
            haze_color: haze.color.to_linear(),
        }
    }
}

impl Material for WaterfallMaterial {
    fn vertex_shader() -> ShaderRef {
        WATERFALL_SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        WATERFALL_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
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
        descriptor.vertex.buffers = vec![
            layout
                .0
                .get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0)])?,
        ];
        // Seen from either side as the islands turn.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// Keeps every waterfall's haze on the global far haze.
pub fn sync_waterfall_haze(haze: Res<FarHaze>, mut materials: ResMut<Assets<WaterfallMaterial>>) {
    if !haze.is_changed() {
        return;
    }
    let color = haze.color.to_linear();
    let ids: Vec<_> = materials.ids().collect();
    for id in ids {
        if let Some(mut m) = materials.get_mut(id)
            && (m.haze.x != haze.start || m.haze.y != haze.density || m.haze_color != color)
        {
            m.haze.x = haze.start;
            m.haze.y = haze.density;
            m.haze_color = color;
        }
    }
}
