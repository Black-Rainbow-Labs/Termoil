// Spotlight: the screen dims away from the cursor, so the cursor's neighborhood stands out.

fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let d = length(tce_offset(uv));
    let lit = mix(0.55, 1.0, exp(-d / 220.0));
    let k = mix(1.0, lit, tce_shown());
    return vec4<f32>(color.rgb * k, color.a);
}
