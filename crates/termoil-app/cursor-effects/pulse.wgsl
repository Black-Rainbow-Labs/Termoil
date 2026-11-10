// Pulse: a light around the cursor that swells and fades slowly.

fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let d = length(tce_offset(uv));
    let pulse = 0.5 + 0.5 * sin(termoil.time * 3.0);
    return tce_add(color, vec3<f32>(0.4, 0.7, 1.0) * exp(-d / (30.0 + pulse * 25.0)) * (0.2 + pulse * 0.3));
}
