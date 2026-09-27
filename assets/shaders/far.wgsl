// Pieced far-layer material (src/look/far.rs): unlit albedo (base × vertex
// color), darkened by baked AO on opaque models, plus emissive, hazed toward
// the global haze color with distance:
// haze = 1 - exp(-((d - start) × density)²), scaled per material. The model's
// colour takes the same grade as the toon surfaces first (not additive glows).
// `FarHaze::amount` mirrors the haze; keep them in step.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::view,
}
#import pieced::grade::grade

#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping::tone_mapping
#endif

// What a fully occluded far surface is multiplied by (violet, like the toon AO).
const FAR_AO_TINT: vec3<f32> = vec3<f32>(0.34, 0.27, 0.52);

struct Far {
    base_color: vec4<f32>,
    // rgb: emissive; w: AO strength (0 = vertex alpha is opacity).
    emissive: vec4<f32>,
    // rgb: haze color; w: this material's haze amount.
    haze_color: vec4<f32>,
    // x: start m, y: density per m, z: grade amount.
    haze: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> far: Far;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var albedo = far.base_color.rgb;
    var vertex_a = 1.0;
#ifdef VERTEX_COLORS
    albedo = albedo * in.color.rgb;
    vertex_a = in.color.a;
#endif
    let ao_k = far.emissive.w;
    // Opaque: vertex alpha is AO. Otherwise it is opacity, as before.
    let alpha = far.base_color.a * select(vertex_a, 1.0, ao_k > 0.0);
    let open = 1.0 - ao_k * (1.0 - saturate(vertex_a));
    albedo = albedo * mix(FAR_AO_TINT, vec3<f32>(1.0), open);

    let d = max(distance(in.world_position.xyz, view.world_position) - far.haze.x, 0.0) * far.haze.y;
    let h = (1.0 - exp(-d * d)) * far.haze_color.w;
    // Graded before the haze, so fully hazed models still melt into the sky.
    var rgb = albedo + far.emissive.rgb;
    rgb = mix(rgb, grade(rgb), far.haze.z);
    rgb = mix(rgb, far.haze_color.rgb, h);
    var out = vec4<f32>(rgb, alpha);
#ifdef TONEMAP_IN_SHADER
    out = tone_mapping(out, view.color_grading);
#endif
    return out;
}
