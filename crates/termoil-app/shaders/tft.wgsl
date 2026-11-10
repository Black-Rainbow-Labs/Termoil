// TFT: an old LCD panel seen up close. Each group of three pixels is one red, one green and one
// blue stripe, with a thin dark gap between the cells.

const STRIPE: f32 = 0.35;
const GAP: f32 = 0.18;

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let px = uv * termoil.size;
    let column = u32(px.x) % 3u;
    let lit = vec3<f32>(f32(column == 0u), f32(column == 1u), f32(column == 2u));
    let stripe = vec3<f32>(1.0 - STRIPE) + lit * STRIPE * 1.6;
    let row = fract(px.y / 3.0);
    let gap = select(1.0, 1.0 - GAP, row > 0.8);
    return vec4<f32>(color.rgb * stripe * gap, color.a);
}
