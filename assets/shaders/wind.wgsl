// Pieced wind sway (src/look/wind.rs, M4 chunk 6): the breeze the grass,
// flowers, bushes and tree crowns sway in. A vertex offset only: nothing the
// simulation reads moves. `sway_offset` in wind.rs mirrors this; keep them in
// step.

#define_import_path pieced::wind

const WIND_DIR: vec2<f32> = vec2<f32>(0.8, -0.6);
const GUST_SPEED: f32 = 7.0;

fn gust(t: f32) -> f32 {
    let s = 0.5
        + 0.26 * sin(0.23 * t)
        + 0.15 * sin(0.61 * t + 1.7)
        + 0.09 * sin(1.37 * t + 0.4);
    return clamp(s, 0.0, 1.0);
}

fn gust_at(t: f32, p: vec2<f32>) -> f32 {
    return gust(t - dot(p, WIND_DIR) / GUST_SPEED);
}

// The offset for a vertex whose phase comes from `anchor` (world), at
// `weight` (0 rooted .. 1 tip) with `amplitude` (m).
fn sway_offset(t: f32, anchor: vec3<f32>, weight: f32, amplitude: f32) -> vec3<f32> {
    let g = gust_at(t, anchor.xz);
    let phase = anchor.x * 0.73 + anchor.z * 1.19;
    let bend = amplitude * weight * (0.3 + 0.7 * g) * (0.65 + 0.35 * sin(1.9 * t + phase));
    let flutter = amplitude * weight * (0.15 + 0.25 * g) * sin(3.7 * t + phase * 1.7);
    let along = vec3<f32>(WIND_DIR.x, 0.0, WIND_DIR.y);
    let across = vec3<f32>(-WIND_DIR.y, 0.0, WIND_DIR.x);
    return along * bend + across * flutter - vec3<f32>(0.0, 0.2 * abs(bend) * weight, 0.0);
}

// A material's sway (x amplitude, y from, z to, w weighting: 0 off,
// 1 by model height, 2 baked in the normal's length) for a vertex at local
// `local` / world `world` of a mesh whose origin is `origin`.
fn wind_sway(
    params: vec4<f32>,
    t: f32,
    local: vec3<f32>,
    normal_length: f32,
    world: vec3<f32>,
    origin: vec3<f32>,
) -> vec3<f32> {
    if params.w < 0.5 || params.x <= 0.0 {
        return vec3<f32>(0.0);
    }
    if params.w < 1.5 {
        let x = clamp((local.y - params.y) / max(params.z - params.y, 1e-4), 0.0, 1.0);
        return sway_offset(t, origin, x * x * (3.0 - 2.0 * x), params.x);
    }
    let w = clamp(normal_length - 1.0, 0.0, 1.0);
    return sway_offset(t, world, w, params.x);
}
