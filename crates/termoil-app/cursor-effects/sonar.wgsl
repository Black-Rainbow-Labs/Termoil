// Sonar: thin rings travel outward from the cursor, like a radar ping.

fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let d = length(tce_offset(uv));
    let wave = fract(termoil.time * 0.6) * 120.0;
    let ring = exp(-abs(d - wave) / 2.5) * (1.0 - wave / 120.0);
    return tce_add(color, vec3<f32>(0.3, 1.0, 0.6) * ring * 0.6);
}
