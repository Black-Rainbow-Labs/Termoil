// Rainbow: a halo around the cursor that cycles through every color.

fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let o = tce_offset(uv);
    let angle = atan2(o.y, o.x) / 6.28318;
    let light = tce_hue(angle + termoil.time * 0.3) * exp(-length(o) / 35.0) * 0.45;
    return tce_add(color, light);
}
