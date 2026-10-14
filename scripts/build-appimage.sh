#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DIST_DIR="$PROJECT_ROOT/dist"
APPDIR="$PROJECT_ROOT/build/AppDir"

echo "==> Preparing AppDir for AppImage build..."
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor"
mkdir -p "$APPDIR/usr/share/glib-2.0/schemas"
mkdir -p "$DIST_DIR"

BIN_SRC="$PROJECT_ROOT/target/release/WhattyTheShell"
if [ ! -f "$BIN_SRC" ]; then
  echo "==> Building WhattyTheShell in release mode..."
  cargo build --release --manifest-path "$PROJECT_ROOT/Cargo.toml"
fi

# 1. Install binary and aliases into AppDir
cp "$BIN_SRC" "$APPDIR/usr/bin/whatty"
chmod 755 "$APPDIR/usr/bin/whatty"

for alias in whatyy whattytheshell whattheshell wts WhattyTheShell; do
  ln -sf "whatty" "$APPDIR/usr/bin/$alias"
done

# 2. Desktop file and root desktop symlink
cp "$PROJECT_ROOT/assets/io.github.whattytheshell.desktop" "$APPDIR/usr/share/applications/"
cp "$PROJECT_ROOT/assets/io.github.whattytheshell.desktop" "$APPDIR/io.github.whattytheshell.desktop"

# 3. Icons and AppStream metadata
cp -r "$PROJECT_ROOT/assets/icons/hicolor/"* "$APPDIR/usr/share/icons/hicolor/"
cp "$PROJECT_ROOT/assets/icons/hicolor/256x256/apps/io.github.whattytheshell.png" "$APPDIR/io.github.whattytheshell.png"
mkdir -p "$APPDIR/usr/share/metainfo"
cp "$PROJECT_ROOT/assets/io.github.whattytheshell.metainfo.xml" "$APPDIR/usr/share/metainfo/"

# 4. GSettings schemas (GTK4 needs schemas compiled)
if [ -d "/usr/share/glib-2.0/schemas" ]; then
  cp -r /usr/share/glib-2.0/schemas/* "$APPDIR/usr/share/glib-2.0/schemas/" 2>/dev/null || true
  if command -v glib-compile-schemas >/dev/null 2>&1; then
    glib-compile-schemas "$APPDIR/usr/share/glib-2.0/schemas" || true
  fi
fi

# 5. AppRun script
cat << 'EOF' > "$APPDIR/AppRun"
#!/usr/bin/env bash
set -e

HERE="$(dirname "$(readlink -f "${0}")")"

export PATH="${HERE}/usr/bin:${PATH}"
export XDG_DATA_DIRS="${HERE}/usr/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"

if [ -d "${HERE}/usr/share/glib-2.0/schemas" ]; then
  export GSETTINGS_SCHEMA_DIR="${HERE}/usr/share/glib-2.0/schemas:${GSETTINGS_SCHEMA_DIR}"
fi

# Do not bundle host GPU/Mesa/Vulkan drivers so hardware acceleration matches kernel modules
if [ -d "${HERE}/usr/lib" ]; then
  export LD_LIBRARY_PATH="${HERE}/usr/lib:${LD_LIBRARY_PATH}"
fi

exec "${HERE}/usr/bin/whatty" "$@"
EOF

chmod +x "$APPDIR/AppRun"

# 6. Ensure appimagetool is available
APPIMAGETOOL="${APPIMAGETOOL:-appimagetool}"
if ! command -v "$APPIMAGETOOL" >/dev/null 2>&1; then
  echo "==> appimagetool not found in PATH. Checking local cache..."
  TOOL_PATH="$PROJECT_ROOT/build/appimagetool"
  if [ ! -f "$TOOL_PATH" ]; then
    echo "==> Downloading appimagetool..."
    mkdir -p "$PROJECT_ROOT/build"
    curl -sSL "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage" -o "$TOOL_PATH"
    chmod +x "$TOOL_PATH"
  fi
  APPIMAGETOOL="$TOOL_PATH"
fi

VERSION=$(grep -m1 '^version = ' "$PROJECT_ROOT/Cargo.toml" | cut -d '"' -f 2)
OUTPUT_APPIMAGE="$DIST_DIR/WhattyTheShell-${VERSION}-x86_64.AppImage"

echo "==> Generating AppImage with $APPIMAGETOOL..."
export ARCH=x86_64
# If running inside Docker/container or environment without FUSE:
if [ -n "$APPIMAGE_EXTRACT_AND_RUN" ] || [ ! -e /dev/fuse ]; then
  export APPIMAGE_EXTRACT_AND_RUN=1
fi

"$APPIMAGETOOL" "$APPDIR" "$OUTPUT_APPIMAGE"

echo "==> AppImage created at: $OUTPUT_APPIMAGE"
