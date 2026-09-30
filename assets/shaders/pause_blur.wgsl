// The pause menu's blur (src/menu/blur.rs): one pass, run once when the game
// pauses, from the frozen 3D image into a quarter-size image. A 7×7 Gaussian of
// bilinear taps spread `params.z` source texels apart; the quarter-size result
// is stretched back over the screen (bilinear), which smooths it further.

#import bevy_ui::ui_vertex_output::UiVertexOutput

@group(1) @binding(0) var world_texture: texture_2d<f32>;
@group(1) @binding(1) var world_sampler: sampler;
// xy: one source texel in uv; z: tap spacing in texels; w: unused.
@group(1) @binding(2) var<uniform> params: vec4<f32>;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let step = params.xy * params.z;
    var sum = vec3<f32>(0.0);
    var weight = 0.0;
    for (var y = -3; y <= 3; y++) {
        for (var x = -3; x <= 3; x++) {
            let o = vec2<f32>(f32(x), f32(y));
            let w = exp(-dot(o, o) / 7.0);
            sum += textureSampleLevel(world_texture, world_sampler, in.uv + o * step, 0.0).rgb * w;
            weight += w;
        }
    }
    return vec4<f32>(sum / weight, 1.0);
}
