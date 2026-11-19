# WhattyTheShell! Blueprint

## System Overview
`WhattyTheShell!` (`v0.4.3`) is a GTK4 GPU-accelerated terminal emulator written in Rust (2024 edition). It pairs a `vte`-based terminal state machine and scrollback grid with zero-copy Vulkan DMA-BUF rendering (`wgpu` + `ash` + `gdk4::DmabufTextureBuilder`), multi-pane split tabs (`gtk4::Notebook` + nested `gtk4::Paned`), pseudo-terminal process management (`openpty`), saved SSH session profiles with remote `tmux`/`screen` multiplexer bootstrap, Secret Service (`org.freedesktop.secrets`) keyring storage + UNIX socket `SSH_ASKPASS` prompts, TOML session/preference persistence, and VS Code Marketplace theme integration.

## Local Interface & Invariants
- `[Cargo.toml:package]`: Defines crate `WhattyTheShell` (`version = "0.4.3"`, `edition = "2024"`), default feature `vulkan` (plus optional `opengl`), GTK4 dependency `gtk4` with `v4_14` feature for `GdkDmabufTexture`, and Debian/RPM packaging metadata installing `/usr/bin/whatty` with symlink aliases `whatyy`, `whattytheshell`, `whattheshell`, `wts`, and `WhattyTheShell`.
- `[CONTEXT.md]`: Domain glossary defining canonical terminology across Terminal Preferences & Theming, Packaging & Distribution, Text Interaction & Ergonomics, and Process Concurrency & Performance.

## Subdirectories & System Boundaries
- `src`: [./src/README.md](./src/README.md) - Core Rust application crate containing the GTK4 UI, `TerminalView` widget, PTY lifecycle, configuration persistence, theme management, and child subsystems [./src/render/README.md](./src/render/README.md) and [./src/term/README.md](./src/term/README.md).
- `scripts`: [./scripts/README.md](./scripts/README.md) - Packaging (`.deb`, `.rpm`, `.AppImage`, `.tar.gz`), local installation/uninstallation, release automation, and memory-monitoring shell scripts.

## Global Invariants
- Zero-copy frame presentation requires GTK4 `>= 4.14` (`v4_14` feature) and a Vulkan-capable `wgpu::Device` supporting `VK_EXT_external_memory_dma_buf` and `VK_KHR_external_memory_fd` (`[Cargo.toml:dependencies.gtk4]`, `[src/render/dmabuf.rs:DmabufFrame::new]`).
- PTY streaming executes asynchronously on the GTK main event loop (`g_unix_fd_add_full`) with a `64 * 1024` byte (`64 KiB`) per-wakeup read cap to prevent UI event-loop starvation (`[src/widget.rs:TerminalView::with_shell_and_working_dir]`).
- Split panes are bounded to `MAX_PANES_PER_TAB = 4` per tab page (`[src/split.rs:MAX_PANES_PER_TAB]`), primary scrollback is bounded to `10_000` rows (`[src/term/mod.rs:SCROLLBACK]`), and GPU instance buffers are bounded to `256 * 100` (`25,600`) cells (`[src/render/pipeline.rs:TerminalRenderer::new]`).
- Closing a pane, tab, or window deterministically terminates the child process group (`SIGHUP`/`SIGKILL` to `-pid`), closes duplicated DMA-BUF FDs via GDK release callbacks, drops both DMA-BUF frames when background tabs unmap, and invokes `libc::malloc_trim(0)` (`[src/pty.rs:Pty::terminate]`, `[src/render/dmabuf.rs:DmabufFrame::build_gdk_texture]`, `[src/widget.rs:TerminalView::terminate]`).
- Saved SSH session credentials are stored exclusively via `org.freedesktop.secrets` (`[src/ssh.rs:SecretServiceClient]`), authorized over a `0o600` UNIX stream socket via `SO_PEERCRED` + single-use 128-bit nonces (`[src/ssh.rs:AskpassBroker]`, `[src/ssh.rs:AskpassAttempt]`), and `SavedSsh` tabs record `pre_ssh_local_dir` so restarts reopen disconnected local shells in the prior local working directory (`[src/widget.rs:SessionOrigin]`).

## Global Side Effects
- Reads and writes user configuration at `$XDG_CONFIG_HOME/WhattyTheShell!/config.toml` (`0o600` permissions in a `0o700` directory; `[src/config.rs:ConfigManager]`), connects to user session D-Bus `org.freedesktop.secrets` for SSH secrets (`[src/ssh.rs:SecretServiceClient]`), and binds `$XDG_RUNTIME_DIR/whatty/askpass-<pid>.sock` (`[src/ssh.rs:AskpassBroker]`).
- Fetches theme search results and VSIX archives from the VS Code Marketplace (`https://marketplace.visualstudio.com/_apis/public/gallery/extensionquery`) and caches extracted JSON palettes under `$XDG_CACHE_HOME/WhattyTheShell!/themes/` (`[src/theme_marketplace.rs:search_themes]`, `[src/theme_marketplace.rs:download_and_extract_theme_with_progress]`).
- Allocates kernel PTY master/slave file descriptors (`libc::openpty`), spawns interactive login shells, and inspects `/proc/<pid>/cwd` (`[src/pty.rs:Pty]`).
- Updates user desktop entry icons in `$XDG_DATA_HOME/applications/io.github.whattytheshell.desktop` (`[src/settings_dialog.rs:update_user_desktop_icon]`) and builds/installs system packages via `./scripts/` (`[scripts/package-all.sh]`, `[scripts/install.sh]`).

## System Verification
- Run: `cargo test`
