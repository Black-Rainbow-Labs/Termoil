// Game Boy: the four greens of the original handheld's screen. Brightness is mapped to the four
// shades with an ordered dither, and a faint pixel grid sits over everything.

/// The four shades, darkest first.
fn gb_shade(i: i32) -> vec3<f32> {
    switch i {
        case 0: { return vec3<f32>(0.059, 0.220, 0.059); }
        case 1: { return vec3<f32>(0.188, 0.384, 0.188); }
        case 2: { return vec3<f32>(0.545, 0.675, 0.059); }
        default: { return vec3<f32>(0.608, 0.737, 0.059); }
    }
}

/// A 2x2 ordered dither threshold for a pixel.
fn gb_dither(px: vec2<f32>) -> f32 {
    let x = i32(px.x) % 2 == 1;
    let y = i32(px.y) % 2 == 1;
    let top = select(0.0, 0.5, x);
    let bottom = select(0.75, 0.25, x);
    return select(top, bottom, y) - 0.375;
}

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let px = uv * termoil.size;
    let light = dot(color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let level = clamp(i32(floor(light * 3.99 + gb_dither(px) * 0.6 + 0.2)), 0, 3);
    let grid = select(1.0, 0.92, fract(px.x / 2.0) < 0.5 && fract(px.y / 2.0) < 0.5);
    return vec4<f32>(gb_shade(level) * grid, color.a);
}
