# Stage 8: Intermediate Zoom Out Report (Post-Stage 7 Cohesion & Simplification Audit)

- **Target Document:** [docs/designs/ssh_sessions_and_multiplexer_bootstrap.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/docs/designs/ssh_sessions_and_multiplexer_bootstrap.md) (`1–416`)
- **Standard:** [zoom-out-standard.md](file:///home/gfidelis/.gemini/config/plugins/egm/resources/document_standards/zoom-out-standard.md)
- **Prior Stage Baselines:** [zoom_out_report.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/zoom_out_report.md) (Stage 5), [comprehension_report.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/comprehension_report.md) (Stage 6), [critic_report.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/critic_report.md) (Stage 7)
- **Round:** `1`

## Reviewer Execution Status

| Reviewer | Role / Focus | Status | Findings (🔴 / 🟡 / 🟢) |
| :--- | :--- | :--- | :--- |
| `egm-zoom-out` | Post-Stage 7 Architectural Cohesion, Bloat Prevention & Simplification | `completed` | 5 🔴 / 4 🟡 / 1 🟢 |

---

## Executive Architectural Verdict: Trim vs. Patch Matrix

Naively patching every Stage 7 Critic finding (`critic_report.md`) onto the current design would **increase architectural bloat and coupling**—adding GTK4 `Overlay` widgets and `Pty` hot-swapping for `Ctrl+Shift+R`, cross-thread `SendWeakRef` + oneshot reply plumbing for `AskpassBroker`, inline keyring mutation during SSH handshakes, and a bespoke `ControlMaster` override layer.

Instead, **trimming 4 speculative surfaces and unifying 2 existing abstractions** eliminates entire classes of Stage 5, 6, and 7 blockers at the root while preserving 100% of the core user value:

| Stage 7 Critic Topic | Naive Patch Risk (Bloat) | Recommended Stage 8 Verdict |
| :--- | :--- | :--- |
| **`Ctrl+Shift+R` & clickable `Close/Reconnect`** (`[design-1-4]`) | High bloat: requires `gtk4::Overlay` around `TerminalView` (`src/widget.rs`), coupling `TerminalView` to `ConfigManager`/`AskpassBroker`, or `Paned` replacement surgery in `src/main.rs`. | **CUT** `Ctrl+Shift+R` and clickable `Close/Reconnect` from v1. Keep hold-open error banner + `Enter` to close; fix the `POLLHUP`/`waitpid` race and `Pty::terminate()` PID-reuse guard. |
| **`AskpassBroker` threading, `AgentOrDefault`, & inline keyring save** (`[design-1-2]`) | High bloat: adds `SendWeakRef` + oneshot reply channel + multi-prompt nonce state machine + inline `CreateItem` write-back during SSH auth. | **SIMPLIFY**: Run non-blocking `UnixListener` directly on GTK main loop (`unix_fd_add_local`), attach `SSH_ASKPASS`/nonces **only** when a keyring-backed `auth_kind` has a stored secret (never for `AgentOrDefault`), and cut `"Save updated password to keyring"` from the modal. |
| **`--ssh-askpass` self-reexec vs. `SSH_ASKPASS` contract** (`[design-1-1]`, `[design-1-7]`) | High risk: dynamic `libgtk-4`/`libvulkan` load makes `< 5 ms` / `< 8 MB` RSS impossible for self-reexec; `WHATTY_SSH_ASKPASS_SOCKET` leaks to nested `whatty` windows. | **SIMPLIFY / DECIDE CLEANLY**: Either extract a tiny `std`-only `[[bin]]` (`whatty-askpass`) or dispatch solely on `WHATTY_SSH_ASKPASS_SOCKET` + `WHATTY_SSH_ASKPASS_NONCE` (dropping `--ssh-askpass` mismatch, scrubbing `WHATTY_SSH_ASKPASS_*` in `Pty::spawn_inner`, and calibrating RSS/latency claims). |
| **Remote `sh -c '...'` quoting & menu quirks** (`[design-1-3]`) | Medium risk if solved with `base64 -d` (not portable to OpenBSD/minimal BSD targets in line `11`). | **SIMPLIFY**: Use single-line POSIX `sh -c '...'` with zero `'`, zero `\`, and zero `\n`; let `tmux`/`screen` pick default session names when unnamed (eliminating `whatty-1` collision); drop `(or 0)`; filter `screen -ls` `(Dead` entries. |
| **Forced `ControlMaster=auto` & profile reordering** (`[design-1-6]`) | Medium bloat: overrides user `~/.ssh/config`, hits `108`-byte `sun_path` limit, lacks Preferences UI toggle and reorder API/buttons. | **TRIM OR CONTAIN**: Prefer deferring `ControlMaster` to `~/.ssh/config` (eliminating `use_control_master` + `ControlPath` risks), OR share one `ssh_runtime_dir()` with `< 88`-byte guard + Tab 4 toggle. Cut profile reordering from Goal `42` unless UI/API are explicitly added. |
| **Dedicated `## Risks & Mitigations` & `The Alternatives Rule`** (`[design-1-5]`, `[structural-1-1]`, `[structural-1-2]`) | Zero bloat if paired with deduplication! Currently `Heilmeier Catechism Summary` repeats `The Problem`/`Goals`/`NFRs`. | **CONSOLIDATE**: Replace duplicated Heilmeier bullets (`15–29`) with a short Overview, add a concise `## Risks & Mitigations` section, remove `"Rejected by author:"` / `"(Option 1)"` artifacts, and trim circular policy restatements (Alternatives D, F, G). |

---

## Detailed Findings

### 🔴 Blockers

#### `[zoom-out-s8-1]` `AskpassBroker` Cross-Thread Plumbing, `AgentOrDefault` Coupling, and Inline Keyring Mutation Should Be Simplified Rather Than Patched
- **Scope:** [docs/designs/ssh_sessions_and_multiplexer_bootstrap.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/docs/designs/ssh_sessions_and_multiplexer_bootstrap.md#L177-L204), `L260`, `L338-L340`, `L382`
- **Severity:** `🔴 Blocker`
- **Architectural Verdict:** **Simplify architecture (`zoom-out-standard.md` §2 Minimalist Data Flow & §4 Codebase Reuse).** Do **not** patch Stage 7 `[design-1-2]` by building a `SendWeakRef` + reply-channel + multi-attempt state machine + inline keyring update pipeline.
- **Rationale:**
  1. `AskpassBroker` currently assumes a background listener thread that calls `glib::MainContext::default().invoke_local(...)` (`200`, `340`), which panics off the main thread. Yet [src/widget.rs](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/widget.rs#L17-L61) already establishes `unix_fd_add_local` on the GTK main loop. Registering the non-blocking `UnixListener` directly via `unix_fd_add_local` (and spawning `std::thread::spawn` + `async_channel` + `glib::spawn_future_local` only for the D-Bus `SecretServiceClient` lookup, matching [src/settings_dialog.rs](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/settings_dialog.rs#L990-L1141) rather than [src/theme_marketplace.rs](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/theme_marketplace.rs)) eliminates cross-thread GTK marshaling (`invoke_local` / `SendWeakRef` / reply channels) completely.
  2. Setting `SSH_ASKPASS_REQUIRE=force` and registering broker nonces for `SshAuthKind::AgentOrDefault` (`141–145`, `182`) creates the undefined `secret_kind` ambiguity flagged in `[design-1-2]`. When `auth_kind == AgentOrDefault` (or when no secret is stored in keyring for that profile), omitting `SSH_ASKPASS` / `SSH_ASKPASS_REQUIRE` lets OpenSSH interact natively with the live PTY (`/dev/tty`), removing that entire branch from the broker.
  3. Offering `"Save updated password to keyring"` inside the transient Askpass modal (`202`) triggers D-Bus `CreateItem` writes and unlock prompts mid-handshake. Credential editing already belongs in Preferences (`SSH Sessions` tab, `372`).
  4. Keeping `gtk4::gio::DBusConnection` (via `gio::bus_get_sync`, removing the conflated `for_address_sync` mention on `177`) avoids adding `zbus`/`tokio`/`libsecret-tools` dependencies, provided the spec places any `org.freedesktop.Secret.Prompt.Completed` signal wait inside a per-worker `glib::MainContext`/`glib::MainLoop` with a bounded timeout and defines graceful fallback when `org.freedesktop.secrets` is unavailable.
- **Concrete Recommendation:** Move `AskpassBroker`'s `UnixListener` accept loop onto the GTK main loop (`unix_fd_add_local`), keeping only `SecretServiceClient` D-Bus I/O on a worker thread with a local `glib::MainContext`/`MainLoop`; omit `SSH_ASKPASS` / `SSH_ASKPASS_REQUIRE=force` / broker registration when `auth_kind == AgentOrDefault`; cut `"Save updated password to keyring"` (`202`) from the transient modal dialog.

#### `[zoom-out-s8-2]` Cut `Ctrl+Shift+R` Reconnect and Clickable `Close/Reconnect` UI (Eliminates Bloat & 6+ Cross-Stage Findings)
- **Scope:** [docs/designs/ssh_sessions_and_multiplexer_bootstrap.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/docs/designs/ssh_sessions_and_multiplexer_bootstrap.md#L24), `L47`, `L94-L98`, `L242-L243`, `L351-L352`, `L364`
- **Severity:** `🔴 Blocker`
- **Architectural Verdict:** **Cut from v1 scope (`zoom-out-standard.md` §1 Root Problem Focus & §2 YAGNI).** Addressing Stage 7 `[design-1-4]` by implementing `Ctrl+Shift+R` and clickable Close/Reconnect controls would introduce heavy structural bloat (`gtk4::Overlay` wrappers or `Paned` widget-swap surgery).
- **Rationale:**
  1. `TerminalView` is a pure `gtk4::Widget` subclass rendering DMA-BUF textures with no child widgets or `gtk4::Overlay` ([src/widget.rs](file:///home/gfidelis/Code/rust/WhattyTheShell!/src/widget.rs#L105-L108)), and holds no reference to `ConfigManager` or `AskpassBroker`. Re-launching is already two clicks away in the `HeaderBar` quick-launch menu, while keeping the pane open until `Enter` solves SSH error visibility.
  2. Cutting `Ctrl+Shift+R` does **not** remove the need to fix the underlying PTY teardown bugs:
     - Add a short bounded reap poll on `SavedSsh` `POLLHUP`/EOF before checking exit status so fast exits don't return `None` from `try_wait()`.
     - Guard `Pty::terminate()` with `if self.last_exit_code.is_none()` so already-reaped PIDs are never signaled.
     - Drop any `RefCell` borrow on `TerminalViewInner` before calling `emit_by_name("child-exited")`, and unify dismissal on `Return`/`KP_Enter` (removing conflicting `Escape` on `364`) returning `glib::Propagation::Stop`.
- **Concrete Recommendation:** Remove `"clicks Close/Reconnect"` (`24`) and `"Ctrl+Shift+R to reconnect"` (`47`, `243`) from the spec and banner; keep `Enter`-to-close hold-open behavior with the `POLLHUP` reap guard, `RefCell` borrow-drop before `"child-exited"`, and `last_exit_code.is_some()` guard in `Pty::terminate()`.

#### `[zoom-out-s8-3]` Resolve the Broken `SSH_ASKPASS` Dispatch Contract & Uncalibrated `< 5 ms` / `< 8 MB` RSS Claim
- **Scope:** [docs/designs/ssh_sessions_and_multiplexer_bootstrap.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/docs/designs/ssh_sessions_and_multiplexer_bootstrap.md#L19), `L25`, `L43`, `L72`, `L77`, `L109`, `L193-L194`, `L218`, `L259`, `L339`, `L381`
- **Severity:** `🔴 Blocker`
- **Architectural Verdict:** **Fix structural contradiction (`zoom-out-standard.md` §2 Simplicity & §4 Platform Standards).**
- **Rationale:** OpenSSH executes `$SSH_ASKPASS` directly (`<current_exe()>` on `218`) with `<prompt>` as `argv[1]` and never passes `--ssh-askpass`. Thus `src/main.rs` checking only `--ssh-askpass` (`381`) never triggers under OpenSSH, while running `whatty --ssh-askpass "<prompt>"` makes `args().nth(1)` (`339`) equal `"--ssh-askpass"`. Checking `WHATTY_SSH_ASKPASS_SOCKET` in `main()` (`193`) also risks hijacking normal `whatty` launches unless scrubbed on PTY spawns. Meanwhile, dynamic linking of `gtk4` and `wgpu`/`ash` in `Cargo.toml` loads shared libraries via `ld.so` before `fn main()`, invalidating the `< 5 ms` / `RSS < 8 MB` claim on `259`.
- **Concrete Recommendation:** Either add a tiny `std`-only Cargo `[[bin]]` helper (`whatty-askpass` using `serde_json` already in `Cargo.toml:15`), or drop `--ssh-askpass` flag parsing, dispatch exclusively on `WHATTY_SSH_ASKPASS_SOCKET` + `WHATTY_SSH_ASKPASS_NONCE`, scrub `WHATTY_SSH_ASKPASS_*` in `Pty::spawn_inner`, and calibrate line `259` to `"avoids GTK/wgpu initialization"`.

#### `[zoom-out-s8-4]` Portable `sh -c '...'` Quoting Across `fish`/`tcsh`/BSD & Simplified Remote Selector
- **Scope:** [docs/designs/ssh_sessions_and_multiplexer_bootstrap.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/docs/designs/ssh_sessions_and_multiplexer_bootstrap.md#L11), `L23`, `L72`, `L216`, `L223-L232`, `L341`
- **Severity:** `🔴 Blocker`
- **Architectural Verdict:** **Fix quoting structurally and trim selector quirks (`zoom-out-standard.md` §2 Simplicity & §4 Platform Standards).** Beware of over-complex `base64 -d` workarounds (`OpenBSD` lacks `base64` in base; older BSD uses `-D`).
- **Rationale:** Constructing a **single-line** POSIX `sh -c '...'` payload containing **zero embedded single quotes `'`, zero backslashes `\`, and zero newlines `\n`** parses identically under `sh`, `bash`, `zsh`, `dash`, `fish`, and `tcsh` with zero decoder dependencies. Fix `screen -ls` filtering to exclude `(Dead` sockets, quote exact selection targets (`tmux attach-session -t -- "$sel"` / `screen -d -r "$sel"`), define `Enter`/invalid/`Ctrl+D` behavior, and trim `whatty-1` collision naming + `[E] (or 0)` dual hotkeys.
- **Concrete Recommendation:** Specify the single-line quote-free/backslash-free/newline-free `sh -c '...'` invariant, add `screen -ls` `(Dead` filtering and safe POSIX indexing, and let `tmux new-session`/`screen` choose native default session names when unnamed.

#### `[zoom-out-s8-5]` Broken `cargo test --lib` Verification Gates and Phase 2 vs. Phase 4 `mod ssh;` Build Ordering
- **Scope:** [docs/designs/ssh_sessions_and_multiplexer_bootstrap.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/docs/designs/ssh_sessions_and_multiplexer_bootstrap.md#L314-L316), `L343`, `L381`, `L402-L405`
- **Severity:** `🔴 Blocker`
- **Architectural Verdict:** **Fix build/phase ordering (`zoom-out-standard.md` §4 Pattern Consistency).**
- **Rationale:** `Cargo.toml` defines only binary target `WhattyTheShell` with no `src/lib.rs`, so `cargo test --lib ...` (`314–316`) fails immediately, and Phase 2 (`315`) tests `ssh` while `mod ssh;` is deferred to Phase 4 (`381`). Also line `403` omits the `bypass_multiplexer_probe` unit test promised on line `343`.
- **Concrete Recommendation:** Move `mod ssh;` registration to Phase 2, update phase test commands to `cargo test --bin WhattyTheShell config::tests` (etc.), and align lines `343` and `403`.

---

### 🟡 Majors

#### `[zoom-out-s8-6]` Addressing Stage 7 `Risks & Mitigations` and `The Alternatives Rule` Reduces Net Bloat When Paired with Heilmeier Deduplication
- **Scope:** [docs/designs/ssh_sessions_and_multiplexer_bootstrap.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/docs/designs/ssh_sessions_and_multiplexer_bootstrap.md#L15-L58), `L249`, `L264-L307`
- **Severity:** `🟡 Major`
- **Architectural Verdict:** **Consolidate & trim prose (`zoom-out-standard.md` §0 Context Sufficiency & §2 Simplicity).** Satisfies Stage 7 `[design-1-5]`, `[structural-1-1]`, `[structural-1-2]`, and `[style-spec-1-1]`/`[style-spec-1-8]` while shortening the spec.
- **Concrete Recommendation:** Replace duplicated `Heilmeier Catechism Summary` (`15–29`) with a 1-paragraph executive summary (removing `"Mid-term/Final exam"`), add a concise `## Risks & Mitigations` section (`108`-byte `sun_path`, `SIGKILL` stale sockets, unavailable/locked `org.freedesktop.secrets`), remove `"Rejected by author:"` and `"(Option 1)"` in `## Alternatives Considered`, and fold circular non-goal restatements (`D`/`F`/`G`) cleanly into `## Non-Goals` (or state concrete technical trade-offs).

#### `[zoom-out-s8-7]` Centralize `pre_ssh_local_dir` and Split-Pane Ownership in `TerminalView` Instead of Patching 6+ Call Sites in `src/main.rs`
- **Scope:** [docs/designs/ssh_sessions_and_multiplexer_bootstrap.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/docs/designs/ssh_sessions_and_multiplexer_bootstrap.md#L244-L247), `L360-L364`, `L374`, `L383-L386`
- **Severity:** `🟡 Major`
- **Architectural Verdict:** **Structural fix via deep module placement (`zoom-out-standard.md` §3 Right Placement & §4 Codebase Reuse).**
- **Concrete Recommendation:** Have `TerminalView::current_working_dir()` return `pre_ssh_local_dir` for `SavedSsh` (fixing `win.new-tab`, tab-bar double-click, `copy_path_btn`, and `restore_name_btn` in one place), separate SSH initial tab labels from `custom_title`, define `"primary terminal"` as `split::find_terminals(&page).first()`, specify deleted-profile split fallback to a local shell, share a private `TerminalView::with_pty_and_origin(...)` helper in `src/widget.rs`, and include line `1380` in `src/settings_dialog.rs`.

#### `[zoom-out-s8-8]` Trim or Properly Bound Forced `ControlMaster=auto` Override and Unimplemented Profile Reordering
- **Scope:** [docs/designs/ssh_sessions_and_multiplexer_bootstrap.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/docs/designs/ssh_sessions_and_multiplexer_bootstrap.md#L42), `L166`, `L196`, `L211`, `L213`, `L328`, `L372`, `L387`
- **Severity:** `🟡 Major`
- **Architectural Verdict:** **Trim speculative controls (`zoom-out-standard.md` §2 YAGNI & §4 Leverage Existing).**
- **Concrete Recommendation:** Defer `ControlMaster` to `~/.ssh/config` (removing `use_control_master` and unexpanded `$XDG_RUNTIME_DIR` `ControlPath` bugs cleanly), or share `ssh_runtime_dir()` with an `88`-byte `sun_path` cap and expose the Tab 4 switch; cut profile reordering from Goal `42` (or add `move_ssh_profile` + Up/Down buttons); use `MenuButton::set_create_popup_func` in `src/main.rs` so `HeaderBar` stays synced with `ConfigManager`; and clarify conditional `-i` + `host`/`port == 0` validation.

#### `[zoom-out-s8-9]` Clarify `Cargo.toml` Zero-Crate Primitives (`serde_json` Already Present), Config Recovery Contradiction, and Test Seams
- **Scope:** [docs/designs/ssh_sessions_and_multiplexer_bootstrap.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/docs/designs/ssh_sessions_and_multiplexer_bootstrap.md#L66), `L172`, `L177`, `L194`, `L197`, `L262`, `L329-L331`, `L338-L343`, `L402-L405`
- **Severity:** `🟡 Major`
- **Architectural Verdict:** **Reuse existing crates & add minimal test seams (`zoom-out-standard.md` §3 Right Placement & §4 Codebase Reuse).**
- **Concrete Recommendation:** Reuse `serde_json` already in `Cargo.toml:15`, `gtk4::glib::uuid_string_random()`, and `/dev/urandom` / `libc::getrandom` (keeping `Cargo.toml` untouched); resolve the corrupt-config auto-save contradiction (`172` vs. `main.rs:1485`) + `OpenOptionsExt::mode(0o600)` + `v0.4.3` downgrade overwrite caveat (`262`); add `ConfigManager::load_or_default_in(&Path)` and pure headless test seams for `AskpassBroker`.

---

### 🟢 Nits

#### `[zoom-out-s8-10]` Diagram Drift, Misattributed Threading Reference, and Mixed UI Language
- **Scope:** [docs/designs/ssh_sessions_and_multiplexer_bootstrap.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/docs/designs/ssh_sessions_and_multiplexer_bootstrap.md#L37), `L58`, `L103`, `L107`, `L161`, `L177`, `L249`, `L259`, `L387`
- **Severity:** `🟢 Nit`
- **Concrete Recommendation:** Fix Mermaid socket path (`$XDG_RUNTIME_DIR/whattytheshell/askpass-{pid}.sock`) and subgraph module labels; cite `src/settings_dialog.rs` instead of `src/theme_marketplace.rs` on line `177`; state `default_multiplexer_order()` explicitly; remove `"with ~"` over-generalization (`37`) and orphan `---` (`249`); normalize `"Sessões SSH Salvas"` (`387`) vs. English Preferences copy (`372`).
