# Stage 6: Naive Reader Comprehension Report

- **Target Document:** [docs/designs/ssh_sessions_and_multiplexer_bootstrap.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/docs/designs/ssh_sessions_and_multiplexer_bootstrap.md) (`FeatureRange`: `1..309`, `DetailedRange`: `310..416`)
- **Goldfish Context:** Intentionally withheld (zero-context audit)
- **Round:** `1`

## Reviewer Execution Status

| Reviewer | Role / Focus | Status | Findings (🔴 / 🟡 / 🟢) |
| :--- | :--- | :--- | :--- |
| `egm-comprehension` | Zero-Context Naive Reader Paraphrase & Ambiguity Audit | `completed` | 3 🔴 / 7 🟡 / 2 🟢 |

---

### Paraphrase: What problem is the document trying to solve and how does it propose to solve it?

WhattyTheShell! is a GTK4 GPU-accelerated terminal emulator in Rust that currently only spawns local login shells (`Pty::spawn_with_shell`) and reads local `/proc/<fg_pgrp>/cwd` paths. Because of this, users connecting to remote machines via `ssh` face four issues: (1) there are no saved SSH profiles or OS keyring credential integration, forcing repeated password/passphrase entry when `ssh-agent` or `ControlMaster` is unavailable; (2) `ssh` starts an ephemeral login shell unless the user manually discovers and attaches to remote `tmux` or `screen` sessions; (3) the 200ms title timer and split/duplicate actions read the local client CWD instead of preserving the SSH target; and (4) non-zero `ssh` connection failures immediately close the tab via `child-exited`, hiding OpenSSH error output.

The document proposes solving this by:
1. Persisting non-secret SSH profile metadata (`SshProfile`) atomically (`0600`) in `$XDG_CONFIG_HOME/WhattyTheShell!/config.toml` and storing optional SSH passwords and key passphrases in `org.freedesktop.secrets` over `gtk4::gio::DBusConnection`, keyed by an immutable profile UUID.
2. Providing a pre-GTK `--ssh-askpass` mode inside the `whatty` binary that communicates over a `0600` Unix domain socket (`AskpassBroker`) using 128-bit single-session nonces—returning stored keyring secrets automatically on the first password/passphrase prompt and displaying a modal GTK4 fallback dialog for host-key confirmation (`yes/no`), 2FA/OTP codes, missing secrets, or retries.
3. Launching saved profiles via a single `ssh -t` PTY process running a POSIX `sh -c '...'` bootstrap script that checks `tmux` and `screen` in configured priority order and displays an interactive menu to attach to an existing session, create a new multiplexed session, or start an ephemeral login shell (or `exec` directly into `${SHELL:-/bin/sh} -l` if neither multiplexer exists).
4. Tracking `SessionOrigin` (`Local` vs. `SavedSsh { profile_id, pre_ssh_local_dir }`) on `TerminalView` so splits and duplicates launch new `ssh -t` sessions for that profile, the 200ms CWD timer preserves the SSH profile tab title, non-zero `ssh` exits hold the pane open with a red diagnostic banner until dismissed or reconnected, and closing/restarting the application restores SSH tabs as disconnected local shells in `pre_ssh_local_dir`.

### Paraphrase: How would you implement this step by step?

1. **Phase 1 — Config schema & atomic persistence (`src/config.rs`)**:
   - Define `MultiplexerKind` (`Tmux`, `Screen`, `#[serde(other)] Unknown`), `SshAuthKind` (`AgentOrDefault`, `PasswordKeyring`, `IdentityFile`), and `SshProfile`, and add `#[serde(default)] pub ssh_profiles: Vec<SshProfile>` to `Config` (default empty).
   - Add `ConfigManager` helpers (`ssh_profiles`, `get_ssh_profile`, `upsert_ssh_profile`, `remove_ssh_profile`).
   - Update `ConfigManager::load_or_default()` to ensure `0700` permissions on the config directory and copy unparseable non-empty `config.toml` files to `config.toml.corrupt.<unix_ts>` before returning `Config::default()`.
   - Update `ConfigManager::save()` to write to `config.toml.tmp` with mode `0600`, call `sync_all()`, and atomically rename to `config.toml`.
   - Add unit tests for `SshProfile` round-trip, `#[serde(other)]`, `0600` atomic save, and corrupt TOML backup.

