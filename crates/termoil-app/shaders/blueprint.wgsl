// Blueprint: white lines on drafting blue. Text and lines become white strokes, everything else
// the blue of an architect's print, with a faint grid ruled every 16 pixels.

const PAPER: vec3<f32> = vec3<f32>(0.06, 0.22, 0.48);
const INK: vec3<f32> = vec3<f32>(0.92, 0.96, 1.0);

/// Reads the brightness of the frame at a position.
fn blueprint_light(p: vec2<f32>) -> f32 {
    let s = textureSampleLevel(termoil_frame, termoil_sampler, p, 0.0).rgb;
    return dot(s, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let px = uv * termoil.size;
    let background = blueprint_light(vec2<f32>(0.002, 0.998));
    let ink = clamp(abs(blueprint_light(uv) - background) * 1.6, 0.0, 1.0);
    let grid = select(0.0, 0.12, fract(px.x / 16.0) < 1.0 / 16.0 || fract(px.y / 16.0) < 1.0 / 16.0);
    let rgb = mix(PAPER + grid * INK * 0.5, INK, ink);
    return vec4<f32>(rgb, color.a);
}
