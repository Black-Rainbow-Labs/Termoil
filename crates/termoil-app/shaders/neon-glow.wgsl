// Neon glow: the neon synthwave look first made as a shader file on the Mac. Text bloom, a drifting
// magenta to cyan wash, a pulsing cursor halo, soft scanlines and a vignette.

const GLOW_STRENGTH: f32 = 0.9;
const GLOW_RADIUS: f32 = 2.5;
const WASH_STRENGTH: f32 = 0.10;
const SCANLINES: f32 = 0.08;

/// Perceived brightness of a color.
fn neon_glow_luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.299, 0.587, 0.114));
}

/// Samples a ring of 8 points around uv and keeps only the bright ones, giving text a halo.
fn neon_glow_bloom(uv: vec2<f32>, radius_px: f32) -> vec3<f32> {
    let px = radius_px / termoil.size;
    var sum = vec3<f32>(0.0);
    for (var i = 0; i < 8; i = i + 1) {
        let a = f32(i) * 0.785398;
        let s = textureSampleLevel(termoil_frame, termoil_sampler, uv + vec2<f32>(cos(a), sin(a)) * px, 0.0).rgb;
        sum = sum + s * smoothstep(0.30, 0.85, neon_glow_luma(s));
    }
    return sum / 8.0;
}

/// A slowly drifting magenta to cyan gradient laid across the screen.
fn neon_glow_wash(uv: vec2<f32>, t: f32) -> vec3<f32> {
    let k = uv.x * 0.7 + uv.y * 0.3 + t * 0.05;
    return 0.5 + 0.5 * cos(6.28318 * (k + vec3<f32>(0.0, 0.33, 0.67)));
}

/// A pulsing halo around the focused cursor. A cursor effect chosen beside this shader turns it off
/// (TERMOIL_SHADER_CURSOR is 0 then).
fn neon_glow_halo(uv: vec2<f32>, t: f32) -> vec3<f32> {
    let d = distance(uv * termoil.size, termoil.cursor);
    let pulse = 0.7 + 0.3 * sin(t * 3.0);
    let shown = select(0.0, 1.0, termoil.cursor.x >= 0.0);
    return vec3<f32>(1.0, 0.3, 0.8) * exp(-d / 45.0) * 0.45 * pulse * shown * TERMOIL_SHADER_CURSOR;
}

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let t = termoil.time;
    var rgb = color.rgb;
    rgb = rgb + (neon_glow_bloom(uv, GLOW_RADIUS) + neon_glow_bloom(uv, GLOW_RADIUS * 2.5) * 0.6) * GLOW_STRENGTH;
    rgb = mix(rgb, rgb * neon_glow_wash(uv, t) * 1.6, WASH_STRENGTH);
    rgb = rgb + neon_glow_halo(uv, t);
    rgb = rgb * (1.0 - SCANLINES + SCANLINES * sin(uv.y * termoil.size.y * 3.14159));
    let edge = uv * (1.0 - uv);
    rgb = rgb * clamp(pow(edge.x * edge.y * 16.0, 0.2), 0.0, 1.0);
    return vec4<f32>(rgb * color.a, color.a);
}
