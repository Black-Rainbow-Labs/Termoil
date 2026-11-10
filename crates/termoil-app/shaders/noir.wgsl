// Noir: high contrast black and white, like a film noir still. Colors become grays on a steep
// curve, so text is crisp white and the background deep black.

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let light = dot(color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let curved = smoothstep(0.08, 0.85, light);
    let d = length(uv - 0.5);
    let vignette = 1.0 - smoothstep(0.45, 0.9, d) * 0.5;
    return vec4<f32>(vec3<f32>(curved * vignette), color.a);
}
