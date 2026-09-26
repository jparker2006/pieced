// Pieced far-layer material (src/look/far.rs): unlit albedo (base × vertex
// color) plus emissive, hazed toward the global haze color with distance:
// haze = 1 - exp(-((d - start) × density)²), scaled per material.
// `FarHaze::amount` mirrors this; keep them in step.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::view,
}

#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping::tone_mapping
#endif

struct Far {
    base_color: vec4<f32>,
    emissive: vec4<f32>,
    // rgb: haze color; w: this material's haze amount.
    haze_color: vec4<f32>,
    // x: start m, y: density per m.
    haze: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> far: Far;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var albedo = far.base_color;
#ifdef VERTEX_COLORS
    albedo = albedo * in.color;
#endif
    let d = max(distance(in.world_position.xyz, view.world_position) - far.haze.x, 0.0) * far.haze.y;
    let h = (1.0 - exp(-d * d)) * far.haze_color.w;
    let rgb = mix(albedo.rgb + far.emissive.rgb, far.haze_color.rgb, h);
    var out = vec4<f32>(rgb, albedo.a);
#ifdef TONEMAP_IN_SHADER
    out = tone_mapping(out, view.color_grading);
#endif
    return out;
}
