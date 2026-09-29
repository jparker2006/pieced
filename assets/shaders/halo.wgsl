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
// Must match HALO_NEAR_FADE in halo.rs.
const NEAR_FADE: vec2<f32> = vec2<f32>(0.42, 0.8);
// Halos smaller than this (m) never fade: the guns' own glows.
const NEAR_FADE_MIN_SIZE: f32 = 0.6;

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

    // A halo that would swell over much of the screen (an orb's fire a metre
    // from the eye) fades out instead of washing the whole view: by its
    // size over its distance, from NEAR_FADE.x to NEAR_FADE.y. The guns'
    // own glows (a chamber crystal, a muzzle flash) stay well under it.
    let reach = size / max(eye_distance, 1e-3);
    let near = select(1.0 - smoothstep(NEAR_FADE.x, NEAR_FADE.y, reach), 1.0,
        size < NEAR_FADE_MIN_SIZE);

    var out: VertexOutput;
    out.position = position_world_to_clip(world);
    out.uv = v.uv;
    out.glow = srgb_to_linear(srgb) * intensity * near;
    // A dark or zero-intensity halo adds nothing: collapse its quad to a
    // point so it costs no fragments (additive overdraw is the halo cost).
    if max(max(out.glow.r, out.glow.g), out.glow.b) < 1e-3 {
        out.position = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let mask = textureSample(halo_texture, halo_sampler, in.uv).r;
    return vec4<f32>(in.glow * mask, 0.0);
}
