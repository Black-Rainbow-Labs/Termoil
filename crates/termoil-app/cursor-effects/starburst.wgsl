// Starburst: thin rays of light shine out from the cursor and slowly turn.

fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let o = tce_offset(uv);
    let a = atan2(o.y, o.x) + termoil.time * 0.4;
    let rays = pow(abs(cos(a * 4.0)), 30.0);
    let light = rays * exp(-length(o) / 45.0) + exp(-length(o) / 10.0) * 0.4;
    return tce_add(color, vec3<f32>(1.0, 1.0, 0.85) * light * 0.45);
}
