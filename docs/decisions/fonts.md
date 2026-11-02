# Fonts: fontique fallback on Windows (TDD question 4)

Measured on 2026-11-01 on this machine (Windows 11 24H2, build 26100.6584) by `system_fallback_matrix` in
`crates/termoil-font/tests/font.rs`, which prints the face chosen for each sample and fails when any sample has no face.

The primary font is the vendored JetBrains Mono 2.304. For a character the primary face lacks, `FontGrid::face_for` asks
`fontique` for the system fallback of the character's script (from `unicode-script`), and for emoji the colour emoji family.

| Script | Sample | Face chosen | Glyph present |
|---|---|---|---|
| Han | 中 | Yu Gothic UI | yes |
| Hiragana | あ | Yu Gothic UI | yes |
| Hangul | 한 | Malgun Gothic | yes |
| Cyrillic | Ж | JetBrains Mono | yes |
| Greek | α | JetBrains Mono | yes |
| Hebrew | ע | Segoe UI | yes |
| Arabic | ع | Segoe UI | yes |
| Thai | ก | Leelawadee UI | yes |
| Devanagari | क | Nirmala UI | yes |
| Emoji | 😀 | Segoe UI Emoji | yes |
| Symbol | ✓ | JetBrains Mono | yes |
| Technical | ⌘ | JetBrains Mono | yes |

## Decision

`fontique` enumerates the Windows fonts and gives a correct fallback for every script tested, including CJK and emoji, so
it stays. Han falls back to the Japanese UI face (Yu Gothic UI) because no locale is passed; a user who wants a Chinese
face puts it in `font.fallback`, which is tried before the system fallback. Choosing the fallback by the user's locale is
left as an improvement. The same matrix runs on macOS and Linux with the M4 apps.
