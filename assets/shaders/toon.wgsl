// Pieced toon material (src/look/toon.rs, M2 Amendment B): three soft cartoon
// tones against one key light (lit, mid, violet shadow), fill from the sky
// colours plus a teal fill from the galaxy side, baked AO from the vertex
// colour's alpha, a crisp cartoon highlight on shiny palette surfaces, a rim
// light, emissive, then the colour grade. No PBR light loops: every parameter
// comes from this material's own uniform, a copy of the global ToonLighting.
// `toon_shade_linear` in toon.rs mirrors this math on the CPU; keep them in step.

#import bevy_pbr::{
    forward_io::{Vertex, VertexOutput},
    mesh_bindings::mesh,
    mesh_functions,
    mesh_view_bindings::{globals, view},
    morph::{morph_position, morph_normal, morph_tangent},
    skinning,
    view_transformations::position_world_to_clip,
}
#import pieced::grade::grade
#import pieced::wind::sway

#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping::tone_mapping
#endif

// Keep in step with surfaces.rs (MAX_SURFACES, SURFACE_MATCH).
const MAX_SURFACES: u32 = 16u;
const SURFACE_MATCH: f32 = 0.002;

struct Toon {
    base_color: vec4<f32>,
    // rgb: emissive × strength; w: AO strength (global × material; 0 = none).
    emissive: vec4<f32>,
    // xyz: unit vector toward the key light; w: shadow/mid threshold (N·L).
    key_direction: vec4<f32>,
    // rgb: lit tone; w: tone edge half-width (N·L).
    key_color: vec4<f32>,
    // rgb: shadow tone.
    shadow_tint: vec4<f32>,
    // xyz: unit vector toward the fill light.
    fill_direction: vec4<f32>,
    fill_color: vec4<f32>,
    // rgb: rim color; w: rim strength.
    rim_color: vec4<f32>,
    // x: rim power, y: detail strength, zw: detail UV scale.
    params: vec4<f32>,
    // rgb: mid tone; w: surface table size.
    mid_tint: vec4<f32>,
    sky_fill: vec4<f32>,
    ground_fill: vec4<f32>,
    // rgb: full-occlusion multiplier.
    ao_tint: vec4<f32>,
    // rgb: highlight colour; w: highlight edge softness.
    highlight: vec4<f32>,
    // x: mid/lit threshold (N·L), y: in-tone gradient, z: highlight view bias.
    bands: vec4<f32>,
    // This material's own surface: specular, shininess, sheen, tint.
    surface: vec4<f32>,
    surface_keys: array<vec4<f32>, 16>,
    surface_params: array<vec4<f32>, 16>,
    // The wind sway (M4 chunk 6, wind.wgsl): x amplitude m, y/z the height
    // band, w the weighting (0 off, 1 model height, 2 baked).
    sway: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> toon: Toon;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var detail_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var detail_sampler: sampler;

// The palette surface an albedo is painted with, or this material's own.
fn surface_of(albedo: vec3<f32>) -> vec4<f32> {
    let count = min(u32(toon.mid_tint.w), MAX_SURFACES);
    for (var i = 0u; i < count; i = i + 1u) {
        let d = abs(albedo - toon.surface_keys[i].xyz);
        if max(max(d.x, d.y), d.z) < SURFACE_MATCH {
            return toon.surface_params[i];
        }
    }
    return toon.surface;
}

// Bevy 0.19's mesh vertex shader (bevy_pbr/src/render/mesh.wgsl), plus the
// wind sway (M4 chunk 6): a world-space offset for materials that sway, zero
// for every other. Only what's drawn moves; colliders and hitboxes never do.
#ifdef MORPH_TARGETS
fn morph_vertex(vertex_in: Vertex, instance_index: u32) -> Vertex {
    var vertex = vertex_in;
    let first_vertex = mesh[instance_index].first_vertex_index;
    let vertex_index = vertex.index - first_vertex;
    let weight_count = bevy_pbr::morph::layer_count(instance_index);
    for (var i: u32 = 0u; i < weight_count; i ++) {
        let weight = bevy_pbr::morph::weight_at(i, instance_index);
        if weight == 0.0 {
            continue;
        }
        vertex.position += weight * morph_position(vertex_index, i, instance_index);
#ifdef VERTEX_NORMALS
        vertex.normal += weight * morph_normal(vertex_index, i, instance_index);
#endif
#ifdef VERTEX_TANGENTS
        vertex.tangent += vec4(weight * morph_tangent(vertex_index, i, instance_index), 0.0);
#endif
    }
    return vertex;
}
#endif

@vertex
fn vertex(vertex_no_morph: Vertex) -> VertexOutput {
    var out: VertexOutput;
#ifdef MORPH_TARGETS
    var vertex = morph_vertex(vertex_no_morph, vertex_no_morph.instance_index);
#else
    var vertex = vertex_no_morph;
#endif
    let mesh_world_from_local = mesh_functions::get_world_from_local(vertex_no_morph.instance_index);
#ifdef SKINNED
    var world_from_local = skinning::skin_model(
        vertex.joint_indices,
        vertex.joint_weights,
        vertex_no_morph.instance_index
    );
#else
    var world_from_local = mesh_world_from_local;
#endif
    var normal_length = 1.0;
#ifdef VERTEX_NORMALS
    normal_length = length(vertex.normal);
#ifdef SKINNED
    out.world_normal = skinning::skin_normals(world_from_local, vertex.normal);
#else
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex_no_morph.instance_index
    );
#endif
#endif
#ifdef VERTEX_POSITIONS
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
    let offset = sway(
        toon.sway,
        globals.time,
        vertex.position,
        normal_length,
        out.world_position.xyz,
        mesh_world_from_local[3].xyz,
    );
    out.world_position = vec4<f32>(out.world_position.xyz + offset, out.world_position.w);
    out.position = position_world_to_clip(out.world_position.xyz);
#endif
#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex.tangent,
        vertex_no_morph.instance_index
    );
