// src/pty.rs
//! Spawns a child process attached to a pseudo-terminal.
//!
//! We avoid `forkpty` (unsafe in a multi-threaded GTK process) and instead use
//! `openpty` + `std::process::Command` with a `pre_exec` hook that makes the
//! slave the controlling terminal of a new session.

use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};

pub struct Pty {
    master: OwnedFd,
    child: Child,
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
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .env("LANG", lang)
            .env("LC_ALL", lc_all);

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
        Ok(Pty { master, child })
    }

    /// Spawn the user's login shell (`$SHELL`, falling back to `/bin/sh`), optionally setting cwd.
    pub fn spawn_shell(cols: u16, rows: u16, cwd: Option<&std::path::Path>) -> io::Result<Self> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        Self::spawn(&shell, &["-l"], cols, rows, cwd)
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

        let child_proc = std::path::PathBuf::from(format!("/proc/{}/cwd", self.child.id()));
        if let Ok(link) = std::fs::read_link(&child_proc) {
            if link.is_dir() {
                return Some(link);
            }
        }

        None
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
}

impl Drop for Pty {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
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
}
