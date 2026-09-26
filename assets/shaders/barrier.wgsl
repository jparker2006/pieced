// Pieced island barrier (src/arena/visuals/barrier.rs): a shimmering,
// translucent rune curtain standing on the arena's edge (target T11). It is
// invisible from afar, fades in within a few metres of the camera (the
// player's eye) and brightens where the player touches it. Alpha-blended, so
// it tints bright and dark skies alike.
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
    let px = max(fwidth(u), 1e-4);

    // A translucent sheet with soft vertical light bands drifting sideways.
    let ripple = sin(v * 0.9 - time * 1.2) * 0.35;
    let bands = 0.5 + 0.5 * sin(u * 1.9 + ripple * 2.0 + time * 0.5);
    let sheet = 0.45 + 0.55 * bands * bands;

    // Thin bright wavy flow lines running up the curtain.
    let pitch = 1.7;
    let wave = u + ripple + 0.25 * sin(v * 1.7 + time * 0.8 + floor(u / pitch) * 2.1);
    let dl = abs(fract(wave / pitch) - 0.5) * pitch;
    let flow = 1.0 - smoothstep(0.012, 0.012 + px * 1.5, dl);

    // Small runes drifting slowly upward, each flickering on its own beat.
    let cell = barrier.touch.z;
    let q = vec2<f32>(u / cell, (v - time * 0.2) / (cell * 1.4));
    let id = floor(q);
    let f = (fract(q) - 0.5) * vec2<f32>(1.0, 1.4);
    let h = hash(id);
    let gpx = max(fwidth(q.x), 1e-4);
    let glyph = 1.0 - smoothstep(0.035, 0.035 + gpx * 1.5, rune(f * 1.3, h));
    let flicker = 0.5 + 0.5 * sin(time * 2.3 + h * 40.0);
    let show = step(0.45, fract(h * 7.13));
    let runes = glyph * flicker * show;

    // Brightest at the foot, fading out toward the top.
    let height = barrier.reveal.z;
    let fade_up = 1.0 - smoothstep(height * 0.3, height, v);
    let foot = exp(-max(v, 0.0) / 0.16) * 0.55;

    // Where the player touches it: rippling rings around the nearest point.
    let reach = barrier.touch.x;
    let touch = exp(-(d * d) / (reach * reach)) * barrier.touch.y;
    let rings = 0.55 + 0.45 * sin(d * 11.0 - time * 5.0);

    // A faint blue sheet, bright cyan lines and runes, a glowing foot, and
    // rippling light where it is touched.
    let line = max(flow, runes * barrier.rune_color.w);
    let lit = touch * rings;
    let alpha = clamp(
        (barrier.color.w * sheet + 0.6 * line + foot * 0.6) * fade_up + lit * 0.6,
        0.0,
        0.92,
    ) * reveal;
    var rgb = mix(barrier.rune_color.rgb, barrier.color.rgb * 1.25, clamp(line + foot + lit, 0.0, 1.0));
    rgb = rgb + barrier.color.rgb * lit * 0.5;

    var out = vec4<f32>(rgb, alpha);
#ifdef TONEMAP_IN_SHADER
    out = tone_mapping(out, view.color_grading);
#endif
    return out;
}
