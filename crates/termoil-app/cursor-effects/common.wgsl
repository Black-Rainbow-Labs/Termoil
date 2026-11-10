// Helpers every cursor effect may use. Termoil adds this file when a cursor effect is chosen, after
// the shader and before the effect. termoil.cursor is the center of the cursor cell in pixels, and
// negative when no cursor is shown.

/// 1 when a cursor is shown, 0 when not.
fn tce_shown() -> f32 {
    return select(0.0, 1.0, termoil.cursor.x >= 0.0);
}

/// The pixel offset of a position from the cursor.
fn tce_offset(uv: vec2<f32>) -> vec2<f32> {
    return uv * termoil.size - termoil.cursor;
}

/// The frame as drawn, before any shader, at a position.
fn tce_read(p: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(termoil_frame, termoil_sampler, clamp(p, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb;
}

/// A fully saturated color for a hue from 0 to 1.
fn tce_hue(h: f32) -> vec3<f32> {
    return clamp(abs(fract(h + vec3<f32>(0.0, 0.6667, 0.3333)) * 6.0 - 3.0) - 1.0, vec3<f32>(0.0), vec3<f32>(1.0));
}

/// A pseudo random number from 0 to 1 for a position.
fn tce_hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

/// Light added to a color, kept within the window's opacity.
fn tce_add(color: vec4<f32>, light: vec3<f32>) -> vec4<f32> {
    return vec4<f32>(color.rgb + light * tce_shown() * color.a, color.a);
}
