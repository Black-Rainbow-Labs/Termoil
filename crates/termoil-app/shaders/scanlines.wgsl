// Scanlines: only the dark lines between the rows of a tube, with the colors left as they are. The
// lightest of the effects, close to the retro effect Windows Terminal offers.

const DEPTH: f32 = 0.28;
const PITCH: f32 = 3.0;

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let y = uv.y * termoil.size.y;
    let wave = 0.5 + 0.5 * cos(y * 6.28318 / PITCH);
    let shade = 1.0 - DEPTH * wave;
    return vec4<f32>(color.rgb * shade, color.a);
}
