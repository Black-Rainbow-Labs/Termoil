#!/usr/bin/env bash
# Builds the Linux packages for Terminus: an AppImage and a .deb. Run it on Linux, or in WSL
# from the Windows checkout (tools/release.ps1 does this through wsl.exe).
#
#   installer/linux/build.sh [--skip-build]
#
# Output goes to installer/dist/: terminus-<version>-x86_64.AppImage and
# terminus_<version>_amd64.deb. The printed last two lines are those two paths.
#
# Environment:
#   TERMINUS_VERSION     overrides the version read from the workspace Cargo.toml
#   TERMINUS_BUILD_DATE  the build date text the About box shows (default: today)
#   CARGO_TARGET_DIR     where cargo builds (default: /tmp/terminus-linux-target, on the Linux
#                        file system, because building on /mnt/c under WSL is slow)
#   TERMINUS_PACKAGE_WORK  where the package trees are staged (default: /tmp/terminus-package-work),
#                        on the Linux file system because dpkg-deb refuses the 777 permissions of /mnt/c
#
# Build requirements: the toolchain in rust-toolchain.toml, tic (ncurses-bin), dpkg-deb,
# python3 (to cut the icon sizes out of the .ico), and curl for the first download of appimagetool.
# libfontconfig.so.1 must be installed; its development package is not needed.
set -euo pipefail
shopt -s inherit_errexit

SELF=$(readlink -f "${BASH_SOURCE[0]}")
REPO=$(readlink -f "$(dirname "$SELF")/../..")
DIST="$REPO/installer/dist"
WORK="${TERMINUS_PACKAGE_WORK:-/tmp/terminus-package-work}"
STAGE="$WORK/stage"
TOOLS="$DIST/tools"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/terminus-linux-target}"
export PATH="$HOME/.cargo/bin:$PATH"

# appimagetool is MIT licensed. The version and its SHA-256 are pinned so a changed download is noticed.
APPIMAGETOOL_VERSION=1.9.1
APPIMAGETOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0

skip_build=0
[ "${1:-}" = "--skip-build" ] && skip_build=1

# Prints a step heading.
say() { printf '\n== %s\n' "$*" >&2; }

# Prints the workspace version from Cargo.toml, unless TERMINUS_VERSION is set.
workspace_version() {
    if [ -n "${TERMINUS_VERSION:-}" ]; then echo "$TERMINUS_VERSION"; return; fi
    awk '/^\[workspace.package\]/ {on=1; next} /^\[/ {on=0} on && /^version *=/ {gsub(/[" ]/, "", $3); print $3; exit}' "$REPO/Cargo.toml"
}

# Makes pkg-config find fontconfig on a machine that has the library but not its development
# package. The sys crate only needs something to link against, and libfontconfig.so.1 is that.
ensure_fontconfig_link() {
    if pkg-config --exists fontconfig 2>/dev/null; then return; fi
    local lib
    lib=$(ldconfig -p | awk '/libfontconfig\.so\.1 / && /x86-64/ {print $NF; exit}')
    [ -n "$lib" ] || { echo "libfontconfig.so.1 is not installed (package libfontconfig1)" >&2; exit 1; }
    local shim="$WORK/fontconfig-link"
    mkdir -p "$shim/pkgconfig"
    ln -sf "$lib" "$shim/libfontconfig.so"
    printf 'Name: fontconfig\nDescription: link to the installed runtime library\nVersion: 2.15.0\nLibs: -L%s -lfontconfig\n' "$shim" > "$shim/pkgconfig/fontconfig.pc"
    export PKG_CONFIG_PATH="$shim/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
}

# Builds the two release binaries.
build_binaries() {
    say "cargo build --release"
    ensure_fontconfig_link
    export TERMINUS_BUILD_DATE="${TERMINUS_BUILD_DATE:-$(date +%Y-%m-%d)}"
    cargo build --release -p terminus-cli -p terminus-app --manifest-path "$REPO/Cargo.toml"
}

