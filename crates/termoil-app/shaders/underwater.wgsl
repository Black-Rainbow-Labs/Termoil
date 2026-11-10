// Underwater: the window seen through a moving sheet of water. The picture ripples gently, a blue
// green tint settles over it, and soft bands of light drift across.

const RIPPLE: f32 = 0.0025;

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let t = termoil.time;
    let wave = vec2<f32>(
        sin(uv.y * 22.0 + t * 1.6) + sin(uv.y * 9.0 - t * 0.9),
        cos(uv.x * 18.0 + t * 1.3) + cos(uv.x * 7.0 - t * 0.7),
    ) * RIPPLE * 0.5;
    let s = textureSampleLevel(termoil_frame, termoil_sampler, clamp(uv + wave, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb;
    let caustic = pow(0.5 + 0.5 * sin(uv.x * 30.0 + sin(uv.y * 12.0 + t) * 2.0 + t * 0.8), 3.0) * 0.025;
    let tint = vec3<f32>(0.75, 0.95, 1.05);
    let rgb = s * tint + vec3<f32>(0.0, 0.03, 0.05) + caustic * vec3<f32>(0.6, 0.9, 1.0);
    return vec4<f32>(rgb, color.a);
}
