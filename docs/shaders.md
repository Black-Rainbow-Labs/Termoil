# Custom shaders

## Built in shaders

Termoil ships 22 shaders, built into the program (`crates/termoil-app/shaders/`). Set one by name:

```toml
[window]
shader = "crt"
```

| Name | What it looks like |
|---|---|
| `neon-glow` | the neon look first made as a shader file on the Mac: text bloom, a drifting magenta to cyan wash, a pulsing cursor halo, soft scanlines and a vignette. It is first in the shader list |
| `crt` | a curved glass tube with scanlines, an aperture grille, color fringes at the edges and dark corners |
| `retro` | an old terminal monitor in the style of cool-retro-term: soft curve, glow, scanlines, noise and flicker |
| `neon` | text glows like neon tubing, colors pushed to full saturation |
| `amber` | monochrome amber phosphor, like an IBM 3270 |
| `green-phosphor` | monochrome green phosphor with a slow phosphor trail, like a VT100 |
| `scanlines` | only the dark lines between rows, colors unchanged |
| `bloom` | a soft halo of light around bright text |
| `vhs` | worn video tape: rows wobble, colors drift and a band of noise rolls up the screen |
| `glitch` | a digital signal that breaks into shifted, split bands for a moment every couple of seconds |
| `chromatic` | red and blue slide apart toward the edges, as through a cheap lens |
| `vignette` | corners and edges fall gently into shadow |
| `film-grain` | fine moving grain, stronger in the dark parts |
| `tft` | an old LCD seen up close: red, green and blue stripes |
| `game-boy` | the four greens of the original handheld, dithered |
| `sepia` | the warm brown tones of an old photograph |
| `noir` | high contrast black and white |
| `hologram` | a projected cyan display with sweeping lines and flicker |
| `synthwave` | an 80s sunset grade: purple shadows, pink highlights, a magenta wash |
| `underwater` | the picture ripples under a blue green tint with drifting light |
| `cursor-glow` | a soft pulsing light around the cursor |
| `blueprint` | white lines on drafting blue with a faint grid |

A shader runs over the frame the color theme drew, so any theme combines with any shader. The theme picker
(`choose-theme`, in the command palette) has the shaders in a second list beside the themes, and the cursor effects
in a third list beside the shaders:

- Left and Right move between the theme list, the shader list and the cursor effect list, and Tab goes round them.
  A window narrower than 72 columns leaves out the cursor effect list.
- Typing filters the list that has the keys; Up and Down move through it and preview the theme, shader or cursor
  effect.
- Enter keeps all three and writes them to the configuration file. A shader file set by path is left as it is
  unless a shader was picked in the list, and the cursor effect is left as it is unless one was picked.
- Escape puts back the theme, the shader and the cursor effect that were in effect.

`termoil +list-shaders` prints the names with what each looks like, and `termoil +set-shader <name>` sets one (`none`
turns the shader off). Both are catalogue actions, so the palette, keybindings, IPC and the MCP server reach them too.

Inspiration came from cool-retro-term, the CRT shaders of Timothy Lottes, the shaders people share for Ghostty, and
the retro effect of Windows Terminal. The shaders here are written from scratch.

## Cursor effects

A cursor effect is a light or a distortion drawn around the cursor after the shader, set with `window.cursor_effect`.
It combines with any shader, or runs with no shader. Termoil ships 20 of them (`crates/termoil-app/cursor-effects/`):
`glow`, `pulse`, `neon-halo`, `sonar`, `spotlight`, `crosshair`, `row`, `column`, `rainbow`, `heartbeat`, `sparkle`,
`orbit`, `flame`, `frost`, `magnify`, `ripple`, `beacon`, `starburst`, `invert` and `aurora`.

```toml
[window]
shader = "crt"
cursor_effect = "sonar"
```

When a cursor effect is set, the cursor light of a built in shader (`cursor-glow`, and the halo of `neon-glow`) is
turned off, so the chosen cursor effect replaces it. `termoil +list-cursor-effects` prints the names with what each
looks like, and `termoil +set-cursor-effect <name>` sets one (`none` turns it off).

Termoil builds one module for the shader and the cursor effect: the shader's `termoil_shade` is renamed
`termoil_base_shade`, and a new `termoil_shade` runs it and then the cursor effect. It also defines
`const TERMOIL_SHADER_CURSOR: f32`, which is 0 while a cursor effect is set and 1 otherwise. A shader file can
multiply its own cursor light by `TERMOIL_SHADER_CURSOR` to give way to a cursor effect, and must not declare that
name itself.

The shaders `neon-glow`, `retro`, `vhs`, `glitch`, `film-grain`, `hologram`, `underwater` and `cursor-glow` move, and so do most cursor effects, so the window
redraws at the display rate while one of them is set (see Animation below). The others cost nothing while the
terminal is idle.

## Your own shader

`window.shader` can also name a WGSL file. Termoil draws each frame into a texture, then runs a full screen pass that calls a function from your file once for every pixel and writes the result to the window. Scanlines, a vignette, a CRT curve and a cursor glow are all written this way. Two examples are in `docs/shaders/`: `crt.wgsl` and `cursor-glow.wgsl`.

```toml
[window]
shader = "shaders/crt.wgsl"   # relative to the folder of the config file, or an absolute path
```

A value with no folder and no extension that matches a built in name is the built in shader; anything else is a file.

Only the GPU renderer runs the pass. The window draws on the CPU by default; with `window.renderer = "auto"` (the default) setting a shader starts the GPU and the window switches to it once the device is ready (task-2481). With `window.renderer = "cpu"`, or when no GPU can draw to the window, the setting is ignored and no error is shown.

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
