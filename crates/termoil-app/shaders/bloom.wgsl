// Bloom: a soft halo of light around bright text, with the colors kept. The same idea as the bloom
// shader people use with Ghostty, sampled on a spiral so it stays smooth.

const STRENGTH: f32 = 0.55;
const RADIUS_PX: f32 = 9.0;
const SAMPLES: i32 = 24;

/// Reads the frame at a position.
fn bloom_read(p: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(termoil_frame, termoil_sampler, p, 0.0).rgb;
}

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    var sum = vec3<f32>(0.0);
    var weight = 0.0;
    for (var i = 0; i < SAMPLES; i = i + 1) {
        let t = (f32(i) + 0.5) / f32(SAMPLES);
        let a = f32(i) * 2.39996;
        let r = sqrt(t) * RADIUS_PX;
        let s = bloom_read(uv + vec2<f32>(cos(a), sin(a)) * r / termoil.size);
        let bright = max(max(s.r, s.g), s.b);
        let w = 1.0 - t;
        sum = sum + s * smoothstep(0.45, 1.0, bright) * w;
        weight = weight + w;
    }
    return vec4<f32>(color.rgb + sum / weight * STRENGTH * 2.0, color.a);
}
