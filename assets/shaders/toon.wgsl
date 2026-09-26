// Pieced toon material (src/look/toon.rs): two hard bands against one key
// light, a violet shadow band with a teal fill, a thin rim light and an
// emissive channel. No PBR light loops: every light parameter comes from this
// material's own uniform, a copy of the global ToonLighting resource.
// `toon_shade` in toon.rs mirrors this math on the CPU; keep them in step.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::view,
}

#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping::tone_mapping
#endif

struct Toon {
    base_color: vec4<f32>,
    // rgb: emissive × strength.
    emissive: vec4<f32>,
    // xyz: unit vector toward the key light; w: band threshold (N·L).
    key_direction: vec4<f32>,
    // rgb: key color; w: band half-width (anti-aliasing).
    key_color: vec4<f32>,
    shadow_tint: vec4<f32>,
    // xyz: unit vector toward the fill light.
    fill_direction: vec4<f32>,
    fill_color: vec4<f32>,
    // rgb: rim color; w: rim strength.
    rim_color: vec4<f32>,
    // x: rim power, y: detail strength, zw: detail UV scale.
    params: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> toon: Toon;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var detail_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var detail_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var albedo = toon.base_color;
#ifdef VERTEX_COLORS
    albedo = albedo * in.color;
#endif
#ifdef TOON_DETAIL
#ifdef VERTEX_UVS_A
    let detail = textureSample(detail_texture, detail_sampler, in.uv * toon.params.zw);
    albedo = vec4<f32>(albedo.rgb * mix(vec3<f32>(1.0), detail.rgb, toon.params.y), albedo.a);
#endif
#endif

    let n = normalize(in.world_normal);

    // Two bands. The edge is a tiny smoothstep, widened to at least a pixel's
    // worth of N·L change on curved surfaces so it never aliases.
    let ndl = dot(n, toon.key_direction.xyz);
    let w = max(toon.key_color.w, fwidth(ndl));
    let lit = smoothstep(toon.key_direction.w - w, toon.key_direction.w + w, ndl);

    // Shadow band: violet tint plus a teal fill from the other side. Never black.
    let fill = max(dot(n, toon.fill_direction.xyz), 0.0);
    let shadow = albedo.rgb * (toon.shadow_tint.rgb + toon.fill_color.rgb * fill);
    let bright = albedo.rgb * toon.key_color.rgb;
    var rgb = mix(shadow, bright, lit);

    // Thin rim, half strength on the shadow side.
    let to_eye = normalize(view.world_position - in.world_position.xyz);
    let grazing = pow(1.0 - saturate(dot(n, to_eye)), toon.params.x);
    rgb = rgb + toon.rim_color.rgb * grazing * toon.rim_color.w * (0.5 + 0.5 * lit);

    rgb = rgb + toon.emissive.rgb;

    var out = vec4<f32>(rgb, albedo.a);
#ifdef TOON_ADDITIVE
    out = vec4<f32>(out.rgb * out.a, 0.0);
#endif
#ifdef TONEMAP_IN_SHADER
    out = tone_mapping(out, view.color_grading);
#endif
    return out;
}
