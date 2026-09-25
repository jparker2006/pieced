// Pieced target material: StandardMaterial lighting plus a fresnel rim light and a
// small self-light lift, so the dummy pops against any background (and in
// greyscale) without dark outlines.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}
#endif

struct TargetRim {
    color: vec4<f32>,
    // x: rim exponent, y: rim strength, z: self-light lift, w: unused
    params: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> rim: TargetRim;

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    var color = apply_pbr_lighting(pbr_input);
    let n_dot_v = saturate(dot(pbr_input.N, pbr_input.V));
    let fresnel = pow(1.0 - n_dot_v, rim.params.x) * rim.params.y;
    let lifted = color.rgb
        + pbr_input.material.base_color.rgb * rim.params.z
        + rim.color.rgb * fresnel;
    out.color = main_pass_post_lighting_processing(pbr_input, vec4<f32>(lifted, color.a));
#endif
    return out;
}
