// Crosshair: faint lines run through the cursor's row and column across the whole window.

fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let o = abs(tce_offset(uv));
    let line = exp(-o.y / 1.5) + exp(-o.x / 1.5);
    return tce_add(color, vec3<f32>(0.5, 0.8, 1.0) * min(line, 1.0) * 0.18);
}