2. **Phase 2 — Keyring D-Bus client, Askpass broker, and SSH/bootstrap builders (`src/ssh.rs`, `src/main.rs`)**:
   - Create `src/ssh.rs` with `SecretServiceClient` using `gio::DBusConnection` on worker threads (`OpenSession("plain", "")`, `SearchItems`, `Unlock` + `Prompt` `Completed`, `GetSecrets`, `CreateItem`, `Delete`) with attributes `application = "io.github.whattytheshell"`, `profile_id`, and `secret_kind` (`"password"` or `"passphrase"`).
   - Implement `run_askpass_client_and_exit()` to read `WHATTY_SSH_ASKPASS_SOCKET`, `WHATTY_SSH_ASKPASS_NONCE`, `SSH_ASKPASS_PROMPT`, and prompt `argv`, exchange newline-delimited JSON with the Unix socket, and either print the secret + `\n` (exit `0`) or exit `1`.
   - Wire the pre-GTK askpass check as the first thing in `fn main()` in `src/main.rs` before `Application::builder()`.
   - Implement `AskpassBroker` binding a `0600` `UnixListener` at `$XDG_RUNTIME_DIR/whattytheshell/askpass-<pid>.sock` (with `0700` `/tmp/whattytheshell-<uid>` fallback), generating 128-bit nonces via `register_attempt`, serving first-attempt password/passphrase secrets from `SecretServiceClient`, and marshaling confirm/2FA/retry/missing-secret prompts to the GTK main thread to show a modal dialog.
   - Implement `build_ssh_invocation` (validating `host`/`user`, expanding `~/`, conditionally adding `ControlMaster`, `-p`, `-i`, `--`, destination, `sh -c` payload unless `bypass_multiplexer_probe` is set, and `SSH_ASKPASS` env vars) and `build_remote_bootstrap_script` (checking `tmux`/`screen` in `priority` order, listing sessions, prompting on `/dev/tty`, or `exec`ing `${SHELL:-/bin/sh} -l`).
   - Add unit tests for leading-dash rejection, `--` separator placement, `tmux`/`screen` priority ordering, `bypass_multiplexer_probe`, and `AskpassBroker` IPC classification.

3. **Phase 3 — `Pty` extensions & `TerminalView` session lifecycle (`src/pty.rs`, `src/widget.rs`)**:
   - In `src/pty.rs`, add `Pty::spawn_with_env` delegating to `spawn_inner` with `cmd.env(k, v)`, add `last_exit_code: Option<i32>` and `Pty::try_wait_exit_code` (supporting `128 + signal`), and update `Pty::terminate` to send `SIGHUP`, poll `try_wait()` for up to `30 ms`, and escalate to `SIGKILL` + `wait()` if still running.
   - In `src/widget.rs`, define `SessionOrigin` (`Local` vs. `SavedSsh { profile_id, pre_ssh_local_dir }`), add `session_origin` and `awaiting_error_dismiss` to `TerminalViewInner`, add `TerminalView::with_ssh_profile` and getters, hold failed `SavedSsh` panes open with a red diagnostic banner on non-zero exit instead of emitting `"child-exited"`, and emit `"child-exited"` on `Return`/`KP_Enter` (or `Escape`) while awaiting dismissal.
   - Add unit tests in `src/pty.rs` and `src/widget.rs`.

4. **Phase 4 — Preferences UI, `HeaderBar` quick-launch, tab lifecycle & documentation (`src/settings_dialog.rs`, `src/main.rs`, docs)**:
   - In `src/settings_dialog.rs`, add the `SSH Sessions` tab (`ListBox` + profile form + `Save`/`Delete`/`Connect` buttons) and replace `page.downcast::<TerminalView>()` calls with `crate::split::find_terminals(&page)`.
   - In `src/main.rs`, initialize `AskpassBroker::start(&window)` in `app.connect_activate`, register `win.launch-ssh-profile`, add the `HeaderBar` SSH `MenuButton`, extend `create_tab_cb`/`do_split`/`dup_btn` to propagate `SavedSsh` and record `pre_ssh_local_dir`, skip `SavedSsh` tabs in the 200ms CWD timer, and save `SavedTab` with `working_dir = pre_ssh_local_dir` on close.
   - Update `README.md`, `src/README.md`, and `CONTEXT.md`, and verify with `cargo build`, `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`, and manual E2E checks.

