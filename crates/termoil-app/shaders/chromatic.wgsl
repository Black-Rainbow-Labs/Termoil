// Chromatic: the red and blue parts of the picture slide apart toward the edges of the window, as
// they do through a cheap lens. The center stays sharp.

const AMOUNT: f32 = 0.003;

/// Reads the frame at a position.
fn chroma_read(p: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(termoil_frame, termoil_sampler, p, 0.0).rgb;
}

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let from_center = uv - 0.5;
    let off = from_center * dot(from_center, from_center) * AMOUNT * 8.0;
    let rgb = vec3<f32>(chroma_read(uv + off).r, color.g, chroma_read(uv - off).b);
    return vec4<f32>(rgb, color.a);
}
