// Pieced waterfalls (src/far/waterfall.rs): a solid, bright water body with
// soft highlights scrolling down it, on a unit strip (x -0.5..0.5 across,
// y 0 at the lip to -1 at the bottom) that the model's node scales to its real
// width and length. The water is nearly opaque, feathers at its edges, foams at
// the lip, dissolves over its lower half (into the mist halo) and hazes toward
// the far layer's haze color with distance. Output is premultiplied alpha.

#import bevy_pbr::{
    mesh_functions::{get_world_from_local, mesh_position_local_to_world},
    mesh_view_bindings::{view, globals},
    view_transformations::position_world_to_clip,
}

#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping::tone_mapping
#endif

struct Waterfall {
    // rgb: water, a: opacity.
    body: vec4<f32>,
    highlight: vec4<f32>,
    // x: fall speed m/s, y: highlight band length m, z: stripe width m, w: seed.
    flow: vec4<f32>,
    // x: haze start m, y: haze density per m, z: haze amount.
    haze: vec4<f32>,
    haze_color: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> fall: Waterfall;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec3<f32>,
    // x: 0..1 across, y: 0 (lip) .. 1 (bottom).
    @location(1) strip: vec2<f32>,
    // x: width m, y: length m.
    @location(2) size: vec2<f32>,
}

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    let m = get_world_from_local(v.instance_index);
    let world = mesh_position_local_to_world(m, vec4<f32>(v.position, 1.0));
    var out: VertexOutput;
    out.position = position_world_to_clip(world.xyz);
    out.world = world.xyz;
    out.strip = vec2<f32>(v.position.x + 0.5, -v.position.y);
    out.size = vec2<f32>(length(m[0].xyz), length(m[1].xyz));
    return out;
}

const TAU: f32 = 6.2831853;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let across = clamp(in.strip.x, 0.0, 1.0);
    let down = clamp(in.strip.y, 0.0, 1.0);
    let width = max(in.size.x, 0.1);
    let len = max(in.size.y, 0.1);
    let xm = across * width;
    let metres = down * len;

    // Soft vertical stripes of lighter and deeper water.
    let wave = sin(xm / fall.flow.z * 1.7 + fall.flow.w) + 0.6 * sin(xm / fall.flow.z * 0.63 + 1.3);
    let stripe = 0.5 + 0.25 * wave;
    // Long highlights scrolling down, their phase drifting across the width so
    // they ripple rather than march in rows.
    let phase = 0.14 * sin(xm * 0.41 + fall.flow.w) + 0.06 * sin(xm * 1.13);
    let t = (metres - globals.time * fall.flow.x) / fall.flow.y + phase;
    let band = 0.5 + 0.5 * sin(t * TAU);
    let highlight = band * band * band * (0.45 + 0.55 * stripe);

    let foam = 1.0 - smoothstep(0.0, 0.07, down);
    var rgb = fall.body.rgb * (0.82 + 0.3 * stripe)
        + fall.highlight.rgb * (highlight * 0.55 + foam * 0.6);

    let edges = smoothstep(0.0, 0.12, across) * smoothstep(0.0, 0.12, 1.0 - across);
    let tail = 1.0 - smoothstep(0.45, 1.0, down);
    let alpha = fall.body.a * edges * tail;

    let d = max(distance(in.world, view.world_position) - fall.haze.x, 0.0) * fall.haze.y;
    let h = (1.0 - exp(-d * d)) * fall.haze.z;
    rgb = mix(rgb, fall.haze_color.rgb, h);

    var out = vec4<f32>(rgb * alpha, alpha);
#ifdef TONEMAP_IN_SHADER
    out = tone_mapping(out, view.color_grading);
#endif
    return out;
}
