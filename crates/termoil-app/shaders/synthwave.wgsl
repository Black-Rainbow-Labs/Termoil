// Synthwave: an 80s sunset grade. Shadows lean purple and highlights lean pink and orange, a
// magenta to blue gradient washes up from the bottom of the window, and bright text glows.

/// Reads the frame at a position.
fn synth_read(p: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(termoil_frame, termoil_sampler, p, 0.0).rgb;
}

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let light = dot(color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let shadow = vec3<f32>(0.18, 0.04, 0.32);
    let highlight = vec3<f32>(1.0, 0.45, 0.7);
    let grade = mix(shadow, highlight, light);
    var rgb = mix(color.rgb, grade, 0.35) + color.rgb * 0.25;
    let d = 2.0 / termoil.size;
    let glow = (synth_read(uv + vec2<f32>(d.x, 0.0)) + synth_read(uv - vec2<f32>(d.x, 0.0))
        + synth_read(uv + vec2<f32>(0.0, d.y)) + synth_read(uv - vec2<f32>(0.0, d.y))) * 0.25;
    rgb = rgb + glow * vec3<f32>(0.35, 0.15, 0.4);
    let wash = mix(vec3<f32>(0.25, 0.0, 0.35), vec3<f32>(0.0, 0.05, 0.25), uv.y);
    rgb = rgb + wash * smoothstep(0.4, 1.0, uv.y) * 0.5;
    return vec4<f32>(rgb, color.a);
}