---

## Findings — `egm-comprehension`

### Category 1: Authentication, Askpass IPC & Keyring Integration

#### `[comprehension-1-1]` Contradictory `--ssh-askpass` entrypoint condition & broken `argv[1]` extraction
- **Severity:** `🔴 Blocker`
- **Lines:** `193–194`, `218`, `339`, `381`
- **Quotes:**
  - Line `193`: ``At the first line of `fn main()`, before `Application::builder()`, check `std::env::args().any(|a| a == "--ssh-askpass")` or `std::env::var_os("WHATTY_SSH_ASKPASS_SOCKET").is_some() && std::env::args().len() >= 2`:``
  - Line `194`: ``sends a single-line JSON request `{"nonce": "<$WHATTY_SSH_ASKPASS_NONCE>", "prompt": "<argv[1]>", "prompt_hint": "<$SSH_ASKPASS_PROMPT>"}` ``
  - Line `218`: `` `SSH_ASKPASS=<std::env::current_exe()>` ``
  - Line `339`: ``reading `WHATTY_SSH_ASKPASS_SOCKET`, `WHATTY_SSH_ASKPASS_NONCE`, `SSH_ASKPASS_PROMPT`, and `std::env::args().nth(1)` ``
  - Line `381`: ``call `if std::env::args().any(|a| a == "--ssh-askpass") { ssh::run_askpass_client_and_exit(); }` at the top of `fn main()`.``
- **Explanation:** OpenSSH executes `$SSH_ASKPASS` directly (`std::env::current_exe()`) and passes `<prompt>` as `argv[1]` without passing `--ssh-askpass`. Line `381` drops the `WHATTY_SSH_ASKPASS_SOCKET` condition from line `193`, preventing OpenSSH from hitting the askpass path. Conversely, if `--ssh-askpass` is passed explicitly (`whatty --ssh-askpass "Password:"`), `std::env::args().nth(1)` (line `339`) captures `"--ssh-askpass"` instead of the prompt string. Inherited `WHATTY_SSH_ASKPASS_SOCKET` handling in normal `whatty` launches is also unstated.

#### `[comprehension-1-2]` Unsafe `AskpassBroker` threading model (`invoke_local` from worker thread), undefined broker lifecycle, and missing nonce/confirm/auth rules
- **Severity:** `🔴 Blocker`
- **Lines:** `182`, `195–204`, `256`, `260`, `340`, `362`, `382`
- **Quotes:**
  - Line `182`: `` `"secret_kind"`: `"password"` (for `SshAuthKind::PasswordKeyring`) or `"passphrase"` (for `SshAuthKind::IdentityFile`) ``
  - Line `197`: `` `AskpassBroker::register_attempt(profile_id, auth_kind)` generates a random 128-bit hex `nonce` with `password_attempts = 0`.``
  - Line `200`: ``Marshal the prompt request to the GTK main thread via `glib::MainContext::default().invoke_local(...)` to display a modal GTK4 dialog containing the exact `prompt` string:``
  - Line `201`: ``If `prompt_hint == "confirm"` or `prompt.contains("yes/no")`: show `Accept` / `Reject` buttons (or entry for `[fingerprint]`).``
  - Line `202`: ``Otherwise: show a masked `PasswordEntry` (with a `"Save updated password to keyring"` checkbox if it is a password/passphrase prompt).``
  - Line `256`: ``each connection attempt requires a 128-bit random single-session nonce.``
  - Line `260`: ``All `org.freedesktop.secrets` D-Bus calls run off the GTK main thread so locked keyring prompts never block terminal rendering or PTY polling.``
  - Line `382`: ``Initialize `let askpass_broker = ssh::AskpassBroker::start(&window);` inside `app.connect_activate`.``
