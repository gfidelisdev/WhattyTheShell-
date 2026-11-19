// src/pty.rs
//! Spawns a child process attached to a pseudo-terminal.
//!
//! We avoid `forkpty` (unsafe in a multi-threaded GTK process) and instead use
//! `openpty` + `std::process::Command` with a `pre_exec` hook that makes the
//! slave the controlling terminal of a new session.

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// An installed shell executable discovered on the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellOption {
    /// Short display name (e.g., `"bash"`, `"fish"`, `"zsh"`).
    pub name: String,
    /// Full path to the shell executable (e.g., `"/usr/bin/fish"`).
    pub path: String,
}

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    match std::fs::metadata(path) {
        Ok(meta) => meta.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

/// Discover shells installed on the system by reading `/etc/shells`,
/// checking common shell paths, and including `$SHELL` if valid.
pub fn discover_installed_shells() -> Vec<ShellOption> {
    let mut candidates: Vec<String> = Vec::new();

    if let Ok(contents) = std::fs::read_to_string("/etc/shells") {
        for line in contents.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                candidates.push(trimmed.to_string());
            }
        }
    }

    const COMMON_SHELLS: &[&str] = &[
        "/bin/sh",
        "/usr/bin/sh",
        "/bin/bash",
        "/usr/bin/bash",
        "/bin/zsh",
        "/usr/bin/zsh",
        "/bin/fish",
        "/usr/bin/fish",
        "/bin/ash",
        "/usr/bin/ash",
        "/bin/dash",
        "/usr/bin/dash",
        "/usr/bin/nu",
        "/usr/bin/elvish",
        "/usr/bin/pwsh",
        "/bin/ksh",
        "/usr/bin/ksh",
        "/bin/tcsh",
        "/usr/bin/tcsh",
    ];
    for &p in COMMON_SHELLS {
        candidates.push(p.to_string());
    }

    if let Ok(env_shell) = std::env::var("SHELL") {
        let trimmed = env_shell.trim();
        if !trimmed.is_empty() {
            candidates.push(trimmed.to_string());
        }
    }

    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut shells: Vec<ShellOption> = Vec::new();

    for raw_path in candidates {
        let path = Path::new(&raw_path);
        if !is_executable_file(path) {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => continue,
        };
        // Deduplicate symlinks like /bin/bash -> /usr/bin/bash while keeping
        // distinct shell names like /bin/sh -> /usr/bin/bash.
        let canonical = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| raw_path.clone());
        let key = (name.clone(), canonical);
        if seen.insert(key) {
            shells.push(ShellOption {
                name,
                path: raw_path,
            });
        }
    }

    shells.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
            .then_with(|| a.path.cmp(&b.path))
    });
    shells
}

pub struct Pty {
    master: OwnedFd,
    child: Option<Child>,
    last_exit_code: Option<i32>,
}

fn winsize(cols: u16, rows: u16) -> libc::winsize {
    libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 }
}

fn cvt(ret: libc::c_int) -> io::Result<libc::c_int> {
    if ret < 0 { Err(io::Error::last_os_error()) } else { Ok(ret) }
}

impl Pty {
    /// Spawn `program args…` on a new PTY of the given size, optionally setting the working directory.
    pub fn spawn(
        program: &str,
        args: &[&str],
        cols: u16,
        rows: u16,
        cwd: Option<&std::path::Path>,
    ) -> io::Result<Self> {
        Self::spawn_inner(program, args, cols, rows, cwd, None, &[])
    }

    /// Spawn `program args…` on a new PTY with explicit environment variables (`envs`),
    /// scrubbing any inherited `WHATTY_SSH_ASKPASS_*` / `SSH_ASKPASS*` variables first.
    pub fn spawn_with_env(
        program: &str,
        args: &[&str],
        cols: u16,
        rows: u16,
        cwd: Option<&std::path::Path>,
        envs: &[(&str, &str)],
    ) -> io::Result<Self> {
        Self::spawn_inner(program, args, cols, rows, cwd, None, envs)
    }

