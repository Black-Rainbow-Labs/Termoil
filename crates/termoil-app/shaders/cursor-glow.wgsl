// Cursor glow: a soft light around the cursor that pulses slowly, so the cursor is easy to find on
// a large screen. Nothing else changes. A cursor effect chosen beside it turns this light off
// (TERMOIL_SHADER_CURSOR is 0 then).

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let distance_px = distance(uv * termoil.size, termoil.cursor);
    let pulse = 0.75 + 0.25 * sin(termoil.time * 3.0);
    let glow = exp(-distance_px / 40.0) * 0.35 * pulse;
    // No cursor is shown when termoil.cursor is negative.
    let shown = select(0.0, 1.0, termoil.cursor.x >= 0.0);
    let tint = vec3<f32>(0.35, 0.6, 1.0) * glow * shown * TERMOIL_SHADER_CURSOR;
    return vec4<f32>(color.rgb + tint * color.a, color.a);
}
