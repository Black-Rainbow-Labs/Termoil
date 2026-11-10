// Amber: a monochrome amber phosphor screen, like an IBM 3270 or a Wyse terminal. Every color
// becomes a shade of amber, bright text glows a little, and scanlines run across.

const AMBER: vec3<f32> = vec3<f32>(1.0, 0.69, 0.0);

/// Reads the brightness of the frame at a position.
fn amber_light(p: vec2<f32>) -> f32 {
    let s = textureSampleLevel(termoil_frame, termoil_sampler, p, 0.0).rgb;
    return dot(s, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let d = 1.5 / termoil.size;
    let glow = (amber_light(uv + vec2<f32>(d.x, 0.0)) + amber_light(uv - vec2<f32>(d.x, 0.0))
        + amber_light(uv + vec2<f32>(0.0, d.y)) + amber_light(uv - vec2<f32>(0.0, d.y))) * 0.25;
    let light = amber_light(uv) * 0.85 + glow * 0.35;
    let line = 0.86 + 0.14 * sin(uv.y * termoil.size.y * 3.14159);
    let rgb = AMBER * pow(light, 0.9) * line + vec3<f32>(0.03, 0.015, 0.0);
    return vec4<f32>(rgb, color.a);
}
