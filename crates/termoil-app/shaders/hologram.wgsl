// Hologram: a projected science fiction display. Everything turns a translucent cyan, fine lines
// sweep slowly down the screen, a brighter scan bar passes now and then, and the image flickers.

const CYAN: vec3<f32> = vec3<f32>(0.3, 0.95, 1.0);

/// Reads the brightness of the frame at a position.
fn holo_light(p: vec2<f32>) -> f32 {
    let s = textureSampleLevel(termoil_frame, termoil_sampler, p, 0.0).rgb;
    return dot(s, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let t = termoil.time;
    let y = uv.y * termoil.size.y;
    let lines = 0.875 + 0.125 * sin((y - t * 30.0) * 1.2);
    let bar = smoothstep(0.06, 0.0, abs(fract(uv.y - t * 0.15) - 0.5)) * 0.25;
    let flicker = 0.97 + 0.03 * sin(t * 37.0) * sin(t * 13.0);
    let shift = sin(y * 0.05 + t * 2.0) * 0.00035;
    let light = holo_light(vec2<f32>(uv.x + shift, uv.y));
    let edge = (holo_light(uv + vec2<f32>(1.0 / termoil.size.x, 0.0)) - light);
    let rgb = CYAN * (light * lines + abs(edge) * 0.3 + bar * light) * flicker + CYAN * 0.04;
    return vec4<f32>(rgb, color.a);
}
