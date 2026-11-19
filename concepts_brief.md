# Saved SSH Sessions and Remote Multiplexer Bootstrap

* **Author:** Guilherme Fidelis da Silva
* **Date:** 2026-11-17

## Overview

This feature adds saved SSH connection profiles to WhattyTheShell! with operating system keyring credential storage (`org.freedesktop.secrets`) and automatic remote terminal multiplexer discovery. Users create, edit, order, and remove saved SSH profiles (connection target, optional user, port, identity file, keyring password, and custom priority order between `tmux` and `screen`) in a dedicated Preferences tab (`src/settings_dialog.rs`) and launch them from Preferences or a quick-launch menu in the main window `HeaderBar` (`src/main.rs`). Opening, splitting, or duplicating a saved SSH tab runs a single `ssh -t` PTY process (`src/pty.rs`) wrapped in `sh -c '...'` that detects active `tmux` or `screen` sessions in the user's configured order and lets the user attach to an active session, create a new multiplexed session, or open an ephemeral remote shell. When WhattyTheShell! restarts, previously open tabs restore as disconnected local shell tabs in the local directory active before the SSH connection. Unsaved manual `ssh` commands run inside local shells retain existing local working directory behavior on split, duplication, and restart.

## Analysis

### Reviewer Status
All 7 Goldfish lenses completed review (`Mode: BRAINSTORM`): `egm-zoom-out`, `egm-design`, `egm-comprehension`, `egm-implementation-readiness`, `egm-codebase-details`, `egm-structural`, and `egm-inventive-catalyst`.

### Finalized Design Resolutions
1. **Keyring Authentication via Integrated `--ssh-askpass` and Local IPC Modal:**
   - Setting `SSH_ASKPASS_REQUIRE=force` routes all OpenSSH prompts (passwords, key passphrases, unknown host-key confirmations, and 2FA/OTP challenges) away from `/dev/tty`. A static helper would fail host-key checks, block 2FA, or loop on stale passwords, while running the full GUI binary without an early branch would initialize GTK4 and Vulkan on every prompt.
   - *Resolution:* Intercept `--ssh-askpass` at the start of `main()` before `gtk4::Application` or `wgpu` initialization. The headless helper communicates over an ephemeral `0600` Unix domain socket in `$XDG_RUNTIME_DIR` (authenticated by a per-launch nonce) with an in-process Askpass Broker inside WhattyTheShell!. On the first password/passphrase prompt, the broker serves the secret fetched off the main thread from `org.freedesktop.secrets`. On host-key verification (`yes/no`), 2FA/OTP prompts, missing keyring items, or a second password retry, the broker displays a native GTK4 prompt dialog (with an option to update the stored keyring secret).
2. **Narrow Multiplexer Scope (`tmux` and `screen`) inside `sh -c '...'`:**
   - Supporting `zellij`, `byobu`, `abduco`, and `dtach` adds disproportionate protocol fragility (for example, `dtach` has no session listing command and `byobu` duplicates `tmux`/`screen` sessions). Running unquoted POSIX syntax directly over `ssh -t` also fails when the remote account's login shell is non-POSIX (`fish`, `tcsh`, `nu`, `pwsh`).
   - *Resolution:* Restrict multiplexer discovery to **`tmux`** and **`screen`** with a user-configurable priority order (`tmux` -> `screen` or `screen` -> `tmux`). Execute the remote probe and selector inside an explicit POSIX `sh -c '...'` wrapper, scan both multiplexers so active sessions in either tool remain visible, treat `screen -ls` exit codes defensively, support an optional direct-shell bypass for restricted/ForceCommand hosts, and fall back to an ephemeral login shell if neither tool is installed or if the user selects ephemeral mode.
3. **Session Origin Tracking, Split/Duplicate Semantics, and Local Restart Restore:**
   - Currently `TerminalView` (`src/widget.rs`), `do_split`, `dup_btn`, `connect_close_request`, and the 200ms auto-title timer (`src/main.rs`) derive state from `/proc/<fg_pgrp>/cwd` (`src/pty.rs`), which returns the client's local path for `ssh` processes.
   - *Resolution:* Track an explicit `SessionOrigin` (`Local` vs. `SavedSsh { profile_id, pre_ssh_local_dir }`) on `TerminalView`, separate from user-defined `custom_title`. Bypass `/proc` title polling on saved SSH tabs, reuse `Pty::spawn` with child-scoped environment overrides (never mutating global process `env` in Rust 2024), and preserve the SSH profile on split (`src/split.rs`) and duplicate so the new pane opens the remote multiplexer selector rather than mirroring the sibling pane or falling back to a local shell. When WhattyTheShell! exits and restarts, persist `SavedTab` entries as disconnected local tabs pointed at `pre_ssh_local_dir` (defaulting to `$HOME`) so restarts never trigger startup SSH connection storms or masquerade as live remote tabs. Unsaved manual `ssh` commands keep `SessionOrigin::Local` and existing CWD behavior.
