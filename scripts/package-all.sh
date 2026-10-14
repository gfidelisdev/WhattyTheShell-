#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DIST_DIR="$PROJECT_ROOT/dist"

mkdir -p "$DIST_DIR"

echo "=========================================="
echo " Building WhattyTheShell! Release Packages"
echo "=========================================="

echo "==> Step 1: Compiling release binary..."
cargo build --release --manifest-path "$PROJECT_ROOT/Cargo.toml"

echo "==> Step 2: Building DEB package..."
"$SCRIPT_DIR/build-deb.sh"

echo "==> Step 3: Building RPM package..."
"$SCRIPT_DIR/build-rpm.sh"

echo "==> Step 4: Building AppImage package..."
"$SCRIPT_DIR/build-appimage.sh"

echo "==> Step 5: Building Tarball with installer..."
"$SCRIPT_DIR/build-tarball.sh"

echo "==> Step 6: Calculating checksums..."
cd "$DIST_DIR"
rm -f SHA256SUMS.txt
sha256sum *.{deb,rpm,AppImage,tar.gz} 2>/dev/null > SHA256SUMS.txt || true

echo "=========================================="
echo " Build completed successfully!"
echo " Artifacts located in: $DIST_DIR"
ls -lh "$DIST_DIR"
echo "=========================================="
