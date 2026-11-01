#!/usr/bin/env bash
# C2: runs esctest2 against terminus-headless (docs/TEST-SUITE.md section 5).
#
# esctest2 is GPL 2, so it is fetched at a pinned commit into a cache folder and run, never copied
# into this repository. The results are compared with the ratchet file
# corpus/esctest/known-failures.txt: the run fails on a failure that is not listed, and on a
# listed test that now passes, so the file only shrinks.
#
#   tools/esctest/run.sh            run and compare with the ratchet
#   tools/esctest/run.sh --bless    rewrite the ratchet from this run (TERMINUS_BLESS=1 does the same)
#
# The options tell esctest what the terminal is set up to do: xtermWinopsEnabled means window
# operations are allowed, and disableWideChars means 8 bit C1 controls are accepted (Terminus
# accepts them whenever the bytes are not valid UTF-8).
#
# Linux and macOS (esctest needs a Unix pty). On Windows run it inside WSL.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
COMMIT=2798f12149a19c3295e9b4853ab2da4b2eff1b2b
CACHE="${ESCTEST_CACHE:-$ROOT/_agent_output/esctest2}"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
OUT="${ESCTEST_OUT:-$ROOT/_agent_output/esctest}"
mkdir -p "$OUT"

if [ ! -d "$CACHE/.git" ]; then
  git clone --quiet https://github.com/ThomasDickey/esctest2.git "$CACHE"
fi
git -C "$CACHE" fetch --quiet origin "$COMMIT" 2>/dev/null || true
git -C "$CACHE" checkout --quiet "$COMMIT"

cargo build --release --quiet --manifest-path "$ROOT/Cargo.toml" -p terminus-headless --target-dir "$TARGET_DIR"
HEADLESS="$TARGET_DIR/release/terminus-headless"

LOG="$OUT/esctest.log"
rm -f "$LOG"
"$HEADLESS" run --cols 80 --rows 25 -- python3 "$CACHE/esctest/esctest.py" \
  --expected-terminal=xterm --xterm-checksum=400 --xterm-reverse-wrap=383 \
  --max-vt-level=5 --options xtermWinopsEnabled disableWideChars --logfile="$LOG" --timeout=1 --no-print-logs --v=2 || true

BLESS=0
if [ "${1:-}" = "--bless" ] || [ "${TERMINUS_BLESS:-0}" = "1" ]; then BLESS=1; fi
python3 "$ROOT/tools/esctest/ratchet.py" "$LOG" "$ROOT/corpus/esctest/known-failures.txt" "$BLESS" "$OUT/summary.txt"
