// Green phosphor: a P1 green monochrome monitor, like a VT100 or an Apple II. Every color becomes a
// shade of green, light lingers a little to the right of bright text the way slow phosphor smears,
// and scanlines run across.

const GREEN: vec3<f32> = vec3<f32>(0.2, 1.0, 0.35);

/// Reads the brightness of the frame at a position.
fn green_light(p: vec2<f32>) -> f32 {
    let s = textureSampleLevel(termoil_frame, termoil_sampler, p, 0.0).rgb;
    return dot(s, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let step = 1.0 / termoil.size.x;
    var trail = 0.0;
    for (var i = 1; i <= 4; i = i + 1) {
        trail = trail + green_light(uv - vec2<f32>(step * f32(i) * 1.5, 0.0)) / f32(i + 1);
    }
    let light = green_light(uv) + trail * 0.25;
    let line = 0.84 + 0.16 * sin(uv.y * termoil.size.y * 3.14159);
    let rgb = GREEN * pow(min(light, 1.2), 0.85) * line + vec3<f32>(0.0, 0.025, 0.01);
    return vec4<f32>(rgb, color.a);
}
