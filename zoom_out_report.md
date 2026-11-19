# Stage 5: Initial Zoom Out Report

- **Target Document:** [docs/designs/ssh_sessions_and_multiplexer_bootstrap.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/docs/designs/ssh_sessions_and_multiplexer_bootstrap.md) (Full file: lines `1–416`)
- **Standard:** [zoom-out-standard.md](file:///home/gfidelis/.gemini/config/plugins/egm/resources/document_standards/zoom-out-standard.md)
- **Round:** `1`

## Reviewer Execution Status

| Reviewer | Role / Focus | Status | Findings (🔴 / 🟡 / 🟢) |
| :--- | :--- | :--- | :--- |
| `egm-zoom-out` | Architectural Minimalism, YAGNI & Codebase Alignment | `completed` | 3 🔴 / 4 🟡 / 1 🟢 |
| `egm-inventive-catalyst` | TRIZ Contradiction & Simplicity Analysis | `completed` | 2 🔴 / 4 🟡 / 1 🟢 |

---

## Findings — `egm-zoom-out`

### 🔴 Blockers

#### `[zoom-out-1-1]` Broken `SSH_ASKPASS` Dispatch Contract & Unbounded `AskpassBroker` Scope
- **Scope:** Lines `19`, `193–204`, `218–222`, `381`
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:**
  1. **OpenSSH never passes `--ssh-askpass`:** Line `218` sets `SSH_ASKPASS=<std::env::current_exe()>`. OpenSSH executes `$SSH_ASKPASS` directly with `[exe, <prompt>]` and does not pass `--ssh-askpass`. Line `381` checks only `args().any(|a| a == "--ssh-askpass")` (dropping the `WHATTY_SSH_ASKPASS_SOCKET` check from line `193`), launching a second GTK4 + Vulkan GUI window on every SSH prompt. Conversely, line `193`'s `WHATTY_SSH_ASKPASS_SOCKET && args().len() >= 2` check hijacks `main()` for any `whatty <arg>` command invoked with an inherited environment variable; a dedicated marker env var (or symlink) is required.
  2. **Unnecessary IPC & missing lifecycle cleanup:** `build_ssh_invocation` injects `SSH_ASKPASS_REQUIRE=force` and registers an `AskpassBroker` attempt for all profiles, including `AgentOrDefault` profiles with no keyring secret. Routing host-key `yes/no`, 2FA, and interactive passwords out of the live PTY through a Unix socket + JSON IPC + GTK modal dialog violates Minimalist Data Flow, and `AskpassBroker` specifies no nonce expiry or socket cleanup on exit.

#### `[zoom-out-1-2]` Reinventing `org.freedesktop.secrets` Over Raw D-Bus With a Broken Threading Model
- **Scope:** Lines `175–189`, `260`, `272–276`, `338`
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:**
  1. **D-Bus signal / thread mismatch:** Line `177` places calls on `std::thread::spawn` + `async_channel` (citing `src/theme_marketplace.rs`, which only runs blocking HTTP calls), while line `185` waits on `org.freedesktop.Secret.Prompt.Completed`. In `gio::DBusConnection`, D-Bus signal subscriptions dispatch on a running `glib::MainContext`/`MainLoop`; bare `std::thread::spawn` workers have no running `MainLoop`, hanging `Prompt.Completed`.
  2. **Reinventing the wheel & plaintext transport:** Hand-rolling `OpenSession("plain", "")`, `SearchItems`, `Unlock`, `Prompt`, `GetSecrets`, `CreateItem`, and `Delete` GVariant serialization violates Platform Standards (`zoom-out-standard.md`). Alternative B (`272–276`) conflates `sshpass` with Secret Service tooling, whereas pure-Rust Secret Service crates (`oo7` / `secret-service`) compile statically with zero C shared library dependencies and implement encrypted DH+AES sessions cleanly.

#### `[zoom-out-1-3]` Unwired `Ctrl+Shift+R` Reconnect Boundary & Racy `try_wait()` Hold-Open Check
- **Scope:** Lines `24`, `47`, `238`, `243`, `351`, `364`
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:**
  1. **Missing reconnect architecture:** Line `243` prints `"Press Enter to close or Ctrl+Shift+R to reconnect"`, yet neither `src/widget.rs` nor `src/main.rs` defines a reconnect action, shortcut, or signal. `TerminalView` stores only `profile_id` and lacks `ConfigManager` or `AskpassBroker` handles to re-launch `ssh`.
  2. **Racy non-blocking `try_wait()` at PTY EOF:** Lines `238` and `364` call non-blocking `self.child.try_wait()` immediately when the PTY master reads EOF (`Ok(0)`). Because the Linux kernel can close the slave PTY FD (`POLLHUP`/EOF) before the child reaches `EXIT_ZOMBIE` for `waitpid(..., WNOHANG)`, `try_wait()` can return `None` at EOF, letting non-zero SSH exits bypass the hold-open banner.

### 🟡 Majors

#### `[zoom-out-1-4]` Synchronous `30 ms` `Pty::terminate()` Poll Stalls the GTK Main Loop
- **Scope:** Line `352` (vs. [src/pty.rs:311-322](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/pty.rs#L311-L322), [src/main.rs:365,1069,1479](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/main.rs#L365), [CONTEXT.md:123-125](file:///home/gfidelis/Code/rust/WhattyTheShell!/CONTEXT.md#L123-L125))
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Changing `Pty::terminate()` globally (for both local and SSH panes) to send `SIGHUP` and synchronously poll `try_wait()` for up to `30 ms` before `SIGKILL` runs directly on the GTK main thread in `close_tab_page`, `close_others_btn`, and `window.connect_close_request`. Closing multiple panes or stalled SSH processes blocks UI events for `N * 30 ms`, violating the Cooperative Yielding invariant.

#### `[zoom-out-1-5]` Default-On `ControlMaster` Is Premature Optimization That Overrides `~/.ssh/config`
- **Scope:** Lines `25`, `165–166`, `211`, `281–282`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Defaulting `use_control_master` to `true` (`165`) and passing `-o ControlMaster=auto -o ControlPath=$XDG_RUNTIME_DIR/whattytheshell/cm-%C -o ControlPersist=60s` (`211`) overrides user `~/.ssh/config` directives despite Alternative C (`281`). Because single-connection `ssh -t` bootstrap already eliminated double handshakes, forced `ControlPersist=60s` risks stale mux sockets across network/VPN drops.

#### `[zoom-out-1-6]` YAGNI & State-Space Bloat in `SshProfile` (`MultiplexerKind`, `SshAuthKind`, `secret_kind`)
- **Scope:** Lines `51`, `129–168`, `182`, `372`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:**
  1. `Vec<MultiplexerKind>` (`Tmux`, `Screen`, `Unknown`) speculates on unsupported multiplexers excluded by Non-Goals (`51`) while Preferences (`372`) only toggles two orders (`tmux -> screen` vs `screen -> tmux`). A 2-variant enum (`TmuxFirst` / `ScreenFirst`) eliminates empty, duplicate, and `Unknown` states.
  2. Splitting `SshAuthKind` (`AgentOrDefault`, `PasswordKeyring`, `IdentityFile`) and keying D-Bus entries by `"secret_kind"` (`"password"` vs `"passphrase"`, `182`) prevents pairing `identity_file` with `ssh-agent` cleanly and orphans old secrets when users change auth mode. Keying stored secrets solely by `profile_id` with independent `identity_file: Option<PathBuf>` and `use_keyring_secret: bool` removes both issues.

#### `[zoom-out-1-7]` Uncoordinated Integration Gaps & Scope Creep in `src/main.rs` and `src/settings_dialog.rs`
- **Scope:** Lines `172`, `245–247`, `372–374`, `383–387`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:**
  1. **Stale `HeaderBar` quick-launch menu:** `open_settings_dialog` (`src/settings_dialog.rs:248-253`) receives no refresh hook or lazy `set_create_popup_func`, so profile changes require restart to appear in `HeaderBar`.
  2. **Missed `/proc/<fg_pgrp>/cwd` call sites:** `win.new-tab` (`main.rs:1289-1293`), tab-bar double-click (`main.rs:1621-1625`), `copy_path_btn` (`main.rs:877`), and `restore_name_btn` (`main.rs:777`) still call `view.current_working_dir()` (`/proc`).
  3. **Contradictory `custom_title` and corrupt-config save rules:** Line `386` sets `SavedTab.custom_title = false` unconditionally while line `247` checks `"unless manually renamed"`, and `create_tab_cb` (`main.rs:558-562`) uses `custom_title` for initial titles. Meanwhile `window.connect_close_request` auto-saves `open_tabs` (`main.rs:1485`), contradicting line `172`'s *"skips overwriting `config.toml` until the next explicit user save"*.
  4. **Unrelated scope creep in `settings_dialog.rs`:** Line `374` bundles pre-existing split-pane iteration fixes across lines `547, 636, 950, 1366, 1409` (and misses `1380`), which belongs in a separate bugfix rather than the SSH feature design.

### 🟢 Nits

#### `[zoom-out-1-8]` Altitude Drops (Weeds) & Missing Dependency Accounting for UUID/Nonce Generation
- **Scope:** Lines `19`, `66`, `129–167`, `197`, `200`, `224–229`, `241–243`, `256`, `361`, `374`, `387`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** Architecture sections drop below interface altitude into Rust `#[serde(...)]` attributes, `Cell<bool>`/`RefCell` field types, `invoke_local` calls, raw `tmux -F` strings, ANSI escapes, and hardcoded `settings_dialog.rs` line numbers. Additionally, lines `19, 66, 197, 256` require UUIDs and 128-bit random hex nonces while `Cargo.toml` has no `uuid` or `rand` crate and is omitted from Target Files.

---

## Findings — `egm-inventive-catalyst` (TRIZ Contradiction Audit)

### 🔴 Blockers

#### `[inventive-catalyst-1-1]` Headless `--ssh-askpass` Entrypoint, Blind Auth-Prompt Classification, & `ssh` Command Assembly
- **Scope:** Lines `192–222`, `338–343`, `381`
- **Severity:** `🔴 Blocker`
- **Contradiction & Resolution:**
  1. Reusing `whatty` via `SSH_ASKPASS=<current_exe()>` breaks because OpenSSH omits `--ssh-askpass`, making `main.rs` line `381` launch a second GUI window (`io.github.whattytheshell`), whereas checking `WHATTY_SSH_ASKPASS_SOCKET && args().len() >= 2` (`193`) risks hijacking nested `whatty <arg>` commands. **TRIZ (Extraction / Taking Out):** Use a `0700` symlink `whatty-askpass` in `$XDG_RUNTIME_DIR/whattytheshell/` dispatching on `argv[0]` basename (keeping `--ssh-askpass` only for tests).
  2. Treating all `*password*`/`*passphrase*` prompts identically on `password_attempts == 0` leaks a `PasswordKeyring` password onto a local private key passphrase or `~/.ssh/config` `ProxyJump` bastion. **TRIZ (Preliminary Action + Segmentation):** Constrain OpenSSH auth methods by `auth_kind` (`-o IdentitiesOnly=yes` for `IdentityFile`; `-o PubkeyAuthentication=no -o PreferredAuthentications=keyboard-interactive,password` for `PasswordKeyring`; no keyring injection for `AgentOrDefault`) and match prompt target before consuming the keyring attempt.

#### `[inventive-catalyst-1-2]` Failed-Connection Hold-Open, `Ctrl+Shift+R` Reconnect, & `Pty::terminate()` PID Lifecycle
- **Scope:** Lines `24`, `47`, `238–243`, `350–353`, `360–365`, `376–388`
- **Severity:** `🔴 Blocker`
- **Contradiction & Resolution:**
  1. Line `243` prints `Ctrl+Shift+R to reconnect`, yet `src/widget.rs` (`360–365`) and `src/main.rs` (`376–388`) define no `Ctrl+Shift+R` handler, reconnect signal (`"reconnect-requested"`), or pane respawn/replacement. **TRIZ (Inversion):** Emit a decoupled `"reconnect-requested"` signal handled in `src/main.rs` while blocking PTY writes during `awaiting_error_dismiss`.
  2. Once `Pty::try_wait_exit_code()` reaps the child, calling `Pty::terminate()` still runs `libc::kill(-pid, ...)` against a stale/recycled PID (`src/pty.rs:312-322`), while synchronous `30 ms` polling stalls multi-tab close on the GTK main loop. **TRIZ (Extraction):** Guard `kill(-pid, ...)` with `if self.last_exit_code.is_none()` and offload escalated `SIGHUP` -> `SIGKILL` wait onto a detached reaper thread.
  3. OpenSSH forwards the remote shell's last exit code (`grep` -> `1`, `Ctrl+C` -> `130`) on exit, triggering the red error banner on normal exits (`code != 0`). **TRIZ (Blessing in Disguise / Preliminary Action):** Restrict hold-open to transport/auth/pre-exec failures (`code == 255` or pre-exec marker).

### 🟡 Majors

#### `[inventive-catalyst-1-3]` `AskpassBroker` Multi-Window `EADDRINUSE`, Concurrent Modal Blocking, & Save-vs-Connect Race
- **Scope:** Lines `103`, `177–204`, `338–340`, `372–373`, `382`
- **Severity:** `🟡 Major`
- **Contradiction & Resolution:** Initializing `AskpassBroker::start(&window)` in `app.connect_activate` (`382`) collides (`EADDRINUSE`) when a second window launches inside the same single-instance PID (`src/main.rs:35-50`); sequential broker handling blocks Tab B while Tab A waits on modal/D-Bus unlock; `std::thread::spawn` has no `glib::MainLoop` for `Prompt.Completed`; and async `store_secret_async` races clicking `Connect`. **TRIZ (Intermediary + Segmentation + Beforehand Cushioning):** Initialize one process-wide broker in `main()` with per-window `WeakRef` + cancellation handle + concurrent connections + modal queue + scoped worker `glib::MainLoop` (`Unlock`), and gate `Connect` on `store_secret_async` completion with visible keyring error feedback.

#### `[inventive-catalyst-1-4]` Remote `sh -c '...'` Multiplexer Bootstrap Quoting Across Non-POSIX Login Shells
- **Scope:** Lines `23`, `72`, `216`, `224–232`, `341`
- **Severity:** `🟡 Major`
- **Contradiction & Resolution:** Embedded single quotes (`tmux list-sessions -F '#{...}'` on `228`) fail with POSIX `'\''` quoting inside non-POSIX login shells (`fish`, `tcsh`, `nu`, `pwsh`, `elvish`), and unquoted `tmux attach-session -t <name>` lacks `=` exact matching (`-t "=$sel"`). **TRIZ (Preliminary Action + Universality):** Build the bootstrap script with zero internal `'` and zero `\`, using `"` inside outer `'...'` (`!script.contains('\'') && !script.contains('\\')`) and quoting `"=$sel"` / `"$sel"`.

#### `[inventive-catalyst-1-5]` Default `ControlMaster=auto` CLI Override & Missing Runtime Dir Fallback
- **Scope:** Lines `165–166`, `196`, `211`, `281–282`, `342`
- **Severity:** `🟡 Major`
- **Contradiction & Resolution:** Default-true `use_control_master` overrides `~/.ssh/config` `ControlMaster` settings despite Alternative C (`281–282`), hardcodes `$XDG_RUNTIME_DIR` without `/tmp` fallback (`196`), and omits `ServerAliveInterval`. **TRIZ (Dynamics + Beforehand Cushioning):** Default to `InheritSshConfig` (or opt-in managed mode sharing the verified `0700` dir helper + `ServerAliveInterval=15`).

#### `[inventive-catalyst-1-6]` Tab Title / `custom_title` Conflation & Missing Line `1380` in `settings_dialog.rs`
- **Scope:** Lines `45–46`, `170`, `245–247`, `374`, `384–386`
- **Severity:** `🟡 Major`
- **Contradiction & Resolution:** Passing `title: Some(...)` sets `view.custom_title`, breaking mixed split/merged tabs, `"Restaurar Nome Automático"` (`main.rs:777`), `"Duplicar Aba"` (`main.rs:804`), and `SavedTab` close serialization (`247` vs `386`). Line `374` omits `src/settings_dialog.rs:1380`. **TRIZ (Segmentation + Inversion):** Store `profile_name` on `SessionOrigin::SavedSsh`, add `TerminalView::auto_tab_title()`, reserve `custom_title` strictly for manual renames, and extract `for_each_terminal` covering all 6 `settings_dialog.rs` sites (including `1380`).

### 🟢 Nits

#### `[inventive-catalyst-1-7]` `HeaderBar` Quick-Launch Menu Refresh Coupling
- **Scope:** Lines `372–373`, `387`
- **Severity:** `🟢 Nit`
- **Contradiction & Resolution:** Preferences (`372–373`) has no handle to refresh `HeaderBar`'s `MenuButton` (`387`). **TRIZ (Inversion + Dynamics):** Use `gtk4::MenuButton::set_create_popup_func` in `src/main.rs` to rebuild profiles lazily on open (with an empty-state Preferences hint).