# Writes the PNG icon sizes found inside the .ico into the hicolor icon theme under $1.
extract_icons() {
    python3 - "$REPO/installer/windows/terminus.ico" "$1" <<'PY'
import os, struct, sys
ico, root = sys.argv[1], sys.argv[2]
data = open(ico, 'rb').read()
count = struct.unpack_from('<H', data, 4)[0]
for i in range(count):
    w, h, _, _, _, _, size, offset = struct.unpack_from('<BBBBHHII', data, 6 + 16 * i)
    px = w or 256
    png = data[offset:offset + size]
    if not png.startswith(b'\x89PNG'):
        continue
    folder = os.path.join(root, 'usr/share/icons/hicolor', f'{px}x{px}', 'apps')
    os.makedirs(folder, exist_ok=True)
    open(os.path.join(folder, 'terminus.png'), 'wb').write(png)
PY
}

# Writes the .desktop file to the path in $1.
write_desktop_file() {
    cat > "$1" <<'EOF'
[Desktop Entry]
Type=Application
Name=Terminus
GenericName=Terminal
Comment=A fast terminal emulator
Exec=terminus-gui
Icon=terminus
Terminal=false
Categories=System;TerminalEmulator;
Keywords=shell;prompt;command;commandline;cmd;
StartupWMClass=terminus
EOF
}

# Fills the staging tree $1 with everything both packages install under /usr.
stage_files() {
    local root="$1"
    local release="$CARGO_TARGET_DIR/release"
    rm -rf "$root"
    mkdir -p "$root/usr/bin" "$root/usr/share/terminus" "$root/usr/share/applications" \
        "$root/usr/share/terminfo/x" "$root/usr/share/doc/terminus"
    install -m 755 "$release/terminus" "$release/terminus-gui" "$root/usr/bin/"
    strip --strip-unneeded "$root/usr/bin/terminus" "$root/usr/bin/terminus-gui"
    cp -a "$REPO/shell-integration" "$root/usr/share/terminus/shell-integration"
    find "$root/usr/share/terminus" -type d -exec chmod 755 {} +
    find "$root/usr/share/terminus" -type f -exec chmod 644 {} +
    write_desktop_file "$root/usr/share/applications/terminus.desktop"
    chmod 644 "$root/usr/share/applications/terminus.desktop"
    extract_icons "$root"
    tic -x -o "$root/usr/share/terminfo" "$REPO/shell-integration/terminfo/xterm-terminus.terminfo"
    install -m 644 "$REPO/LICENSE-MIT" "$REPO/LICENSE-APACHE" "$root/usr/share/doc/terminus/"
    write_copyright "$root/usr/share/doc/terminus/copyright"
}

# Writes the Debian copyright file to the path in $1.
write_copyright() {
    cat > "$1" <<'EOF'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: Terminus

Files: *
Copyright: Terminus contributors
License: MIT or Apache-2.0
 Terminus is licensed under either of the MIT licence or the Apache licence 2.0, at your option.
 The full texts are in /usr/share/doc/terminus/LICENSE-MIT and
 /usr/share/doc/terminus/LICENSE-APACHE.
EOF
    chmod 644 "$1"
}

# Prints the installed size of the staging tree $1 in kibibytes, for the control file.
installed_size() { du -sk --exclude=DEBIAN "$1" | cut -f1; }

