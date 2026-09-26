// Pieced halo billboards (src/look/halo.rs): a camera-facing quad centered on
// the entity, sized by its world scale, pulled toward the camera by half its
// size so it doesn't clip into the thing it glows around. Color and intensity
// come packed in the MeshTag (sRGB bytes + intensity byte), so every halo
// shares one mesh and one material. Additive: the output is premultiplied with
// alpha 0.

#import bevy_pbr::{
    mesh_functions::{get_world_from_local, get_tag},
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
}

// Must match HALO_MAX_INTENSITY in halo.rs.
const MAX_INTENSITY: f32 = 8.0;

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var halo_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var halo_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // rgb: linear color × intensity.
    @location(1) glow: vec3<f32>,
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    let m = get_world_from_local(v.instance_index);
    let center = m[3].xyz;
    let size = length(m[0].xyz);
    let right = view.world_from_view[0].xyz;
    let up = view.world_from_view[1].xyz;
    let to_eye = view.world_position - center;
    let eye_distance = length(to_eye);
    let pull = min(size * 0.5, max(eye_distance - 0.05, 0.0));
    let world = center + to_eye / max(eye_distance, 1e-4) * pull
        + (right * v.position.x + up * v.position.y) * size;

    let tag = get_tag(v.instance_index);
    let srgb = vec3<f32>(
        f32(tag & 0xFFu),
        f32((tag >> 8u) & 0xFFu),
        f32((tag >> 16u) & 0xFFu),
    ) / 255.0;
    let intensity = f32((tag >> 24u) & 0xFFu) / 255.0 * MAX_INTENSITY;

    var out: VertexOutput;
    out.position = position_world_to_clip(world);
    out.uv = v.uv;
    out.glow = srgb_to_linear(srgb) * intensity;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let mask = textureSample(halo_texture, halo_sampler, in.uv).r;
    return vec4<f32>(in.glow * mask, 0.0);
}
