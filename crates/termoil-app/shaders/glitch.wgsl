// Glitch: a digital signal that breaks up now and then. Most of the time the picture is clean;
// for a moment every couple of seconds, bands of the screen jump sideways and split into red and
// blue, the way a damaged video stream does.

/// Reads the frame at a position.
fn glitch_read(p: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(termoil_frame, termoil_sampler, clamp(p, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb;
}

/// A pseudo random number from 0 to 1 for a position.
fn glitch_hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(269.5, 183.3))) * 43758.5453);
}

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let tick = floor(termoil.time * 12.0);
    let burst = step(0.93, glitch_hash(vec2<f32>(tick, 3.7)));
    let band = floor(uv.y * 18.0 + glitch_hash(vec2<f32>(tick, 9.1)) * 4.0);
    let hit = step(0.6, glitch_hash(vec2<f32>(band, tick))) * burst;
    let jump = (glitch_hash(vec2<f32>(band, tick + 1.0)) - 0.5) * 0.08 * hit;
    let split = 0.004 * hit + 0.0008;
    let p = vec2<f32>(uv.x + jump, uv.y);
    let rgb = vec3<f32>(glitch_read(p + vec2<f32>(split, 0.0)).r, glitch_read(p).g, glitch_read(p - vec2<f32>(split, 0.0)).b);
    return vec4<f32>(rgb, color.a);
}
