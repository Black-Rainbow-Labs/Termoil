// CRT: a curved glass tube with scanlines, an aperture grille, a little color fringing at the edges
// and darker corners. Inspired by the CRT shaders of Timothy Lottes and cool-retro-term.

const CURVE: f32 = 0.0225;
const SCANLINE: f32 = 0.22;
const MASK: f32 = 0.12;
const FRINGE: f32 = 0.0012;

/// Reads the frame at a position.
fn crt_read(p: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(termoil_frame, termoil_sampler, p, 0.0).rgb;
}

/// Bends the picture outward at the edges, as the glass of a tube does. Returns the uv to read from.
fn crt_curve(uv: vec2<f32>) -> vec2<f32> {
    let c = uv * 2.0 - 1.0;
    let bent = c * (1.0 + CURVE * dot(c, c));
    return bent * 0.5 + 0.5;
}

fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let p = crt_curve(uv);
    if (p.x < 0.0 || p.x > 1.0 || p.y < 0.0 || p.y > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let off = (p - 0.5) * FRINGE * 8.0;
    var rgb = vec3<f32>(crt_read(p + off).r, crt_read(p).g, crt_read(p - off).b);
    let px = p * termoil.size;
    rgb = rgb * (1.0 - SCANLINE + SCANLINE * (0.5 + 0.5 * sin(px.y * 3.14159)));
    let column = u32(px.x) % 3u;
    let lit = vec3<f32>(f32(column == 0u), f32(column == 1u), f32(column == 2u));
    rgb = rgb * (vec3<f32>(1.0 - MASK) + lit * 2.0 * MASK);
    let edge = p * (1.0 - p);
    let vignette = clamp(pow(edge.x * edge.y * 18.0, 0.3), 0.0, 1.0);
    return vec4<f32>(rgb * vignette * 1.08, color.a);
}