- **Explanation:**
  - Lines `200` and `340` call `glib::MainContext::default().invoke_local(...)` to marshal from the broker to the GTK main thread, yet `invoke_local` panics if invoked off the main thread (`invoke` is required from worker threads), while the listener architecture itself is unstated.
  - `AskpassBroker::start` return signature, `socket_path` accessor, and `askpass-<pid>.sock` cleanup on shutdown are undefined.
  - Nonce expiration/invalidation across multi-prompt SSH handshakes and error handling for invalid/unknown nonces are undefined.
  - `AgentOrDefault` has no `secret_kind` mapping on line `182`, leaving Case 1 lookup vs. Case 2 fallback and `"Save updated password to keyring"` target `secret_kind` undefined when an `IdentityFile` profile hits a password prompt.
  - Line `201` does not define when `[fingerprint]` shows an entry vs. buttons, or the exact JSON response returned for `confirm` Accept vs. Reject.

#### `[comprehension-1-3]` Underspecified `SecretServiceClient` D-Bus details, synchronous `Prompt` waiting, and keyring failure behavior
- **Severity:** `🟡 Major`
- **Lines:** `177–188`, `338`, `372`
- **Explanation:** `SecretServiceClient` struct fields/constructor are undefined; `CreateItem` secret tuple structure, item properties (`Label`/`Attributes`), and locked-collection prompt handling during `CreateItem`/`Delete` (`!= "/"`) are omitted; how a `std::thread::spawn` worker synchronously waits on `Prompt.Completed` without a `glib::MainLoop` is unstated; and daemon-absent / unlock-canceled behavior in Preferences and `AskpassBroker` is unspecified.

### Category 2: Connection Lifecycle, Error Hold-Open & Reconnection

#### `[comprehension-2-1]` Promised `Ctrl+Shift+R` reconnect and `Close/Reconnect` UI have zero implementation specification (plus `Escape` key contradiction)
- **Severity:** `🔴 Blocker`
- **Lines:** `24`, `47`, `243`, `364`
- **Quotes:**
  - Line `24`: ``keeping failed `SavedSsh` panes open until the user presses `Enter` or clicks Close/Reconnect.``
  - Line `47`: ``until the user presses `Enter` (to close) or triggers reconnection.``
  - Line `243`: ``it feeds `\r\n\x1b[1;31m[WhattyTheShell: SSH connection exited with code <code>. Press Enter to close or Ctrl+Shift+R to reconnect.]\x1b[0m\r\n` into `Terminal::advance`, sets `awaiting_error_dismiss = true`, and emits `"child-exited"` when the user presses `Return`/`KP_Enter` (or reconnects on `Ctrl+Shift+R`).``
  - Line `364`: ``In `key_controller.connect_key_pressed`, when `awaiting_error_dismiss` is true, pressing `Return` / `KP_Enter` or `Escape` emits `"child-exited"`.``
- **Explanation:** Neither `src/widget.rs` (`355–365`) nor `src/main.rs` (`376–388`) specifies any `Ctrl+Shift+R` keybinding, reconnect signal/action, Close/Reconnect button, or PTY respawn logic. Line `364` introduces `Escape` alongside `Return`/`KP_Enter` in conflict with lines `24`, `47`, and `243`, and omits `glib::Propagation::Stop` and double-emit prevention.

### Category 3: Remote POSIX Bootstrap & OpenSSH Invocation

#### `[comprehension-3-1]` Ambiguous `sh -c '...'` escaping across `fish`/`tcsh` and missing bootstrap menu/command edge cases
- **Severity:** `🟡 Major`
- **Lines:** `23`, `72`, `216`, `223–232`, `341`
- **Explanation:** How `<escaped_bootstrap_script>` is escaped for non-POSIX remote shells (`fish`, `tcsh`), exact `tmux new-session`/`screen -S` commands and `whatty-1` collision behavior, safe storage/indexing of `1..K` session names with spaces or `|` in POSIX `sh`, case sensitivity of `N`/`E`/`0`, empty `Enter` behavior when active sessions exist, and exit codes on `Ctrl+C`/`Ctrl+D` are unspecified.

