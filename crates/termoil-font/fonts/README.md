# Termoil Flags

`TermoilFlags.ttf` is a subset of Noto Color Emoji (googlefonts/noto-emoji, the 2026-09-24 build packaged by Arch
Linux as `noto-fonts-emoji 1:20260924-1`). It keeps only the 26 regional indicator letters, the waving black flag
U+1F3F4 and the tag characters U+E0020 to U+E007F, with the GSUB ligatures that join them into flags. The family is
renamed to "Termoil Flags", as the SIL Open Font License asks of a modified version. The licence is `OFL.txt`.

Termoil embeds it because Segoe UI Emoji, the colour emoji face on Windows, has no flag glyphs: a flag such as
U+1F1EF U+1F1F5 drew as the two letters J and P. The face is used only for a flag sequence that the platform's emoji
face does not join into one glyph, so macOS and Linux keep their own flags.

Made with fontTools 4.61:

```python
from fontTools import subset
from fontTools.ttLib import TTFont
font = TTFont("NotoColorEmoji.ttf")
cmap = font.getBestCmap()
keep = [c for c in range(0x1F1E6, 0x1F200) if c in cmap] + [c for c in range(0xE0020, 0xE0080) if c in cmap] + [0x1F3F4]
options = subset.Options(); options.layout_features = ["*"]; options.name_IDs = ["*"]; options.notdef_outline = True
s = subset.Subsetter(options); s.populate(unicodes=keep); s.subset(font)
font["name"].setName("Termoil Flags", 1, 3, 1, 0x409); font["name"].setName("Termoil Flags", 4, 3, 1, 0x409)
font.save("TermoilFlags.ttf")
```
