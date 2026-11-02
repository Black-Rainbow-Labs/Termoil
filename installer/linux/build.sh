#!/usr/bin/env bash
# Builds the Linux packages for Termoil: an AppImage and a .deb. Run it on Linux, or in WSL
# from the Windows checkout (tools/release.ps1 does this through wsl.exe).
#
#   installer/linux/build.sh [--skip-build]
#
# Output goes to installer/dist/: termoil-<version>-x86_64.AppImage and
# termoil_<version>_amd64.deb. The printed last two lines are those two paths.
#
# Environment:
#   TERMOIL_VERSION     overrides the version read from the workspace Cargo.toml
#   TERMOIL_BUILD_DATE  the build date text the About box shows (default: today)
#   CARGO_TARGET_DIR     where cargo builds (default: /tmp/termoil-linux-target, on the Linux
#                        file system, because building on /mnt/c under WSL is slow)
#   TERMOIL_PACKAGE_WORK  where the package trees are staged (default: /tmp/termoil-package-work),
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
WORK="${TERMOIL_PACKAGE_WORK:-/tmp/termoil-package-work}"
STAGE="$WORK/stage"
TOOLS="$DIST/tools"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/termoil-linux-target}"
export PATH="$HOME/.cargo/bin:$PATH"

# appimagetool is MIT licensed. The version and its SHA-256 are pinned so a changed download is noticed.
APPIMAGETOOL_VERSION=1.9.1
APPIMAGETOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0

skip_build=0
[ "${1:-}" = "--skip-build" ] && skip_build=1

# Prints a step heading.
say() { printf '\n== %s\n' "$*" >&2; }

# Prints the workspace version from Cargo.toml, unless TERMOIL_VERSION is set.
workspace_version() {
    if [ -n "${TERMOIL_VERSION:-}" ]; then echo "$TERMOIL_VERSION"; return; fi
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
    export TERMOIL_BUILD_DATE="${TERMOIL_BUILD_DATE:-$(date +%Y-%m-%d)}"
    cargo build --release -p termoil-cli -p termoil-app --manifest-path "$REPO/Cargo.toml"
}

# Writes the PNG icon sizes found inside the .ico into the hicolor icon theme under $1.
extract_icons() {
    python3 - "$REPO/installer/windows/termoil.ico" "$1" <<'PY'
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
    open(os.path.join(folder, 'termoil.png'), 'wb').write(png)
PY
}

# Writes the .desktop file to the path in $1.
write_desktop_file() {
    cat > "$1" <<'EOF'
[Desktop Entry]
Type=Application
Name=Termoil
GenericName=Terminal
Comment=A fast terminal emulator
Exec=termoil-gui
Icon=termoil
Terminal=false
Categories=System;TerminalEmulator;
Keywords=shell;prompt;command;commandline;cmd;
StartupWMClass=termoil
EOF
}

# Fills the staging tree $1 with everything both packages install under /usr.
stage_files() {
    local root="$1"
    local release="$CARGO_TARGET_DIR/release"
    rm -rf "$root"
    mkdir -p "$root/usr/bin" "$root/usr/share/termoil" "$root/usr/share/applications" \
        "$root/usr/share/terminfo/x" "$root/usr/share/doc/termoil"
    install -m 755 "$release/termoil" "$release/termoil-gui" "$root/usr/bin/"
    strip --strip-unneeded "$root/usr/bin/termoil" "$root/usr/bin/termoil-gui"
    cp -a "$REPO/shell-integration" "$root/usr/share/termoil/shell-integration"
    find "$root/usr/share/termoil" -type d -exec chmod 755 {} +
    find "$root/usr/share/termoil" -type f -exec chmod 644 {} +
    write_desktop_file "$root/usr/share/applications/termoil.desktop"
    chmod 644 "$root/usr/share/applications/termoil.desktop"
    extract_icons "$root"
    tic -x -o "$root/usr/share/terminfo" "$REPO/shell-integration/terminfo/xterm-termoil.terminfo"
    install -m 644 "$REPO/LICENSE-MIT" "$REPO/LICENSE-APACHE" "$root/usr/share/doc/termoil/"
    write_copyright "$root/usr/share/doc/termoil/copyright"
}

# Writes the Debian copyright file to the path in $1.
write_copyright() {
    cat > "$1" <<'EOF'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: Termoil

Files: *
Copyright: Termoil contributors
License: MIT or Apache-2.0
 Termoil is licensed under either of the MIT licence or the Apache licence 2.0, at your option.
 The full texts are in /usr/share/doc/termoil/LICENSE-MIT and
 /usr/share/doc/termoil/LICENSE-APACHE.
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
Package: termoil
Version: $version
Section: x11
Priority: optional
Architecture: amd64
Installed-Size: $(installed_size "$root")
Depends: libc6 (>= 2.35), libfontconfig1, libxkbcommon0, libwayland-client0 | libx11-6
Recommends: libvulkan1, libxkbcommon-x11-0, libxcursor1, libwayland-cursor0, libwayland-egl1, xdg-utils
Maintainer: Termoil contributors <noreply@termoil.invalid>
Homepage: https://github.com/Black-Rainbow-Labs/Termoil
Description: Terminal emulator
 A GPU accelerated terminal emulator with tabs and splits, shell integration for bash,
 zsh, fish, nushell and PowerShell, a command line client (termoil) that controls a
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
    local out="$DIST/termoil_${version}_amd64.deb"
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
# for the termoil client, so Termoil.AppImage +new-tab works the same as termoil +new-tab.
write_apprun() {
    cat > "$1/AppRun" <<'EOF'
#!/bin/sh
HERE=$(dirname "$(readlink -f "$0")")
export PATH="$HERE/usr/bin:$PATH"
export TERMINFO_DIRS="$HERE/usr/share/terminfo:${TERMINFO_DIRS:-/usr/share/terminfo:/lib/terminfo:/etc/terminfo}"
case "${1:-}" in
    +*|--version|--help) exec "$HERE/usr/bin/termoil" "$@" ;;
esac
exec "$HERE/usr/bin/termoil-gui" "$@"
EOF
    chmod 755 "$1/AppRun"
}

# Builds the AppImage from the staging tree and prints its path.
build_appimage() {
    local version="$1"
    local appdir="$WORK/Termoil.AppDir"
    say "AppImage"
    rm -rf "$appdir"
    mkdir -p "$appdir"
    cp -a "$STAGE/root/usr" "$appdir/usr"
    cp "$STAGE/root/usr/share/applications/termoil.desktop" "$appdir/termoil.desktop"
    cp "$STAGE/root/usr/share/icons/hicolor/256x256/apps/termoil.png" "$appdir/termoil.png"
    ln -sf termoil.png "$appdir/.DirIcon"
    write_apprun "$appdir"
    local tool out="$DIST/termoil-${version}-x86_64.AppImage"
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
