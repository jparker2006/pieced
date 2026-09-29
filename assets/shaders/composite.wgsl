// The frame's one full-screen pass at the window's resolution (src/render/composite.rs):
// the HUD, drawn at native Retina resolution over a transparent clear (so it holds
// premultiplied color), goes over the 3D image, which is stretched from its render size
// with bilinear filtering (as the old full-screen UI image node did). Both textures are
// sRGB views, so the blend happens in linear light, like the UI's own alpha blending.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var world_texture: texture_2d<f32>;
@group(0) @binding(1) var world_sampler: sampler;
@group(0) @binding(2) var ui_texture: texture_2d<f32>;

@fragment
fn fs_main(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let world = textureSampleLevel(world_texture, world_sampler, in.uv, 0.0).rgb;
    // One UI texel per output pixel: a plain load, no filtering.
    let ui = textureLoad(ui_texture, vec2<i32>(in.position.xy), 0);
    return vec4<f32>(ui.rgb + world * (1.0 - ui.a), 1.0);
}
