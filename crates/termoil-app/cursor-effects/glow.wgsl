// Glow: a steady soft light around the cursor.

fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let d = length(tce_offset(uv));
    return tce_add(color, vec3<f32>(0.45, 0.65, 1.0) * exp(-d / 40.0) * 0.35);
}
