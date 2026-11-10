// Ripple: the picture around the cursor wobbles as if a drop fell into water there.

fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let o = tce_offset(uv);
    let d = length(o);
    let fade = exp(-d / 50.0) * tce_shown();
    if (fade < 0.01) {
        return color;
    }
    let push = sin(d * 0.35 - termoil.time * 6.0) * 1.8 * fade;
    let dir = o / max(d, 0.001);
    let p = uv + dir * push / termoil.size;
    return vec4<f32>(mix(color.rgb, tce_read(p) * color.a, fade), color.a);
}
