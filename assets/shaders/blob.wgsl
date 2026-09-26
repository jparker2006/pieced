// Pieced blob shadow decal (src/look/blob.rs): a soft disc multiplied onto
// the ground. Multiply blending computes dst × src + (1 - src.a) × dst, so
// returning (tint × a, a) darkens the ground toward `tint` by `a`. Per-decal
// opacity (0..255) comes from the MeshTag.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_functions::get_tag,
}

struct Blob {
    color: vec4<f32>,
    // x: radius where the soft edge starts (0..1), y: overall strength.
    params: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> blob: Blob;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var r = 1.0;
#ifdef VERTEX_UVS_A
    r = length(in.uv * 2.0 - 1.0);
#endif
    let mask = 1.0 - smoothstep(blob.params.x, 1.0, r);
    let opacity = f32(get_tag(in.instance_index) & 0xFFu) / 255.0;
    let a = mask * opacity * blob.params.y;
    return vec4<f32>(blob.color.rgb * a, a);
}
