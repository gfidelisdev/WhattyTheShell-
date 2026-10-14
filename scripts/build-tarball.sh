#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DIST_DIR="$PROJECT_ROOT/dist"
STAGE_DIR="$PROJECT_ROOT/build/tarball-stage"

VERSION=$(grep -m1 '^version = ' "$PROJECT_ROOT/Cargo.toml" | cut -d '"' -f 2)
PKG_NAME="WhattyTheShell-${VERSION}-linux-x86_64"
OUTPUT_TAR="$DIST_DIR/${PKG_NAME}.tar.gz"

echo "==> Staging Tarball package for $PKG_NAME..."
rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR/$PKG_NAME/bin"
mkdir -p "$STAGE_DIR/$PKG_NAME/share/applications"
mkdir -p "$STAGE_DIR/$PKG_NAME/share/icons/hicolor"
mkdir -p "$DIST_DIR"

BIN_SRC="$PROJECT_ROOT/target/release/WhattyTheShell"
if [ ! -f "$BIN_SRC" ]; then
  echo "==> Building WhattyTheShell in release mode..."
  cargo build --release --manifest-path "$PROJECT_ROOT/Cargo.toml"
fi

# 1. Binary
cp "$BIN_SRC" "$STAGE_DIR/$PKG_NAME/bin/WhattyTheShell"
chmod 755 "$STAGE_DIR/$PKG_NAME/bin/WhattyTheShell"

# 2. Desktop file
cp "$PROJECT_ROOT/assets/io.github.whattytheshell.desktop" "$STAGE_DIR/$PKG_NAME/share/applications/"
chmod 644 "$STAGE_DIR/$PKG_NAME/share/applications/io.github.whattytheshell.desktop"

# 3. Icons and AppStream metadata
cp -r "$PROJECT_ROOT/assets/icons/hicolor/"* "$STAGE_DIR/$PKG_NAME/share/icons/hicolor/"
mkdir -p "$STAGE_DIR/$PKG_NAME/share/metainfo"
cp "$PROJECT_ROOT/assets/io.github.whattytheshell.metainfo.xml" "$STAGE_DIR/$PKG_NAME/share/metainfo/"

# 4. Installer & Uninstaller scripts
cp "$PROJECT_ROOT/scripts/install.sh" "$STAGE_DIR/$PKG_NAME/install.sh"
cp "$PROJECT_ROOT/scripts/uninstall.sh" "$STAGE_DIR/$PKG_NAME/uninstall.sh"
chmod +x "$STAGE_DIR/$PKG_NAME/install.sh" "$STAGE_DIR/$PKG_NAME/uninstall.sh"

# 5. License & Docs
if [ -f "$PROJECT_ROOT/LICENSE" ]; then
  cp "$PROJECT_ROOT/LICENSE" "$STAGE_DIR/$PKG_NAME/"
fi

# 6. Create Tarball
echo "==> Creating $OUTPUT_TAR..."
tar -czf "$OUTPUT_TAR" -C "$STAGE_DIR" "$PKG_NAME"

echo "==> Tarball created at: $OUTPUT_TAR"
