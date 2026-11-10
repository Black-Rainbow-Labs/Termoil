// Flame: a warm flickering glow that rises above the cursor.

fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let o = tce_offset(uv);
    // Screen y grows downward, so the flame is stretched over the cursor (o.y below 0) and short under it.
    let rise = vec2<f32>(o.x * 1.6, select(o.y * 2.5, o.y * 0.5, o.y < 0.0));
    let flicker = 0.8 + 0.2 * sin(termoil.time * 17.0) * sin(termoil.time * 7.0);
    let heat = exp(-length(rise) / 26.0) * flicker;
    let tint = mix(vec3<f32>(1.0, 0.25, 0.05), vec3<f32>(1.0, 0.85, 0.3), heat);
    return tce_add(color, tint * heat * 0.45);
}
