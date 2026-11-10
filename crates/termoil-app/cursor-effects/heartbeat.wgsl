// Heartbeat: a red glow that beats twice, rests, and beats again.

fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let d = length(tce_offset(uv));
    let p = fract(termoil.time * 0.8);
    let beat = exp(-pow((p - 0.1) * 18.0, 2.0)) + 0.7 * exp(-pow((p - 0.3) * 18.0, 2.0));
    return tce_add(color, vec3<f32>(1.0, 0.25, 0.3) * exp(-d / 38.0) * (0.12 + beat * 0.45));
}