4. **Launch Surface, Failed-Connection Retention, and Config Safety:**
   - `open_settings_dialog` (`src/settings_dialog.rs`) is modal (`820x600`) and has no access to `wgpu` handles, while `child-exited` (`src/widget.rs`) immediately destroys panes/tabs on EOF and hides SSH error output (`Permission denied`, host-key failures, network timeouts).
   - *Resolution:* Keep profile CRUD and multiplexer ordering in a dedicated Preferences tab and add a 1-click quick-launch menu in the main window `HeaderBar` invoking a parameterized `win.launch-ssh-profile` `gio::SimpleAction`. Capture child `ExitStatus` on `Pty` termination/EOF: auto-close on exit code `0`, but keep the pane open on non-zero exit with an inline prompt/banner until the user presses `Enter` (or chooses Reconnect/Close). Harden `ConfigManager` (`src/config.rs`) with immutable UUID profile keys, `#[serde(default)]`, `0700`/`0600` atomic temp-file writes, corrupt-config backup instead of `.unwrap_or_default()`, and pure-Rust Secret Service D-Bus calls off the GTK main thread (`std::thread::spawn` + `async_channel`), while using OpenSSH `ControlMaster=auto` / `ControlPersist` under `$XDG_RUNTIME_DIR` where enabled to speed up splits and duplicates.

## Goals vs. Non-Goals

* **In-Scope Goals:**
  * Provide CRUD management for saved SSH profiles in a dedicated Preferences tab (`src/settings_dialog.rs`) and a 1-click quick-launch launcher in the main window `HeaderBar` (`src/main.rs`).
  * Persist SSH connection metadata (immutable profile ID, name, host or `~/.ssh/config` alias, optional user, port, identity file, and priority order for `tmux` and `screen`) in `$XDG_CONFIG_HOME/WhattyTheShell!/config.toml` using atomic writes.
  * Store passwords and key passphrases exclusively in the OS Secret Service keyring (`org.freedesktop.secrets`), delivered via a headless `whatty --ssh-askpass` mode over a `0600` Unix domain socket IPC with native GTK4 modal fallback for host-key confirmations, 2FA/OTP prompts, and credential updates.
  * Execute a `sh -c '...'` remote bootstrap via single `ssh -t` PTY connections to discover active `tmux` and `screen` sessions in user-configured priority order, offering attach, new multiplexed session, or ephemeral shell options.
  * Propagate saved SSH profile origin on pane split and tab duplication, retain failed non-zero SSH panes open until dismissed (`Enter`/Close/Reconnect), and restore previously connected SSH tabs as disconnected local shells in their prior local directory upon app restart.
* **Out-of-Scope Non-Goals:**
  * Supporting `zellij`, `byobu`, `abduco`, or `dtach` in the remote multiplexer bootstrap.
  * Storing plaintext passwords in `config.toml`, matching `"password:"` strings in the PTY stream, or bundling external `sshpass` binaries.
  * Auto-reconnecting SSH tabs on application launch or re-attaching split panes to the identical active multiplexer session without user selection.
  * Replacing OpenSSH with an embedded Rust SSH client (`russh`) or adding an SFTP browser / tunnel UI.

## Alternatives Weighed

* **Supporting 6 multiplexers (`tmux`, `zellij`, `screen`, `byobu`, `abduco`, `dtach`):** Rejected by author: restricted to `tmux` and `screen` to eliminate `dtach` socket-registry ambiguity, `byobu` backend duplication, and incompatible `zellij` listing formats.
* **Stateless `--ssh-askpass` with `-o StrictHostKeyChecking=accept-new -o NumberOfPasswordPrompts=1` (Option 2):** Rejected by author: cannot handle interactive 2FA/OTP codes or in-flight password updates alongside stored keyring passwords; replaced by Option 1 (`--ssh-askpass` + Unix socket IPC + GTK4 interactive prompt fallback).
* **External `sshpass` + `secret-tool` (Option 3):** Rejected by author: adds external package dependencies and relies on insecure PTY text matching (`"password:"`).
* **Embedded Rust SSH library (`russh`, Option 4):** Rejected by author: sacrifices native OpenSSH `~/.ssh/config`, `ProxyJump`, `ssh-agent`, and FIDO2 hardware key compatibility.
* **Pre-login background SSH probe before opening the GTK tab:** Rejected by author: forces extra TCP/SSH handshakes and duplicate auth prompts compared to a single `ssh -t` PTY bootstrap.
* **Auto-reconnecting saved SSH tabs on app startup or restoring dormant SSH stubs:** Rejected by author: connected SSH tabs must restore on app restart as disconnected local shells in the local directory used prior to SSH.
* **Automatically re-attaching split panes or duplicated tabs to the same active multiplexer session without prompting:** Rejected by author: mirrors the identical remote viewport across panes instead of offering the session picker with new-session and ephemeral-shell options.

## Key Decisions & Staging for Next Steps

* **Authentication:** Headless `whatty --ssh-askpass` invoked before GTK/Vulkan initialization, communicating over an ephemeral `$XDG_RUNTIME_DIR` Unix socket (`0600`, scoped nonce) with an in-process broker backed by pure-Rust `org.freedesktop.secrets` D-Bus calls off the main thread and GTK4 interactive fallback for host-key, 2FA, and password-retry prompts.
* **Multiplexer Scope & Remote Execution:** Exclusive support for `tmux` and `screen` with user-editable priority order, wrapped in `sh -c '...'` over `ssh -t` (with raw-shell bypass option and ephemeral shell fallback).
* **Tab Lifecycle & Session Restore:** Explicit `SessionOrigin` (`Local` vs. `SavedSsh`) on `TerminalView`, bypassing `/proc` CWD title polling for SSH tabs, preserving SSH profile origin on split/duplicate, keeping failed non-zero SSH panes open until `Enter`/Close/Reconnect, and restoring SSH tabs as disconnected local shells in `pre_ssh_local_dir` upon app restart.
* **UI Entry Points:** Dedicated SSH Profiles management tab in Preferences (`src/settings_dialog.rs`) plus a 1-click quick-launch menu in the main window `HeaderBar` (`src/main.rs`) dispatched through `gio::SimpleAction`.