# Writes DEBIAN/control and DEBIAN/md5sums into the package tree $1 for version $2.
write_deb_metadata() {
    local root="$1" version="$2"
    mkdir -p "$root/DEBIAN"
    cat > "$root/DEBIAN/control" <<EOF
Package: terminus
Version: $version
Section: x11
Priority: optional
Architecture: amd64
Installed-Size: $(installed_size "$root")
Depends: libc6 (>= 2.35), libfontconfig1, libxkbcommon0, libwayland-client0 | libx11-6
Recommends: libvulkan1, libxkbcommon-x11-0, libxcursor1, libwayland-cursor0, libwayland-egl1, xdg-utils
Maintainer: Terminus contributors <noreply@terminus.invalid>
Homepage: https://github.com/Black-Rainbow-Labs/Terminus
Description: Terminal emulator
 A GPU accelerated terminal emulator with tabs and splits, shell integration for bash,
 zsh, fish, nushell and PowerShell, a command line client (terminus) that controls a
 running window, and the kitty graphics and keyboard protocols.
EOF
    find "$root/usr" -type f -exec md5sum {} + | sed "s| $root/| |" > "$root/DEBIAN/md5sums"
}


# Builds the .deb from the staging tree and prints its path.
build_deb() {
    local version="$1"
    local root="$WORK/deb-root"
    say "deb"
    rm -rf "$root"
    cp -a "$STAGE/root" "$root"
    write_deb_metadata "$root" "$version"
    local out="$DIST/terminus_${version}_amd64.deb"
    dpkg-deb --root-owner-group -Zxz --build "$root" "$out" >&2
    echo "$out"
}

# Downloads appimagetool into the tools folder when it is missing and returns its path.
fetch_appimagetool() {
    local tool="$TOOLS/appimagetool-$APPIMAGETOOL_VERSION-x86_64.AppImage"
    mkdir -p "$TOOLS"
    if [ ! -f "$tool" ]; then
        curl -fsSL -o "$tool" "https://github.com/AppImage/appimagetool/releases/download/$APPIMAGETOOL_VERSION/appimagetool-x86_64.AppImage"
    fi
    echo "$APPIMAGETOOL_SHA256  $tool" | sha256sum -c - >&2
    chmod +x "$tool"
    echo "$tool"
}

# Writes the AppRun script into the AppDir $1. A first argument beginning with + is a command
# for the terminus client, so Terminus.AppImage +new-tab works the same as terminus +new-tab.
write_apprun() {
    cat > "$1/AppRun" <<'EOF'
#!/bin/sh
HERE=$(dirname "$(readlink -f "$0")")
export PATH="$HERE/usr/bin:$PATH"
export TERMINFO_DIRS="$HERE/usr/share/terminfo:${TERMINFO_DIRS:-/usr/share/terminfo:/lib/terminfo:/etc/terminfo}"
case "${1:-}" in
    +*|--version|--help) exec "$HERE/usr/bin/terminus" "$@" ;;
esac
exec "$HERE/usr/bin/terminus-gui" "$@"
EOF
    chmod 755 "$1/AppRun"
}

# Builds the AppImage from the staging tree and prints its path.
build_appimage() {
    local version="$1"
    local appdir="$WORK/Terminus.AppDir"
    say "AppImage"
    rm -rf "$appdir"
    mkdir -p "$appdir"
    cp -a "$STAGE/root/usr" "$appdir/usr"
    cp "$STAGE/root/usr/share/applications/terminus.desktop" "$appdir/terminus.desktop"
    cp "$STAGE/root/usr/share/icons/hicolor/256x256/apps/terminus.png" "$appdir/terminus.png"
    ln -sf terminus.png "$appdir/.DirIcon"
    write_apprun "$appdir"
    local tool out="$DIST/terminus-${version}-x86_64.AppImage"
    tool=$(fetch_appimagetool)
    ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$tool" --no-appstream "$appdir" "$out" >&2
    echo "$out"
}

# Builds everything and prints the two package paths.
main() {
    mkdir -p "$DIST" "$WORK"
    local version
    version=$(workspace_version)
    [ "$skip_build" = 1 ] || build_binaries
    say "staging version $version"
    stage_files "$STAGE/root"
    local deb appimage
    deb=$(build_deb "$version")
    appimage=$(build_appimage "$version")
    echo "$appimage"
    echo "$deb"
}

main
