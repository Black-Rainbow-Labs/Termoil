// Frost: an icy halo around the cursor with a fine crystal texture.

fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let o = tce_offset(uv);
    let grain = 0.6 + 0.4 * tce_hash(floor(uv * termoil.size / 2.0));
    let cold = exp(-length(o) / 34.0) * grain;
    return tce_add(color, vec3<f32>(0.65, 0.9, 1.0) * cold * 0.35);
}
