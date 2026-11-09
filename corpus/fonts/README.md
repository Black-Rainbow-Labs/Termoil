JetBrains Mono 2.304 (https://github.com/JetBrains/JetBrainsMono/releases/tag/v2.304), SIL Open Font License 1.1, see LICENSE.
Used by the font and rendering tests so their output does not depend on the fonts installed on a machine.

FiLigatureTest-Regular.ttf is JetBrains Mono Regular with its GSUB replaced by one ligature, f and i drawn as one
glyph one cell wide, the way Menlo draws fi (task-2584). It is built by make_fi_ligature_font.py and is under the same
license.
