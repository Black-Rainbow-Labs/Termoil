# Built in shaders, the shader list and the macOS title bar (task-2590)

A design reading, not a measurement.

## What was asked

1. Shaders that combine with the color themes, chosen from a second list on the right of the theme picker. The
   ticket asked for twenty, in the spirit of a neon shader made on the Mac, with CRT and retro ones.
2. On macOS the bar at the top of the window was transparent: the desktop showed through it.

## Built in shaders

`window.shader` already ran a WGSL file over every frame (`docs/shaders.md`). The shaders that ship are WGSL files in
`crates/termoil-app/shaders/`, built into the program with `include_str!` and listed in `crates/termoil-app/src/shaders.rs`.
They follow the same contract as a person's own file, so nothing in the renderer changed.

- `window.shader` takes a built in name as well as a path. A value with no folder separator and no `.` that matches a
  name is the built in shader; anything else is a file, read as before. `gui/shader.rs` keeps a built in shader as
  `Origin::Builtin` and does not poll the disk for it.
- The shader runs over the frame the theme drew, so the theme and the shader are independent settings and any pair
  works. `amber`, `green-phosphor`, `game-boy`, `sepia`, `noir`, `hologram` and `blueprint` replace the theme's colors
  by design; the rest keep them.
- Two catalogue rows: `set-shader <name>` (`none` turns it off; it writes `window.shader` with `settings::with_value`,
  so the rest of the file stays as it was) and `list-shaders`. The palette, keybindings, `termoil +command`, IPC and
  MCP reach them through the catalogue as for every other action.
- The 21 names: crt, retro, neon, amber, green-phosphor, scanlines, bloom, vhs, glitch, chromatic, vignette,
  film-grain, tft, game-boy, sepia, noir, hologram, synthwave, underwater, cursor-glow, blueprint. Sources of the
  ideas: cool-retro-term, Timothy Lottes's CRT shader, the Ghostty shader collections, Windows Terminal's retro
  effect. Each file is written from scratch.

## The shader list in the theme picker

`choose-theme` opens one overlay with two lists. The themes are on the left as before; `none` and the 21 shaders are on
the right. Left, Right and Tab choose the list that takes the keys. Each list has its own filter and highlight. Moving
in either list previews it. Enter writes the theme, and writes the shader only when the person moved in the shader
list, so a shader file set by path is not replaced by a name. Escape puts back both the theme and the exact
`window.shader` value that was in effect. Below 48 columns the shader list is left out.

## The macOS title bar

A translucent window (`window.opacity` below 1) is created with winit's `with_transparent(true)`. On macOS that makes
the NSWindow clear, and the system title bar drawn over a clear window shows the desktop behind it.

For a translucent window with decorations on macOS (not the quick terminal), Termoil now makes the system title bar
transparent, hides its title and runs the content under it (`with_titlebar_transparent`, `with_fullsize_content_view`,
`with_title_hidden`). Termoil draws its own strip in that space, `window.macos_titlebar_height` points high (28, the
height of a standard macOS title bar since Big Sur), in the tab bar's color, with the window title centered. The close,
minimize and zoom buttons stay where they were and sit on the strip. A press on the strip moves the window. The tab bar,
the banner, the panes and the overlays are laid out below it. An opaque window keeps the system title bar, which
already follows the theme's light or dark appearance. `window.macos_titlebar_height = 0` keeps the system title bar
on a translucent window too.

This could only be compiled for macOS from the Windows machine (`cargo clippy --target aarch64-apple-darwin`). task-2593
installed 1.5.10 on the Mac, but the Mac's screen was locked, so how the title bar looks is still to be checked there.

## Found on the installed 1.5.9: a shader DX12 refuses ended the program

The first release of this work (1.5.9) crashed on start on this PC with `window.shader = "crt"`. The window draws
through DX12, whose own compiler (FXC) refused `crt` and `tft` with `error X3500: array reference cannot be used as an
l-value` (they wrote into a vector at a computed index). naga and Vulkan had accepted both, and the offscreen GPU
test runs on Vulkan, so the tests passed. wgpu reports a backend compiler failure as an internal error, and
`termoil-render/src/shader.rs` only opened a validation error scope, so the error reached wgpu's default handler,
which panics. That is a crash for any user shader DX12 refuses, not only these two.

1.5.10 fixes both: the shader pass opens an internal error scope as well, so such a shader shows the banner and the
window draws without it, and `crt`, `tft` and `game-boy` no longer index vectors or arrays at a computed index. The
tests now compile every built in shader on every backend the machine has (`GpuContext::check_shader`), and check that
a module DX12 refuses comes back as an error. With the 1.5.9 `crt` put back, that test fails naming dx12.

## Tests

- `crates/termoil-app/tests/builtin_shaders.rs` compiles every built in shader on DX12, Vulkan and Metal (whichever
  exist), checks a module DX12 refuses is an error, and compiles every built in shader on the GPU, draws a frame of colored
  text with it and checks that it changed the frame, kept the text (at least 4 colors) and kept the alpha.
- `src/shaders.rs` and `src/gui/shader.rs` unit tests: at least 20 shaders with unique names and the entry function,
  names resolve ignoring case, `none` clears, a path stays a file, and the write keeps the rest of the file.
- E6 `shader_list_step` drives the overlay in a real window: Right, type `neon`, the preview, Enter writes both settings,
  the shader compiles in the window, Down previews another shader, and Escape restores.
- E5 runs `+list-shaders` and `+set-shader` through the CLI, and A5 runs both from the window like every row.
