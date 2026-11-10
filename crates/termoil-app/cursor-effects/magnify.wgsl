// Magnify: a small lens over the cursor shows the text under it larger.

const MAGNIFY_RADIUS: f32 = 36.0;

fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let o = tce_offset(uv);
    let d = length(o);
    if (tce_shown() < 0.5 || d > MAGNIFY_RADIUS) {
        return color;
    }
    let zoomed = (termoil.cursor + o * 0.6) / termoil.size;
    let rim = smoothstep(MAGNIFY_RADIUS - 2.0, MAGNIFY_RADIUS, d) * 0.4;
    return vec4<f32>((tce_read(zoomed) + rim) * color.a, color.a);
}
