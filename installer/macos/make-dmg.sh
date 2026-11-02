#!/usr/bin/env bash
# Writes a compressed disk image (.dmg) holding one folder, on a machine with no macOS.
#
#   make-dmg.sh <source folder> <output.dmg> <volume name> [executable path inside the folder ...]
#
# The source folder's contents (for Terminus: Terminus.app) become the volume's root. The steps:
#   1. mkfs.hfsplus makes an empty HFS+ filesystem in a file sized to the folder plus headroom.
#   2. hfsplus (libdmg-hfsplus, planetbeing) copies the folder in with addall, then chmod sets the
#      executable bit on each path given after the volume name, because addall writes mode 644.
#   3. fsck.hfsplus checks the filesystem and the script stops if it is not clean.
#   4. dmg (libdmg-hfsplus, fanquake's fork) wraps the filesystem in a zlib compressed UDIF image.
#
# Tools: mkfs.hfsplus and fsck.hfsplus from the hfsprogs package on PATH, and the two libdmg
# programs under $DMG_TOOLS (default /opt/libdmg), which installer/macos/fetch-dmg-tools.ps1 builds.
# Run it under WSL: tools/release.ps1 and build-on-windows.ps1 do.
set -euo pipefail

DMG_TOOLS="${DMG_TOOLS:-/opt/libdmg}"
HFSPLUS="$DMG_TOOLS/pb/build/hfs/hfsplus"
DMG="$DMG_TOOLS/src/build/dmg/dmg"

# Prints an error and stops.
die() { echo "make-dmg.sh: $*" >&2; exit 1; }

[ $# -ge 3 ] || die "usage: make-dmg.sh <source folder> <output.dmg> <volume name> [executable ...]"
SOURCE=$1; OUTPUT=$2; VOLNAME=$3; shift 3
for tool in mkfs.hfsplus fsck.hfsplus "$HFSPLUS" "$DMG"; do
    command -v "$tool" >/dev/null 2>&1 || [ -x "$tool" ] || die "$tool is missing; run installer/macos/fetch-dmg-tools.ps1"
done
[ -d "$SOURCE" ] || die "$SOURCE is not a folder"

WORK=$(mktemp -d /tmp/terminus-dmg.XXXXXX)
trap 'rm -f "$WORK/volume.img"; rmdir "$WORK" 2>/dev/null || true' EXIT
IMAGE="$WORK/volume.img"

# The image size: the folder's size in megabytes, a third again for the catalog and allocation
# overhead, and 16 MB of headroom.
bytes=$(du -sb "$SOURCE" | cut -f1)
megabytes=$(( bytes / 1048576 * 4 / 3 + 16 ))
truncate -s "${megabytes}M" "$IMAGE"
mkfs.hfsplus -v "$VOLNAME" "$IMAGE" >/dev/null

"$HFSPLUS" "$IMAGE" addall "$SOURCE" / >/dev/null
for executable in "$@"; do
    "$HFSPLUS" "$IMAGE" chmod 755 "/$executable" >/dev/null
done

fsck.hfsplus -fn "$IMAGE" | tail -1
fsck.hfsplus -fn "$IMAGE" | grep -q "appears to be OK" || die "the HFS+ filesystem did not pass fsck.hfsplus"

rm -f "$OUTPUT"
"$DMG" "$IMAGE" "$OUTPUT" >/dev/null
ls -l "$OUTPUT"
