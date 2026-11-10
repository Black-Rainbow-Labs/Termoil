// Neon halo: the pulsing magenta halo of the neon-glow shader, for use with any shader.

fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let d = length(tce_offset(uv));
    let pulse = 0.7 + 0.3 * sin(termoil.time * 3.0);
    return tce_add(color, vec3<f32>(1.0, 0.3, 0.8) * exp(-d / 45.0) * 0.45 * pulse);
}
