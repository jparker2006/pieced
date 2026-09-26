// Pieced island ground (src/look/ground.rs): toon-lit grass (the same two
// bands, violet shadow and teal fill as toon.wgsl, no rim) plus a faint
// glowing build grid drawn in world space. `grid_line_intensity` in ground.rs
// mirrors the line math on the CPU; keep them in step.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::view,
}

#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping::tone_mapping
#endif

struct Ground {
    base_color: vec4<f32>,
    // rgb: line colour; w: line strength.
    grid_color: vec4<f32>,
    // xyz: unit vector toward the key light; w: band threshold (N·L).
    key_direction: vec4<f32>,
    // rgb: key colour; w: band half-width.
    key_color: vec4<f32>,
    shadow_tint: vec4<f32>,
    fill_direction: vec4<f32>,
    fill_color: vec4<f32>,
    // x: cell, yz: origin, w: line half-width.
    grid: vec4<f32>,
    // xy: min, zw: max.
    bounds: vec4<f32>,
    // x: glow width, y: glow strength, z: fade start, w: fade end.
    params: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> ground: Ground;

// Distance (m) to the nearest line of pitch `cell` along one axis.
fn line_distance(coord: f32, cell: f32) -> f32 {
    let f = coord / cell;
    return abs(f - round(f)) * cell;
}

// Anti-aliased coverage of a line of half-width `hw` at distance `d`, for a
// pixel footprint `px` (m): lines thinner than a pixel dim instead of aliasing.
fn line_coverage(d: f32, hw: f32, px: f32) -> f32 {
    let w = max(hw, px);
    return (1.0 - smoothstep(w - px * 0.5, w + px * 0.5, d)) * (hw / w);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var albedo = ground.base_color.rgb;
#ifdef VERTEX_COLORS
    albedo = albedo * in.color.rgb;
#endif

    let n = normalize(in.world_normal);
    let ndl = dot(n, ground.key_direction.xyz);
    let w = max(ground.key_color.w, fwidth(ndl));
    let lit = smoothstep(ground.key_direction.w - w, ground.key_direction.w + w, ndl);
    let fill = max(dot(n, ground.fill_direction.xyz), 0.0);
    let shadow = albedo * (ground.shadow_tint.rgb + ground.fill_color.rgb * fill);
    let bright = albedo * ground.key_color.rgb;
    var rgb = mix(shadow, bright, lit);

    // The build grid, in world space.
    let world = in.world_position.xz;
    let hw = ground.grid.w;
    let inside = step(ground.bounds.x - hw, world.x) * step(world.x, ground.bounds.z + hw)
        * step(ground.bounds.y - hw, world.y) * step(world.y, ground.bounds.w + hw);
    if (ground.grid_color.w > 0.0 && inside > 0.0) {
        let p = world - ground.grid.yz;
        let dx = line_distance(p.x, ground.grid.x);
        let dz = line_distance(p.y, ground.grid.x);
        let px = max(fwidth(world), vec2<f32>(1e-5));
        let core = max(line_coverage(dx, hw, px.x), line_coverage(dz, hw, px.y));
        // A soft glow around the core, which also fades out once it would be
        // narrower than a couple of pixels.
        let gw = ground.params.x;
        let gx = exp(-dx / gw) * clamp(gw / (px.x * 3.0), 0.0, 1.0);
        let gz = exp(-dz / gw) * clamp(gw / (px.y * 3.0), 0.0, 1.0);
        let glow = max(gx, gz);
        let dist = length(view.world_position.xz - world);
        let fade = 1.0 - smoothstep(ground.params.z, ground.params.w, dist);
        let line = ground.grid_color.rgb;
        rgb = mix(rgb, line, clamp(core * ground.grid_color.w * fade, 0.0, 1.0));
        rgb = rgb + line * glow * ground.params.y * fade;
    }

    var out = vec4<f32>(rgb, 1.0);
#ifdef TONEMAP_IN_SHADER
    out = tone_mapping(out, view.color_grading);
#endif
    return out;
}
