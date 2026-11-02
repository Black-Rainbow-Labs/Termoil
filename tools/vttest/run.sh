#!/usr/bin/env bash
# The vttest goldens (docs/TEST-SUITE.md section 5): drives vttest through terminus-headless with
# corpus/vttest/vttest.vtscript and compares every saved screen with corpus/vttest/<name>.txt.
#
# vttest is fetched at a pinned commit into a cache folder and built there; none of it is copied
# into this repository. The goldens are Terminus's screens, each reviewed against what the vttest
# screen says it should show (corpus/vttest/README.md).
#
#   tools/vttest/run.sh            run and compare with the goldens
#   tools/vttest/run.sh --bless    rewrite the goldens from this run (TERMINUS_BLESS=1 does the same)
#
# Linux and macOS (vttest needs a Unix pty). On Windows run it inside WSL.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# vttest-snapshots, label t20251205 (vttest 2.7).
COMMIT=0229d71
CACHE="${VTTEST_CACHE:-$ROOT/_agent_output/vttest-src}"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
OUT="${VTTEST_OUT:-$ROOT/_agent_output/vttest}"
GOLDEN="$ROOT/corpus/vttest"

if [ ! -d "$CACHE/.git" ]; then
  git clone --quiet https://github.com/ThomasDickey/vttest-snapshots.git "$CACHE"
fi
if [ "$(git -C "$CACHE" rev-parse --short=7 HEAD)" != "$COMMIT" ] || [ ! -x "$CACHE/vttest" ]; then
  git -C "$CACHE" checkout --quiet "$COMMIT"
  (cd "$CACHE" && ./configure --quiet > /dev/null && make --quiet > /dev/null)
fi

cargo build --release --quiet --manifest-path "$ROOT/Cargo.toml" -p terminus-headless --target-dir "$TARGET_DIR"
HEADLESS="$TARGET_DIR/release/terminus-headless"

rm -rf "$OUT/snaps"
mkdir -p "$OUT/snaps"
"$HEADLESS" run --cols 80 --rows 24 --script "$GOLDEN/vttest.vtscript" --snaps "$OUT/snaps" -- "$CACHE/vttest"

if [ "${1:-}" = "--bless" ] || [ "${TERMINUS_BLESS:-0}" = "1" ]; then
  cp "$OUT"/snaps/*.txt "$GOLDEN/"
  echo "blessed $(ls "$OUT/snaps" | wc -l) vttest screens into corpus/vttest"
  exit 0
fi

failed=0
for snap in "$OUT"/snaps/*.txt; do
  name="$(basename "$snap")"
  if [ ! -f "$GOLDEN/$name" ]; then
    echo "vttest: $name has no golden"
    failed=1
  elif ! diff -u "$GOLDEN/$name" "$snap" > "$OUT/$name.diff"; then
    echo "vttest: $name differs from its golden; see $OUT/$name.diff"
    head -20 "$OUT/$name.diff"
    failed=1
  fi
done
for golden in "$GOLDEN"/*.txt; do
  if [ ! -f "$OUT/snaps/$(basename "$golden")" ]; then
    echo "vttest: $(basename "$golden") was not produced by this run"
    failed=1
  fi
done
if [ "$failed" = 0 ]; then
  echo "vttest: $(ls "$OUT/snaps" | wc -l) screens match their goldens"
fi
exit "$failed"
