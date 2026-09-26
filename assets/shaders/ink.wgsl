// Pieced ink outline hull (src/look/outline.rs): the outlined mesh drawn again
// with front faces culled, each vertex pushed out in clip space along its
// position-averaged smooth normal by a constant number of pixels. The width is
// set at a reference target height, scales with the actual target, and fades
// to zero between the fade distances. `outline_width_px` and `ink_color` in
// outline.rs mirror this math; keep them in step.

#import bevy_pbr::{
    mesh_functions::{get_world_from_local, mesh_normal_local_to_world, mesh_position_local_to_world},
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
}

#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping::tone_mapping
#endif

struct Ink {
    // rgb: the ink (w = 1) or the base color to derive it from (w = 0).
    color: vec4<f32>,
    // x: width px at the reference height, y: fade start m, z: fade end m,
    // w: reference height px.
    params: vec4<f32>,
    // x: saturation kept, y: darkness, z: max luminance.
    derive: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> ink: Ink;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) outline_normal: vec3<f32>,
#ifdef INK_VERTEX_COLORS
    @location(5) color: vec4<f32>,
#endif
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn derived_ink(c: vec3<f32>) -> vec3<f32> {
    let desat = mix(vec3<f32>(luminance(c)), c, ink.derive.x);
    let k = min(ink.derive.y, ink.derive.z / max(luminance(desat), 1e-5));
    return desat * k;
}

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    let world_from_local = get_world_from_local(v.instance_index);
    let world = mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0));
    let n = normalize(mesh_normal_local_to_world(v.outline_normal, v.instance_index));

    var clip = position_world_to_clip(world.xyz);
    let n_clip = view.clip_from_world * vec4<f32>(n, 0.0);
    // Screen direction of the normal at this vertex (the derivative of the
    // projected position along n), in pixels.
    let dir = (n_clip.xy * clip.w - clip.xy * n_clip.w) * view.viewport.zw;
    let len = length(dir);

    let dist = distance(world.xyz, view.world_position);
    let fade = 1.0 - smoothstep(ink.params.y, ink.params.z, dist);
    let width_px = ink.params.x * (view.viewport.w / ink.params.w) * fade;
    if len > 1e-6 {
        let offset_ndc = dir / len * width_px * 2.0 / view.viewport.zw;
        clip = vec4<f32>(clip.xy + offset_ndc * clip.w, clip.zw);
    }

    var out: VertexOutput;
    out.position = clip;
    if ink.color.w > 0.5 {
        out.color = vec4<f32>(ink.color.rgb, 1.0);
    } else {
        var base = ink.color.rgb;
#ifdef INK_VERTEX_COLORS
        base = base * v.color.rgb;
#endif
        out.color = vec4<f32>(derived_ink(base), 1.0);
    }
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var out = in.color;
#ifdef TONEMAP_IN_SHADER
    out = tone_mapping(out, view.color_grading);
#endif
    return out;
}
