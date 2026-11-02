#!/usr/bin/env bash
# Measures Ghostty 1.4 on the P1 gate file, for the comparison in docs/TEST-SUITE.md section 13.
#
# Usage: tools/bench-ghostty.sh <data file> [runs]
#
# Runs on Linux, or inside WSL (tools/bench-ghostty.ps1 calls it there). It fetches zig 0.17.0
# (the minimum in Ghostty's build.zig.zon) from ziglang.org and clones Ghostty v1.4.0 into one
# scratch folder under $HOME, builds `ghostty-bench` with
#   zig build -Dapp-runtime=none -Demit-bench=true -Doptimize=ReleaseFast
# copies the data file into the scratch folder so it is read from the Linux filesystem, and runs
#   ghostty-bench +terminal-stream --terminal-cols=200 --terminal-rows=60 --data=<file>
# `runs` times (default 3). ghostty-bench prints no timing, so each run is the wall clock time of
# the whole process (start up, terminal set up and file reads included), as in task-2459.
# MB/s is bytes / seconds / 1e6. The last line printed is a JSON row with the runs and the median.
# The scratch folder (the clone, zig, its caches, the build and the data copy) is deleted when the
# script exits, whether it succeeded or not. Nothing is installed system wide.
set -euo pipefail

DATA="${1:?usage: bench-ghostty.sh <data file> [runs]}"
RUNS="${2:-3}"
ZIG_VERSION="0.17.0"
GHOSTTY_TAG="v1.4.0"
GHOSTTY_REPO="https://github.com/ghostty-org/ghostty"
COLS=200
ROWS=60

SCRATCH="$(mktemp -d "$HOME/termoil-bench-ghostty.XXXXXX")"
# Removes the scratch folder this script created.
cleanup() {
  rm -rf -- "$SCRATCH"
}
trap cleanup EXIT

# Downloads and unpacks zig into the scratch folder; prints the path of the zig binary.
fetch_zig() {
  local arch
  arch="$(uname -m)"
  local name="zig-${arch}-linux-${ZIG_VERSION}"
  curl -fsSL -o "$SCRATCH/zig.tar.xz" "https://ziglang.org/download/${ZIG_VERSION}/${name}.tar.xz"
  tar -xJf "$SCRATCH/zig.tar.xz" -C "$SCRATCH"
  rm -f -- "$SCRATCH/zig.tar.xz"
  echo "$SCRATCH/$name/zig"
}

# Clones Ghostty at the tag and builds ghostty-bench with zig $1; prints the path of the binary.
build_ghostty() {
  local zig="$1"
  git clone --quiet --depth 1 --branch "$GHOSTTY_TAG" "$GHOSTTY_REPO" "$SCRATCH/ghostty" 2>/dev/null
  # zig 0.17 has no cache flags on `zig build`; the environment keeps both caches in the scratch folder.
  if ! ZIG_LOCAL_CACHE_DIR="$SCRATCH/zig-cache" ZIG_GLOBAL_CACHE_DIR="$SCRATCH/zig-global-cache" \
      "$zig" build --build-file "$SCRATCH/ghostty/build.zig" --prefix "$SCRATCH/out" -Dapp-runtime=none -Demit-bench=true -Doptimize=ReleaseFast >"$SCRATCH/build.log" 2>&1; then
    tail -40 "$SCRATCH/build.log" >&2
    echo "ghostty build failed" >&2
    exit 1
  fi
  echo "$SCRATCH/out/bin/ghostty-bench"
}

# Runs ghostty-bench $1 on file $2 once; prints MB/s.
run_once() {
  local bench="$1" file="$2" size start end
  size="$(stat -c %s "$file")"
  start="$(date +%s%N)"
  "$bench" +terminal-stream --terminal-cols="$COLS" --terminal-rows="$ROWS" --data="$file" >/dev/null
  end="$(date +%s%N)"
  awk -v b="$size" -v n="$((end - start))" 'BEGIN { printf "%.1f", b / (n / 1e9) / 1e6 }'
}

echo "fetching zig $ZIG_VERSION and Ghostty $GHOSTTY_TAG into $SCRATCH" >&2
ZIG="$(fetch_zig)"
build_start="$(date +%s)"
BENCH="$(build_ghostty "$ZIG")"
echo "built ghostty-bench in $(( $(date +%s) - build_start )) s" >&2

cp -- "$DATA" "$SCRATCH/data.bin"
BYTES="$(stat -c %s "$SCRATCH/data.bin")"
results=()
for i in $(seq 1 "$RUNS"); do
  mbs="$(run_once "$BENCH" "$SCRATCH/data.bin")"
  echo "run $i: $mbs MB/s" >&2
  results+=("$mbs")
done
MEDIAN="$(printf '%s\n' "${results[@]}" | sort -n | sed -n "$(( (RUNS + 1) / 2 ))p")"
echo "Ghostty $GHOSTTY_TAG +terminal-stream ${COLS}x${ROWS}: $MEDIAN MB/s (median of $RUNS, $BYTES bytes)" >&2
RUNS_JSON="$(IFS=,; echo "${results[*]}")"
echo "{\"stream\":\"mixed\",\"bytes\":$BYTES,\"runs_mb_per_s\":[$RUNS_JSON],\"mb_per_s\":$MEDIAN,\"ghostty\":\"$GHOSTTY_TAG\",\"zig\":\"$ZIG_VERSION\",\"cols\":$COLS,\"rows\":$ROWS,\"kernel\":\"$(uname -r)\"}"
