// Pieced waterfalls (src/far/waterfall.rs): additive streaks scrolling down a
// unit strip (x -0.5..0.5 across, y 0 at the lip to -1 at the bottom) that the
// model's node scales to its real width and length. The streaks move with the
// global time, fade in at the lip, out toward the bottom and at the edges, and
// fade with distance like the far layer's haze. Output is premultiplied with
// alpha 0, so it adds to whatever is behind it.

#import bevy_pbr::{
    mesh_functions::{get_world_from_local, mesh_position_local_to_world},
    mesh_view_bindings::{view, globals},
    view_transformations::position_world_to_clip,
}

#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping::tone_mapping
#endif

struct Waterfall {
    body: vec4<f32>,
    streak: vec4<f32>,
    // x: fall speed m/s, y: column width m, z: dash length m, w: seed.
    flow: vec4<f32>,
    // x: haze start m, y: haze density per m, z: haze amount.
    haze: vec4<f32>,
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

fn hash11(x: f32) -> f32 {
    return fract(sin(x * 127.1 + 311.7) * 43758.5453);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let across = clamp(in.strip.x, 0.0, 1.0);
    let down = clamp(in.strip.y, 0.0, 1.0);
    let width = max(in.size.x, 0.1);
    let len = max(in.size.y, 0.1);

    // Streak columns, each with its own speed and phase.
    let columns = max(round(width / fall.flow.y), 3.0);
    let col = floor(across * columns);
    let r = hash11(col + fall.flow.w);
    let r2 = hash11(col * 1.37 + 5.0 + fall.flow.w);
    let metres = down * len;
    let speed = fall.flow.x * (0.8 + 0.45 * r);
    let p = (metres - globals.time * speed) / (fall.flow.z * (0.7 + 0.6 * r2)) + r * 13.0;
    let dash = fract(p);
    let streak = smoothstep(0.0, 0.18, dash) * (1.0 - smoothstep(0.35, 0.75, dash));
    // Thin columns: bright in the middle of each.
    let in_col = fract(across * columns);
    let column = 1.0 - abs(in_col - 0.5) * 1.6;

    let edges = smoothstep(0.0, 0.16, across) * smoothstep(0.0, 0.16, 1.0 - across);
    let lip = 0.55 + 0.45 * smoothstep(0.0, 0.05, down);
    let tail = 1.0 - smoothstep(0.5, 1.0, down);
    let foam = 1.0 - smoothstep(0.0, 0.08, down);

    var rgb = fall.body.rgb * (0.75 + 0.25 * column)
        + fall.streak.rgb * streak * column * 0.9
        + fall.streak.rgb * foam * 0.5;
    rgb = rgb * edges * lip * tail;

    let d = max(distance(in.world, view.world_position) - fall.haze.x, 0.0) * fall.haze.y;
    let h = (1.0 - exp(-d * d)) * fall.haze.z;
    rgb = rgb * (1.0 - h);

    var out = vec4<f32>(rgb, 0.0);
#ifdef TONEMAP_IN_SHADER
    out = tone_mapping(out, view.color_grading);
#endif
    return out;
}
