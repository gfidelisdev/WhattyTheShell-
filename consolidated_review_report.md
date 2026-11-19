# Consolidated Design Review Report

- **Target Document:** [docs/designs/ssh_sessions_and_multiplexer_bootstrap.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/docs/designs/ssh_sessions_and_multiplexer_bootstrap.md) (`416` lines total)
- **Semantic Split:** `FeatureRange = 1..309`, `DetailedRange = 310..416` (dividing heading `## Detailed Implementation Plan` at line `310`)
- **Review Mode:** Consolidated pipeline (`zoom_out -> comprehension -> critic_review -> intermediate_zoom_out -> implementation_readiness`)
- **Overall Verdict:** **FAIL — Revision Required Before Implementation.** While the core direction (delegating SSH execution to OpenSSH `ssh -t`, using a single-connection POSIX `sh -c '...'` multiplexer probe, and tracking `SessionOrigin` per pane) is sound, reviewers identified compile/runtime bugs (`cargo test --lib`, `invoke_local` off the GTK main thread, missing `WHATTY_SSH_ASKPASS_SOCKET` dispatch, `RefCell` `BorrowMutError`, PID-reuse `kill(-pid)`, `POLLHUP`/`try_wait` race, omitted `settings_dialog.rs:1380`), unimplemented UI promises (`Ctrl+Shift+R` reconnect, profile reordering), and `feature-spec-standard.md` compliance gaps (`## Risks & Mitigations`, `The Alternatives Rule`). Applying Stage 8's targeted trims removes several blocker classes while making the architecture smaller.

---

## Stage Reports & Reviewer Execution Matrix

| Stage | Report Artifact | Reviewer Persona | Scope | Status | Findings (🔴 / 🟡 / 🟢) |
| :--- | :--- | :--- | :--- | :--- | :--- |
| Stage 5: Initial Zoom Out | [zoom_out_report.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/zoom_out_report.md) | `egm-zoom-out` | `1..416` | `completed` | 3 🔴 / 4 🟡 / 1 🟢 |
| Stage 5: Initial Zoom Out | [zoom_out_report.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/zoom_out_report.md) | `egm-inventive-catalyst` | `1..416` | `completed` | 2 🔴 / 4 🟡 / 1 🟢 |
| Stage 6: Comprehension | [comprehension_report.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/comprehension_report.md) | `egm-comprehension` | `1..416` (zero context) | `completed` | 3 🔴 / 7 🟡 / 2 🟢 |
| Stage 7: Feature Spec Critic | [critic_report.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/critic_report.md) | `egm-design` | `1..309` | `completed` | 5 🔴 / 7 🟡 / 3 🟢 |
| Stage 7: Feature Spec Critic | [critic_report.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/critic_report.md) | `egm-structural` | `1..309` | `completed` | 2 🔴 / 1 🟡 / 4 🟢 |
| Stage 7: Feature Spec Critic | [critic_report.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/critic_report.md) | `egm-style` | `1..309` | `completed` | 1 🔴 / 8 🟡 / 6 🟢 |
| Stage 8: Intermediate Zoom Out | [intermediate_zoom_out_report.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/intermediate_zoom_out_report.md) | `egm-zoom-out` | `1..416` (post-S7) | `completed` | 5 🔴 / 4 🟡 / 1 🟢 |
| Stage 9: Implementation Readiness | [implementation_readiness_report.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/implementation_readiness_report.md) | `egm-implementation-readiness` | `310..416` | `completed` | 7 🔴 / 8 🟡 / 4 🟢 |
| Stage 9: Implementation Readiness | [implementation_readiness_report.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/implementation_readiness_report.md) | `egm-codebase-details` | `310..416` (+ code xref) | `completed` | 4 🔴 / 8 🟡 / 4 🟢 |
| Stage 9: Implementation Readiness | [implementation_readiness_report.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/implementation_readiness_report.md) | `egm-style` | `310..416` | `completed` | 4 🔴 / 8 🟡 / 3 🟢 |

*(Total: 10 of 10 reviewers `completed`; 0 `not run`.)*

---

## Stage 6 Naive Reader Comprehension Output

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

## Consolidated Findings & Elephant Resolutions

### 🔴 Blockers

