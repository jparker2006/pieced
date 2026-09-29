// Pieced far backdrop (src/perf/far_res.rs, `farres=half`): one full-screen
// triangle at infinite depth (0 in Bevy's reverse-Z) in the world camera's
// opaque pass. It shows the far camera's half-resolution image at each pixel's
// screen position; anything the world camera draws is nearer and hides it.

#import bevy_pbr::mesh_view_bindings::view

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var far_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var far_sampler: sampler;

struct Vertex {
    @location(0) position: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
}

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(v.position.xy, 0.0, 1.0);
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = (in.position.xy - view.viewport.xy) / view.viewport.zw;
    return vec4<f32>(textureSample(far_texture, far_sampler, uv).rgb, 1.0);
}
