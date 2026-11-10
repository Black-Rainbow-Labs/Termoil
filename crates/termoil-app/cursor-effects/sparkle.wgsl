// Sparkle: small points of light twinkle in a cloud around the cursor.

fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let o = tce_offset(uv);
    let cell = floor(o / 6.0);
    let seed = tce_hash(cell);
    let twinkle = max(sin(termoil.time * (2.0 + seed * 4.0) + seed * 40.0), 0.0);
    let star = step(0.9, seed) * pow(twinkle, 8.0) * exp(-length(o) / 60.0);
    return tce_add(color, vec3<f32>(1.0, 0.95, 0.7) * star * 1.2);
}