#### `[CONS-B1]` Broken `SSH_ASKPASS` Dispatch Contract (`--ssh-askpass` vs. `WHATTY_SSH_ASKPASS_SOCKET`), `argv[1]` Extraction, Env Leakage, & Unverified `< 5 ms` / `< 8 MB` RSS
- **Source IDs & Stages:** `[zoom-out-1-1]` (S5), `[inventive-catalyst-1-1]` (S5), `[comprehension-1-1]` (S6), `[design-1-4]` (S7), `[design-1-13]` (S7), `[style-spec-1-3]` (S7), `[style-spec-1-4]` (S7), `[zoom-out-s8-3]` (S8), `[implementation-readiness-1-2]` (S9), `[codebase-details-1-2]` (S9), `[style-plan-1-2]` (S9)
- **Scope:** Lines `19`, `25`, `43`, `72`, `77`, `109`, `192–194`, `218`, `259`, `339`, `381`
- **Discrepancy & Rationale:** Line `218` sets `SSH_ASKPASS=<std::env::current_exe()>` without `--ssh-askpass`. OpenSSH invokes `$SSH_ASKPASS` directly as `[exe, prompt]`, so line `381` checking only `args().any(|a| a == "--ssh-askpass")` launches a second GTK4 instance (`io.github.whattytheshell`) instead of entering askpass mode. Conversely, if `--ssh-askpass` IS passed explicitly (`whatty --ssh-askpass "Password:"`), `std::env::args().nth(1)` (`339`) extracts `"--ssh-askpass"` instead of `"Password:"`. Additionally, `WHATTY_SSH_ASKPASS_*` variables leak into child `LocalCommand`/`ProxyCommand`/nested shells unless scrubbed, and [Cargo.toml:12-25](file:///home/gfidelis/Code/rust/WhattyTheShell!/Cargo.toml#L12-L25) links GTK4 and Vulkan shared libraries dynamically before `fn main()`, invalidating the `< 5 ms` / `< 8 MB` RSS claim (`259`).
- **Elephant Resolution:** **`PROPOSE FIX`** — Follow Stage 8 `[zoom-out-s8-3]`: either add a minimal `std`-only `[[bin]]` (`whatty-askpass`, using existing `serde_json` from `Cargo.toml:15`) installed alongside `whatty`, OR keep single-binary execution by dispatching strictly on `WHATTY_SSH_ASKPASS_SOCKET` + `WHATTY_SSH_ASKPASS_NONCE` (or a `0700` `whatty-askpass` symlink), scrubbing `WHATTY_SSH_ASKPASS_*` inside `Pty::spawn_inner`, fixing `argv` prompt extraction (`args().skip(1).find(|a| a != "--ssh-askpass")`), and calibrating line `259` (removing unverified `< 5 ms` / `< 8 MB` RSS claims when sharing the GUI binary).

#### `[CONS-B2]` Unsafe Cross-Thread `AskpassBroker` Dispatch (`invoke_local`), Single-Instance `connect_activate` Collision, & Credential Classification Gaps
- **Source IDs & Stages:** `[zoom-out-1-1]` (S5), `[inventive-catalyst-1-1]` (S5), `[inventive-catalyst-1-3]` (S5), `[comprehension-1-2]` (S6), `[design-1-1]` (S7), `[design-1-2]` (S7), `[design-1-9]` (S7), `[zoom-out-s8-1]` (S8), `[implementation-readiness-1-3]` (S9), `[codebase-details-1-3]` (S9), `[codebase-details-1-8]` (S9), `[style-plan-1-4]` (S9)
- **Scope:** Lines `103`, `177–204`, `256`, `260`, `340`, `382`
- **Discrepancy & Rationale:** Calling `glib::MainContext::default().invoke_local(...)` (`200`, `340`) off the GTK main thread panics at runtime, and `&gtk4::ApplicationWindow` (`382`) is `!Send`. Initializing `askpass-<pid>.sock` inside `app.connect_activate` (`382`) collides (`EADDRINUSE`) when opening a second window in single-instance `gtk4::Application` ([src/main.rs:35-50](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/main.rs#L35-L50)). Sequential `UnixListener::accept` blocks concurrent tabs/stalled clients; `/tmp/whattytheshell-<uid>` lacks owner-UID, non-symlink, and `SO_PEERCRED` verification; blind `*password*`/`*passphrase*` matching (`199`) leaks target passwords onto local key passphrases or `ProxyJump` bastions; `AgentOrDefault` lacks a `secret_kind` mapping (`182`); `SSH_ASKPASS_PROMPT=none` renders a password box; and `confirm` / `[fingerprint]` JSON responses + nonce expiration are unspecified.
- **Elephant Resolution:** **`PROPOSE FIX`** — Follow Stage 8 `[zoom-out-s8-1]`: register the non-blocking `UnixListener` directly on the GTK main loop via `unix_fd_add_local` (matching [src/widget.rs:17-61](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/widget.rs#L17-L61)) to eliminate cross-thread `invoke_local` plumbing cleanly (or use `invoke` + `SendWeakRef` + reply channel + per-connection timeouts + `SO_PEERCRED` + `/tmp` owner/`!is_symlink()` checks); instantiate one process-wide `AskpassBroker` in `main()` with `Drop` socket unlink; omit `SSH_ASKPASS` / `SSH_ASKPASS_REQUIRE=force` / nonces for `AgentOrDefault` (and pass `-o IdentitiesOnly=yes` / `-o PubkeyAuthentication=no` per `auth_kind` to prevent cross-host/local-key leakage); cut `"Save updated password to keyring"` (`202`) from the transient askpass modal; and specify `confirm`/`none` prompts, nonce cleanup, and exact `AskpassBroker` signatures.

#### `[CONS-B3]` `SecretServiceClient` Worker-Thread `Prompt.Completed` Hang, Locked-Collection Write/Delete Gaps, & API Misattribution
- **Source IDs & Stages:** `[zoom-out-1-2]` (S5), `[inventive-catalyst-1-3]` (S5), `[comprehension-1-3]` (S6), `[design-1-6]` (S7), `[style-spec-1-7]` (S7), `[zoom-out-s8-1]` (S8), `[zoom-out-s8-10]` (S8), `[implementation-readiness-1-4]` (S9), `[codebase-details-1-10]` (S9), `[codebase-details-1-13]` (S9), `[style-plan-1-8]` (S9)
- **Scope:** Lines `175–188`, `260`, `272–276`, `338`, `372`
- **Discrepancy & Rationale:** Subscribing to `org.freedesktop.Secret.Prompt::Completed` (`185`) inside bare `std::thread::spawn` (`177`) hangs without a running thread-default `glib::MainContext`/`MainLoop` (and subscription must precede calling `Prompt`). Lines `187–188` omit locked-collection prompt handling (`Prompt != "/"`) on `Collection.CreateItem` and `Item.Delete`, `Label`/`Attributes`/`content_type`, `Session.Close()`, orphaned secrets on auth-mode changes, and absent-daemon behavior. Line `177` also merges `DBusConnection::for_address_sync` with free function `gtk4::gio::bus_get_sync` and misattributes `std::thread::spawn + async_channel` to [src/theme_marketplace.rs](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/theme_marketplace.rs) instead of [src/settings_dialog.rs:990-1141](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/settings_dialog.rs#L990-L1141).
- **Elephant Resolution:**
  - **`PROPOSE FIX`** on all D-Bus threading/protocol gaps and exact `SecretServiceClient` Rust signatures (`store_secret_async`, `lookup_secret_sync`, `delete_secret_async`), scoped `glib::MainContext`/`MainLoop` + timeout around `Completed`, locked-collection handling on save/delete, daemon-unavailable UX, `gio::bus_get_sync`, and citing `src/settings_dialog.rs`.
  - **`CONTEST` (Partial)** on Stage 5 `[zoom-out-1-2]` recommending adding external `oo7` / `secret-service` crates to `Cargo.toml`: keeping `gtk4::gio::bus_get_sync` preserves the zero-new-crate constraint while leveraging GLib already linked by `gtk4`.

#### `[CONS-B4]` Unwired `Ctrl+Shift+R` Reconnect & `Close/Reconnect` UI Promises (plus Conflicting `Escape` Dismissal)
- **Source IDs & Stages:** `[zoom-out-1-3]` (S5), `[inventive-catalyst-1-2]` (S5), `[comprehension-2-1]` (S6), `[design-1-5]` (S7), `[structural-1-4]` (S7), `[style-spec-1-5]` (S7), `[zoom-out-s8-2]` (S8), `[implementation-readiness-1-5]` (S9), `[codebase-details-1-4]` (S9), `[codebase-details-1-16]` (S9), `[style-plan-1-5]` (S9)
- **Scope:** Lines `24`, `47`, `94–98`, `243`, `358`, `364`, `380–388`
- **Discrepancy & Rationale:** Lines `24`, `47`, and `243` promise clickable `Close/Reconnect` UI and `Ctrl+Shift+R` reconnection inside the red ANSI banner, yet `TerminalView` holds neither `ConfigManager` nor `AskpassBroker`, and lines `310..416` specify zero reconnect wiring, widgets, or respawn paths. Meanwhile, line `364` introduces `Escape` dismissal in conflict with lines `24`, `47`, and `243`.
- **Elephant Resolution:** **`PROPOSE FIX`** — Adopt Stage 8 `[zoom-out-s8-2]`: **CUT** `Ctrl+Shift+R` reconnect and clickable `Close/Reconnect` buttons from v1 (re-launching is already accessible from the `HeaderBar` quick-launch menu, avoiding `gtk4::Overlay` / `Paned` replacement complexity), keep the inline diagnostic banner with `Return`/`KP_Enter`-to-close, remove conflicting `Escape` from line `364`, and clarify exit `0` / `None` disposition. *(Alternative if `Ctrl+Shift+R` must stay: add a `"reconnect-requested"` GObject signal on `TerminalView` handled in `src/main.rs`.)*

#### `[CONS-B5]` `RefCell` `BorrowMutError` Panic on Dismissal, `Pty::terminate` PID-Reuse Signal Hazard, `POLLHUP`/`try_wait` EOF Race, & Main-Loop Stall
- **Source IDs & Stages:** `[zoom-out-1-3]` (S5), `[zoom-out-1-4]` (S5), `[inventive-catalyst-1-2]` (S5), `[design-1-7]` (S7), `[zoom-out-s8-2]` (S8), `[implementation-readiness-1-6]` (S9), `[implementation-readiness-1-7]` (S9), `[codebase-details-1-12]` (S9), `[style-plan-1-5]` (S9), `[style-plan-1-11]` (S9)
- **Scope:** Lines `238`, `242–243`, `351–352`, `364`
- **Discrepancy & Rationale:**
  1. `key_controller.connect_key_pressed` ([src/widget.rs:611-634](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/widget.rs#L611-L634)) holds `w.imp().inner.borrow()` across key handling; emitting `"child-exited"` calls `close_pane_or_tab` -> `tv.terminate()` ([src/widget.rs:1294-1301](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/widget.rs#L1294-L1301)) -> `inner.borrow_mut().take()`, panicking with `BorrowMutError`. Input gating for `im_context.connect_commit` ([src/widget.rs:557-575](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/widget.rs#L557-L575)) and preservation of `Ctrl+Shift+C`/scrollback while `awaiting_error_dismiss == true` are omitted.
  2. Reaping `self.child` in `Pty::try_wait_exit_code()` leaves `self.child.id()` populated; later `Pty::terminate()` ([src/pty.rs:312-322](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/pty.rs#L312-L322)) still calls `libc::kill(-pid, SIGHUP/SIGKILL)` against a potentially recycled OS PID unless guarded by `if self.last_exit_code.is_some()`.
  3. On Linux, PTY master `POLLHUP`/EOF can arrive before kernel zombie reaping makes `waitpid(WNOHANG)` return the exit status, causing single `try_wait()` calls on fast `ssh` failures to return `None` and drop the pane silently; additionally, raw `code != 0` triggers the error banner if a normal interactive session runs a failing command (`grep`/`Ctrl+C`) right before `Ctrl+D`.
  4. Synchronous `30 ms` `try_wait()` polling inside `Pty::terminate()` blocks the GTK main loop for `30 ms × N` hung panes during `close_others_btn` or window close.
- **Elephant Resolution:** **`PROPOSE FIX`** — Drop `inner` borrow guards + reset `awaiting_error_dismiss` before emitting `"child-exited"`, gate `im_context.connect_commit` + PTY writes while preserving copy/scrollback and returning `glib::Propagation::Stop`; guard `Pty::terminate()` on `last_exit_code.is_some()`; add a bounded reap wait on `SavedSsh` EOF (and distinguish pre-exec SSH failures or document shell exit behavior); and avoid synchronous `30 ms × N` main-loop stalls (offload escalated `SIGKILL` reaper or avoid blocking teardown on main thread).

#### `[CONS-B6]` Non-POSIX Remote `sh -c '...'` Quoting Breakage (`fish`/`tcsh`) & Bootstrap Selector Bugs
- **Source IDs & Stages:** `[inventive-catalyst-1-4]` (S5), `[comprehension-3-1]` (S6), `[design-1-3]` (S7), `[style-spec-1-12]` (S7), `[zoom-out-s8-4]` (S8), `[implementation-readiness-1-10]` (S9), `[codebase-details-1-14]` (S9), `[style-plan-1-8]` (S9)
- **Scope:** Lines `11`, `23`, `72`, `216`, `223–233`, `341`
- **Discrepancy & Rationale:** OpenSSH joins remote commands through `$REMOTE_LOGIN_SHELL -c "..."`. A multiline `sh -c '...'` script containing embedded single quotes (`tmux list-sessions -F '#{...}'`) breaks under `fish` (backslash escapes inside `'...'`) and `tcsh` (`Unmatched '.`). Furthermore, `screen -ls` fails to filter `(Dead ???)`/`(Removed)` sockets, `-t <name>` matches prefixes without `-t "=$name"`, `|` delimiter fails if session names contain `|`, dynamic `1..K` selection lacks safe POSIX-`sh` quoting for spaces/leading dashes, `whatty-1` fixed naming collides on split panes, and `[E] (or 0)` / empty `Enter` / `Ctrl+D` / empty-priority behaviors are undefined.
- **Elephant Resolution:** **`PROPOSE FIX`** via Stage 8 `[zoom-out-s8-4]` and Stage 5 `[inventive-catalyst-1-4]`: build a **single-line** POSIX `sh -c '...'` script containing zero internal single quotes `'`, zero backslashes `\`, and zero newlines `\n` (enforced by unit test), filter `Dead` sockets from `LC_ALL=C screen -ls`, use exact target quoting (`tmux attach-session -t "=$sel"` / `screen -d -r "$sel"`), define Enter/invalid/`Ctrl+D`/empty `priority` rules, let `tmux new-session`/`screen` choose default session names when unnamed (`[N]`), and keep `[E]` consistently (dropping `(or 0)`). *(Note: Avoid `base64 -d` wrappers as `OpenBSD` lacks `base64` in base.)*

#### `[CONS-B7]` Broken `cargo test --lib` Commands & Phase 2 vs. Phase 4 `mod ssh;` Build Ordering
- **Source IDs & Stages:** `[zoom-out-s8-5]` (S8), `[implementation-readiness-1-1]` (S9), `[codebase-details-1-1]` (S9), `[style-plan-1-1]` (S9)
- **Scope:** Lines `314–316`, `381`
- **Discrepancy & Rationale:** Verified in [Cargo.toml:1-9](file:///home/gfidelis/Code/rust/WhattyTheShell!/Cargo.toml#L1-L9): the package defines only a binary target (`WhattyTheShell`) with no `src/lib.rs`, so every `cargo test --lib <module>` command (`314–316`) fails. Phase 2 (`315`) also runs `ssh` tests before `mod ssh;` is declared in `src/main.rs` (`381`).
- **Elephant Resolution:** **`PROPOSE FIX`** — Replace `--lib` test commands with `cargo test --bin WhattyTheShell <module>::tests` and move `mod ssh;` declaration to Phase 2.

#### `[CONS-B8]` Violations of `The Alternatives Rule` (`"Rejected by author:"`, Circular Rationales, Dangling `(Option 1)`)
- **Source IDs & Stages:** `[structural-1-2]` (S7), `[style-spec-1-1]` (S7), `[design-1-5]` / `[design-1-14]` (S7), `[zoom-out-s8-6]` (S8)
- **Scope:** Lines `177`, `264–307` (`270`, `276`, `282`, `288`, `294`, `300`, `306`)
- **Discrepancy & Rationale:** All 7 `* **Reason for Rejection**` entries begin with unlinked `"Rejected by author:"` (violating `feature-spec-standard.md#L19-L21`). Alternatives D, F, and G (`288`, `300`, `306`) restate chosen policy instead of concrete trade-offs; Alternative A (`270`) cites orphaned `"(Option 1)"` leaked from `concepts_brief.md`; line `294` asserts an unmeasured `"lowest"` superlative; and line `177`'s D-Bus crate trade-off (`gio` vs `zbus`/`oo7`/`libsecret-1`) is omitted.
- **Elephant Resolution:** **`PROPOSE FIX`** — Remove `"Rejected by author:"` and `"(Option 1)"`, keep technical alternatives (`A`, `B`, `C`, `E`, plus brief `gio` vs `zbus`/`oo7`/`libsecret-1` note) with calibrated concrete trade-offs, and fold pure non-goal restatements (`D`, `F`, `G`) into `## Non-Goals` (or state real technical trade-offs such as startup handshake/`MaxStartups` storms and `TIOCSWINSZ` contention).

#### `[CONS-B9]` Missing Dedicated `## Risks & Mitigations` Section & Unaddressed Operational/Security Limits
- **Source IDs & Stages:** `[structural-1-1]` (S7), `[style-spec-1-2]` (S7), `[design-1-5]` / `[design-1-12]` (S7), `[zoom-out-s8-6]` (S8)
- **Scope:** Lines `21–24`, `208`, `251–262`
- **Discrepancy & Rationale:** `FeatureRange` omits the mandatory `## Risks & Mitigations` / `## Risks & Limits` section required by `feature-spec-standard.md#L23-L39`, leaving runtime failure modes unaddressed: `108`-byte `sockaddr_un.sun_path` bounds, stale sockets after `SIGKILL`, missing/locked `org.freedesktop.secrets`, missing `ssh` (`ENOENT`), OpenSSH `< 8.4` (`SSH_ASKPASS_REQUIRE`), `RemoteCommand` conflict in `~/.ssh/config`, and large profile menu scaling.
- **Elephant Resolution:** **`PROPOSE FIX`** — Add a concise `## Risks & Mitigations` section covering these constraints while condensing duplicated `Heilmeier Catechism Summary` bullets (`[CONS-M10]`) so total spec length stays compact.

---

### 🟡 Majors

#### `[CONS-M1]` Omitted 6th `page.downcast::<TerminalView>()` Site at `src/settings_dialog.rs:1380`
- **Source IDs & Stages:** `[zoom-out-1-7]` (S5), `[inventive-catalyst-1-6]` (S5), `[zoom-out-s8-7]` (S8), `[implementation-readiness-1-13]` (S9), `[codebase-details-1-5]` (S9), `[style-plan-1-3]` (S9)
- **Scope:** Line `374` vs. [src/settings_dialog.rs:1378-1384](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/settings_dialog.rs#L1378-L1384)
- **Discrepancy & Rationale:** Line `374` updates lines `547, 636, 950, 1366, 1409` in `src/settings_dialog.rs` to use `split::find_terminals(&page)`, missing line `1380` (`opacity_scale` `<= 100%` branch calling `tv.set_saturation_boost(1.0)`), leaving split panes stuck supersaturated.
- **Elephant Resolution:**
  - **`PROPOSE FIX`** to include line `1380` and extract a shared `for_each_terminal` helper in `src/settings_dialog.rs`.
  - **`CONTEST`** Stage 5 `[zoom-out-1-7]` bullet #4 claiming split traversal in `settings_dialog.rs` is unrelated scope creep: SSH tabs explicitly support `Ctrl+Shift+E`/`O` splits, so live font/color/theme/saturation changes must apply across split SSH panes cleanly.

#### `[CONS-M2]` Default-On `ControlMaster=auto` CLI Override, Unexpanded `$XDG_RUNTIME_DIR` `ControlPath`, & Missing Toggle
- **Source IDs & Stages:** `[zoom-out-1-5]` (S5), `[inventive-catalyst-1-5]` (S5), `[comprehension-3-2]` (S6), `[design-1-8]` (S7), `[structural-1-7]` (S7), `[style-spec-1-6]` (S7), `[zoom-out-s8-8]` (S8), `[implementation-readiness-1-11]` (S9), `[codebase-details-1-10]` (S9)
- **Scope:** Lines `25`, `165–166`, `196`, `211`, `281–282`, `342`, `372`
- **Discrepancy & Rationale:** OpenSSH does not expand `$XDG_RUNTIME_DIR` inside `-o ControlPath=...` (`211`), line `211` lacks line `196`'s `/tmp` fallback and `0700` dir creation, long prefixes + OpenSSH's temp suffix risk the `108`-byte `sun_path` limit, forced `-o ControlMaster=auto` overrides user `~/.ssh/config` directives (contradicting Alternative C `281`), and `SshProfile::use_control_master` has no toggle in Tab 4 (`372`).
- **Elephant Resolution:** **`PROPOSE FIX`** — Prefer Stage 8's trim (defer `ControlMaster` to user `~/.ssh/config` and drop `use_control_master` + `-o ControlPath`), OR resolve `ssh_runtime_dir()` in Rust with `< 88`-byte guard, `ServerAliveInterval`, and Tab 4 switch.

#### `[CONS-M3]` `TerminalView::current_working_dir()`, `custom_title` Contradiction, Split Ownership, & `src/main.rs` Gaps
- **Source IDs & Stages:** `[zoom-out-1-7]` (S5), `[inventive-catalyst-1-6]` (S5), `[comprehension-4-2]` (S6), `[design-1-10]` (S7), `[zoom-out-s8-7]` (S8), `[implementation-readiness-1-14]` (S9), `[implementation-readiness-1-18]` (S9), `[codebase-details-1-8]` (S9), `[codebase-details-1-9]` (S9), `[codebase-details-1-12]` (S9), `[style-plan-1-9]` (S9)
- **Scope:** Lines `244–247`, `362`, `383–386`
- **Discrepancy & Rationale:** Calling `current_working_dir()` from `win.new-tab`, tab-bar double-click, `copy_path_btn`, and `restore_name_btn` reads `/proc/<ssh_pid>/cwd` on SSH panes; setting initial tab title via `title: Some(...)` marks `view.custom_title` (breaking `247` vs `386`, `dup_btn` `" (cópia)"`, and `merge_tabs`); `"primary terminal"` (`386`) is undefined for split trees; `do_split` signature contradicts `383` vs `384` and doesn't pre-check `MAX_PANES_PER_TAB = 4` before spawning `ssh`; deleted-profile split fallback and `AskpassBroker: Clone` are missing; and `with_ssh_profile` duplicates 850 lines of `with_shell_and_working_dir`.
- **Elephant Resolution:** **`PROPOSE FIX`** — Centralize `pre_ssh_local_dir` inside `TerminalView::current_working_dir()`, decouple SSH profile display name from `custom_title` (preserving `247` manual renames), define `"primary terminal"` via `split::find_terminals(&page).first()`, check `MAX_PANES_PER_TAB` before spawning, align `do_split`/`create_tab_cb` (`CreateTabFn`) + `AskpassBroker: Clone` + deleted-profile local fallback, and extract private `TerminalView::with_pty_and_origin(...)`.

#### `[CONS-M4]` Dropped Profile Reordering, Password-Edit Semantics, `HeaderBar` Sync, & Validation Gaps
- **Source IDs & Stages:** `[zoom-out-1-7]` (S5), `[inventive-catalyst-1-7]` (S5), `[comprehension-4-1]` (S6), `[design-1-6]` / `[design-1-12]` (S7), `[zoom-out-s8-8]` (S8), `[implementation-readiness-1-12]` (S9), `[codebase-details-1-10]` (S9), `[codebase-details-1-11]` (S9), `[style-plan-1-6]` (S9)
- **Scope:** Lines `42`, `208`, `213`, `328`, `370`, `372–373`, `387`
- **Discrepancy & Rationale:** Goal `42` promises reordering (`ConfigManager` and Tab 4 omit controls); `HeaderBar` `MenuButton` (`387`) has no refresh hook when Preferences saves/deletes profiles (plus `pack_start`/`pack_end` & empty state omitted); line `370` claims `"testing/saving keyring passwords"` with no test button on `372`; `PasswordEntry` edit-blank behavior (preserve vs clear), `Auth Type` field sensitivity, save-then-`Connect` async race, validation UI feedback, conditional `-i` vs `auth_kind`, and `host` whitespace/`@`/`port == 0` checks are omitted.
- **Elephant Resolution:** **`PROPOSE FIX`** — Cut reordering from Goal `42` (or add `move_ssh_profile` + Up/Down controls), remove `"testing"` from `370`, use `MenuButton::set_create_popup_func` in `main.rs` (`pack_end`/`pack_start` + empty state), gate `Connect` on async save completion, clarify empty-password-preserves + explicit clear toggle, conditional `-i`, and reject `@`/whitespace/`port == 0`.

#### `[CONS-M5]` `SshProfile` Schema Simplification (`MultiplexerKind` & `SshAuthKind` YAGNI)
- **Source IDs & Stages:** `[zoom-out-1-6]` (S5)
- **Scope:** Lines `51`, `129–168`, `182`, `326`, `372`
- **Discrepancy & Rationale:** Storing `Vec<MultiplexerKind>` (`Tmux`, `Screen`, `Unknown`) speculates on unsupported multiplexers excluded by Non-Goals (`51`) and permits invalid empty/duplicate states when UI only toggles two orders (`tmux -> screen` vs `screen -> tmux`). Keying D-Bus secrets by `secret_kind` couples `SshAuthKind` awkwardly and orphans secrets when auth mode changes.
- **Elephant Resolution:** **`PROPOSE FIX`** (Recommended simplification: replace `Vec<MultiplexerKind>` with `TmuxFirst` / `ScreenFirst` enum and key keyring secrets strictly by `profile_id`; or if keeping `Vec<MultiplexerKind>`, explicitly define `default_multiplexer_order()` and ignore empty/`Unknown` entries).

#### `[CONS-M6]` Missing UUID & 128-Bit CSPRNG Nonce Primitives Within Zero-Crate Boundary
- **Source IDs & Stages:** `[zoom-out-1-8]` (S5), `[comprehension-5-1]` (S6), `[zoom-out-s8-9]` (S8), `[implementation-readiness-1-8]` (S9), `[codebase-details-1-6]` (S9), `[style-plan-1-7]` (S9)
- **Scope:** Lines `19`, `66`, `177`, `194`, `197`, `319–397`
- **Discrepancy & Rationale:** [Cargo.toml:10-33](file:///home/gfidelis/Code/rust/WhattyTheShell!/Cargo.toml#L10-L33) has no `uuid` or `rand`/`getrandom` crate and is outside Target Files (`319–397`). Note that `[implementation-readiness-1-8]` inaccurately claimed `serde_json` is also missing from `Cargo.toml`, whereas [Cargo.toml:15](file:///home/gfidelis/Code/rust/WhattyTheShell!/Cargo.toml#L15) already includes `serde_json = { version = "1.0", features = ["preserve_order"] }`.
- **Elephant Resolution:** **`PROPOSE FIX`** for UUID/nonce primitives (specify `gtk4::glib::uuid_string_random()` and `/dev/urandom` or `libc::getrandom` via existing `libc = "0.2"` in `Cargo.toml:23`, plus reusing existing `serde_json`), and **`CONTEST`** `[implementation-readiness-1-8]`'s inaccurate claim that `serde_json` is absent from `Cargo.toml`.

#### `[CONS-M7]` `ConfigManager` Persistence Weaknesses & Auto-Save Overwrite Contradiction
- **Source IDs & Stages:** `[zoom-out-1-7]` (S5), `[design-1-11]` (S7), `[style-spec-1-9]` (S7), `[zoom-out-s8-9]` (S8), `[implementation-readiness-1-9]` (S9), `[codebase-details-1-7]` (S9), `[style-plan-1-15]` (S9)
- **Scope:** Lines `171–173`, `261–262`, `328–331`
- **Discrepancy & Rationale:** Creating `config.toml.tmp` before setting `0600` leaves a `0644` race window (`OpenOptionsExt::mode(0o600)` required); [src/config.rs:178](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/config.rs#L178) uses `.unwrap_or_default()`, discarding I/O errors; existing dir permissions aren't forced to `0700`; `.corrupt.<unix_ts>` omits `0600` + collision suffix; window close calls `set_open_tabs` (`save()`) automatically, contradicting line `172` (*"skips overwriting config.toml until the next explicit user save"*); and `v0.4.3` downgrade saves strip `ssh_profiles` (`262`).
- **Elephant Resolution:** **`PROPOSE FIX`** across all six items and replace `"100% compatible with v0.4.3"` (`262`) with `"unchanged"` + explicit downgrade overwrite note.

#### `[CONS-M8]` Missing Headless Unit-Test Seams & Verification Plan Mismatches
- **Source IDs & Stages:** `[zoom-out-s8-9]` (S8), `[implementation-readiness-1-15]` (S9), `[codebase-details-1-7]` (S9), `[codebase-details-1-12]` (S9), `[style-plan-1-10]` (S9), `[comprehension-6-2]` (S6)
- **Scope:** Lines `331`, `343`, `365`, `400–410`
- **Discrepancy & Rationale:** `ConfigManager` hardcodes `dirs_next::config_dir()` (and Rust 2024 makes `env::set_var` `unsafe` + racy); `AskpassBroker` and `TerminalView` tests require GTK/`wgpu`/live D-Bus unless decoupled behind test seams; `bypass_multiplexer_probe` (`343`) is absent from line `403`; negative tests (`user` rejection, invalid nonce, askpass IPC, post-reap `terminate`) and E2E UI/error checks are missing; and line `409` / E2E pass assertions are imprecise.
- **Elephant Resolution:** **`PROPOSE FIX`** — Add `ConfigManager::load_or_default_in(&Path)`, extract headless prompt-classification and exit-handling helpers, align `bypass_multiplexer_probe` naming (`403` vs `343`), add missing negative unit tests, and complete manual E2E pass criteria (`408–410`).

#### `[CONS-M9]` Unspecified Internal Contracts & Signature Precision in `DetailedRange`
- **Source IDs & Stages:** `[comprehension-5-2]` (S6), `[style-plan-1-8]` (S9), `[codebase-details-1-15]` (S9), `[codebase-details-1-16]` (S9)
- **Scope:** Lines `161`, `326`, `338`, `350–352`, `362`
- **Discrepancy & Rationale:** `Pty::spawn_inner` ([src/pty.rs:112-215](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/pty.rs#L112-L215)) unconditionally appends `-l` unless conditionalized for non-shell `program` (`"ssh"`); `with_ssh_profile` (`362`) omits how palette/config styling and `cols`/`rows` are passed vs `with_shell_and_working_dir`; `default_multiplexer_order()` (`161`) and `SecretServiceClient` (`338`) signatures are omitted; and `[comprehension-5-2]` noted existing struct definitions (`SavedTab`) aren't inlined for zero-context readers.
- **Elephant Resolution:**
  - **`PROPOSE FIX`** in `DetailedRange` (`310..416`) to document `Pty::spawn_inner` non-`-l` handling, `with_ssh_profile` palette/config/dimension sourcing (via shared `with_pty_and_origin`), `default_multiplexer_order()`, and `SecretServiceClient` Rust signatures.
  - **`CONTEST`** `[comprehension-5-2]`'s expectation of inlining existing codebase definitions (`SavedTab`, `Terminal::advance`) inside `FeatureRange` (`1..309`), since existing codebase contracts belong in `src/README.md`.

#### `[CONS-M10]` Uncalibrated Claims & Redundant `Heilmeier Catechism Summary`
- **Source IDs & Stages:** `[style-spec-1-3]` (S7), `[style-spec-1-8]` (S7), `[design-1-13]` (S7), `[zoom-out-s8-6]` (S8), `[zoom-out-s8-10]` (S8)
- **Scope:** Lines `15–29`, `259`
- **Discrepancy & Rationale:** Lines `15–29` repeat `The Problem`, `Goals`, and `NFRs`, contain academic grant headings (`Mid-term exam`, `Final exam`), line `25` says `"1 SSH connection per terminal pane (shared via ControlMaster)"`, and line `259` omits a conjunction in `"< 5 ms RSS < 8 MB"` while claiming unverified numbers.
- **Elephant Resolution:** **`PROPOSE FIX`** — Replace lines `15–29` with a concise 1-paragraph summary (replacing academic labels with `Unit Verification` / `End-to-End Verification`), fix `"1 ssh -t PTY process per terminal pane"` (`25`), and fix syntax + calibrate line `259`.

---

### 🟢 Nits

1. **`[CONS-N1]` Mermaid Diagram Drift (`103`, `107`)** — `[structural-1-3]`, `[style-spec-1-6]`, `[zoom-out-s8-10]` → **`PROPOSE FIX`**: Update socket label to `$XDG_RUNTIME_DIR/whattytheshell/askpass-{pid}.sock` (escaping `{pid}`), and move `AskpassShim`/`RemoteSh` out of the `src/pty.rs` subgraph.
2. **`[CONS-N2]` Mixed UI Language (`"Sessões SSH Salvas"`, `387`)** — `[comprehension-6-1]`, `[style-plan-1-14]`, `[zoom-out-s8-10]` → **`PROPOSE FIX`**: Align new UI strings consistently (e.g., all English in `SSH Sessions` Preferences and `HeaderBar` tooltip, or explicitly document matching `main.rs`'s Portuguese tooltips vs English Preferences labels).
3. **`[CONS-N3]` Internal Tooling Jargon (`58`, `317`, `392`, `416`)** — `[style-spec-1-14]`, `[comprehension-6-2]`, `[style-plan-1-13]` → **`PROPOSE FIX`**: Replace `"/egm-brainstorm phase"` and `"EGM contracts"` / `"*Drafted with EGM*"` with `"concepts_brief.md"` and `"architecture contracts (README.md, src/README.md, CONTEXT.md)"`.
4. **`[CONS-N4]` Minor Implementation Plan Polish (`161`, `315`, `343`, `348`, `352`, `372`, `400`)** — `[implementation-readiness-1-16]`, `[implementation-readiness-1-17]`, `[implementation-readiness-1-19]`, `[codebase-details-1-15]`, `[style-plan-1-15]`, `[comprehension-6-2]` → **`PROPOSE FIX`**: Document `gtk4::PasswordEntry` import and `Port` widget (`Entry` vs `SpinButton`) in `src/settings_dialog.rs` (`372`), `Pty::terminate` poll interval (`2 ms`, `352`), temporary `#[allow(dead_code)]` across Phases 2–3 (`315`/`400`), `(e.g., 255)` (`348`), and `bypass_multiplexer_probe remote command omission` (`343`).
5. **`[CONS-N5]` `src/README.md` Wildcard `[ssh.rs:*]` & `CONTEXT.md` Structure (`394`, `396`)** — `[codebase-details-1-16]`, `[style-plan-1-12]`, `[style-plan-1-15]` → **`PROPOSE FIX`**: Enumerate exact `[ssh.rs:...]` symbols plus `SshAuthKind`, `win.launch-ssh-profile`, and `TerminalView::with_ssh_profile`, and target a `## Remote SSH Sessions` section + `_Avoid_` aliases in `CONTEXT.md`.
6. **`[CONS-N6]` Spec Prose Calibration (`20`, `37`, `42`, `72`, `224`, `275`)** — `[style-spec-1-10]`, `[style-spec-1-11]`, `[style-spec-1-13]`, `[style-spec-1-15]`, `[design-1-15]`, `[zoom-out-s8-10]` → **`PROPOSE FIX`**: Replace `"1-click"` with `"HeaderBar quick-launch menu"`, qualify `"with ~"` (`"with the client's local working directory"`), add `/opt/homebrew/bin` to remote `PATH` (`224`), note `read choice` (`K >= 10`), normalize `"Saved SSH Profile"` vs `"Saved SSH Session"`, and clarify `scripts/build-appimage.sh` staging vs `.deb`/`.rpm` packaging (`275`).
7. **`[CONS-N7]` Formatting Symmetry & Cross-Reference Anchors (`7–308`, `311–396`)** — `[structural-1-5]`, `[structural-1-6]`, `[structural-1-7]`, `[style-plan-1-9]`, `[style-plan-1-14]` → **`PROPOSE FIX`** on link consistency, Goals/Non-Goals symmetry, removing orphan `---` (`249`), and aligning `Phase 2` vs `Phase 3` descriptions (`26` vs `315`); **`CONTEST`** converting intentional step-lists into uniform bullets where sequence matters.
8. **`[CONS-N8]` Over-Prescription of Existing Codebase Context for Zero-Context Readers** — `[comprehension-5-2]` → **`CONTEST`** inlining full existing codebase definitions (`SavedTab`, `find_terminals`) inside `FeatureRange`, as repository module contracts fulfill that role without bloating design specs.
