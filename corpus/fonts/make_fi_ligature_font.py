"""Builds FiLigatureTest-Regular.ttf from JetBrainsMono-Regular.ttf (task-2584).

The test font has one ligature, f followed by i drawn as the f glyph, whose advance is one cell. That is
how Menlo's fi ligature behaves: one glyph one cell wide for two cells of text. The frame builder test in
crates/termoil-render/tests/clusters.rs uses it so the case runs on a machine without Menlo.

Run with: python corpus/fonts/make_fi_ligature_font.py
"""

from pathlib import Path

from fontTools.feaLib.builder import addOpenTypeFeaturesFromString
from fontTools.ttLib import TTFont

FAMILY = "Fi Ligature Test"


def build(here: Path) -> None:
    """Reads JetBrains Mono, replaces its GSUB with the single fi ligature, renames it and saves it."""
    font = TTFont(here / "JetBrainsMono-Regular.ttf")
    if "GSUB" in font:
        del font["GSUB"]
    addOpenTypeFeaturesFromString(font, "languagesystem DFLT dflt;\nfeature liga { sub f i by f; } liga;\n")
    for record in font["name"].names:
        if record.nameID in (1, 16):
            record.string = FAMILY
        elif record.nameID == 4:
            record.string = FAMILY + " Regular"
        elif record.nameID == 6:
            record.string = "FiLigatureTest-Regular"
    font.save(here / "FiLigatureTest-Regular.ttf")


if __name__ == "__main__":
    build(Path(__file__).resolve().parent)