#endif
#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex_no_morph.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_no_morph.instance_index, mesh_world_from_local[3]);
#endif
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var albedo = toon.base_color.rgb;
    var alpha = toon.base_color.a;
    var ao = 1.0;
#ifdef VERTEX_COLORS
    albedo = albedo * in.color.rgb;
#ifdef TOON_VERTEX_OPACITY
    alpha = alpha * in.color.a;
#else
    ao = in.color.a;
#endif
#endif
    // Looked up before the detail texture, so grain never breaks the match.
    let surface = surface_of(albedo);
#ifdef TOON_DETAIL
#ifdef VERTEX_UVS_A
    let detail = textureSample(detail_texture, detail_sampler, in.uv * toon.params.zw);
    albedo = albedo * mix(vec3<f32>(1.0), detail.rgb, toon.params.y);
#endif
#endif

    let n = normalize(in.world_normal);
    let v = normalize(view.world_position - in.world_position.xyz);
    let key = toon.key_direction.xyz;

    // Three tones with soft edges, widened to at least a pixel's worth of N·L
    // change on curved surfaces so they never alias.
    let ndl = dot(n, key);
    let w = max(toon.key_color.w, fwidth(ndl));
    let to_mid = smoothstep(toon.key_direction.w - w, toon.key_direction.w + w, ndl);
    let to_lit = smoothstep(toon.bands.x - w, toon.bands.x + w, ndl);
    let hemi = mix(toon.ground_fill.rgb, toon.sky_fill.rgb, n.y * 0.5 + 0.5);
    let fill = toon.fill_color.rgb * max(dot(n, toon.fill_direction.xyz), 0.0);
    let shadow = toon.shadow_tint.rgb + fill + hemi;
    let mid = toon.mid_tint.rgb + (fill + hemi) * 0.5;
    let tone = mix(mix(shadow, mid, to_mid), toon.key_color.rgb, to_lit)
        * max(1.0 + toon.bands.y * (ndl - toon.bands.x), 0.0);

    // Baked AO darkens toward violet, never black.
    let open = 1.0 - toon.emissive.w * (1.0 - saturate(ao));
    var rgb = albedo * tone * mix(toon.ao_tint.rgb, vec3<f32>(1.0), open);

    // The cartoon highlight: a crisp blob where N·H^shininess passes 0.5, plus
    // a broad sheen; metals tint it toward their own hue. Lit side only.
    // Its light blends from the key toward one fixed to the camera (above,
    // a little behind and right: VIEW_HIGHLIGHT in toon.rs), so gleams sit on
    // top-front edges from any view.
    let rig = normalize(view.world_from_view[0].xyz * 0.3 + view.world_from_view[1].xyz * 0.9
        + view.world_from_view[2].xyz * 0.3);
    let h = normalize(normalize(mix(key, rig, toon.bands.z)) + v);
    let ndh = max(dot(n, h), 0.0);
    let sp = pow(ndh, max(surface.y, 1.0));
    let sw = max(toon.highlight.w, fwidth(sp));
    let blob = smoothstep(0.5 - sw, 0.5 + sw, sp);
    let broad = pow(ndh, max(surface.y * 0.2, 1.0));
    let hue = albedo / max(max(max(albedo.r, albedo.g), albedo.b), 1e-4);
    let highlight = mix(toon.highlight.rgb, hue, surface.w);
    rgb = rgb + highlight * surface.x * (blob + surface.z * broad) * to_mid * open;

    // Rim, weaker on the shadow side.
    let grazing = pow(1.0 - saturate(dot(n, v)), toon.params.x);
    rgb = rgb + toon.rim_color.rgb * grazing * toon.rim_color.w * (0.4 + 0.6 * to_mid) * open;

    rgb = rgb + toon.emissive.rgb;

    var out = vec4<f32>(rgb, alpha);
#ifdef TOON_ADDITIVE
    out = vec4<f32>(out.rgb * out.a, 0.0);
#else
    out = vec4<f32>(grade(out.rgb), out.a);
#endif
#ifdef TONEMAP_IN_SHADER
    out = tone_mapping(out, view.color_grading);
#endif
    return out;
}