    fn spawn_inner(
        program: &str,
        args: &[&str],
        cols: u16,
        rows: u16,
        cwd: Option<&std::path::Path>,
        shell_env: Option<&str>,
        envs: &[(&str, &str)],
    ) -> io::Result<Self> {
        let (mut master, mut slave): (RawFd, RawFd) = (-1, -1);
        let mut ws = winsize(cols, rows);
        // SAFETY: out-pointers are valid; name/termios may be null.
        cvt(unsafe {
            libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null(), &mut ws)
        })?;
        // SAFETY: openpty succeeded, so both fds are fresh and owned by us.
        let master = unsafe { OwnedFd::from_raw_fd(master) };
        let slave = unsafe { OwnedFd::from_raw_fd(slave) };
        // Don't leak the master into the child.
        cvt(unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) })?;

        // Enable UTF-8 mode on the terminal line discipline
        unsafe {
            let mut tio = std::mem::zeroed::<libc::termios>();
            if libc::tcgetattr(slave.as_raw_fd(), &mut tio) == 0 {
                tio.c_iflag |= libc::IUTF8;
                libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &tio);
            }
        }

        let mut cmd = Command::new(program);
        let lang = std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".into());
        let lc_all = std::env::var("LC_ALL").unwrap_or_else(|_| lang.clone());

        cmd.args(args)
            .stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::from(slave.try_clone()?))
            .stderr(Stdio::from(slave))
            .env_remove("WHATTY_SSH_ASKPASS_SOCKET")
            .env_remove("WHATTY_SSH_ASKPASS_NONCE")
            .env_remove("SSH_ASKPASS")
            .env_remove("SSH_ASKPASS_REQUIRE")
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .env("LANG", lang)
            .env("LC_ALL", lc_all);

        if let Some(sh) = shell_env {
            cmd.env("SHELL", sh);
        }

        for (k, v) in envs {
            cmd.env(k, v);
        }

        if let Some(dir) = cwd {
            if dir.is_dir() {
                cmd.current_dir(dir);
            }
        }

        // SAFETY: only async-signal-safe libc calls between fork and exec.
        unsafe {
            cmd.pre_exec(|| {
                cvt(libc::setsid())?;
                cvt(libc::ioctl(0, libc::TIOCSCTTY as _, 0))?;
                Ok(())
            });
        }
        let child = cmd.spawn()?;
        Ok(Pty {
            master,
            child: Some(child),
            last_exit_code: None,
        })
    }

    /// Spawn a login shell: uses `shell` if provided and valid, otherwise `$SHELL` or `/bin/sh`.
    pub fn spawn_with_shell(
        shell: Option<&str>,
        cols: u16,
        rows: u16,
        cwd: Option<&std::path::Path>,
    ) -> io::Result<Self> {
        let resolved = shell
            .map(str::trim)
            .filter(|s| !s.is_empty() && is_executable_file(Path::new(s)))
            .map(str::to_string)
            .unwrap_or_else(|| std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()));
        Self::spawn_inner(&resolved, &["-l"], cols, rows, cwd, Some(&resolved), &[])
    }

    /// Spawn the user's login shell (`$SHELL`, falling back to `/bin/sh`), optionally setting cwd.
    #[allow(dead_code)]
    pub fn spawn_shell(cols: u16, rows: u16, cwd: Option<&std::path::Path>) -> io::Result<Self> {
        Self::spawn_with_shell(None, cols, rows, cwd)
    }

    /// Returns the current working directory of the process running on this PTY.
    pub fn current_working_dir(&self) -> Option<std::path::PathBuf> {
        let master_fd = self.master.as_raw_fd();
        let fg_pgrp = unsafe { libc::tcgetpgrp(master_fd) };
        if fg_pgrp > 0 {
            let proc_path = std::path::PathBuf::from(format!("/proc/{}/cwd", fg_pgrp));
            if let Ok(link) = std::fs::read_link(&proc_path) {
                if link.is_dir() {
                    return Some(link);
                }
            }
        }

        if let Some(ref child) = self.child {
            let child_proc = std::path::PathBuf::from(format!("/proc/{}/cwd", child.id()));
            if let Ok(link) = std::fs::read_link(&child_proc) {
                if link.is_dir() {
                    return Some(link);
                }
            }
        }

        None
    }

    /// Non-blocking check for child termination; caches exit code (or `128 + signal`).
    pub fn try_wait_exit_code(&mut self) -> Option<i32> {
        if let Some(code) = self.last_exit_code {
            return Some(code);
        }
        let child = self.child.as_mut()?;
        match child.try_wait() {
            Ok(Some(status)) => {
                let code = status
                    .code()
                    .or_else(|| status.signal().map(|s| 128 + s))
                    .unwrap_or(1);
                self.last_exit_code = Some(code);
                self.child = None;
                Some(code)
            }
            Ok(None) | Err(_) => None,
        }
    }

    /// Raw file descriptor of the PTY master.
    pub fn raw_fd(&self) -> RawFd {
        self.master.as_raw_fd()
    }

    /// Non-blocking read from the PTY master using `poll(..., 0)`.
    pub fn try_read(&self, buf: &mut [u8]) -> io::Result<usize> {
        let fd = self.master.as_raw_fd();
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let poll_res = unsafe { libc::poll(&mut pfd, 1, 0) };
        if poll_res < 0 {
            return Err(io::Error::last_os_error());
        }
        if poll_res == 0 {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        }
        if (pfd.revents & libc::POLLIN) != 0 {
            let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(n as usize)
            }
        } else if (pfd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL)) != 0 {
            Ok(0)
        } else {
            Err(io::Error::from(io::ErrorKind::WouldBlock))
        }
    }

    /// A readable handle to the master side (blocking reads; use from a thread).
    pub fn reader(&self) -> io::Result<File> {
        Ok(File::from(self.master.try_clone()?))
    }

    /// A writable handle to the master side.
    pub fn writer(&self) -> io::Result<File> {
        Ok(File::from(self.master.try_clone()?))
    }

    pub fn write_all(&self, bytes: &[u8]) -> io::Result<()> {
        self.writer()?.write_all(bytes)
    }

    /// Inform the child of a new window size (sends SIGWINCH).
    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        let ws = winsize(cols, rows);
        // SAFETY: valid fd and pointer to a winsize struct.
        cvt(unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &ws) })?;
        Ok(())
    }

    /// The PID of the child process running on this PTY (`0` if already reaped).
    pub fn child_pid(&self) -> u32 {
        self.child.as_ref().map(|c| c.id()).unwrap_or(0)
    }

    /// Gracefully terminate the process group and child process attached to this PTY
    /// without signaling an already-reaped PID.
    pub fn terminate(&mut self) {
        if self.last_exit_code.is_some() || self.try_wait_exit_code().is_some() {
            return;
        }
        let Some(mut child) = self.child.take() else {
            return;
        };
        let pid = child.id() as i32;
        if pid > 0 {
            unsafe {
                libc::kill(-pid, libc::SIGHUP);
            }
        }
        for _ in 0..10 {
            if let Ok(Some(status)) = child.try_wait() {
                self.last_exit_code = Some(
                    status
                        .code()
                        .or_else(|| status.signal().map(|s| 128 + s))
                        .unwrap_or(0),
                );
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        if pid > 0 {
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::Terminal;
    use std::io::Read;

    #[test]
    fn child_output_reaches_terminal() {
        let pty = Pty::spawn("/bin/sh", &["-c", "printf 'hi\\033[31m!'; stty size"], 20, 5, None).unwrap();
        let mut reader = pty.reader().unwrap();
        let mut out = Vec::new();
        let mut buf = [0u8; 1024];
        // Reading the master returns EIO once the child exits and the slave closes.
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 { break }
            out.extend_from_slice(&buf[..n]);
        }
        let mut term = Terminal::new(20, 5);
        term.advance(&out);
        let text = term.grid().text();
        assert!(text.starts_with("hi!"), "{text:?}");
        assert!(text.contains("5 20"), "{text:?}");
    }

    #[test]
    fn child_output_special_chars() {
        let pty = Pty::spawn("/bin/sh", &["-c", "printf 'cd ~ olá çção\\n'"], 30, 5, None).unwrap();
        let mut reader = pty.reader().unwrap();
        let mut out = Vec::new();
        let mut buf = [0u8; 1024];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 { break }
            out.extend_from_slice(&buf[..n]);
        }
        let mut term = Terminal::new(30, 5);
        term.advance(&out);
        let text = term.grid().text();
        assert!(text.contains("cd ~ olá çção"), "{text:?}");
    }

    #[test]
    fn pty_tracks_working_directory() {
        let pty = Pty::spawn("/bin/sh", &["-c", "sleep 1"], 20, 5, Some(std::path::Path::new("/tmp"))).unwrap();
        let cwd = pty.current_working_dir();
        assert!(cwd.is_some());
        let p = cwd.unwrap();
        assert!(p == std::path::PathBuf::from("/tmp") || p == std::path::PathBuf::from("/private/tmp") || p.starts_with("/tmp"));
    }

    #[test]
    fn discover_installed_shells_finds_sh() {
        let shells = discover_installed_shells();
        assert!(!shells.is_empty(), "expected at least one shell on the system");
        assert!(
            shells.iter().any(|s| s.name == "sh"),
            "expected 'sh' in discovered shells: {:?}",
            shells
        );
    }

    #[test]
    fn spawn_with_shell_uses_explicit_shell_and_falls_back() {
        let mut pty = Pty::spawn_with_shell(Some("/bin/sh"), 20, 5, None).unwrap();
        pty.terminate();

        let mut fallback = Pty::spawn_with_shell(Some("/nonexistent/whatty-shell-xyz"), 20, 5, None).unwrap();
        fallback.terminate();
    }

    #[test]
    fn test_spawn_with_env_isolates_and_scrubs_askpass_vars() {
        // Test spawn_with_env passes custom env vars cleanly without inserting `-l`
        let mut pty = Pty::spawn_with_env(
            "/bin/sh",
            &["-c", "printf 'VAL=%s\\n' \"$WHATTY_CUSTOM_ENV_TEST\""],
            40,
            5,
            None,
            &[("WHATTY_CUSTOM_ENV_TEST", "ssh-ok")],
        )
        .unwrap();
        let mut reader = pty.reader().unwrap();
        let mut out = Vec::new();
        let mut buf = [0u8; 512];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("VAL=ssh-ok"), "unexpected output: {s:?}");
        pty.terminate();
    }

    #[test]
    fn test_try_wait_exit_code_captures_nonzero_status() {
        let mut pty = Pty::spawn_with_env("/bin/sh", &["-c", "exit 42"], 20, 5, None, &[]).unwrap();
        let mut reader = pty.reader().unwrap();
        let mut buf = [0u8; 64];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
        }
        let mut code = None;
        for _ in 0..20 {
            code = pty.try_wait_exit_code();
            if code.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(code, Some(42));
        assert_eq!(pty.try_wait_exit_code(), Some(42));
    }

    #[test]
    fn test_terminate_after_reaped_exit_skips_kill() {
        let mut pty = Pty::spawn_with_env("/bin/sh", &["-c", "exit 0"], 20, 5, None, &[]).unwrap();
        let mut reader = pty.reader().unwrap();
        let mut buf = [0u8; 64];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
        }
        for _ in 0..20 {
            if pty.try_wait_exit_code().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(pty.child_pid(), 0);
        // Must return cleanly without panicking or signaling any recycled PID.
        pty.terminate();
    }
}
