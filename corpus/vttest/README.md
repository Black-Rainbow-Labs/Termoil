# vttest goldens

vttest (Thomas Dickey, fetched from `vttest-snapshots` at commit 0229d71, label t20251205, vttest 2.7) is
driven through `termoil-headless run --script` by `tools/vttest/run.sh`. None of vttest is copied here. This
folder holds Termoil's own files:

- `vttest.vtscript`: the driver. It enters menus 1, 2 and 8 from the main menu and saves every screen that
  stops at "Push <RETURN>".
- `<name>.txt`: one golden per saved screen. The text of the screen comes first, for a person to read, then
  `--- cells` and every row in the full cell dump format, so attributes, colours and wide characters are
  compared too.

The run fails when a screen differs from its golden, when a golden has no screen, and when a screen has no
golden. Goldens change only with `TERMOIL_BLESS=1` (or `--bless`), and the commit says which screen changed
and why.

## Review

Every golden was read against what its vttest screen says it should show, on 2026-11-01:

| Goldens | vttest menu | What was checked |
|---|---|---|
| `main-menu` | main menu | the menu, version 2.7 (20251205) |
| `cursor-01` to `cursor-02` | 1, cursor movements | an unbroken border of `*` and `+` with the frame of `E` centred and one free position around the text, at 80 and then 132 columns |
| `cursor-03` to `cursor-04` | 1 | autowrap mixed with controls, at 80 and 132 columns: `I` to `Z` down the left margin and `i` to `z` down the right |
| `cursor-05` | 1 | cursor controls inside escape sequences: four identical lines `A B C D E F G H I` |
| `cursor-06` | 1 | leading zeros in escape sequences: "This is a correct sentence" two lines below the heading |
| `screen-01` | 2, screen features | three full lines of `*` with no gap (wrap around) |
| `screen-02` | 2 | the two tab stop lines are identical |
| `screen-03` to `screen-06` | 2 | 132 and 80 column modes, light and dark background: the column ruler fits the width and the staircase of lines is not cut |
| `screen-07` to `screen-10` | 2 | soft and jump scrolling in a two line region and over the whole screen: the region scrolls and nothing outside it moves |
| `screen-11` to `screen-12` | 2 | origin mode: the lines are at the rows the text names |
| `screen-13` to `screen-14` | 2 | the graphic rendition pattern: the cell dump has bold, underline, blink and inverse on exactly the words that name them |
| `screen-15` | 2 | save and restore cursor: ten of each character in every column and a 5 by 4 block of `A` at the top left |
| `vt102-01` to `vt102-07` | 8, VT102 insert and delete | at 80 columns: the accordion test leaves A, X and the message; insert mode gives `A***...***B`; delete character gives `AB`; the right column is staggered by one; ICH gives the spaced alphabet twice |
| `vt102-08` to `vt102-14` | 8 | the same tests at 132 columns |

## Not covered by goldens

These vttest menus need a person or are documented as unsupported. Run them by hand with the installed build
before a release (`vttest` inside Termoil) and record the result in the release notes.

- Menu 3, character sets: the DEC special graphics line drawing appears in `screen-15`; the national
  replacement character sets are checked by hand.
- Menu 4, double size characters (DECDHL, DECDWL): Termoil draws them at single size. Documented
  unsupported in 1.0.
- Menu 5, keyboard: needs keys pressed by a person; the key encoding is covered by the input tables in
  `crates/termoil-input`.
- Menu 6, terminal reports: covered by `corpus/vt/reports.test` and esctest2.
- Menu 7, VT52 mode: documented unsupported.
- Menus 9 to 11: checked by hand; esctest2 covers most of what menu 11 tests.
