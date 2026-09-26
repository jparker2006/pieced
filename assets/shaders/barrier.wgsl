// Pieced island barrier (src/arena/visuals/barrier.rs): a shimmering,
// translucent rune curtain standing on the arena's edge (target T11). It is
// invisible from afar, fades in within a few metres of the camera (the
// player's eye) and brightens where the player touches it. Additive.
// `barrier_reveal` in barrier.rs mirrors the fade on the CPU.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::{globals, view},
}

#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping::tone_mapping
#endif

struct Barrier {
    // rgb: curtain colour; w: curtain strength.
    color: vec4<f32>,
    // rgb: rune colour; w: rune strength.
    rune_color: vec4<f32>,
    // x: fully shown within (m), y: gone beyond (m), z: height (m), w: speed.
    reveal: vec4<f32>,
    // x: touch radius (m), y: touch strength, z: rune cell (m), w: unused.
    touch: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> barrier: Barrier;

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

// Distance to a rune glyph in a cell (f in -0.5..0.5): a diamond, a ring or a
// bar-and-dot, chosen by the cell's hash.
fn rune(f: vec2<f32>, h: f32) -> f32 {
    if (h < 0.4) {
        return abs(abs(f.x) * 1.5 + abs(f.y) - 0.3);
    } else if (h < 0.7) {
        return abs(length(f * vec2<f32>(1.0, 0.8)) - 0.2);
    }
    let bar = max(abs(f.x) - 0.035, abs(f.y) - 0.26);
    let spot = length(f - vec2<f32>(0.0, 0.34)) - 0.06;
    return max(min(bar, spot), 0.0);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.world_position.xyz;
    let eye = view.world_position;
    let d = distance(eye, p);
    let reveal = 1.0 - smoothstep(barrier.reveal.x, barrier.reveal.y, d);
    if (reveal <= 0.001) {
        discard;
    }
    let n = normalize(in.world_normal);
    // Along-the-wall and up coordinates (m).
    let t = normalize(vec2<f32>(-n.z, n.x));
    let u = dot(p.xz, t);
    let v = p.y;
    let time = globals.time * barrier.reveal.w;

    // A curtain of soft vertical light bands drifting sideways and rippling.
    let ripple = sin(v * 0.8 - time * 1.1) * 0.6;
    let bands = 0.5 + 0.5 * sin(u * 2.3 + ripple + time * 0.6);
    let fine = 0.5 + 0.5 * sin(u * 6.1 - time * 1.7 + v * 0.35);
    let curtain = 0.25 + 0.55 * bands * bands + 0.2 * fine;

    // Runes drifting slowly upward, each flickering on its own beat.
    let cell = barrier.touch.z;
    let q = vec2<f32>(u / cell, (v - time * 0.25) / (cell * 1.3));
    let id = floor(q);
    let f = fract(q) - 0.5;
    let h = hash(id);
    let px = max(fwidth(q.x), 1e-4);
    let glyph = 1.0 - smoothstep(0.02, 0.02 + px * 1.5, rune(f, h));
    let flicker = 0.55 + 0.45 * sin(time * 2.3 + h * 40.0);
    let show = step(0.35, fract(h * 7.13));
    let runes = glyph * flicker * show;

    // Bright at the foot, fading out toward the top.
    let height = barrier.reveal.z;
    let fade_up = 1.0 - smoothstep(height * 0.35, height, v);
    let foot = exp(-max(v, 0.0) / 0.35) * 0.9;

    // Where the player touches it: a bright ring of ripples around the
    // nearest point.
    let reach = barrier.touch.x;
    let touch = exp(-(d * d) / (reach * reach)) * barrier.touch.y;
    let rings = 0.6 + 0.4 * sin(d * 9.0 - time * 5.0);

    let strength = (barrier.color.w * curtain + foot) * fade_up + touch * rings;
    var rgb = barrier.color.rgb * strength + barrier.rune_color.rgb * runes * barrier.rune_color.w * (fade_up + touch);
    rgb = rgb * reveal;

    var out = vec4<f32>(rgb, 0.0);
#ifdef TONEMAP_IN_SHADER
    out = tone_mapping(out, view.color_grading);
#endif
    return out;
}