#### `[comprehension-3-2]` Missing `$XDG_RUNTIME_DIR` fallback for `ControlPath` and missing `use_control_master` control in Preferences UI
- **Severity:** `🟡 Major`
- **Lines:** `166`, `196`, `211`, `372`
- **Explanation:** Line `211` specifies `ControlPath=$XDG_RUNTIME_DIR/whattytheshell/cm-%C` without the `/tmp/whattytheshell-<uid>` fallback or directory creation guarantees from line `196`, and Preferences Tab 4 (`372`) exposes no control for `SshProfile.use_control_master`.

### Category 4: Preferences UI, Profile Lifecycle & Tab Restoration

#### `[comprehension-4-1]` Dropped profile-reordering capability, blank-password overwrite risk, missing deleted-profile fallback, and missing `HeaderBar` menu refresh
- **Severity:** `🟡 Major`
- **Lines:** `42`, `208`, `328`, `372`, `384`, `387`
- **Explanation:** Goal `42` promises reordering profiles, which is absent from `ConfigManager` (`328`) and Preferences Tab 4 (`372`). Blank `PasswordEntry` semantics on edit, `port == 0` / empty `name` / `@` host validation, deleted-profile split/duplicate fallback (`384`), UI validation error display, and `HeaderBar` `MenuButton` refresh on save/delete (`387`) are unspecified.

#### `[comprehension-4-2]` Contradictory `SavedTab.custom_title` handling on close and undefined "active" vs. "primary" terminal
- **Severity:** `🟡 Major`
- **Lines:** `245`, `247`, `385–386`
- **Explanation:** Line `247` preserves `custom_title` when manually renamed, whereas line `386` sets `custom_title = false` unconditionally. Lines `245` and `386` refer to `"active TerminalView"` and `"primary terminal"` without defining them for split tabs or explaining initial tab-title assignment without setting `custom_title = true`.

### Category 5: Underspecified Dependencies & Assumed Codebase Context

#### `[comprehension-5-1]` Missing `Cargo.toml` crate specification for JSON IPC and UUID generation
- **Severity:** `🟡 Major`
- **Lines:** `66`, `177`, `194`, `319–397`
- **Explanation:** `Cargo.toml` is omitted from `Target Files to Create/Modify`, leaving unstated whether `serde_json` or `uuid` exist, may be added, or must be replaced by manual JSON logic and GLib/urandom UUIDs.

#### `[comprehension-5-2]` Unspecified existing module interfaces referenced as implementation prerequisites
- **Severity:** `🟡 Major`
- **Lines:** `170`, `177`, `243`, `350–352`, `362`, `364`, `374`, `383`
- **Explanation:** A zero-context reader cannot tell from the spec alone how `TerminalView::with_ssh_profile` (`362`) receives theme/palette/cols/rows settings, how `Pty::terminate` guards against PID reuse after `try_wait_exit_code()` reaps `child` (`352`), or the interfaces of `SavedTab`, `Terminal::advance`, `crate::split::find_terminals`, `src/theme_marketplace.rs`, and `create_tab_cb`/`do_split`/`dup_btn`.

### Category 6: Nits & Minor Inconsistencies

#### `[comprehension-6-1]` Mixed Portuguese tooltip in an otherwise English UI
- **Severity:** `🟢 Nit`
- **Lines:** `65`, `202`, `243`, `372`, `387`
- **Explanation:** `HeaderBar` tooltip `"Sessões SSH Salvas"` (`387`) is in Portuguese while all other UI strings in the document are in English.

#### `[comprehension-6-2]` Minor omissions, unexpanded acronym, and test-list discrepancy
- **Severity:** `🟢 Nit`
- **Lines:** `58`, `161`, `172`, `213`, `317`, `343`, `403`, `414–416`
- **Explanation:** `"EGM"` is undefined; `default_multiplexer_order()` return value (`[Tmux, Screen]`) and `MultiplexerKind::Unknown` / empty priority handling are implied; `config.toml.corrupt.<unix_ts>` omits `0600` permissions and same-second collision rules; `-i <expanded_path>` vs. `auth_kind != IdentityFile` is unclear; and line `403`'s named tests omit the `bypass_multiplexer_probe` test promised on line `343`.
