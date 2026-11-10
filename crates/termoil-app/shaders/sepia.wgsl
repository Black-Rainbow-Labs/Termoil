// Sepia: the warm brown tones of an old photograph, with slightly lifted blacks and darker corners.

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let c = color.rgb;
    let toned = vec3<f32>(
        dot(c, vec3<f32>(0.393, 0.769, 0.189)),
        dot(c, vec3<f32>(0.349, 0.686, 0.168)),
        dot(c, vec3<f32>(0.272, 0.534, 0.131)),
    );
    let lifted = toned * 0.9 + vec3<f32>(0.06, 0.045, 0.03);
    let d = length(uv - 0.5);
    let vignette = 1.0 - smoothstep(0.4, 0.85, d) * 0.4;
    return vec4<f32>(min(lifted, vec3<f32>(1.0)) * vignette, color.a);
}
