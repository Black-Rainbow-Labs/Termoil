// A CRT look: barrel curve, scanlines and a vignette. Set window.shader to this file's path.

/// Bends the picture outward at the edges. Returns the uv to read from.
fn curve(uv: vec2<f32>) -> vec2<f32> {
    let centred = uv * 2.0 - 1.0;
    let bent = centred * (1.0 + 0.04 * dot(centred, centred));
    return bent * 0.5 + 0.5;
}

fn terminus_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let p = curve(uv);
    if (p.x < 0.0 || p.x > 1.0 || p.y < 0.0 || p.y > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let texel = textureSampleLevel(terminus_frame, terminus_sampler, p, 0.0);
    let line = 0.88 + 0.12 * sin(p.y * terminus.size.y * 3.14159);
    let edge = p * (1.0 - p);
    let vignette = clamp(pow(edge.x * edge.y * 16.0, 0.25), 0.0, 1.0);
    return vec4<f32>(texel.rgb * line * vignette, texel.a);
}
