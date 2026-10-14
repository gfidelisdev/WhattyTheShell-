#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DIST_DIR="$PROJECT_ROOT/dist"
RPMBUILD_DIR="$PROJECT_ROOT/build/rpmbuild"

VERSION=$(grep -m1 '^version = ' "$PROJECT_ROOT/Cargo.toml" | cut -d '"' -f 2)

echo "==> Staging RPM package for whattytheshell version $VERSION..."
rm -rf "$RPMBUILD_DIR"
mkdir -p "$RPMBUILD_DIR"/{BUILD,RPMS,SOURCES,SPECS,SRPMS,BUILDROOT}
mkdir -p "$DIST_DIR"

BIN_SRC="$PROJECT_ROOT/target/release/WhattyTheShell"
if [ ! -f "$BIN_SRC" ]; then
  echo "==> Building WhattyTheShell in release mode..."
  cargo build --release --manifest-path "$PROJECT_ROOT/Cargo.toml"
fi

# Stage files into SOURCES directory
cp "$BIN_SRC" "$RPMBUILD_DIR/SOURCES/WhattyTheShell"
cp "$PROJECT_ROOT/assets/io.github.whattytheshell.desktop" "$RPMBUILD_DIR/SOURCES/io.github.whattytheshell.desktop"
mkdir -p "$RPMBUILD_DIR/SOURCES/icons"
cp -r "$PROJECT_ROOT/assets/icons/hicolor" "$RPMBUILD_DIR/SOURCES/icons/"
mkdir -p "$RPMBUILD_DIR/SOURCES/metainfo"
cp "$PROJECT_ROOT/assets/io.github.whattytheshell.metainfo.xml" "$RPMBUILD_DIR/SOURCES/metainfo/"

# Generate spec file
SPEC_FILE="$RPMBUILD_DIR/SPECS/whattytheshell.spec"
cat << EOF > "$SPEC_FILE"
Name:           whattytheshell
Version:        ${VERSION}
Release:        1%{?dist}
Summary:        GTK4 GPU-accelerated terminal emulator with Vulkan rendering
License:        MIT
URL:            https://github.com/gfidelis/WhattyTheShell
AutoReqProv:    yes
Requires:       gtk4 >= 4.14, vulkan-loader

%description
WhattyTheShell is a modern GTK4 terminal emulator featuring zero-copy
Vulkan dmabuf rendering, tab management, and VS Code themes.

%prep

%build

%install
rm -rf %{buildroot}
mkdir -p %{buildroot}/usr/bin
mkdir -p %{buildroot}/usr/share/applications
mkdir -p %{buildroot}/usr/share/icons/hicolor
mkdir -p %{buildroot}/usr/share/metainfo

cp %{_sourcedir}/WhattyTheShell %{buildroot}/usr/bin/whatty
chmod 755 %{buildroot}/usr/bin/whatty

for alias in whatyy whattytheshell whattheshell wts WhattyTheShell; do
  ln -sf whatty %{buildroot}/usr/bin/\$alias
done

cp %{_sourcedir}/io.github.whattytheshell.desktop %{buildroot}/usr/share/applications/
chmod 644 %{buildroot}/usr/share/applications/io.github.whattytheshell.desktop

cp -r %{_sourcedir}/icons/hicolor/* %{buildroot}/usr/share/icons/hicolor/

cp %{_sourcedir}/metainfo/io.github.whattytheshell.metainfo.xml %{buildroot}/usr/share/metainfo/
chmod 644 %{buildroot}/usr/share/metainfo/io.github.whattytheshell.metainfo.xml

%files
/usr/bin/whatty
/usr/bin/whatyy
/usr/bin/whattytheshell
/usr/bin/whattheshell
/usr/bin/wts
/usr/bin/WhattyTheShell
/usr/share/applications/io.github.whattytheshell.desktop
/usr/share/icons/hicolor/*/apps/io.github.whattytheshell.png
/usr/share/metainfo/io.github.whattytheshell.metainfo.xml

%post
if [ -x /usr/bin/update-desktop-database ]; then
    /usr/bin/update-desktop-database -q /usr/share/applications 2>/dev/null || :
fi
if [ -x /usr/bin/gtk-update-icon-cache ]; then
    /usr/bin/gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor 2>/dev/null || :
fi

%postun
if [ \$1 -eq 0 ]; then
    if [ -x /usr/bin/update-desktop-database ]; then
        /usr/bin/update-desktop-database -q /usr/share/applications 2>/dev/null || :
    fi
    if [ -x /usr/bin/gtk-update-icon-cache ]; then
        /usr/bin/gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor 2>/dev/null || :
    fi
fi

%changelog
* Wed Oct 14 2026 WhattyTheShell Contributors <noreply@github.com> - ${VERSION}-1
- Initial release package
EOF

if command -v rpmbuild >/dev/null 2>&1; then
  echo "==> Building RPM with rpmbuild..."
  rpmbuild --define "_topdir $RPMBUILD_DIR" -bb "$SPEC_FILE"
  cp "$RPMBUILD_DIR"/RPMS/*/*.rpm "$DIST_DIR/"
elif command -v cargo-generate-rpm >/dev/null 2>&1; then
  echo "==> Building RPM with cargo-generate-rpm..."
  cargo generate-rpm --output "$DIST_DIR"
else
  echo "Error: neither rpmbuild nor cargo-generate-rpm found!" >&2
  exit 1
fi

echo "==> RPM package created at:"
ls -l "$DIST_DIR"/*.rpm
