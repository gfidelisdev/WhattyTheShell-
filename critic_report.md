# Stage 7: Feature Spec Critic Report (`FeatureRange`: `1..309`)

- **Target Document:** [docs/designs/ssh_sessions_and_multiplexer_bootstrap.md](file:///home/gfidelis/Code/rust/WhattyTheShell!/docs/designs/ssh_sessions_and_multiplexer_bootstrap.md#L1-L309)
- **Standard:** [feature-spec-standard.md](file:///home/gfidelis/.gemini/config/plugins/egm/resources/document_standards/feature-spec-standard.md)
- **Round:** `1`

## Reviewer Execution Status

| Reviewer | Role / Focus | Status | Findings (🔴 / 🟡 / 🟢) |
| :--- | :--- | :--- | :--- |
| `egm-design` | Architecture, Security, Concurrency, Edge Cases & Unmitigated Risks | `completed` | 5 🔴 / 7 🟡 / 3 🟢 |
| `egm-structural` | Heading Hierarchy, Template Compliance, Symmetry & Cross-References | `completed` | 2 🔴 / 1 🟡 / 4 🟢 |
| `egm-style` | Clarity, Precision, Brevity, Framing & Calibration | `completed` | 1 🔴 / 8 🟡 / 6 🟢 |

---

## Findings — `egm-design`

### 🔴 Blockers

#### `[design-1-1]` `AskpassBroker` Threading Panic (`invoke_local`), Unbounded Socket Concurrency, & `/tmp` Security
- **Scope:** Lines `103`, `195–204`
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** Line `200` marshals GTK4 prompts via `glib::MainContext::default().invoke_local(...)` from an off-main-thread socket listener (which panics at runtime; cross-thread `invoke` + reply channel or main-loop `unix_fd_add_local` is required). Single-threaded sequential accept blocks concurrent SSH tabs and hung clients. Moreover, `/tmp/whattytheshell-<uid>` fallback (`196`) checks only `0700` without verifying `uid == getuid()`, non-symlink (`symlink_metadata`), or socket `SO_PEERCRED` (`uid == getuid()`).

#### `[design-1-2]` Prompt Classification & `password_attempts` Logic Leaks/Misroutes Credentials
- **Scope:** Lines `182`, `197–203`
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** Treating all empty-hint `*password*`/`*passphrase*` prompts identically on `password_attempts == 0` causes a `PasswordKeyring` profile with a passphrase-protected local key or `~/.ssh/config` `ProxyJump` bastion to feed the target server password to the local key/bastion. `SSH_ASKPASS_PROMPT=none` informational prompts render a masked `PasswordEntry`; `AgentOrDefault` has no `secret_kind` mapping on line `182`; `"Save updated password to keyring"` (`202`) lacks `AgentOrDefault`/`IdentityFile` scoping; and `confirm` / `[fingerprint]` JSON return values and nonce invalidation/unknown-nonce error responses are undefined.

#### `[design-1-3]` Non-POSIX Remote Shell Quoting Breakage & Remote Bootstrap Selector Bugs
- **Scope:** Lines `23`, `72`, `216`, `223–233`
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** Multiline `sh -c '...'` with embedded single quotes (`tmux list-sessions -F '#{...}'`) fails under remote `fish` (interprets backslash inside `'...'`) and `tcsh` (`Unmatched '.`). Additionally, `[N]` fixed default `whatty-1` collides on splits/second tabs; `tmux attach-session -t <name>` does prefix matching without `=` (`-t "=$name"`); `|` delimiter breaks on session names containing `|`; `screen -ls` includes `(Dead ???)`/`(Removed)` entries and uses `-d -r` unconditionally; `[E] (or 0)` / invalid input / `Enter` / `Ctrl+D` rules are missing.

#### `[design-1-4]` Pre-GTK `--ssh-askpass` Entrypoint Mismatch & Child Env Leakage
- **Scope:** Lines `43`, `72`, `77`, `109`, `192–194`, `218–222`, `259`
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** Line `218` sets `SSH_ASKPASS=<std::env::current_exe()>` without `--ssh-askpass`, whereas line `194` reads `"prompt": "<argv[1]>"` (`argv[1]` equals `"--ssh-askpass"` when `--ssh-askpass` is passed explicitly). Furthermore, `WHATTY_SSH_ASKPASS_SOCKET` and `WHATTY_SSH_ASKPASS_NONCE` propagate into `ssh` child processes (`LocalCommand`, `ProxyCommand`, nested local shells), risking `main()` hijack on `whatty <arg>`.

#### `[design-1-5]` Promised `Ctrl+Shift+R` Reconnect & `Close/Reconnect` UI Has No Supporting Architecture
- **Scope:** Lines `24`, `47`, `94–98`, `240–243`
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** Lines `24`, `47`, and `243` promise clickable `Close/Reconnect` and `Ctrl+Shift+R` reconnect, yet `TerminalView` (`src/widget.rs`) holds neither `ConfigManager` nor `AskpassBroker`, and lines `1..309` specify no reconnect signal/action, clickable overlay controls, or IME/key write gating (`im_context.connect_commit`) during `awaiting_error_dismiss`.

### 🟡 Majors

#### `[design-1-6]` `SecretServiceClient` D-Bus Protocol Gaps (Locked Collections, Signal Race, Threading, Stale Secrets)
- **Scope:** Lines `175–188`, `260`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** `Collection.CreateItem` (`187`) and `Item.Delete` (`188`) can return non-`"/"` prompt paths when locked; `signal_subscribe` on `std::thread::spawn` (`177`) hangs without a thread-default `glib::MainContext`/`MainLoop` + pre-call subscription + timeout; line `177` conflates `for_address_sync` with `gio::bus_get_sync` and misattributes thread spawning to `src/theme_marketplace.rs` (actually in `src/settings_dialog.rs`); `Session.Close()`, auth-mode change secret cleanup, and missing `org.freedesktop.secrets` fallback are unstated.

#### `[design-1-7]` `Pty::try_wait_exit_code()` Races with PTY `POLLHUP`/`EIO` on Fast Exit
- **Scope:** Lines `238`, `242–243`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Calling a single non-blocking `try_wait()` immediately on PTY master `POLLHUP`/EOF races child teardown (`exit_files()` precedes zombie visibility), returning `None` and dropping failed SSH error banners. Once reaped, `Pty::terminate()` must also guard `kill(-pid, ...)` on `last_exit_code.is_some()`.

#### `[design-1-8]` `ControlMaster=auto` Socket Path Fallback, Dir Creation, `sun_path` Length, & `RemoteCommand` Conflict
- **Scope:** Lines `166`, `196`, `211`, `216`, `281`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** OpenSSH does not expand `$XDG_RUNTIME_DIR` inside `-o ControlPath=...` (`211`), omits line `196`'s `/tmp` fallback and `0700` dir creation, approaches Linux's `108`-byte `sun_path` limit (plus OpenSSH's temp suffix), conflicts with `~/.ssh/config` `RemoteCommand` when appending `sh -c '...'`, and overrides `~/.ssh/config` `ControlMaster` directives despite Alternative C (`281`).

#### `[design-1-9]` Multi-Window `GApplication` Activation Breaks Single-PID `AskpassBroker` Socket
- **Scope:** Lines `103`, `196`, `200`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** `gtk4::Application` (`io.github.whattytheshell`) routes a second launch to the existing PID (`app.connect_activate`), so per-window binding of `askpass-<pid>.sock` collides (`EADDRINUSE`) unless scoped as a process-wide singleton tracking originating windows per nonce.

#### `[design-1-10]` `SessionOrigin` Lifecycle Inconsistencies (`custom_title`, `MAX_PANES_PER_TAB = 4`, Deleted Profiles, `merge_tabs`)
- **Scope:** Lines `244–247`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Setting initial SSH tab labels via `title: Some(...)` marks `view.custom_title`, breaking `"Restaurar Nome Automático"` (`main.rs:764`), `"Duplicar Aba"` (`" (cópia)"`), and line `247` (`unless manually renamed`). `do_split` (`main.rs:462`) spawns `TerminalView` before checking `MAX_PANES_PER_TAB = 4`, spawning a 5th `ssh` session only to discard it. Deleted-profile split/duplicate fallback and `merge_tabs` (`Local` + `SavedSsh`) title syncing are unspecified.

#### `[design-1-11]` `ConfigManager` Corrupt-Backup Overwrite Contradiction & Read-Error Masking
- **Scope:** Lines `171–173`, `262`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** `window.connect_close_request` / `close_tab_page` auto-saves `open_tabs` (`config.rs:250`), contradicting line `172` (*"skips overwriting config.toml until the next explicit user save"*). Existing `fs::read_to_string(...).unwrap_or_default()` silently masks I/O errors, `config.toml.tmp` needs `OpenOptionsExt::mode(0o600)` at creation, `.corrupt.<unix_ts>` needs `0600` + collision suffix, and `v0.4.3` exit saves drop `ssh_profiles` (`262`).

#### `[design-1-12]` Standard Compliance Gap: Missing `Risks & Limits`, Environment Constraints, Validation, & UI Scalability
- **Scope:** Lines `21–24`, `42`, `208`, `213`, `251–262`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** No dedicated `## Risks & Mitigations` / `## Risks & Limits` section covers missing `ssh` (`ENOENT`), OpenSSH `< 8.4` (`SSH_ASKPASS_REQUIRE=force` unsupported), `sun_path` `108`-byte limit, or large `HeaderBar` profile menu overflow. Validation (`208`) omits whitespace/NUL/`@` in `host` (when `user` is `Some`), `port == 0`, conditional `-i` vs `auth_kind` (`213`), and profile reordering controls (`42`).

### 🟢 Nits

#### `[design-1-13]` Unverified Performance & Memory Claims (`< 5 ms`, `< 8 MB` RSS) for `--ssh-askpass`
- **Scope:** Lines `25`, `258–259`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** `Cargo.toml` links `gtk4` (`libgtk-4.so.1`) and graphics libraries dynamically via `ld-linux` before `fn main()`, making `< 8 MB` RSS and `< 5 ms` startup unverified (and line `259` lacks a conjunction before `RSS < 8 MB`; line `25` has contradictory `"1 SSH connection per terminal pane (shared via ControlMaster)"`).

#### `[design-1-14]` Minor Gaps in `Alternatives Considered` Phrasing and D-Bus Crate Evaluation
- **Scope:** Lines `177`, `264–307`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** Line `177` rejects `tokio`/`zbus`/`libsecret-1` without listing that trade-off in `Alternatives Considered`, all 7 entries begin with unlinked `"Rejected by author:"`, Alternatives D/F/G restate policies instead of technical trade-offs, and Alternative A references dangling `"(Option 1)"` (`270`).

#### `[design-1-15]` Remote Bootstrap `PATH` Coverage and Menu Input Semantics (`[1..K]`, `default_multiplexer_order`)
- **Scope:** Lines `72`, `161`, `224`, `230–232`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** Clarify full-line `read choice` for multi-digit `K >= 10`, `/opt/homebrew/bin` in `PATH` for macOS servers, `[E]` vs `[E] (or 0)` inconsistency, and explicit `default_multiplexer_order()` return value (`161`).

---

## Findings — `egm-structural`

### 🔴 Blockers

#### `[structural-1-1]` Missing Dedicated `Risks & Limits` / `Risks & Mitigations` Section
- **Scope:** Lines `1–309` overall (specifically lines `21–24` and `251–262`)
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** `feature-spec-standard.md` mandates dedicated `Risks & Limits` / `Risks & Mitigations` coverage (constraints, failure modes, security implications, scalability limits, mitigations). `FeatureRange` has only three summary sub-bullets in `Heilmeier Catechism Summary` (`21–24`), omitting `sun_path` limits, unavailable/locked Secret Service daemons, stale sockets after `SIGKILL`, and broker timeout/concurrency limits.

#### `[structural-1-2]` `Alternatives Considered` Violates `The Alternatives Rule`
- **Scope:** Lines `264–307` (`270`, `276`, `282`, `288`, `294`, `300`, `306`)
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** Every `* **Reason for Rejection**` bullet begins with `"Rejected by author:"` without linking an issue, discussion, or PR (`feature-spec-standard.md#L19-L21`); Alternatives D, F, and G restate chosen policy instead of concrete technical/scope trade-offs; and Alternative A (`270`) contains a dangling `"(Option 1)"` reference carried over from `concepts_brief.md`.

### 🟡 Majors

#### `[structural-1-3]` Architecture Mermaid Diagram Contradicts Section 4 & Security NFRs on Socket Path (Plus Unescaped `<pid>` Tag and Subgraph Scope Label)
- **Scope:** Lines `103`, `107` (cross-referenced against `196`, `256`)
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Line `103` places the socket at `$XDG_RUNTIME_DIR/whatty-askpass-<pid>.sock`, contradicting Section 4 (`196`: `$XDG_RUNTIME_DIR/whattytheshell/askpass-<pid>.sock`) and NFR (`256`: `0700` directory). Unescaped `<pid>` in Mermaid labels can be parsed as an HTML tag, and `subgraph Subproc` (`107`) labels `AskpassShim` and `RemoteSh` as `src/pty.rs` instead of `src/main.rs` and `src/ssh.rs`.

### 🟢 Nits

#### `[structural-1-4]` Internal Mismatch on Error Hold-Open Dismissal/Reconnect Trigger
- **Scope:** Line `24` vs. Line `243`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** Line `24` claims `"until the user presses Enter or clicks Close/Reconnect"`, while line `243` specifies only keyboard triggers (`Return`/`KP_Enter` or `Ctrl+Shift+R`) with no clickable UI control.

#### `[structural-1-5]` Asymmetric Horizontal Rule (`---`) Dividers
- **Scope:** Lines `7`, `249`, `308`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** Line `249` places an orphan divider before `## Non-Functional Requirements`, whereas peer H2 headings do not use dividers.

#### `[structural-1-6]` Section, List, and File-Link Formatting Asymmetries
- **Scope:** Lines `21–29`, `40–54`, `88–248`, `90–275`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** `## Goals` uses bold lead-ins (`* **Title**:`) while `## Non-Goals` uses plain bullets; `### 2.`–`### 6.` include `(src/...)` paths while `### 1.` does not; `### 4.` and `### 5.` use numbered sublists for non-sequential components while peers use `*`; file paths switch from Markdown links (`11–58`) to unlinked backticks (`90–275`).

#### `[structural-1-7]` Unlinked Forward References, Missing `ControlPath` Fallback Note, and Metric Punctuation
- **Scope:** Lines `26–29`, `211`, `259`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** `Phase 1..4`, `Mid-term exam`, and `Final exam` (`26–29`) lack links to sections outside `FeatureRange`; `ControlPath` (`211`) lacks line `196`'s fallback note; line `259` omits punctuation/conjunction between `< 5 ms` and `RSS < 8 MB`.

---

## Findings — `egm-style` (Feature Spec)

### 🔴 Blockers

#### `[style-spec-1-1]` Violations of Mandatory "The Alternatives Rule", Circular Rationales, and Dangling `(Option 1)` Reference
- **Scope:** Lines `270`, `276`, `282`, `288`, `294`, `300`, `306`
- **Severity:** `🔴 Blocker`
- **Discrepancy & Rationale:** Same unlinked `"Rejected by author:"` attribution across all 7 alternatives, circular policy restatements on lines `300` and `306`, dangling `"(Option 1)"` on line `270`, plus an unmeasured superlative `"achieves the lowest memory and network overhead"` on line `294` (calibrate to avoiding an extra SSH handshake, process, and duplicate authentication).

### 🟡 Majors

#### `[style-spec-1-2]` Missing Dedicated `Risks & Limits` / `Risks & Mitigations` Coverage
- **Scope:** Lines `21–24`, `251–262`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Spec lacks a dedicated `## Risks & Mitigations` section covering daemon unavailability, 108-byte `sockaddr_un` limits, stale sockets after `SIGKILL`, and deleted-profile pane behavior.

#### `[style-spec-1-3]` Uncalibrated Performance/Memory Claims (`< 5 ms`, `RSS < 8 MB`), Self-Contradictory Connection Count, and Broken Syntax
- **Scope:** Lines `25`, `259`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** `< 5 ms` startup and `RSS < 8 MB` lack measurements (`ld.so` maps `libgtk-4`/`libvulkan` before `main()`); `"1 SSH connection per terminal pane (shared via OpenSSH ControlMaster=auto when enabled)"` is self-contradictory; line `259` lacks punctuation/conjunction before `RSS < 8 MB`.

#### `[style-spec-1-4]` Contradictory `--ssh-askpass` Invocation and `argv[1]` Contract
- **Scope:** Lines `19`, `43`, `72`, `77`, `109`, `193–194`, `218`, `259`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Unify `whatty --ssh-askpass` phrasing across prose with `SSH_ASKPASS=<current_exe()>` (`218`) and fix `argv[1]` extraction when `--ssh-askpass` is present.

#### `[style-spec-1-5]` Contradictory Failed-SSH Dismissal UX and Unspecified `Ctrl+Shift+R` Handler
- **Scope:** Lines `24`, `47`, `243`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Remove `"clicks Close/Reconnect"` (`24`) or specify the widget, and define the `Ctrl+Shift+R` handler promised on line `243`.

#### `[style-spec-1-6]` Conflicting Askpass Socket Paths and Missing `ControlPath` Fallback
- **Scope:** Lines `103`, `196`, `211`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Align Mermaid line `103` with line `196` and specify `$XDG_RUNTIME_DIR` fallback + `0700` dir creation for `ControlPath` (`211`).

#### `[style-spec-1-7]` Misattributed Worker-Thread Pattern Reference and Conflated `gio` D-Bus API
- **Scope:** Line `177`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** `src/theme_marketplace.rs` does not spawn threads (`src/settings_dialog.rs:990-1141` uses `std::thread::spawn` + `async_channel` + `glib::spawn_future_local`); `DBusConnection::for_address_sync` and `gio::bus_get_sync` are separate APIs merged with `/`.

#### `[style-spec-1-8]` Cross-Section Redundancy and Academic Jargon in `Heilmeier Catechism Summary`
- **Scope:** Lines `15–29`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Condense duplicated Heilmeier bullets and replace `"Mid-term exam"` / `"Final exam"` (`27–29`) with engineering terms (`Unit Verification` / `End-to-End Verification`).

#### `[style-spec-1-9]` `"100% compatible"` Hyperbole and Uncalibrated Rollback Compatibility Claim
- **Scope:** Lines `261–262`
- **Severity:** `🟡 Major`
- **Discrepancy & Rationale:** Replace `"100% compatible with v0.4.3"` with `"unchanged"` and explicitly document that `v0.4.3` running `ConfigManager::save()` on exit drops `ssh_profiles` from `config.toml`.

### 🟢 Nits

#### `[style-spec-1-10]` Promotional / Imprecise `"in one click"` and `"1-click quick-launch"` Phrasing
- **Scope:** Lines `20`, `42`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** Opening a `HeaderBar` `MenuButton` and selecting a profile takes two clicks, and splits use shortcuts/context menu; use `"from the HeaderBar quick-launch menu"`.

#### `[style-spec-1-11]` Terminology Drift Between Profile Config and Live Session
- **Scope:** Lines `13`, `17`, `35`, `42`, `65`, `68`, `80`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** Standardize on `Saved SSH Profile` (config entity) and `Saved SSH Session` / `Saved SSH Tab` (runtime instance).

#### `[style-spec-1-12]` Ambiguous Remote Selector Keys (`[E] (or 0)`), `whatty-1` Collision, and `<primary_mux>` Formatting
- **Scope:** Lines `72`, `231–232`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** Clarify `[E]` vs `0`, case-insensitivity, `whatty-1` duplicates, and backtick `` `<primary_mux>` ``.

#### `[style-spec-1-13]` Over-Generalized Tab Title Overwrite Claim (`"with ~"`)
- **Scope:** Line `37`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** State `"with the client's local working directory (e.g., ~)"` rather than `"with ~"` unconditionally.

#### `[style-spec-1-14]` Internal Workflow Command Jargon (`"/egm-brainstorm phase"`)
- **Scope:** Line `58`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** Replace with `"None (see concepts_brief.md)."`

#### `[style-spec-1-15]` Imprecise Packaging Manifest Claim for External Binaries
- **Scope:** Line `275`
- **Severity:** `🟢 Nit`
- **Discrepancy & Rationale:** `scripts/build-appimage.sh` stages files into `AppDir` rather than declaring OS deps, and `Cargo.toml` holds only `[package.metadata.*]` tables for deb/rpm. Clarify exact packaging effect.
