# Stage 9: Implementation Readiness Report (`DetailedRange`: `310..416`)

- **Target Document:** [docs/designs/ssh_sessions_and_multiplexer_bootstrap.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/docs/designs/ssh_sessions_and_multiplexer_bootstrap.md#L310-L416)
- **Standard:** [implementation-readiness-standard.md](file:///home/gfidelis/.gemini/config/plugins/egm/resources/document_standards/implementation-readiness-standard.md)
- **Round:** `1`
- **Verdict:** **FAIL — Not Implementation-Ready**

## Reviewer Execution Status

| Reviewer | Role / Focus | Status | Findings (🔴 / 🟡 / 🟢) |
| :--- | :--- | :--- | :--- |
| `egm-implementation-readiness` | Actionability, Zero-Decision Rule, File Scope, Phasing & Test Plan | `completed` | 7 🔴 / 8 🟡 / 4 🟢 |
| `egm-codebase-details` | Exact Codebase Symbols, Signatures, Crates & Invariants | `completed` | 4 🔴 / 8 🟡 / 4 🟢 |
| `egm-style` | Plan Precision, Consistency & Step Hygiene (`310..416`) | `completed` | 4 🔴 / 8 🟡 / 3 🟢 |

---

## Findings — `egm-implementation-readiness`

### 🔴 Blockers

#### `[implementation-readiness-1-1]` `cargo test --lib ...` targets a non-existent library crate
- **Scope:** Lines `314–316`
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** Phases 1–3 verify with `cargo test --lib config`, `cargo test --lib ssh`, `cargo test --lib pty`, and `cargo test --lib widget`. [Cargo.toml](file:///home/gfidelis/Code/rust/WhattyTheShell!/Cargo.toml#L1-L10) defines only a binary crate (`WhattyTheShell`) with no `src/lib.rs`, so `--lib` fails immediately. Phase 2 (`315`) also tests `ssh` before `mod ssh;` is added to `src/main.rs` in Phase 4 (`381`). Use `cargo test --bin WhattyTheShell <module>::tests` and wire `mod ssh;` in Phase 2.

#### `[implementation-readiness-1-2]` `--ssh-askpass` intercept fails when invoked by OpenSSH
- **Scope:** Line `381` (vs. lines `193`, `218`, `339`)
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** Line `381` checks only `args().any(|a| a == "--ssh-askpass")`, omitting `WHATTY_SSH_ASKPASS_SOCKET` (`193`), whereas `build_ssh_invocation` sets `SSH_ASKPASS=<current_exe()>` (`218`) without `--ssh-askpass`. Conversely, passing `--ssh-askpass` manually makes `args().nth(1)` (`339`) read `"--ssh-askpass"` instead of the prompt text. Specify both checks, skip `--ssh-askpass` when extracting the prompt arg, and scrub `WHATTY_SSH_ASKPASS_*` for local shells.

#### `[implementation-readiness-1-3]` `AskpassBroker` GTK dispatch panics across threads and lacks exact API/lifecycle contracts
- **Scope:** Lines `340`, `382`
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** Calling `glib::MainContext::default().invoke_local` off the GTK main thread panics, and `&gtk4::ApplicationWindow` is `!Send`. Use `glib::MainContext::default().invoke(...)` + `glib::SendWeakRef<ApplicationWindow>` + reply channel (or drive the non-blocking `UnixListener` on the GLib main loop via `unix_fd_add_local`). Specify `AskpassBroker::start(&ApplicationWindow)` return type, `socket_path()`, `register_attempt`/`unregister_attempt`, stale socket/nonce cleanup, timeouts, `AgentOrDefault` mapping, and `confirm` JSON output.

#### `[implementation-readiness-1-4]` `SecretServiceClient` D-Bus signatures, worker-thread signal waiting, and locked-collection handling are underspecified
- **Scope:** Line `338`
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** Method names lack Rust parameter/return/error signatures. `org.freedesktop.Secret.Prompt::Completed` never dispatches in `std::thread::spawn` unless that thread creates and runs a `glib::MainContext`/`MainLoop`. `CreateItem` secret tuple/properties (`Label`, `Attributes`, `content_type`), locked-collection `Unlock` on `CreateItem`/`Delete`, multi-match handling, and missing D-Bus service behavior must be explicitly specified.

#### `[implementation-readiness-1-5]` Advertised `Ctrl+Shift+R` reconnect feature is completely missing from the implementation plan
- **Scope:** Lines `360–365`, `380–387` (vs. line `243`)
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** Banner line `243` instructs users to press `Ctrl+Shift+R to reconnect`, yet `src/widget.rs` and `src/main.rs` specify zero keybinding, signal, or PTY-respawn/pane-replacement implementation. Either remove `Ctrl+Shift+R` / `Close/Reconnect` or define exact reconnect wiring.

#### `[implementation-readiness-1-6]` `RefCell` `BorrowMutError` panic on `"child-exited"` and unspecified input suppression when `awaiting_error_dismiss` is true
- **Scope:** Line `364`
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** `key_controller.connect_key_pressed` ([src/widget.rs](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/widget.rs#L611-L633)) holds `w.imp().inner.borrow()`; emitting `"child-exited"` synchronously calls `tv.terminate()` -> `inner.borrow_mut().take()`, panicking with `BorrowMutError`. Must drop `inner` borrow first, reset `awaiting_error_dismiss`, block PTY writes in both `key_controller` and `im_context.connect_commit`, preserve copy/scrollback shortcuts (`Ctrl+Shift+C`, `Shift+PageUp`/`PageDown`), return `glib::Propagation::Stop`, and reconcile `Escape` (`364`) vs. `243`.

#### `[implementation-readiness-1-7]` `Pty::try_wait_exit_code` EOF race and `Pty::terminate` PID-reuse hazard
- **Scope:** Lines `351–352`, `364`
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** Reaping `self.child` in `try_wait_exit_code()` leaves `self.child.id()` populated, so later `Pty::terminate()` calls `libc::kill(-pid, ...)` against a potentially recycled PID unless `terminate()` returns early when `last_exit_code.is_some()`. Furthermore, a single non-blocking `try_wait()` (`WNOHANG`) on PTY `POLLHUP` races child `exit_group()` and can return `Ok(None)`, silently dropping the SSH failure banner.

### 🟡 Majors

#### `[implementation-readiness-1-8]` No UUID or 128-bit CSPRNG nonce mechanism within file boundary
- **Scope:** Lines `326`, `340` (vs. [Cargo.toml](file:///home/gfidelis/Code/rust/WhattyTheShell!/Cargo.toml#L10-L33))
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** `Cargo.toml` lacks `uuid`, `rand`, `getrandom`, and `serde_json` and is not in Target Files (`319–397`). Specify exact zero-crate helpers (`glib::uuid_string_random()`, `libc::getrandom` / `/dev/urandom`, small JSON helper) or add `Cargo.toml` to Target Files.

#### `[implementation-readiness-1-9]` `ConfigManager` gaps (test path injection, profile reordering, read-error backup, `default_multiplexer_order`)
- **Scope:** Lines `326–331`, `402`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** `ConfigManager` lacks `load_or_default_in(dir: &Path)` for Rust 2024 unit tests (`env::set_var` is `unsafe` and racy), lacks profile reordering (`42`), catches only `toml::from_str` rather than `fs::read` / UTF-8 errors (`unwrap_or_default()`), and omits `default_multiplexer_order()`, `upsert_ssh_profile` in-place rules, and `0600` backup permissions.

#### `[implementation-readiness-1-10]` `build_remote_bootstrap_script` quoting, `screen -ls` parsing, and POSIX menu edge cases are unspecified
- **Scope:** Line `341`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Unspecified quoting across remote `fish`/`tcsh` shells, `tmux new-session` / `screen -S` creation and `whatty-1` collision handling, filtering `(Dead ???)` entries from `screen -ls`, empty/unknown `priority` slices, POSIX-safe storage of dynamic `1..K` names with spaces, and `Enter`/`EOF`/invalid-choice rules.

#### `[implementation-readiness-1-11]` `build_ssh_invocation` `ControlPath` `$XDG_RUNTIME_DIR` non-expansion and validation gaps
- **Scope:** Line `342`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** OpenSSH does not expand `$XDG_RUNTIME_DIR` inside `-o ControlPath=...` unless resolved in Rust with `/tmp/whattytheshell-<uid>` fallback and `0700` dir creation; `host` whitespace/`@`, `port == 0`, empty `identity_file`, and non-empty `DISPLAY` preservation are omitted.

#### `[implementation-readiness-1-12]` Missing `HeaderBar` menu synchronization and Preferences Tab 4 UX/data contracts
- **Scope:** Lines `372–373`, `387`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** `open_settings_dialog` has no callback or `MenuButton::set_create_popup_func` to refresh `HeaderBar` when profiles change; Tab 4 omits `use_control_master` switch, reorder buttons, blank `PasswordEntry` edit semantics, `SshAuthKind` field visibility, validation/error feedback, and async keyring error display.

#### `[implementation-readiness-1-13]` `split::find_terminals` replacement misses `src/settings_dialog.rs:1380`
- **Scope:** Line `374`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Misses the 6th `page.downcast::<TerminalView>()` at [src/settings_dialog.rs:1380](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/settings_dialog.rs#L1378-L1384) (`opacity_scale` `<= 100%` reset).

#### `[implementation-readiness-1-14]` Unspecified `SavedSsh` edge cases in `src/main.rs` (`restore_name_btn`, deleted profiles, `new-tab`, `custom_title` contradiction, `HeaderBar` layout)
- **Scope:** Lines `383–387`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Line `386` forces `custom_title = false` in contradiction with line `247`; `"primary terminal"` is undefined in split trees; `restore_name_btn` (`764`), empty `commit_rename` (`680`), `win.new-tab` (`1289`), and tab-bar double-click (`1621`) still call `current_working_dir()` (`/proc/<ssh_pid>/cwd`); deleted-profile split/duplicate fallback is missing; `HeaderBar` `pack_start`/`pack_end` and empty state are unstated.

#### `[implementation-readiness-1-15]` Unit tests in Verification Plan cannot run as specified and omit critical edge cases
- **Scope:** Lines `402–405`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** `ConfigManager`, `AskpassBroker`, and `TerminalView` unit tests lack test constructors/mock seams away from `$XDG_CONFIG_HOME`, `&ApplicationWindow`, D-Bus, and `wgpu::Device`. Missing unit tests for `user` whitespace/leading-dash rejection, invalid nonce rejection, `--ssh-askpass` client IPC, and `Pty::terminate()` post-reap safety.

### 🟢 Nits

#### `[implementation-readiness-1-16]` Transient `dead_code` warnings in Phase 2 vs. zero-warning build requirement
- **Scope:** Lines `315`, `400`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** Clarify temporary `#[allow(dead_code)]` allowances in Phases 2–3 before Phase 4 removes them to satisfy line `400`.

#### `[implementation-readiness-1-17]` `Pty::terminate` 30 ms poll interval unspecified
- **Scope:** Line `352`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** Specify sleep step (e.g., `2 ms`) and document `Pty::spawn_inner` updated signature.

#### `[implementation-readiness-1-18]` Factoring shared `TerminalView` initialization vs. duplicating `with_shell_and_working_dir`
- **Scope:** Line `362`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** Explicitly extract a shared private `with_pty(..., origin: SessionOrigin)` helper instead of duplicating ~850 lines of `with_shell_and_working_dir`.

#### `[implementation-readiness-1-19]` Test name wording `bypass_multiplexer_probe flag omission`
- **Scope:** Line `343`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** `bypass_multiplexer_probe` omits the remote `sh -c '...'` command argument, not an SSH CLI flag.

---

## Findings — `egm-codebase-details`

### 🔴 Blockers

#### `[codebase-details-1-1]` Non-Existent `--lib` Target in Phase 1–3 Verification Commands
- **Scope:** Lines `314–316` (vs. [Cargo.toml:1-9](file:///home/gfidelis/Code/rust/WhattyTheShell!/Cargo.toml#L1-L9), [src/main.rs:14-25](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/main.rs#L14-L25))
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** `Cargo.toml` defines a binary-only crate (`WhattyTheShell`) with no `[lib]` section and no `src/lib.rs`. Running `cargo test --lib <filter>` fails with `no library targets found`; checkpoints must use `cargo test --bin WhattyTheShell <filter>` or `cargo test <filter>`.

#### `[codebase-details-1-2]` `--ssh-askpass` Intercept Drops `WHATTY_SSH_ASKPASS_SOCKET` Detection & Breaks `argv[1]` Prompt Parsing
- **Scope:** Lines `339`, `381` (cross-checked with `193`, `218`, [src/main.rs:33-37](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/main.rs#L33-L37))
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** Line `218` sets `SSH_ASKPASS=<current_exe()>`. OpenSSH passes `<prompt>` in `argv[1]` without `--ssh-askpass`, so line `381` checking only `args().any(|a| a == "--ssh-askpass")` launches a second GTK4 `Application` (`io.github.whattytheshell`). Conversely, passing `--ssh-askpass` makes `args().nth(1)` (`339`) return `"--ssh-askpass"` instead of the prompt string (`args().skip(1).find(|a| a != "--ssh-askpass")` plus env check needed).

#### `[codebase-details-1-3]` `glib::MainContext::invoke_local` Called From `AskpassBroker` Worker Thread Will Panic at Runtime
- **Scope:** Lines `340`, `382` (cross-checked with `200`, `Cargo.lock:929-988`, [src/settings_dialog.rs:990-1141](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/settings_dialog.rs#L990-L1141))
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** In `glib 0.22.10`, `MainContext::invoke_local` asserts `self.is_owner()` and panics off the main thread. Cross-thread marshaling requires `invoke` with `glib::SendWeakRef<ApplicationWindow>` (since `ApplicationWindow` / `WeakRef` are `!Send`) or `async_channel` + `glib::spawn_future_local` matching `src/settings_dialog.rs:990-1141`.

#### `[codebase-details-1-4]` Promised `Ctrl+Shift+R` Reconnect Banner Has Zero Implementation Specification
- **Scope:** Lines `364`, `380–388` (cross-checked with `47`, `243`, [src/widget.rs:601-885](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/widget.rs#L601-L885), [src/main.rs:1210-1216](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/main.rs#L1210-L1216))
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** Banner line `243` prints `"Press Enter to close or Ctrl+Shift+R to reconnect."`, yet lines `355–388` specify zero signal, action, or pane replacement/respawn wiring for `Ctrl+Shift+R`. Either define the reconnect handler or remove `Ctrl+Shift+R` from the banner.

### 🟡 Majors

#### `[codebase-details-1-5]` Omitted `page.downcast::<TerminalView>()` Site at `src/settings_dialog.rs:1380`
- **Scope:** Line `374` (vs. [src/settings_dialog.rs:1378-1384](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/settings_dialog.rs#L1378-L1384))
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Line `374` replaces lines `547, 636, 950, 1366, 1409`, missing line `1380` (`opacity_scale` `<= 100%` branch resetting `set_saturation_boost(1.0)`), leaving split panes stuck supersaturated.

#### `[codebase-details-1-6]` Missing UUID & 128-Bit Nonce Generation Mechanism Without `Cargo.toml` in Target Scope
- **Scope:** Lines `319–397`, `326`, `340` (vs. [Cargo.toml:10-33](file:///home/gfidelis/Code/rust/WhattyTheShell!/Cargo.toml#L10-L33))
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** `Cargo.toml` lacks `uuid`, `rand`, and `getrandom` and is omitted from Target Files (`319–397`). Plan must specify `gtk4::glib::uuid_string_random()` / `/dev/urandom` explicitly so `Cargo.toml` stays untouched.

#### `[codebase-details-1-7]` `ConfigManager::load_or_default` Untestable Under Rust 2024 Edition Without Path-Scoped Loader
- **Scope:** Lines `328–331`, `402` (vs. [Cargo.toml:4](file:///home/gfidelis/Code/rust/WhattyTheShell!/Cargo.toml#L4), [src/config.rs:163-195](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/config.rs#L163-L195))
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** `ConfigManager::load_or_default()` hardcodes `dirs_next::config_dir()`, and Rust `2024` makes `std::env::set_var` `unsafe`. Tests need `load_or_default_in(dir: &Path)` plus `OpenOptionsExt::mode(0o600)` on `config.toml.tmp` at `O_CREAT`.

#### `[codebase-details-1-8]` Contradictory `do_split` Signature & Missing `AskpassBroker` / Multi-Activate Wiring in `src/main.rs`
- **Scope:** Lines `340`, `382–384` (vs. [src/main.rs:35-37,421-506,806,909,931,1263-1321,1570,1627](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/main.rs#L421-L506))
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Line `383` passes a launch target to `do_split`, conflicting with `384` (inspection of focused pane) and all 4 callers (`909, 931, 1311, 1321`). `type CreateTabFn`, `win.new-tab`, tab-bar double-click, cloning `askpass_broker` (`Clone` on `AskpassBroker` via `Arc`) + `config_manager`, and multi-activate `EADDRINUSE` / `Drop` cleanup on `askpass-<pid>.sock` are omitted.

#### `[codebase-details-1-9]` `custom_title` Collision Between Initial SSH Profile Name and Manual Rename
- **Scope:** Lines `385–386` (cross-checked with `247`, [src/main.rs:547-562,764-788,1461-1476](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/main.rs#L547-L562))
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Line `386` sets `custom_title = false` unconditionally, contradicting `247` (`unless manually renamed`). In `src/main.rs:547-562`, passing `title: Some(...)` marks `view.custom_title`; meanwhile `restore_name_btn` (`764–788`) still calls `current_working_dir()` and overwrites SSH labels unless updated for `SavedSsh`.

#### `[codebase-details-1-10]` Omitted Profile Reordering, `use_control_master` UI, Password-Edit Rules, & D-Bus Prompt Handling
- **Scope:** Lines `328`, `338`, `340`, `372` (cross-checked with `42`, `166`, `187–188`, `202`)
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** `ConfigManager` and Tab 4 omit profile reordering (`42`) and `use_control_master` toggle (`166`); `PasswordEntry` edit-blank behavior and auth-kind switch cleanup + validation UI are undefined; `Collection.CreateItem` and `Item.Delete` prompt paths (`!= "/"`) and modal `"Save updated password to keyring"` wiring (`202`) are omitted.

#### `[codebase-details-1-11]` `HeaderBar` SSH Menu Not Refreshed After Preferences Edits
- **Scope:** Line `387` (vs. `371–374`, [src/settings_dialog.rs:248-253](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/settings_dialog.rs#L248-L253), [src/main.rs:1724-1737](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/main.rs#L1724-L1737))
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** `open_settings_dialog` receives no menu handle or refresh callback, and line `387` omits specifying `MenuButton::set_create_popup_func`, so Preferences edits won't appear in `HeaderBar` dynamically.

#### `[codebase-details-1-12]` PTY EOF / `try_wait` Timing Race, Recycled-PID `kill(-pid)`, & Unfactored `with_ssh_profile`
- **Scope:** Lines `351–352`, `362–365`, `405` (vs. [src/pty.rs:277,312-322](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/pty.rs#L277-L322), [src/widget.rs:402-1256,557](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/widget.rs#L402-L1256))
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Non-blocking `try_wait()` on `POLLHUP` (`500`) can return `Ok(None)`; `Pty::terminate()` (`312–322`) must guard `kill(-pid)` on `last_exit_code.is_none()`; `with_ssh_profile` omits shared `with_pty_and_origin` factoring + headless test constructor (`405`); `key_controller` omits `glib::Propagation::Stop`, `im_context.connect_commit` gating, and `Ctrl+Shift+C`/scrollback preservation. *(Refined detail: emitting `"child-exited"` inside `key_controller` while holding `inner.borrow()` also causes `BorrowMutError` in `tv.terminate()` — see `[implementation-readiness-1-6]`.)*

### 🟢 Nits

#### `[codebase-details-1-13]` `gio::bus_get_sync` Path Notation
- **Scope:** Line `177`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** In `gio 0.22.10`, `bus_get_sync` is a free function `gtk4::gio::bus_get_sync`, not an associated method on `DBusConnection`.

#### `[codebase-details-1-14]` Non-POSIX Shell Quoting & `whatty-1` Name Collision
- **Scope:** Lines `228`, `231`, `341`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** Single quotes inside `tmux list-sessions -F '#{...}'` inside `sh -c '...'` break `fish`/`tcsh` unless the script is single-line without internal single quotes, and fixed `-s whatty-1` collides when multiple sessions are created.

#### `[codebase-details-1-15]` `default_multiplexer_order` Definition & Preferences `Port` / `PasswordEntry` Import Details
- **Scope:** Lines `161`, `326`, `372` (vs. [src/settings_dialog.rs:5-10](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/settings_dialog.rs#L5-L10))
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** Omit explicit `fn default_multiplexer_order() -> Vec<MultiplexerKind>`, `Port (optional)` widget type (`Entry` vs `SpinButton`), and importing `gtk4::PasswordEntry` in `src/settings_dialog.rs`.

#### `[codebase-details-1-16]` Minor Doc Consistency (`Escape`, `src/README.md` Omissions, `spawn_inner` `-l` Flag)
- **Scope:** Lines `243`, `350`, `364`, `394`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** Line `364` closes on `Escape` in conflict with line `243`; line `394` omits `SshAuthKind` and `TerminalView::with_ssh_profile` from `src/README.md`; `Pty::spawn_inner` (`src/pty.rs:112-215`) unconditionally appends `-l` unless conditionalized for non-shell `program` (`"ssh"`).

---

## Findings — `egm-style` (Implementation Plan)

### 🔴 Blockers

#### `[style-plan-1-1]` Invalid `cargo test --lib` Verification Commands (`CALIBRATE`)
- **Scope:** Lines `314–316`
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** `Cargo.toml` defines only binary crate `WhattyTheShell` with no `src/lib.rs`. Replace `cargo test --lib ...` with `cargo test --bin WhattyTheShell <module_filter>`.

#### `[style-plan-1-2]` Broken `--ssh-askpass` Intercept & `argv[1]` Indexing (`CALIBRATE`)
- **Scope:** Lines `339`, `381` (vs. `193`, `218`)
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** Line `381` omits `WHATTY_SSH_ASKPASS_SOCKET`, while line `339` uses `nth(1)` (which returns `"--ssh-askpass"` when the flag is passed). Calibrate both intercept condition and prompt indexing.

#### `[style-plan-1-3]` Omitted `page.downcast::<TerminalView>()` Site at `src/settings_dialog.rs:1380` (`CALIBRATE`)
- **Scope:** Line `374`
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** Include line `1380` (`opacity_scale <= 100%` saturation reset) so split panes are not left supersaturated.

#### `[style-plan-1-4]` Invalid Cross-Thread `invoke_local` Dispatch & Unspecified `AskpassBroker` Contract (`CALIBRATE`)
- **Scope:** Lines `340`, `382` (vs. `256`)
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** `invoke_local` panics off the main thread (`invoke` or `unix_fd_add_local` required). Specify `AskpassBroker::start(&window)` return/error signature, `Drop` cleanup, `0700` dir creation, and `$XDG_RUNTIME_DIR` fallback.

### 🟡 Majors

#### `[style-plan-1-5]` Missing `Ctrl+Shift+R` Reconnect Implementation & Hold-Open Edge Cases (`CALIBRATE`)
- **Scope:** Lines `358`, `364` (vs. `243`)
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Resolve promised `Ctrl+Shift+R` reconnect, `Escape` dismissal, exact red banner wording, `Some(0)` / `None` (`POLLHUP` race) handling, and key/copy behavior on dead PTY.

#### `[style-plan-1-6]` Unimplemented `"testing keyring passwords"` Claim, Missing `use_control_master` Toggle, & `HeaderBar` Wiring (`CALIBRATE`)
- **Scope:** Lines `370`, `372`, `387` (vs. `326`)
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Remove `"testing"` on `370` (or add button), expose `use_control_master` in Tab 4, define blank `PasswordEntry` save behavior, validation feedback, `HeaderBar` placement/empty state, and dynamic refresh mechanism.

#### `[style-plan-1-7]` Unspecified UUID & 128-Bit Nonce Primitives Within `Cargo.toml` File Boundary (`CALIBRATE`)
- **Scope:** Lines `319–397`, `328–331`, `340`, `372`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Specify `glib::uuid_string_random()` / `/dev/urandom` explicitly so `Cargo.toml` remains unmodified.

#### `[style-plan-1-8]` Underspecified `src/ssh.rs` Signatures, Quoting Contract, & Invocation Flags (`CALIBRATE`)
- **Scope:** Lines `338`, `341`, `342` (vs. `232`)
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Provide full Rust signatures on line `338` (`store_secret_async`, `lookup_secret_sync`, `delete_secret_async`), distinguish inner script vs outer `sh -c '...'` on `341` + zero-session menu / empty `priority`, and document all `build_ssh_invocation` flags/env vars on `342`.

#### `[style-plan-1-9]` Contradictory `custom_title` Restore Behavior & Underspecified `LaunchTarget` / Title Wiring (`CALIBRATE`)
- **Scope:** Lines `383–386` (vs. `247`)
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Fix `custom_title = false` vs `247`, name `LaunchTarget` enum cleanly, align `CreateTabFn`/`create_terminal_view`, clarify initial title assignment vs `set_custom_title`, and specify duplicated SSH tab title.

#### `[style-plan-1-10]` Missing Unit-Test Seams & Verification Plan Coverage Mismatches (`CALIBRATE`)
- **Scope:** Lines `328–331`, `340`, `365`, `402–405`, `408–410`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Add `ConfigManager` path-scoped loader (`load_or_default_in`) and `AskpassBroker`/GTK headless test seams, align `bypass_multiplexer_probe` (`343` vs `403`) + negative tests, and clarify E2E checks + complete line `409`.

#### `[style-plan-1-11]` Missing Dead-Child Guard & Poll Interval in `Pty::terminate` (`CALIBRATE`)
- **Scope:** Line `352`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Specify `try_wait()` poll interval and guard `kill(-pid, ...)` when `last_exit_code.is_some()`.

#### `[style-plan-1-12]` Wildcard `[ssh.rs:*]` & Omitted Contract Symbols in `src/README.md` (`CALIBRATE`)
- **Scope:** Line `394`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Replace `[ssh.rs:*]` with explicit exported symbols plus `SshAuthKind`, `win.launch-ssh-profile`, and `TerminalView::with_ssh_profile` / getters.

### 🟢 Nits

#### `[style-plan-1-13]` Internal Workflow Jargon (`"EGM contracts"`, `"EGM package contracts"`)
- **Scope:** Lines `317`, `392`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** Replace with `"architecture contracts"` / `"package documentation"`.

#### `[style-plan-1-14]` Phase Scope Drift, Inconsistent Symbol Backticking, & Mixed Tooltip Language
- **Scope:** Lines `314–317`, `379`, `383`, `387`, `392`, `409`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** Align Phase `2` (`build_remote_bootstrap_script`) vs line `26` (`Phase 3`), standardize `HeaderBar`/`header_bar` formatting, unify Markdown link formatting, and align Portuguese `"Sessões SSH Salvas"` (`387`) with English UI copy.

#### `[style-plan-1-15]` Minor Phrasing Precision (`OpenOptionsExt::mode`, `exec`, `(e.g., 255)`, `CONTEXT.md` Target)
- **Scope:** Lines `330`, `341`, `348`, `396`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** Specify `OpenOptionsExt::mode(0o600)` on `O_CREAT`, replace `` `exec`s `` with ``replaces the shell via `exec```, calibrate `(255)` to `(e.g., 255)`, and name target `CONTEXT.md` section + `_Avoid_` aliases.
