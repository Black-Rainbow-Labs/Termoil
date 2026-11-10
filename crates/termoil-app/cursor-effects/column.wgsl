// Column: a soft band of light down the cursor's column.

fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let o = abs(tce_offset(uv));
    let band = 1.0 - smoothstep(4.0, 12.0, o.x);
    return tce_add(color, vec3<f32>(0.6, 0.5, 1.0) * band * 0.08);
}
