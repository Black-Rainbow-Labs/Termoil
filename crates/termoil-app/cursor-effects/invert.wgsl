// Invert: a small disc around the cursor shows the colors inverted.

fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let d = length(tce_offset(uv));
    let inside = (1.0 - smoothstep(13.0, 15.0, d)) * tce_shown();
    let flipped = vec3<f32>(color.a) - color.rgb;
    return vec4<f32>(mix(color.rgb, flipped, inside), color.a);
}
