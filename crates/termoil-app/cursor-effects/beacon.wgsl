// Beacon: a beam turns around the cursor like a lighthouse.

fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let o = tce_offset(uv);
    let a = atan2(o.y, o.x);
    let sweep = pow(max(cos(a - termoil.time * 2.0), 0.0), 24.0);
    let beam = sweep * exp(-length(o) / 90.0) + exp(-length(o) / 12.0) * 0.5;
    return tce_add(color, vec3<f32>(1.0, 0.9, 0.55) * beam * 0.4);
}
