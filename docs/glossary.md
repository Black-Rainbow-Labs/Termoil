# Glossary

The terminal terms these pages use, each explained in a sentence or two.

| Term | Meaning |
|---|---|
| **Terminal emulator** | A program that shows the text a command line program prints and sends it your key presses. Termoil is one; so are Windows Terminal, Ghostty, Alacritty, WezTerm and Kitty |
| **Shell** | The program that reads your commands and runs them: PowerShell, bash, zsh, fish, nushell or cmd. The terminal starts the shell; the shell starts everything else |
| **PTY** | Pseudo terminal. The pair of pipes the operating system gives a terminal emulator so the programs in it believe they are talking to a real terminal |
| **ConPTY** | Windows' pseudo terminal, also called the pseudo console. Termoil ships its own copy, because the one built into Windows removes image sequences |
| **Escape sequence** | A run of bytes starting with the escape character that tells the terminal to do something instead of print text, such as move the cursor or change colour |
| **CSI** | Control Sequence Introducer, `ESC [`. Starts most escape sequences, for example `ESC [ 2 J` to clear the screen |
| **SGR** | Select Graphic Rendition, the CSI sequence that sets colours and styles such as bold, italic and underline |
| **OSC** | Operating System Command, `ESC ]`. Carries longer messages such as the window title (OSC 0 and 2), the working directory (OSC 7), hyperlinks (OSC 8), the clipboard (OSC 52) and prompt marks (OSC 133) |
| **DCS and APC** | Device Control String and Application Program Command: escape sequences that carry a payload. Sixel images travel in DCS and kitty graphics in APC |
| **24 bit colour** | Also called true colour. Any of 16.7 million colours given as red, green and blue, instead of a number from a palette |
| **The 256 colour palette** | The 16 ANSI colours, a 6 by 6 by 6 colour cube and 24 greys. Programs pick a colour by number and the theme decides the first 16 |
| **Scrollback** | The lines that have scrolled off the top of the screen and that you can scroll back to see |
| **Alternate screen** | A second screen that full screen programs such as editors use, so the shell's output is still there when they exit |
| **Grapheme cluster** | What a reader sees as one character, which can be several code points: an emoji family, a flag, or a letter with accents. Termoil keeps each cluster in one cell, or two when it is wide |
| **Wide character** | A character that takes two cells, such as Chinese, Japanese and Korean text and most emoji |
| **Ligature** | One glyph a font draws for several characters, such as `=>` drawn as an arrow |
| **Sprite** | A character Termoil draws itself instead of taking it from the font: box drawing, block elements, braille and powerline symbols, so they join cleanly at any size |
| **Kitty graphics protocol** | A way for a program to send an image to the terminal and place it among the text. Supports animation and shared memory |
| **Sixel** | An older image format from DEC terminals, still used by many tools |
| **Kitty keyboard protocol** | A key encoding that tells the program exactly which key and modifiers were pressed, including ones the legacy encoding cannot express such as `Ctrl+Enter` |
| **Bracketed paste** | Marks around pasted text so a shell or editor knows it was pasted and does not run it line by line |
| **Shell integration** | A small script in the shell that marks where each prompt, command and output begins, so the terminal can jump between prompts and knows each command's exit code |
| **Exit code** | The number a command returns when it finishes. 0 means success; anything else is a failure |
| **Synchronized output** | A mode (DEC 2026) in which a program tells the terminal to hold the screen until the program has finished drawing it, so you never see a half drawn frame |
| **terminfo** | The database programs on macOS and Linux read to learn what a terminal can do. Termoil's entry is `xterm-termoil` |
| **Quick terminal** | A Termoil window that slides in from the edge of the screen when you press a system wide key, and slides away again |
| **Command palette** | A searchable list of every action, opened with `Ctrl+Shift+P` |
| **Action** | One thing Termoil can do, such as `new-tab` or `screen`. Keys, the palette, the command line and the MCP server all run actions |
| **Catalogue** | The list of every action with its parameters, in `termoil-protocol`. The command reference and the MCP tools are generated from it |
| **IPC socket** | The named pipe (Windows) or Unix socket the running Termoil listens on, which `termoil +command` connects to |
| **MCP** | Model Context Protocol, the way AI agents such as Claude call tools. `termoil +mcp` offers Termoil's actions as MCP tools |
| **Minimum contrast** | A setting that lifts a text colour too close to its background until it reaches a readable contrast ratio, 4.5 to 1 by default |
| **Renderer** | The part that draws a frame. Termoil draws on the processor by default and can draw on the graphics card |
| **Shader** | A small program run on the graphics card for every pixel. Termoil runs a WGSL shader of your own over every frame when you set one |
| **WGSL** | WebGPU Shading Language, the language Termoil's custom shaders are written in |
| **p50 and p99** | The median and the 99th percentile of a set of measurements. A p99 latency of 5 ms means 99 key presses in 100 reached the screen within 5 ms |
