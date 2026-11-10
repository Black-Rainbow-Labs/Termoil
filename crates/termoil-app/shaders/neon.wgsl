// Neon: text glows like neon tubing. Bright colors bloom outward, the colors are pushed toward
// full saturation, and faint scanlines run across the glass.

const BLOOM: f32 = 0.9;
const SATURATION: f32 = 1.45;

/// Reads the frame at a position.
fn neon_read(p: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(termoil_frame, termoil_sampler, p, 0.0).rgb;
}

/// The light from text near a position: two rings of samples, the bright parts weighted most.
fn neon_bloom(p: vec2<f32>) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    for (var ring = 1; ring <= 2; ring = ring + 1) {
        let d = f32(ring) * 3.0 / termoil.size;
        for (var i = 0; i < 8; i = i + 1) {
            let a = f32(i) * 0.785398 + f32(ring) * 0.39;
            let s = neon_read(p + vec2<f32>(cos(a), sin(a)) * d);
            let bright = max(max(s.r, s.g), s.b);
            sum = sum + s * smoothstep(0.35, 1.0, bright) / f32(ring);
        }
    }
    return sum / 12.0;
}

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let base = color.rgb;
    let grey = dot(base, vec3<f32>(0.2126, 0.7152, 0.0722));
    let vivid = clamp(mix(vec3<f32>(grey), base, SATURATION), vec3<f32>(0.0), vec3<f32>(1.0));
    var rgb = vivid + neon_bloom(uv) * BLOOM;
    let line = 0.93 + 0.07 * sin(uv.y * termoil.size.y * 3.14159);
    rgb = rgb * line;
    return vec4<f32>(rgb, color.a);
}
