#!/usr/bin/env sh
# install.sh - Installer for WhattyTheShell!
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Detect installation scope: system (/usr/local) vs user ($HOME/.local)
if [ "$(id -u)" -eq 0 ]; then
  PREFIX="${PREFIX:-/usr/local}"
  echo "==> Installing WhattyTheShell! system-wide into $PREFIX..."
else
  PREFIX="${PREFIX:-$HOME/.local}"
  echo "==> Installing WhattyTheShell! for user $(whoami) into $PREFIX..."
fi

BINDIR="$PREFIX/bin"
DATADIR="$PREFIX/share"
APPDIR="$DATADIR/applications"
ICONDIR="$DATADIR/icons/hicolor"
META_DIR="$DATADIR/whattytheshell"

# 1. Create target directories
mkdir -p "$BINDIR" "$APPDIR" "$META_DIR"

# 2. Install executable
if [ -f "$SCRIPT_DIR/bin/WhattyTheShell" ]; then
  SRC_BIN="$SCRIPT_DIR/bin/WhattyTheShell"
elif [ -f "$SCRIPT_DIR/bin/whatty" ]; then
  SRC_BIN="$SCRIPT_DIR/bin/whatty"
elif [ -f "$SCRIPT_DIR/WhattyTheShell" ]; then
  SRC_BIN="$SCRIPT_DIR/WhattyTheShell"
elif [ -f "$SCRIPT_DIR/whatty" ]; then
  SRC_BIN="$SCRIPT_DIR/whatty"
elif [ -f "$SCRIPT_DIR/../target/release/WhattyTheShell" ]; then
  SRC_BIN="$SCRIPT_DIR/../target/release/WhattyTheShell"
elif [ -f "$SCRIPT_DIR/../target/debug/WhattyTheShell" ]; then
  SRC_BIN="$SCRIPT_DIR/../target/debug/WhattyTheShell"
else
  echo "Error: Binary WhattyTheShell not found in $SCRIPT_DIR!" >&2
  exit 1
fi

cp "$SRC_BIN" "$BINDIR/whatty"
chmod 755 "$BINDIR/whatty"

# 3. Create command aliases
ALIASES="whatyy whattytheshell whattheshell wts WhattyTheShell"
for alias in $ALIASES; do
  ln -sf "whatty" "$BINDIR/$alias"
done

# 4. Install Desktop Entry
if [ -f "$SCRIPT_DIR/share/applications/io.github.whattytheshell.desktop" ]; then
  cp "$SCRIPT_DIR/share/applications/io.github.whattytheshell.desktop" "$APPDIR/"
elif [ -f "$SCRIPT_DIR/assets/io.github.whattytheshell.desktop" ]; then
  cp "$SCRIPT_DIR/assets/io.github.whattytheshell.desktop" "$APPDIR/"
elif [ -f "$SCRIPT_DIR/../assets/io.github.whattytheshell.desktop" ]; then
  cp "$SCRIPT_DIR/../assets/io.github.whattytheshell.desktop" "$APPDIR/"
elif [ -f "$SCRIPT_DIR/io.github.whattytheshell.desktop" ]; then
  cp "$SCRIPT_DIR/io.github.whattytheshell.desktop" "$APPDIR/"
fi
chmod 644 "$APPDIR/io.github.whattytheshell.desktop"

# 5. Install Icons
for size in 16 24 32 48 64 128 256 512; do
  for icon_name in io.github.whattytheshell.png io.github.whattytheshell-classic.png; do
    SRC_ICON=""
    if [ -f "$SCRIPT_DIR/share/icons/hicolor/${size}x${size}/apps/$icon_name" ]; then
      SRC_ICON="$SCRIPT_DIR/share/icons/hicolor/${size}x${size}/apps/$icon_name"
    elif [ -f "$SCRIPT_DIR/assets/icons/hicolor/${size}x${size}/apps/$icon_name" ]; then
      SRC_ICON="$SCRIPT_DIR/assets/icons/hicolor/${size}x${size}/apps/$icon_name"
    elif [ -f "$SCRIPT_DIR/../assets/icons/hicolor/${size}x${size}/apps/$icon_name" ]; then
      SRC_ICON="$SCRIPT_DIR/../assets/icons/hicolor/${size}x${size}/apps/$icon_name"
    fi

    if [ -n "$SRC_ICON" ]; then
      mkdir -p "$ICONDIR/${size}x${size}/apps"
      cp "$SRC_ICON" "$ICONDIR/${size}x${size}/apps/$icon_name"
      chmod 644 "$ICONDIR/${size}x${size}/apps/$icon_name"
    fi
  done
done

# 6. Install AppStream metadata
METAINFO_DIR="$DATADIR/metainfo"
mkdir -p "$METAINFO_DIR"
if [ -f "$SCRIPT_DIR/share/metainfo/io.github.whattytheshell.metainfo.xml" ]; then
  cp "$SCRIPT_DIR/share/metainfo/io.github.whattytheshell.metainfo.xml" "$METAINFO_DIR/"
  chmod 644 "$METAINFO_DIR/io.github.whattytheshell.metainfo.xml"
elif [ -f "$SCRIPT_DIR/assets/io.github.whattytheshell.metainfo.xml" ]; then
  cp "$SCRIPT_DIR/assets/io.github.whattytheshell.metainfo.xml" "$METAINFO_DIR/"
  chmod 644 "$METAINFO_DIR/io.github.whattytheshell.metainfo.xml"
fi

# 7. Install uninstaller copy
if [ -f "$SCRIPT_DIR/uninstall.sh" ]; then
  cp "$SCRIPT_DIR/uninstall.sh" "$META_DIR/uninstall.sh"
  chmod 755 "$META_DIR/uninstall.sh"
fi

# 8. Update Desktop and Icon caches
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APPDIR" >/dev/null 2>&1 || true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$DATADIR/icons/hicolor" >/dev/null 2>&1 || true
fi

echo "==> Installation complete!"
echo "    Main command: $BINDIR/whatty"
echo "    Aliases:      whatyy, whattytheshell, whattheshell, wts, WhattyTheShell"
echo "    Desktop icon: Added to application menu as 'WhattyTheShell!'"
if [ "$(id -u)" -ne 0 ]; then
  case ":$PATH:" in
    *:"$BINDIR":*) ;;
    *) echo "    Note: Make sure $BINDIR is in your PATH." ;;
  esac
fi
echo "    Uninstaller saved at: $META_DIR/uninstall.sh"
