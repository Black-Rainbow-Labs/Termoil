// Row: a soft band of light across the cursor's row.

fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let o = abs(tce_offset(uv));
    let band = 1.0 - smoothstep(6.0, 16.0, o.y);
    return tce_add(color, vec3<f32>(0.5, 0.6, 1.0) * band * 0.08);
}
