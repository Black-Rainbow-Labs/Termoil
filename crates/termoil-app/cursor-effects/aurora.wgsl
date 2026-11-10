// Aurora: a ribbon of shifting green and violet light waves along the cursor's row.

fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let o = tce_offset(uv);
    let wave = sin(o.x * 0.04 + termoil.time * 1.5) * 6.0;
    let band = exp(-abs(o.y - wave) / 7.0) * exp(-abs(o.x) / 260.0);
    let tint = mix(vec3<f32>(0.2, 1.0, 0.6), vec3<f32>(0.7, 0.3, 1.0), 0.5 + 0.5 * sin(o.x * 0.02 + termoil.time));
    return tce_add(color, tint * band * 0.3);
}
