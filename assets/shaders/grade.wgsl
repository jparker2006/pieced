// Pieced colour grade (src/look/grade.rs): the targets' vivid, punchy look as a
// few ALU ops at the end of each lit shader, instead of a full-screen pass.
// Works in gamma-2 space (sqrt), which is close enough to perceptual for a
// grade: a vibrance lift (strongest on muted colours, so saturated palette
// colours never blow out; faded out in the darks), a contrast lift around a
// mid pivot, and a slight split tone (cool shadows, warm highlights). The
// cameras keep
// Tonemapping::None; call this on the final linear colour before
// `tone_mapping`. `grade` in grade.rs mirrors it; keep them in step.
//
// Use from any material shader:  #import pieced::grade::grade

#define_import_path pieced::grade

const GRADE_VIBRANCE: f32 = 0.3;
const GRADE_CONTRAST: f32 = 1.1;
const GRADE_PIVOT: f32 = 0.45;
const GRADE_COOL: vec3<f32> = vec3<f32>(-0.008, 0.0, 0.018);
const GRADE_WARM: vec3<f32> = vec3<f32>(0.024, 0.012, -0.02);

fn grade(linear_rgb: vec3<f32>) -> vec3<f32> {
    var p = sqrt(max(linear_rgb, vec3<f32>(0.0)));
    let l = dot(p, vec3<f32>(0.2126, 0.7152, 0.0722));
    let spread = max(max(p.r, p.g), p.b) - min(min(p.r, p.g), p.b);
    let vibrance = GRADE_VIBRANCE * saturate(1.0 - spread) * smoothstep(0.08, 0.45, l);
    p = vec3<f32>(l) + (p - vec3<f32>(l)) * (1.0 + vibrance);
    p = (p - vec3<f32>(GRADE_PIVOT)) * GRADE_CONTRAST + vec3<f32>(GRADE_PIVOT);
    p = p + mix(GRADE_COOL, GRADE_WARM, smoothstep(0.15, 0.85, l));
    p = clamp(p, vec3<f32>(0.0), vec3<f32>(1.0));
    return p * p;
}
