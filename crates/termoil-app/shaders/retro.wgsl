// Retro: an old terminal monitor in the style of cool-retro-term. A gentle curve, soft scanlines,
// a faint glow that bleeds from the text, film noise and a slow flicker.

const CURVE: f32 = 0.03;
const GLOW: f32 = 0.35;

/// Reads the frame at a position.
fn retro_read(p: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(termoil_frame, termoil_sampler, p, 0.0).rgb;
}

/// A pseudo random number from 0 to 1 for a position.
fn retro_hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

/// The average of eight samples around a position: the light that bleeds from bright text.
fn retro_glow(p: vec2<f32>) -> vec3<f32> {
    let d = 2.5 / termoil.size;
    var sum = vec3<f32>(0.0);
    for (var i = 0; i < 8; i = i + 1) {
        let a = f32(i) * 0.785398;
        sum = sum + retro_read(p + vec2<f32>(cos(a), sin(a)) * d);
    }
    return sum / 8.0;
}

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let c = uv * 2.0 - 1.0;
    let p = (c * (1.0 + CURVE * dot(c, c))) * 0.5 + 0.5;
    if (p.x < 0.0 || p.x > 1.0 || p.y < 0.0 || p.y > 1.0) {
        return vec4<f32>(0.02, 0.02, 0.02, 1.0);
    }
    var rgb = retro_read(p) + retro_glow(p) * GLOW;
    let px = p * termoil.size;
    rgb = rgb * (0.85 + 0.15 * sin(px.y * 3.14159));
    let noise = retro_hash(px + fract(termoil.time) * 100.0) - 0.5;
    rgb = rgb + noise * 0.035;
    rgb = rgb * (0.97 + 0.03 * sin(termoil.time * 60.0));
    let edge = p * (1.0 - p);
    let vignette = clamp(pow(edge.x * edge.y * 14.0, 0.35), 0.0, 1.0);
    return vec4<f32>(rgb * vignette, color.a);
}
