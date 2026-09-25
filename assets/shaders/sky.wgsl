// Pieced sky dome: zenith → horizon gradient, sun glow and an anti-aliased sun
// disc. At and below the horizon it returns the distance-fog color exactly (same
// view bindings the PBR fog reads), so distant terrain melts into the sky.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::{view, lights, fog},
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
    // x: cos(sun disc radius), y: glow exponent, z: glow strength, w: gradient exponent
    params: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> sky: Sky;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dir = normalize(in.world_position.xyz - view.world_position.xyz);

    // The sun is the shadow-casting directional light (a skylight fill may exist).
    var sun_dir = vec3<f32>(0.0, 1.0, 0.0);
    var sun_light = vec3<f32>(0.0);
    for (var i = 0u; i < lights.n_directional_lights; i = i + 1u) {
        let light = lights.directional_lights[i];
        if ((light.flags & 1u) != 0u) {
            sun_dir = light.direction_to_light;
            sun_light = light.color.rgb * view.exposure;
        }
    }
    let cos_sun = dot(dir, sun_dir);

    // Three-stop gradient: gold horizon, pale clear blue, deep blue overhead.
    let t = pow(clamp(dir.y, 0.0, 1.0), sky.params.w);
    var color = mix(sky.horizon.rgb, sky.mid.rgb, smoothstep(0.0, 0.35, t));
    color = mix(color, sky.zenith.rgb, smoothstep(0.3, 1.0, t));

    // Warm glow around the sun, strongest low in the sky.
    color += sky.sun.rgb * pow(max(cos_sun, 0.0), sky.params.y) * sky.params.z;

    // Horizon haze: blend to the exact fog color at and below the horizon.
    var fog_color = sky.horizon.rgb;
#ifdef DISTANCE_FOG
    fog_color = fog.base_color.rgb;
    if (fog.directional_light_color.a > 0.0) {
        let scattering = pow(max(cos_sun, 0.0), fog.directional_light_exponent) * sun_light;
        fog_color += scattering * fog.directional_light_color.rgb * fog.directional_light_color.a;
    }
#endif
    let haze = 1.0 - smoothstep(-0.015, 0.16, dir.y);
    color = mix(color, fog_color, haze);

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
