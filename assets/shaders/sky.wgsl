// Pieced sky dome: zenith → horizon gradient, sun glow and an anti-aliased sun
// disc toward the toon key light. At and below the horizon it returns the far
// layer's haze color exactly, so distant terrain melts into the sky.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::view,
}

#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping::{tone_mapping, screen_space_dither}
#import bevy_render::maths::powsafe
#endif

struct Sky {
    zenith: vec4<f32>,
    mid: vec4<f32>,
    horizon: vec4<f32>,
    sun: vec4<f32>,
    haze: vec4<f32>,
    // xyz: unit vector toward the sun.
    sun_direction: vec4<f32>,
    // x: cos(sun disc radius), y: glow exponent, z: glow strength, w: gradient exponent
    params: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> sky: Sky;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dir = normalize(in.world_position.xyz - view.world_position.xyz);
    let cos_sun = dot(dir, sky.sun_direction.xyz);

    // Three-stop gradient: gold horizon, pale clear blue, deep blue overhead.
    let t = pow(clamp(dir.y, 0.0, 1.0), sky.params.w);
    var color = mix(sky.horizon.rgb, sky.mid.rgb, smoothstep(0.0, 0.35, t));
    color = mix(color, sky.zenith.rgb, smoothstep(0.3, 1.0, t));

    // Warm glow around the sun.
    color += sky.sun.rgb * pow(max(cos_sun, 0.0), sky.params.y) * sky.params.z;

    // Horizon haze: blend to the exact far-layer haze at and below the horizon.
    let haze = 1.0 - smoothstep(-0.015, 0.16, dir.y);
    color = mix(color, sky.haze.rgb, haze);

    // Sun disc with a pixel-wide soft edge (MSAA doesn't smooth shader edges).
    let edge = max(fwidth(cos_sun), 1e-6);
    let disc = smoothstep(sky.params.x - edge, sky.params.x + edge, cos_sun);
    color = mix(color, sky.sun.rgb, disc);

    var out = vec4<f32>(color, 1.0);
#ifdef TONEMAP_IN_SHADER
    out = tone_mapping(out, view.color_grading);
#ifdef DEBAND_DITHER
    var rgb = powsafe(out.rgb, 1.0 / 2.2);
    rgb += screen_space_dither(in.position.xy);
    out = vec4<f32>(powsafe(rgb, 2.2), out.a);
#endif
#endif
    return out;
}
