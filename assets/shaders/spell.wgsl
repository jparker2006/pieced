// Pieced spell glow (src/fx/material.rs): unlit and additive. The shape's
// vertex colours (linear rgb, alpha = glow weight) are multiplied by a
// per-entity tint and intensity packed into the MeshTag (sRGB bytes +
// intensity byte, the same packing as the halos) and by the material's gain.
// One material serves every glowing spell shape, so they batch. Output is
// premultiplied with alpha 0, which the Add blend turns into pure addition.

#import bevy_pbr::{
    mesh_functions::{get_world_from_local, get_tag, mesh_position_local_to_world},
    view_transformations::position_world_to_clip,
}

// Must match HALO_MAX_INTENSITY in src/look/halo.rs (pack_halo).
const MAX_INTENSITY: f32 = 8.0;

struct Spell {
    // rgb: global gain.
    gain: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> spell: Spell;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    // Linear rgb, already weighted.
    @location(0) glow: vec3<f32>,
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    let m = get_world_from_local(v.instance_index);
    let world = mesh_position_local_to_world(m, vec4<f32>(v.position, 1.0));

    let tag = get_tag(v.instance_index);
    let tint = srgb_to_linear(vec3<f32>(
        f32(tag & 0xFFu),
        f32((tag >> 8u) & 0xFFu),
        f32((tag >> 16u) & 0xFFu),
    ) / 255.0);
    let intensity = f32((tag >> 24u) & 0xFFu) / 255.0 * MAX_INTENSITY;

    var out: VertexOutput;
    out.position = position_world_to_clip(world.xyz);
    out.glow = v.color.rgb * v.color.a * tint * intensity * spell.gain.rgb;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.glow, 0.0);
}
