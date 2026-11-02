# Custom shaders

`window.shader` names a WGSL file. Termoil draws each frame into a texture, then runs a full screen pass that calls a function from your file once for every pixel and writes the result to the window. Scanlines, a vignette, a CRT curve and a cursor glow are all written this way. Two examples are in `docs/shaders/`: `crt.wgsl` and `cursor-glow.wgsl`.

```toml
[window]
shader = "shaders/crt.wgsl"   # relative to the folder of the config file, or an absolute path
```

Only the GPU renderer runs the pass. With `window.renderer = "cpu"`, or when no GPU can draw to the window, the setting is ignored and no error is shown.

## What your file defines

One function with this name and signature:

```wgsl
fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32>
```

`uv` is the position of the pixel in the frame, from (0, 0) at the top left to (1, 1) at the bottom right. `color` is the frame's colour at that position, premultiplied by its alpha (alpha is 1 everywhere unless `window.opacity` is below 1). The return value is written to the window as it is, premultiplied, so a translucent window needs `rgb` multiplied by the alpha you return.

You can define any other functions, constants and structs. Do not define entry points (`@vertex`, `@fragment`) or declare `@group` or `@binding` variables; Termoil adds them.

## What Termoil adds

Termoil appends this to your file, so every name below is available inside your functions:

```wgsl
struct TermoilUniforms {
    size: vec2<f32>,     // frame size in pixels
    cursor: vec2<f32>,   // centre of the focused pane's cursor cell in pixels, negative when no cursor is shown
    time: f32,           // seconds since the shader was loaded
    pad0: f32, pad1: f32, pad2: f32,
}
@group(0) @binding(0) var termoil_frame: texture_2d<f32>;
@group(0) @binding(1) var termoil_sampler: sampler;           // linear filtering, clamped at the edges
@group(0) @binding(2) var<uniform> termoil: TermoilUniforms;
```

The variable you read is `termoil`, so the values are `termoil.size`, `termoil.cursor` and `termoil.time`. To read the frame at a different position (a curve, a blur, a glow of the text) call `textureSampleLevel(termoil_frame, termoil_sampler, other_uv, 0.0)`. Use `textureSampleLevel` and not `textureSample`, because `textureSample` is rejected inside code that branches on a value that varies per pixel.

Pixel positions are `uv * termoil.size`. Because your code comes first in the module and Termoil's code follows it, line numbers in error messages are the line numbers of your file.

## Animation

When your file contains `termoil.time` (outside a `//` comment), Termoil redraws the window at the display rate for as long as the shader is set, so the effect moves while the terminal is idle. A shader that does not read the time costs nothing while the terminal is idle.

## Errors

A file that cannot be read, or does not compile, does not stop the window. The window keeps drawing without the pass, a red banner at the top of the window shows `Shader error, drawing without it:` followed by the path and the message, and the same text is printed on stderr. Fix the file and save it: the banner goes away and the shader starts.

## Reloading

The shader is read again when the config file reloads, and when the shader file's modified time changes. The modified time is checked once every `window.shader_poll_ms` milliseconds (1000 by default) while a shader is set. Changing `window.shader` to another path or to an empty string takes effect at once.
