// Vignette: the corners and edges of the window fall gently into shadow, drawing the eye to the
// middle. The colors are not changed otherwise.

const STRENGTH: f32 = 0.55;

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let aspect = termoil.size.x / max(termoil.size.y, 1.0);
    let d = (uv - 0.5) * vec2<f32>(aspect, 1.0) / max(aspect, 1.0);
    let fall = smoothstep(0.25, 0.75, length(d) * 1.2);
    return vec4<f32>(color.rgb * (1.0 - fall * STRENGTH), color.a);
}
