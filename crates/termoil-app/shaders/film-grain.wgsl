// Film grain: a fine moving grain over the whole picture, stronger in the dark parts, as on film
// stock. The grain changes every frame, so the window redraws while it is set.

const GRAIN: f32 = 0.07;

/// A pseudo random number from 0 to 1 for a position.
fn grain_hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let px = floor(uv * termoil.size);
    let n = grain_hash(px + floor(termoil.time * 24.0) * vec2<f32>(17.0, 59.0)) - 0.5;
    let light = dot(color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let amount = GRAIN * (1.2 - light);
    return vec4<f32>(color.rgb + n * amount, color.a);
}
