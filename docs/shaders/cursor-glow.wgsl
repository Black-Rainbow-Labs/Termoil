// A soft glow around the cursor that pulses slowly. It reads termoil.time, so the window redraws
// at the display rate while this shader is set. Set window.shader to this file's path.

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let distance_px = distance(uv * termoil.size, termoil.cursor);
    let pulse = 0.75 + 0.25 * sin(termoil.time * 3.0);
    let glow = exp(-distance_px / 40.0) * 0.35 * pulse;
    // No cursor is shown when termoil.cursor is negative.
    let shown = select(0.0, 1.0, termoil.cursor.x >= 0.0);
    let tint = vec3<f32>(0.35, 0.6, 1.0) * glow * shown;
    return vec4<f32>(color.rgb + tint * color.a, color.a);
}
