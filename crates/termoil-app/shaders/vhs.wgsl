// VHS: a worn video tape. Rows wobble sideways, the color channels drift apart, a band of noise
// rolls slowly up the screen, and the picture is a little soft and washed out.

/// Reads the frame at a position.
fn vhs_read(p: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(termoil_frame, termoil_sampler, clamp(p, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb;
}

/// A pseudo random number from 0 to 1 for a position.
fn vhs_hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let t = termoil.time;
    let band_y = fract(t * 0.07);
    let in_band = smoothstep(0.03, 0.0, abs(uv.y - band_y));
    let row = floor(uv.y * termoil.size.y / 2.0);
    let wobble = (sin(uv.y * 40.0 + t * 3.0) * 0.0003) + (vhs_hash(vec2<f32>(row, floor(t * 20.0))) - 0.5) * 0.00075;
    let shift = wobble + in_band * 0.004;
    let p = vec2<f32>(uv.x + shift, uv.y);
    let spread = 0.00125 + in_band * 0.002;
    var rgb = vec3<f32>(vhs_read(p + vec2<f32>(spread, 0.0)).r, vhs_read(p).g, vhs_read(p - vec2<f32>(spread, 0.0)).b);
    let soft = (vhs_read(p + vec2<f32>(1.5 / termoil.size.x, 0.0)) + vhs_read(p - vec2<f32>(1.5 / termoil.size.x, 0.0))) * 0.5;
    rgb = mix(rgb, soft, 0.15);
    let noise = vhs_hash(uv * termoil.size + fract(t) * 91.0);
    rgb = rgb + (noise - 0.5) * (0.025 + in_band * 0.175);
    rgb = rgb * vec3<f32>(1.01, 0.99, 1.02) * 0.975 + 0.015;
    return vec4<f32>(rgb, color.a);
}
