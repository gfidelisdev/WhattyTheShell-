# scripts

## Overview
Shell scripts for building distribution packages (`.deb`, `.rpm`, `.AppImage`, `.tar.gz`), installing/uninstalling binary and desktop assets locally, monitoring runtime process resources, and automating version bumps and Git releases.

## Interface
- `[build-appimage.sh:main]`: Stages `build/AppDir`, compiles or copies `target/release/WhattyTheShell` to `usr/bin/whatty` with command symlinks, bundles icons/metadata/GSettings schemas, generates `AppRun`, and invokes `$APPIMAGETOOL` (default `appimagetool`) to produce `dist/WhattyTheShell-${VERSION}-x86_64.AppImage`.
- `[build-deb.sh:main]`: Stages `build/deb-stage` with `DEBIAN/control` (`Depends: libgtk-4-1 (>= 4.14), libvulkan1`), `postinst`, `postrm`, binary/aliases, icons, and desktop metadata, and builds `dist/whattytheshell_${VERSION}_amd64.deb` via `dpkg-deb` or `cargo-deb`.
- `[build-rpm.sh:main]`: Stages `build/rpmbuild`, generates `SPECS/whattytheshell.spec` (`Requires: gtk4 >= 4.14, vulkan-loader`), and builds an `.rpm` package into `dist/` via `rpmbuild` or `cargo-generate-rpm`.
- `[build-tarball.sh:main]`: Packages release binary, desktop entry, icons, `install.sh`, `uninstall.sh`, and optional `LICENSE` into `dist/WhattyTheShell-${VERSION}-linux-x86_64.tar.gz`.
- `[install.sh:main]`: POSIX `sh` installer deploying `whatty`, symlink aliases (`whatyy`, `whattytheshell`, `whattheshell`, `wts`, `WhattyTheShell`), desktop file, AppStream metadata, hicolor icons (`16..512`), and `uninstall.sh` into `$PREFIX` (`/usr/local` as root or `$HOME/.local` as non-root).
- `[uninstall.sh:main]`: POSIX `sh` uninstaller removing installed binary, symlink aliases, desktop entry, AppStream metadata, icons, and `$PREFIX/share/whattytheshell` from `$PREFIX`.
- `[package-all.sh:main]`: Orchestrates `cargo build --release`, `build-deb.sh`, `build-rpm.sh`, `build-appimage.sh`, `build-tarball.sh`, and generates `dist/SHA256SUMS.txt`.
- `[release.sh:main]`: Prompts interactively (when `[ -t 0 ]`) for a version or increments the patch component in `Cargo.toml`, updates `Cargo.lock`, runs `package-all.sh`, and creates/pushes a Git commit and annotated tag `v${NEW_VERSION}`.
- `[mem-watch.sh:main]`: Polls `/proc/$PID/status`, `/proc/$PID/fd`, and `pgrep -P` every second for the newest running `whatty` process to print `HORA`, `VmRSS`, `THREADS`, `FDS`, and `PROCESSOS_FILHOS`.

## Invariants
- `[build-appimage.sh:set -e]`, `[build-deb.sh:set -e]`, `[build-rpm.sh:set -e]`, `[build-tarball.sh:set -e]`, `[install.sh:set -e]`, `[uninstall.sh:set -e]`, `[package-all.sh:set -e]`, `[release.sh:set -e]`: All build, install, uninstall, and release scripts execute under `set -e` and abort on the first failing command.
- `[build-appimage.sh:VERSION]`, `[build-deb.sh:VERSION]`, `[build-rpm.sh:VERSION]`, `[build-tarball.sh:VERSION]`: Package scripts extract `VERSION` from the first `^version = ` line in `Cargo.toml` and build `target/release/WhattyTheShell` only when it does not already exist; `install.sh` always rebuilds when invoked from the source tree with `cargo` present.
- `[build-appimage.sh:alias]`, `[build-deb.sh:alias]`, `[build-rpm.sh:alias]`, `[install.sh:ALIASES]`, `[uninstall.sh:COMMANDS]`: The primary binary is installed as `whatty` alongside five symlink aliases: `whatyy`, `whattytheshell`, `whattheshell`, `wts`, and `WhattyTheShell`.
- `[build-appimage.sh:APPIMAGE_EXTRACT_AND_RUN]`: Enables `APPIMAGE_EXTRACT_AND_RUN=1` when `/dev/fuse` is absent; generated `AppRun` does not bundle host GPU/Mesa/Vulkan drivers so hardware acceleration matches kernel modules.
- `[release.sh:NEW_VERSION]`: Enforces semver regex `^[0-9]+\.[0-9]+(\.[0-9]+)?(-[a-zA-Z0-9.]+)?$` and requires `.git` and a non-empty `CURRENT_VERSION` in `Cargo.toml`.
- `[mem-watch.sh:PID]`: Exits with status `1` if `pgrep -n -x whatty` finds no running `whatty` process.

## Side Effects
- `[build-appimage.sh:main]`, `[build-deb.sh:main]`, `[build-rpm.sh:main]`, `[build-tarball.sh:main]`, `[package-all.sh:main]`: Delete and recreate staging directories under `build/`, compile release artifacts in `target/`, write distributable packages and `SHA256SUMS.txt` under `dist/`, and (`build-appimage.sh`) download `appimagetool-x86_64.AppImage` via `curl -sSL` from GitHub when missing.
- `[install.sh:main]` & `[uninstall.sh:main]`: Write or delete files under `$PREFIX` (`/usr/local` or `$HOME/.local`) and invoke `update-desktop-database` and `gtk-update-icon-cache`.
- `[release.sh:main]`: Mutates `Cargo.toml` via `sed -i -E`, updates `Cargo.lock`, runs `package-all.sh`, executes `git add -A`, `git commit`, `git tag` (or `git tag -f`), and pushes branch and tag to `origin`.
- `[mem-watch.sh:main]`: Reads `/proc/$PID/status` and `/proc/$PID/fd` and spawns `pgrep -P "$PID"` in a 1-second loop.

## Verification
- Run: `cargo test`
