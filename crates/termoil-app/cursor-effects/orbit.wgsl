// Orbit: three small lights circle the cursor.

fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let o = tce_offset(uv);
    var light = 0.0;
    for (var i = 0; i < 3; i = i + 1) {
        let a = termoil.time * 2.2 + f32(i) * 2.0944;
        let p = vec2<f32>(cos(a), sin(a)) * 22.0;
        light = light + exp(-length(o - p) / 3.0);
    }
    return tce_add(color, vec3<f32>(0.6, 0.85, 1.0) * light * 0.8);
}
