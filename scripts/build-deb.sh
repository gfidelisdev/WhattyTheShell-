#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DIST_DIR="$PROJECT_ROOT/dist"
STAGE_DIR="$PROJECT_ROOT/build/deb-stage"

VERSION=$(grep -m1 '^version = ' "$PROJECT_ROOT/Cargo.toml" | cut -d '"' -f 2)
PKG_NAME="whattytheshell_${VERSION}_amd64"
DEB_FILE="$DIST_DIR/${PKG_NAME}.deb"

echo "==> Staging Debian package $PKG_NAME..."
rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR/DEBIAN"
mkdir -p "$STAGE_DIR/usr/bin"
mkdir -p "$STAGE_DIR/usr/share/applications"
mkdir -p "$STAGE_DIR/usr/share/icons/hicolor"
mkdir -p "$DIST_DIR"

BIN_SRC="$PROJECT_ROOT/target/release/WhattyTheShell"
if [ ! -f "$BIN_SRC" ]; then
  echo "==> Building WhattyTheShell in release mode..."
  cargo build --release --manifest-path "$PROJECT_ROOT/Cargo.toml"
fi

# 1. Install binary and symlinks
cp "$BIN_SRC" "$STAGE_DIR/usr/bin/whatty"
chmod 755 "$STAGE_DIR/usr/bin/whatty"

for alias in whatyy whattytheshell whattheshell wts WhattyTheShell; do
  ln -sf "whatty" "$STAGE_DIR/usr/bin/$alias"
done

# 2. Desktop file
cp "$PROJECT_ROOT/assets/io.github.whattytheshell.desktop" "$STAGE_DIR/usr/share/applications/"
chmod 644 "$STAGE_DIR/usr/share/applications/io.github.whattytheshell.desktop"

# 3. Icons and AppStream metadata
cp -r "$PROJECT_ROOT/assets/icons/hicolor/"* "$STAGE_DIR/usr/share/icons/hicolor/"
mkdir -p "$STAGE_DIR/usr/share/metainfo"
cp "$PROJECT_ROOT/assets/io.github.whattytheshell.metainfo.xml" "$STAGE_DIR/usr/share/metainfo/"
chmod 644 "$STAGE_DIR/usr/share/metainfo/io.github.whattytheshell.metainfo.xml"

# 4. DEBIAN/control
INSTALLED_SIZE=$(du -sk "$STAGE_DIR" | cut -f1)
cat << EOF > "$STAGE_DIR/DEBIAN/control"
Package: whattytheshell
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: amd64
Installed-Size: ${INSTALLED_SIZE}
Maintainer: WhattyTheShell Contributors <noreply@github.com>
Depends: libgtk-4-1 (>= 4.14), libvulkan1
Description: GTK4 GPU-accelerated terminal emulator with Vulkan rendering
 WhattyTheShell is a modern GTK4 terminal emulator featuring zero-copy
 Vulkan dmabuf rendering, tab management, and VS Code themes.
EOF

# 5. DEBIAN/postinst & postrm for desktop database & icon cache update
cat << 'EOF' > "$STAGE_DIR/DEBIAN/postinst"
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi
exit 0
EOF
chmod 755 "$STAGE_DIR/DEBIAN/postinst"

cat << 'EOF' > "$STAGE_DIR/DEBIAN/postrm"
#!/bin/sh
set -e
if [ "$1" = "remove" ] || [ "$1" = "purge" ]; then
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database -q /usr/share/applications || true
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
    fi
fi
exit 0
EOF
chmod 755 "$STAGE_DIR/DEBIAN/postrm"

# 6. Build package using dpkg-deb or cargo-deb
if command -v dpkg-deb >/dev/null 2>&1; then
  echo "==> Building DEB package with dpkg-deb..."
  dpkg-deb --build --root-owner-group "$STAGE_DIR" "$DEB_FILE"
elif command -v cargo-deb >/dev/null 2>&1; then
  echo "==> Building DEB package with cargo-deb..."
  cargo deb --no-build --output "$DIST_DIR" --manifest-path "$PROJECT_ROOT/Cargo.toml"
else
  echo "Error: neither dpkg-deb nor cargo-deb found!" >&2
  exit 1
fi

echo "==> DEB package created at: $DEB_FILE"
