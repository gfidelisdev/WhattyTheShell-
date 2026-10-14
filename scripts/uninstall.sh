#!/usr/bin/env sh
# uninstall.sh - Uninstaller for WhattyTheShell!
set -e

if [ "$(id -u)" -eq 0 ]; then
  PREFIX="${PREFIX:-/usr/local}"
  echo "==> Uninstalling WhattyTheShell! from $PREFIX..."
else
  PREFIX="${PREFIX:-$HOME/.local}"
  echo "==> Uninstalling WhattyTheShell! from $PREFIX..."
fi

BINDIR="$PREFIX/bin"
DATADIR="$PREFIX/share"
APPDIR="$DATADIR/applications"
ICONDIR="$DATADIR/icons/hicolor"
META_DIR="$DATADIR/whattytheshell"

# 1. Remove executables and symlinks
COMMANDS="whatty whatyy whattytheshell whattheshell wts WhattyTheShell"
for cmd in $COMMANDS; do
  if [ -e "$BINDIR/$cmd" ] || [ -L "$BINDIR/$cmd" ]; then
    rm -f "$BINDIR/$cmd"
  fi
done

# 2. Remove desktop file and AppStream metadata
rm -f "$APPDIR/io.github.whattytheshell.desktop"
rm -f "$DATADIR/metainfo/io.github.whattytheshell.metainfo.xml"

# 3. Remove icons
for size in 16 24 32 48 64 128 256 512; do
  rm -f "$ICONDIR/${size}x${size}/apps/io.github.whattytheshell.png"
done

# 4. Remove uninstaller and metadata directory
rm -rf "$META_DIR"

# 5. Update Desktop and Icon caches
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APPDIR" >/dev/null 2>&1 || true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$DATADIR/icons/hicolor" >/dev/null 2>&1 || true
fi

echo "==> WhattyTheShell! has been successfully uninstalled."
