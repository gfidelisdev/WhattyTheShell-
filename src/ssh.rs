// src/ssh.rs
//! Saved SSH execution, OS keyring integration (`org.freedesktop.secrets`),
//! OpenSSH `SSH_ASKPASS` IPC broker, and portable POSIX multiplexer bootstrap.

use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gtk4::gio;
use gtk4::glib;
use gtk4::glib::variant::ObjectPath;
use gtk4::prelude::*;
use serde::{Deserialize, Serialize};

use crate::config::{MultiplexerKind, SshAuthKind, SshProfile};

const SECRET_SERVICE_BUS: &str = "org.freedesktop.secrets";
const SECRET_SERVICE_PATH: &str = "/org/freedesktop/secrets";
const DEFAULT_COLLECTION_PATH: &str = "/org/freedesktop/secrets/aliases/default";
const SECRET_SERVICE_IFACE: &str = "org.freedesktop.Secret.Service";
const SECRET_COLLECTION_IFACE: &str = "org.freedesktop.Secret.Collection";
const SECRET_ITEM_IFACE: &str = "org.freedesktop.Secret.Item";
const SECRET_PROMPT_IFACE: &str = "org.freedesktop.Secret.Prompt";
const SECRET_SESSION_IFACE: &str = "org.freedesktop.Secret.Session";
const APP_ATTRIBUTE_ID: &str = "io.github.whattytheshell";

const MAX_ASKPASS_JSON_BYTES: usize = 4096;
const NONCE_TTL: Duration = Duration::from_secs(120);

/// Verify and return the private `0700` runtime directory for askpass/ControlMaster sockets.
pub fn ssh_runtime_dir() -> io::Result<PathBuf> {
    let uid = unsafe { libc::geteuid() };
    if let Some(base) = dirs_next::runtime_dir() {
        let candidate = base.join("whattytheshell");
        if candidate.as_os_str().len() < 64 && ensure_verified_private_dir(&candidate, uid).is_ok()
        {
            return Ok(candidate);
        }
    }
    let fallback = PathBuf::from(format!("/tmp/whattytheshell-{uid}"));
    ensure_verified_private_dir(&fallback, uid)?;
    Ok(fallback)
}

fn ensure_verified_private_dir(dir: &Path, expected_uid: u32) -> io::Result<()> {
    match fs::symlink_metadata(dir) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "Runtime directory {} is not a real directory",
                        dir.display()
                    ),
                ));
            }
            if meta.uid() != expected_uid {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "Runtime directory {} has unexpected owner UID",
                        dir.display()
                    ),
                ));
            }
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(dir)?;
            let meta = fs::symlink_metadata(dir)?;
            if meta.file_type().is_symlink() || !meta.is_dir() || meta.uid() != expected_uid {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "Failed to verify created runtime directory {}",
                        dir.display()
                    ),
                ));
            }
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Client for Linux Secret Service (`org.freedesktop.secrets`) over `gio::bus_get_sync`.
pub struct SecretServiceClient;

struct SecretSessionGuard {
    conn: gio::DBusConnection,
    session_path: String,
}

impl Drop for SecretSessionGuard {
    fn drop(&mut self) {
        let _ = self.conn.call_sync(
            Some(SECRET_SERVICE_BUS),
            &self.session_path,
            SECRET_SESSION_IFACE,
            "Close",
            None,
            None,
            gio::DBusCallFlags::NONE,
            5_000,
            gio::Cancellable::NONE,
        );
    }
}

impl SecretServiceClient {
    fn open_session() -> Result<(gio::DBusConnection, SecretSessionGuard), String> {
        let conn = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
            .map_err(|e| format!("Failed to connect to session D-Bus: {e}"))?;

        let params = ("plain", glib::Variant::from("")).to_variant();
        let reply = conn
            .call_sync(
                Some(SECRET_SERVICE_BUS),
                SECRET_SERVICE_PATH,
                SECRET_SERVICE_IFACE,
                "OpenSession",
                Some(&params),
                Some(glib::VariantTy::new("(vo)").expect("valid OpenSession signature")),
                gio::DBusCallFlags::NONE,
                10_000,
                gio::Cancellable::NONE,
            )
            .map_err(|e| format!("Secret Service unavailable or OpenSession failed: {e}"))?;

        let session_path = reply
            .child_value(1)
            .str()
            .ok_or_else(|| "Secret Service returned non-string session path".to_string())?
            .to_string();

        Ok((conn.clone(), SecretSessionGuard { conn, session_path }))
    }

    fn build_attributes(profile_id: &str, secret_kind: &str) -> HashMap<String, String> {
        let mut attrs = HashMap::new();
        attrs.insert("application".to_string(), APP_ATTRIBUTE_ID.to_string());
        attrs.insert("profile_id".to_string(), profile_id.to_string());
        attrs.insert("secret_kind".to_string(), secret_kind.to_string());
        attrs
    }

    fn await_prompt_if_needed(conn: &gio::DBusConnection, prompt_path: &str) -> Result<(), String> {
        if prompt_path.is_empty() || prompt_path == "/" {
            return Ok(());
        }

        let ctx = glib::MainContext::new();
        ctx.with_thread_default(|| {
            let main_loop = glib::MainLoop::new(Some(&ctx), false);
            let outcome: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));

            let loop_sig = main_loop.clone();
            let outcome_sig = outcome.clone();
            let _sub = conn.subscribe_to_signal(
                Some(SECRET_SERVICE_BUS),
                Some(SECRET_PROMPT_IFACE),
                Some("Completed"),
                Some(prompt_path),
                None,
                gio::DBusSignalFlags::NONE,
                move |sig_ref| {
                    let dismissed = sig_ref
                        .parameters
                        .child_value(0)
                        .get::<bool>()
                        .unwrap_or(true);
                    if let Ok(mut slot) = outcome_sig.lock() {
                        *slot = Some(dismissed);
                    }
                    loop_sig.quit();
                },
            );

            let loop_to = main_loop.clone();
            let timed_out = Arc::new(Mutex::new(false));
            let timed_out_cb = timed_out.clone();
            let timeout_src = glib::timeout_source_new(
                Duration::from_secs(60),
                None,
                glib::Priority::DEFAULT,
                move || {
                    if let Ok(mut flag) = timed_out_cb.lock() {
                        *flag = true;
                    }
                    loop_to.quit();
                    glib::ControlFlow::Break
                },
            );
            timeout_src.attach(Some(&ctx));

            let call_res = conn.call_sync(
                Some(SECRET_SERVICE_BUS),
                prompt_path,
                SECRET_PROMPT_IFACE,
                "Prompt",
                Some(&("",).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                10_000,
                gio::Cancellable::NONE,
            );
            if let Err(e) = call_res {
                timeout_src.destroy();
                return Err(format!("Secret Service Prompt call failed: {e}"));
            }

            if outcome.lock().ok().and_then(|g| *g).is_none()
                && !timed_out.lock().map(|g| *g).unwrap_or(false)
            {
                main_loop.run();
            }

            timeout_src.destroy();

            if timed_out.lock().map(|g| *g).unwrap_or(false) {
                return Err("Keyring unlock prompt timed out".to_string());
            }
            match outcome.lock().ok().and_then(|g| *g) {
                Some(false) => Ok(()),
                Some(true) | None => Err("Keyring unlock prompt was dismissed".to_string()),
            }
        })
        .map_err(|_| "Failed to enter GLib thread-default MainContext".to_string())?
    }

    fn unlock_paths(
        conn: &gio::DBusConnection,
        raw_paths: &[String],
    ) -> Result<Vec<String>, String> {
        if raw_paths.is_empty() {
            return Ok(Vec::new());
        }
        let obj_paths: Vec<ObjectPath> = raw_paths
            .iter()
            .filter_map(|p| ObjectPath::try_from(p.as_str()).ok())
            .collect();
        if obj_paths.is_empty() {
            return Ok(Vec::new());
        }
        let reply = conn
            .call_sync(
                Some(SECRET_SERVICE_BUS),
                SECRET_SERVICE_PATH,
                SECRET_SERVICE_IFACE,
                "Unlock",
                Some(&(obj_paths,).to_variant()),
                Some(glib::VariantTy::new("(aoo)").expect("valid Unlock signature")),
                gio::DBusCallFlags::NONE,
                10_000,
                gio::Cancellable::NONE,
            )
            .map_err(|e| format!("Secret Service Unlock failed: {e}"))?;

        let prompt_path = reply.child_value(1).str().unwrap_or("/").to_string();
        Self::await_prompt_if_needed(conn, &prompt_path)?;

        let mut unlocked = Vec::new();
        let arr = reply.child_value(0);
        for child in arr.iter() {
            if let Some(s) = child.str() {
                unlocked.push(s.to_string());
            }
        }
        if unlocked.is_empty() && prompt_path != "/" {
            // Re-collect requested paths after a prompt completes.
            unlocked.extend(raw_paths.iter().cloned());
        }
        Ok(unlocked)
    }

    fn search_items(
        conn: &gio::DBusConnection,
        attrs: &HashMap<String, String>,
    ) -> Result<(Vec<String>, Vec<String>), String> {
        let reply = conn
            .call_sync(
                Some(SECRET_SERVICE_BUS),
                SECRET_SERVICE_PATH,
                SECRET_SERVICE_IFACE,
                "SearchItems",
                Some(&(attrs.clone(),).to_variant()),
                Some(glib::VariantTy::new("(aoao)").expect("valid SearchItems signature")),
                gio::DBusCallFlags::NONE,
                10_000,
                gio::Cancellable::NONE,
            )
            .map_err(|e| format!("Secret Service SearchItems failed: {e}"))?;

        let extract_paths = |idx: usize| -> Vec<String> {
            reply
                .child_value(idx)
                .iter()
                .filter_map(|v| v.str().map(|s| s.to_string()))
                .collect()
        };
        Ok((extract_paths(0), extract_paths(1)))
    }

    /// Synchronously look up a stored keyring secret for `profile_id` and `secret_kind`
    /// (`"password"` or `"passphrase"`). Safe to call from worker threads.
    pub fn lookup_secret_sync(
        profile_id: &str,
        secret_kind: &str,
    ) -> Result<Option<String>, String> {
        let (conn, session) = Self::open_session()?;
        let attrs = Self::build_attributes(profile_id, secret_kind);
        let (unlocked, locked) = Self::search_items(&conn, &attrs)?;

        let item_path = if let Some(first) = unlocked.first() {
            first.clone()
        } else if !locked.is_empty() {
            let newly_unlocked = Self::unlock_paths(&conn, &locked)?;
            match newly_unlocked.first() {
                Some(p) => p.clone(),
                None => return Ok(None),
            }
        } else {
            return Ok(None);
        };

        let obj_list = vec![
            ObjectPath::try_from(item_path.as_str())
                .map_err(|e| format!("Invalid item object path: {e}"))?,
        ];
        let sess_obj = ObjectPath::try_from(session.session_path.as_str())
            .map_err(|e| format!("Invalid session object path: {e}"))?;

        let reply = conn
            .call_sync(
                Some(SECRET_SERVICE_BUS),
                SECRET_SERVICE_PATH,
                SECRET_SERVICE_IFACE,
                "GetSecrets",
                Some(&(obj_list, sess_obj).to_variant()),
                Some(glib::VariantTy::new("(a{o(oayays)})").expect("valid GetSecrets signature")),
                gio::DBusCallFlags::NONE,
                10_000,
                gio::Cancellable::NONE,
            )
            .map_err(|e| format!("Secret Service GetSecrets failed: {e}"))?;

        let dict = reply.child_value(0);
        for entry in dict.iter() {
            let secret_tuple = entry.child_value(1);
            let value_var = secret_tuple.child_value(2);
            if let Some(bytes) = Vec::<u8>::from_variant(&value_var) {
                let text = String::from_utf8(bytes)
                    .map_err(|_| "Stored keyring secret is not valid UTF-8".to_string())?;
                return Ok(Some(text));
            }
        }
        Ok(None)
    }

    pub fn store_secret_sync(
        profile_id: &str,
        label: &str,
        secret_kind: &str,
        secret: &str,
    ) -> Result<(), String> {
        let (conn, session) = Self::open_session()?;
        Self::unlock_paths(&conn, &[DEFAULT_COLLECTION_PATH.to_string()])?;

        let attrs = Self::build_attributes(profile_id, secret_kind);
        let mut props: HashMap<String, glib::Variant> = HashMap::new();
        props.insert(
            "org.freedesktop.Secret.Item.Label".to_string(),
            label.to_variant(),
        );
        props.insert(
            "org.freedesktop.Secret.Item.Attributes".to_string(),
            attrs.to_variant(),
        );

        let sess_obj = ObjectPath::try_from(session.session_path.as_str())
            .map_err(|e| format!("Invalid session path: {e}"))?;
        let secret_tuple = (
            sess_obj,
            Vec::<u8>::new(),
            secret.as_bytes().to_vec(),
            "text/plain; charset=utf-8".to_string(),
        );

        let params = (props, secret_tuple, true).to_variant();
        let reply = conn
            .call_sync(
                Some(SECRET_SERVICE_BUS),
                DEFAULT_COLLECTION_PATH,
                SECRET_COLLECTION_IFACE,
                "CreateItem",
                Some(&params),
                Some(glib::VariantTy::new("(oo)").expect("valid CreateItem signature")),
                gio::DBusCallFlags::NONE,
                10_000,
                gio::Cancellable::NONE,
            )
            .map_err(|e| format!("Secret Service CreateItem failed: {e}"))?;

        let prompt_path = reply.child_value(1).str().unwrap_or("/").to_string();
        Self::await_prompt_if_needed(&conn, &prompt_path)
    }

    pub fn delete_secret_sync(profile_id: &str, secret_kind: Option<&str>) -> Result<(), String> {
        let (conn, _session) = Self::open_session()?;
        let mut attrs = HashMap::new();
        attrs.insert("application".to_string(), APP_ATTRIBUTE_ID.to_string());
        attrs.insert("profile_id".to_string(), profile_id.to_string());
        if let Some(kind) = secret_kind {
            attrs.insert("secret_kind".to_string(), kind.to_string());
        }

        let (unlocked, locked) = Self::search_items(&conn, &attrs)?;
        let mut all_paths = unlocked;
        if !locked.is_empty() {
            all_paths.extend(Self::unlock_paths(&conn, &locked)?);
        }

        for item_path in all_paths {
            let reply = conn
                .call_sync(
                    Some(SECRET_SERVICE_BUS),
                    &item_path,
                    SECRET_ITEM_IFACE,
                    "Delete",
                    None,
                    Some(glib::VariantTy::new("(o)").expect("valid Delete signature")),
                    gio::DBusCallFlags::NONE,
                    10_000,
                    gio::Cancellable::NONE,
                )
                .map_err(|e| format!("Secret Service Item.Delete failed: {e}"))?;
            let prompt_path = reply.child_value(0).str().unwrap_or("/").to_string();
            Self::await_prompt_if_needed(&conn, &prompt_path)?;
        }
        Ok(())
    }

    /// Asynchronously store a password/passphrase in `org.freedesktop.secrets`.
    pub fn store_secret_async(
        profile_id: String,
        label: String,
        secret_kind: String,
        secret: String,
        tx: async_channel::Sender<Result<(), String>>,
    ) {
        std::thread::spawn(move || {
            let res = Self::store_secret_sync(&profile_id, &label, &secret_kind, &secret);
            let _ = tx.send_blocking(res);
        });
    }

    /// Asynchronously delete stored keyring secrets matching `profile_id` (and optional `secret_kind`).
    pub fn delete_secret_async(
        profile_id: String,
        secret_kind: Option<String>,
        tx: async_channel::Sender<Result<(), String>>,
    ) {
        std::thread::spawn(move || {
            let res = Self::delete_secret_sync(&profile_id, secret_kind.as_deref());
            let _ = tx.send_blocking(res);
        });
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AskpassRequest {
    nonce: String,
    prompt: String,
    #[serde(default)]
    prompt_hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AskpassResponse {
    ok: bool,
    #[serde(default)]
    secret: String,
}

/// Extract the OpenSSH prompt text regardless of whether `--ssh-askpass` was passed in `argv`.
pub fn extract_askpass_prompt_arg<I: IntoIterator<Item = String>>(args: I) -> String {
    args.into_iter()
        .skip(1)
        .find(|a| a != "--ssh-askpass")
        .unwrap_or_default()
}

/// Headless pre-GTK askpass client entrypoint (`SSH_ASKPASS`). Never initializes GTK or wgpu.
pub fn run_askpass_client_and_exit() -> ! {
    let exit_code = match run_askpass_client_once(
        std::env::var_os("WHATTY_SSH_ASKPASS_SOCKET").as_deref(),
        std::env::var("WHATTY_SSH_ASKPASS_NONCE").ok().as_deref(),
        &extract_askpass_prompt_arg(std::env::args()),
        std::env::var("SSH_ASKPASS_PROMPT")
            .ok()
            .as_deref()
            .unwrap_or(""),
    ) {
        Ok(Some(secret)) => {
            let mut stdout = io::stdout().lock();
            if writeln!(stdout, "{secret}").is_ok() && stdout.flush().is_ok() {
                0
            } else {
                1
            }
        }
        Ok(None) | Err(_) => 1,
    };
    std::process::exit(exit_code);
}

fn run_askpass_client_once(
    socket_os: Option<&std::ffi::OsStr>,
    nonce: Option<&str>,
    prompt: &str,
    prompt_hint: &str,
) -> io::Result<Option<String>> {
    let sock_path = socket_os
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "missing askpass socket"))?;
    let nonce_str =
        nonce.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "missing askpass nonce"))?;

    let mut stream = UnixStream::connect(sock_path)?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    stream.set_read_timeout(Some(Duration::from_secs(120)))?;

    let req = AskpassRequest {
        nonce: nonce_str.to_string(),
        prompt: prompt.to_string(),
        prompt_hint: prompt_hint.to_string(),
    };
    let line =
        serde_json::to_string(&req).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    writeln!(stream, "{line}")?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut resp_line = String::new();
    let n = reader
        .by_ref()
        .take(MAX_ASKPASS_JSON_BYTES as u64)
        .read_line(&mut resp_line)?;
    if n == 0 {
        return Ok(None);
    }
    let resp: AskpassResponse = serde_json::from_str(resp_line.trim_end())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    if resp.ok {
        Ok(Some(resp.secret))
    } else {
        Ok(None)
    }
}

/// Deterministic prompt classification for incoming OpenSSH askpass requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskpassPromptClassification {
    /// First matching keyring credential lookup (`"password"` or `"passphrase"`).
    KeyringLookup { secret_kind: &'static str },
    /// Interactive host-key verification prompt (`yes/no` or fingerprint).
    ConfirmDialog { expects_fingerprint: bool },
    /// Informational prompt (`SSH_ASKPASS_PROMPT=none`).
    InfoDialog,
    /// Interactive secret/OTP/retry prompt via masked `PasswordEntry`.
    SecretDialog,
}

/// Classify an OpenSSH askpass prompt without GTK/D-Bus side effects.
pub fn classify_askpass_prompt(
    auth_kind: SshAuthKind,
    prompt: &str,
    prompt_hint: &str,
    password_attempts: u32,
) -> AskpassPromptClassification {
    let hint = prompt_hint.trim().to_ascii_lowercase();
    let lower = prompt.to_ascii_lowercase();

    if hint == "none" {
        return AskpassPromptClassification::InfoDialog;
    }
    if hint == "confirm" || lower.contains("yes/no") {
        return AskpassPromptClassification::ConfirmDialog {
            expects_fingerprint: lower.contains("[fingerprint]"),
        };
    }

    if hint.is_empty() && password_attempts == 0 && !lower.contains("fingerprint") {
        match auth_kind {
            SshAuthKind::PasswordKeyring if lower.contains("password") => {
                return AskpassPromptClassification::KeyringLookup {
                    secret_kind: "password",
                };
            }
            SshAuthKind::IdentityFile if lower.contains("passphrase") => {
                return AskpassPromptClassification::KeyringLookup {
                    secret_kind: "passphrase",
                };
            }
            _ => {}
        }
    }

    AskpassPromptClassification::SecretDialog
}

#[derive(Clone)]
struct AttemptRecord {
    profile_id: String,
    profile_name: String,
    auth_kind: SshAuthKind,
    window: glib::SendWeakRef<gtk4::ApplicationWindow>,
    password_attempts: u32,
    created_at: Instant,
}

struct BrokerState {
    socket_path: PathBuf,
    attempts: HashMap<String, AttemptRecord>,
}

/// Process-wide Unix domain socket broker serving OpenSSH `SSH_ASKPASS` prompts.
#[derive(Clone)]
pub struct AskpassBroker {
    state: Arc<Mutex<BrokerState>>,
    _listener_guard: Arc<ListenerGuard>,
}

struct ListenerGuard {
    socket_path: PathBuf,
    source_id: Mutex<Option<glib::SourceId>>,
    _listener: UnixListener,
}

impl Drop for ListenerGuard {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.source_id.lock()
            && let Some(id) = guard.take()
        {
            id.remove();
        }
        let _ = fs::remove_file(&self.socket_path);
    }
}

/// RAII token revoking an SSH connection's askpass nonce when dropped.
pub struct AskpassAttempt {
    nonce: String,
    socket_path: PathBuf,
    state: Option<Arc<Mutex<BrokerState>>>,
}

impl AskpassAttempt {
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn nonce(&self) -> &str {
        &self.nonce
    }
}

impl Drop for AskpassAttempt {
    fn drop(&mut self) {
        if let Some(ref state) = self.state
            && let Ok(mut guard) = state.lock()
        {
            guard.attempts.remove(&self.nonce);
        }
    }
}

fn generate_nonce_hex() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    let mut f = fs::File::open("/dev/urandom")?;
    f.read_exact(&mut bytes)?;
    let mut out = String::with_capacity(32);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    Ok(out)
}

fn verify_peer_cred(stream: &UnixStream) -> io::Result<()> {
    let mut cred = libc::ucred {
        pid: 0,
        uid: u32::MAX,
        gid: u32::MAX,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let ret = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut libc::ucred as *mut libc::c_void,
            &mut len,
        )
    };
    if ret != 0 {
        return Err(io::Error::last_os_error());
    }
    let expected_uid = unsafe { libc::geteuid() };
    if cred.uid != expected_uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Rejecting askpass peer with mismatched UID",
        ));
    }
    Ok(())
}

fn send_askpass_response(stream: &mut UnixStream, resp: &AskpassResponse) {
    if let Ok(line) = serde_json::to_string(resp) {
        let _ = writeln!(stream, "{line}");
        let _ = stream.flush();
    }
}

fn read_askpass_request(stream: &UnixStream) -> io::Result<AskpassRequest> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    let mut raw = Vec::with_capacity(512);
    let mut limited = stream
        .try_clone()?
        .take((MAX_ASKPASS_JSON_BYTES + 1) as u64);
    let mut byte = [0u8; 1];
    loop {
        match limited.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                if byte[0] == b'\n' {
                    break;
                }
                raw.push(byte[0]);
                if raw.len() > MAX_ASKPASS_JSON_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Askpass request exceeds max length",
                    ));
                }
            }
            Err(e) => return Err(e),
        }
    }
    serde_json::from_slice(&raw).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

impl AskpassBroker {
    /// Start the process-wide `UnixListener` on the GTK main loop using `unix_fd_add_local`.
    pub fn start() -> io::Result<Self> {
        let dir = ssh_runtime_dir()?;
        let socket_path = dir.join(format!("askpass-{}.sock", std::process::id()));
        let _ = fs::remove_file(&socket_path);

        let listener = UnixListener::bind(&socket_path)?;
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;

        let listener_fd = listener.as_raw_fd();
        let state = Arc::new(Mutex::new(BrokerState {
            socket_path: socket_path.clone(),
            attempts: HashMap::new(),
        }));

        let listener_clone = listener.try_clone()?;
        let state_accept = state.clone();
        let source_id = crate::widget::unix_fd_add_local(
            listener_fd,
            glib::IOCondition::IN,
            move |_fd, _cond| {
                loop {
                    match listener_clone.accept() {
                        Ok((stream, _addr)) => {
                            let state_conn = state_accept.clone();
                            handle_accepted_stream(stream, state_conn);
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                        Err(e) => {
                            eprintln!("[Whatty] AskpassBroker accept error: {e}");
                            break;
                        }
                    }
                }
                glib::ControlFlow::Continue
            },
        );

        Ok(Self {
            state,
            _listener_guard: Arc::new(ListenerGuard {
                socket_path,
                source_id: Mutex::new(Some(source_id)),
                _listener: listener,
            }),
        })
    }

    pub fn socket_path(&self) -> PathBuf {
        self.state
            .lock()
            .map(|g| g.socket_path.clone())
            .unwrap_or_default()
    }

    /// Register a 128-bit single-connection nonce associated with an `ApplicationWindow`.
    pub fn register_attempt(
        &self,
        window: &gtk4::ApplicationWindow,
        profile_id: String,
        profile_name: String,
        auth_kind: SshAuthKind,
    ) -> io::Result<AskpassAttempt> {
        let nonce = generate_nonce_hex()?;
        let weak_win = glib::SendWeakRef::from(window.downgrade());
        let socket_path = {
            let mut guard = self
                .state
                .lock()
                .map_err(|_| io::Error::other("Broker state poisoned"))?;
            let now = Instant::now();
            guard
                .attempts
                .retain(|_, v| now.duration_since(v.created_at) < NONCE_TTL);
            guard.attempts.insert(
                nonce.clone(),
                AttemptRecord {
                    profile_id,
                    profile_name,
                    auth_kind,
                    window: weak_win,
                    password_attempts: 0,
                    created_at: now,
                },
            );
            guard.socket_path.clone()
        };

        Ok(AskpassAttempt {
            nonce,
            socket_path,
            state: Some(self.state.clone()),
        })
    }
}

#[derive(Debug, Clone)]
enum SocketEvalOutcome {
    Reject,
    ReplyOk(String),
    NeedModal {
        window: glib::SendWeakRef<gtk4::ApplicationWindow>,
        profile_name: String,
        classification: AskpassPromptClassification,
        prompt: String,
    },
}

fn process_request_with_lookup<F>(
    state: &Arc<Mutex<BrokerState>>,
    req: &AskpassRequest,
    lookup_fn: F,
) -> SocketEvalOutcome
where
    F: FnOnce(&str, &str) -> Result<Option<String>, String>,
{
    let snapshot = {
        let mut guard = match state.lock() {
            Ok(g) => g,
            Err(_) => return SocketEvalOutcome::Reject,
        };
        let now = Instant::now();
        guard
            .attempts
            .retain(|_, v| now.duration_since(v.created_at) < NONCE_TTL);

        let rec = match guard.attempts.get_mut(&req.nonce) {
            Some(r) => r,
            None => return SocketEvalOutcome::Reject,
        };

        let class = classify_askpass_prompt(
            rec.auth_kind,
            &req.prompt,
            &req.prompt_hint,
            rec.password_attempts,
        );
        if matches!(class, AskpassPromptClassification::KeyringLookup { .. }) {
            rec.password_attempts = rec.password_attempts.saturating_add(1);
        }
        (
            rec.profile_id.clone(),
            rec.profile_name.clone(),
            rec.window.clone(),
            class,
        )
    };

    let (profile_id, profile_name, window, classification) = snapshot;
    match classification {
        AskpassPromptClassification::KeyringLookup { secret_kind } => {
            match lookup_fn(&profile_id, secret_kind) {
                Ok(Some(secret)) => SocketEvalOutcome::ReplyOk(secret),
                Ok(None) | Err(_) => SocketEvalOutcome::NeedModal {
                    window,
                    profile_name,
                    classification: AskpassPromptClassification::SecretDialog,
                    prompt: req.prompt.clone(),
                },
            }
        }
        other => SocketEvalOutcome::NeedModal {
            window,
            profile_name,
            classification: other,
            prompt: req.prompt.clone(),
        },
    }
}

fn handle_accepted_stream(stream: UnixStream, state: Arc<Mutex<BrokerState>>) {
    let (tx, rx) = async_channel::bounded::<(UnixStream, SocketEvalOutcome)>(1);

    std::thread::spawn(move || {
        if verify_peer_cred(&stream).is_err() {
            return;
        }
        let req = match read_askpass_request(&stream) {
            Ok(r) => r,
            Err(_) => {
                let mut s = stream;
                send_askpass_response(
                    &mut s,
                    &AskpassResponse {
                        ok: false,
                        secret: String::new(),
                    },
                );
                return;
            }
        };
        let outcome =
            process_request_with_lookup(&state, &req, SecretServiceClient::lookup_secret_sync);
        match outcome {
            SocketEvalOutcome::Reject => {
                let mut s = stream;
                send_askpass_response(
                    &mut s,
                    &AskpassResponse {
                        ok: false,
                        secret: String::new(),
                    },
                );
            }
            SocketEvalOutcome::ReplyOk(secret) => {
                let mut s = stream;
                send_askpass_response(&mut s, &AskpassResponse { ok: true, secret });
            }
            modal => {
                let _ = tx.send_blocking((stream, modal));
            }
        }
    });

    glib::spawn_future_local(async move {
        if let Ok((
            mut stream,
            SocketEvalOutcome::NeedModal {
                window,
                profile_name,
                classification,
                prompt,
            },
        )) = rx.recv().await
        {
            let parent = window.upgrade();
            let resp =
                show_askpass_modal_dialog(parent.as_ref(), &profile_name, classification, &prompt)
                    .await;
            send_askpass_response(&mut stream, &resp);
        }
    });
}

async fn show_askpass_modal_dialog(
    parent: Option<&gtk4::ApplicationWindow>,
    profile_name: &str,
    classification: AskpassPromptClassification,
    prompt: &str,
) -> AskpassResponse {
    let (tx, rx) = async_channel::bounded::<AskpassResponse>(1);

    let builder = gtk4::Window::builder()
        .title(format!("SSH Authentication — {profile_name}"))
        .modal(true)
        .resizable(false)
        .default_width(440);
    let dialog = match parent {
        Some(win) => builder.transient_for(win).build(),
        None => builder.build(),
    };

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);

    let prompt_lbl = gtk4::Label::new(Some(prompt));
    prompt_lbl.set_wrap(true);
    prompt_lbl.set_xalign(0.0);
    prompt_lbl.set_selectable(true);
    vbox.append(&prompt_lbl);

    let sent = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let send_once = |tx: &async_channel::Sender<AskpassResponse>,
                     sent: &Arc<std::sync::atomic::AtomicBool>,
                     dlg: &gtk4::Window,
                     resp: AskpassResponse| {
        if !sent.swap(true, std::sync::atomic::Ordering::SeqCst) {
            let _ = tx.send_blocking(resp);
            dlg.close();
        }
    };

    let btn_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    btn_box.set_halign(gtk4::Align::End);

    match classification {
        AskpassPromptClassification::InfoDialog => {
            let ok_btn = gtk4::Button::with_label("OK");
            ok_btn.add_css_class("suggested-action");
            let tx_ok = tx.clone();
            let sent_ok = sent.clone();
            let dlg_ok = dialog.clone();
            ok_btn.connect_clicked(move |_| {
                send_once(
                    &tx_ok,
                    &sent_ok,
                    &dlg_ok,
                    AskpassResponse {
                        ok: true,
                        secret: String::new(),
                    },
                );
            });
            btn_box.append(&ok_btn);
        }
        AskpassPromptClassification::ConfirmDialog {
            expects_fingerprint,
        } => {
            let fp_entry = if expects_fingerprint {
                let entry = gtk4::Entry::new();
                entry.set_placeholder_text(Some("yes / no / fingerprint"));
                vbox.append(&entry);
                Some(entry)
            } else {
                None
            };

            let reject_btn = gtk4::Button::with_label("Reject");
            let tx_rej = tx.clone();
            let sent_rej = sent.clone();
            let dlg_rej = dialog.clone();
            reject_btn.connect_clicked(move |_| {
                send_once(
                    &tx_rej,
                    &sent_rej,
                    &dlg_rej,
                    AskpassResponse {
                        ok: false,
                        secret: String::new(),
                    },
                );
            });

            let accept_btn = gtk4::Button::with_label("Accept");
            accept_btn.add_css_class("suggested-action");
            let tx_acc = tx.clone();
            let sent_acc = sent.clone();
            let dlg_acc = dialog.clone();
            let fp_acc = fp_entry.clone();
            accept_btn.connect_clicked(move |_| {
                let val = fp_acc
                    .as_ref()
                    .map(|e| e.text().trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "yes".to_string());
                send_once(
                    &tx_acc,
                    &sent_acc,
                    &dlg_acc,
                    AskpassResponse {
                        ok: true,
                        secret: val,
                    },
                );
            });

            if let Some(entry) = fp_entry {
                let tx_ent = tx.clone();
                let sent_ent = sent.clone();
                let dlg_ent = dialog.clone();
                entry.connect_activate(move |e| {
                    let val = e.text().trim().to_string();
                    let secret = if val.is_empty() {
                        "yes".to_string()
                    } else {
                        val
                    };
                    send_once(
                        &tx_ent,
                        &sent_ent,
                        &dlg_ent,
                        AskpassResponse { ok: true, secret },
                    );
                });
            }

            btn_box.append(&reject_btn);
            btn_box.append(&accept_btn);
        }
        AskpassPromptClassification::KeyringLookup { .. }
        | AskpassPromptClassification::SecretDialog => {
            let entry = gtk4::PasswordEntry::new();
            entry.set_show_peek_icon(true);
            entry.set_hexpand(true);
            vbox.append(&entry);

            let cancel_btn = gtk4::Button::with_label("Cancel");
            let tx_can = tx.clone();
            let sent_can = sent.clone();
            let dlg_can = dialog.clone();
            cancel_btn.connect_clicked(move |_| {
                send_once(
                    &tx_can,
                    &sent_can,
                    &dlg_can,
                    AskpassResponse {
                        ok: false,
                        secret: String::new(),
                    },
                );
            });

            let submit_btn = gtk4::Button::with_label("Submit");
            submit_btn.add_css_class("suggested-action");
            let tx_sub = tx.clone();
            let sent_sub = sent.clone();
            let dlg_sub = dialog.clone();
            let ent_sub = entry.clone();
            submit_btn.connect_clicked(move |_| {
                send_once(
                    &tx_sub,
                    &sent_sub,
                    &dlg_sub,
                    AskpassResponse {
                        ok: true,
                        secret: ent_sub.text().to_string(),
                    },
                );
            });

            let tx_act = tx.clone();
            let sent_act = sent.clone();
            let dlg_act = dialog.clone();
            entry.connect_activate(move |e| {
                send_once(
                    &tx_act,
                    &sent_act,
                    &dlg_act,
                    AskpassResponse {
                        ok: true,
                        secret: e.text().to_string(),
                    },
                );
            });

            btn_box.append(&cancel_btn);
            btn_box.append(&submit_btn);
        }
    }

    vbox.append(&btn_box);
    dialog.set_child(Some(&vbox));

    let tx_close = tx.clone();
    let sent_close = sent.clone();
    dialog.connect_close_request(move |_| {
        if !sent_close.swap(true, std::sync::atomic::Ordering::SeqCst) {
            let _ = tx_close.send_blocking(AskpassResponse {
                ok: false,
                secret: String::new(),
            });
        }
        glib::Propagation::Proceed
    });

    dialog.present();

    rx.recv().await.unwrap_or(AskpassResponse {
        ok: false,
        secret: String::new(),
    })
}

/// Build the single-line, single-quote-free, backslash-free remote POSIX bootstrap script.
///
/// The script safely parses inside `sh -c '...'` regardless of whether the remote login shell
/// is `sh`, `bash`, `zsh`, `dash`, `fish`, or `tcsh`.
pub fn build_remote_bootstrap_script(priority: &[MultiplexerKind]) -> String {
    let mut ordered: Vec<&'static str> = Vec::new();
    for kind in priority {
        let name = match kind {
            MultiplexerKind::Tmux => "tmux",
            MultiplexerKind::Screen => "screen",
            MultiplexerKind::Unknown => continue,
        };
        if !ordered.contains(&name) {
            ordered.push(name);
        }
    }

    if ordered.is_empty() {
        return "exec \"${SHELL:-/bin/sh}\" -l".to_string();
    }

    let mux_list = ordered.join(" ");
    let mut parts: Vec<String> = Vec::new();
    parts.push("PATH=\"$PATH:/usr/local/bin:/opt/homebrew/bin:$HOME/.local/bin\"".to_string());
    parts.push("avail=\"\"; primary=\"\"".to_string());
    parts.push(format!(
        "for m in {mux_list}; do if command -v \"$m\" >/dev/null 2>&1; then avail=\"$avail $m\"; [ -z \"$primary\" ] && primary=\"$m\"; fi; done"
    ));
    parts.push("if [ -z \"$primary\" ]; then exec \"${SHELL:-/bin/sh}\" -l; fi".to_string());
    parts.push(
        "rows=$(for m in $avail; do case \"$m\" in tmux) tmux list-sessions -F \"#{session_windows} windows (#{?session_attached,attached,detached})|#{session_name}\" 2>/dev/null | while IFS= read -r l; do [ -n \"$l\" ] && echo \"tmux|$l\"; done ;; screen) LC_ALL=C screen -ls 2>/dev/null | while IFS= read -r l; do case \"$l\" in *\"(Dead\"*|*\"(Removed\"*) continue ;; *\"(Attached)\"*|*\"(Detached)\"*|*\"(Multi,\"*) s=$(printf \"%s\" \"$l\" | sed \"s/^[[:space:]]*//\" | cut -f1); [ -n \"$s\" ] && echo \"screen|screen|$s\" ;; esac; done ;; esac; done)".to_string()
    );
    parts.push(
        concat!(
            "if [ -z \"$rows\" ]; then ",
            "echo \"\"; ",
            "echo \"=== WhattyTheShell Remote Session ($primary) ===\"; ",
            "echo \"[1] Start new $primary session (default)\"; ",
            "echo \"[2] Start ephemeral shell without multiplexer\"; ",
            "while :; do ",
            "printf \"Select option [1]: \" >/dev/tty; ",
            "IFS= read -r choice </dev/tty || exit 0; ",
            "case \"$choice\" in \"\"|1) ",
            "if [ \"$primary\" = \"tmux\" ]; then exec tmux new-session; else exec screen; fi ;; ",
            "2) exec \"${SHELL:-/bin/sh}\" -l ;; ",
            "*) echo \"Invalid selection.\" >/dev/tty ;; ",
            "esac; done; ",
            "fi"
        )
        .to_string(),
    );
    parts.push(concat!(
        "count=$(printf \"%s\" \"$rows\" | wc -l | tr -d \" \"); ",
        "while :; do ",
        "echo \"\" >/dev/tty; ",
        "echo \"=== Active Remote Multiplexer Sessions ===\" >/dev/tty; ",
        "i=0; ",
        "printf \"%s\" \"$rows\" | { while IFS= read -r r; do ",
        "i=$((i + 1)); rest=${r#*|}; info=${rest%%|*}; target=${rest#*|}; ",
        "echo \"[$i] ${r%%|*}: $target ($info)\" >/dev/tty; ",
        "done; }; ",
        "echo \"[N] New multiplexed session ($primary)\" >/dev/tty; ",
        "echo \"[E] Ephemeral shell\" >/dev/tty; ",
        "printf \"Select session [1]: \" >/dev/tty; ",
        "IFS= read -r choice </dev/tty || exit 0; ",
        "[ -z \"$choice\" ] && choice=1; ",
        "case \"$choice\" in ",
        "[Nn]) printf \"Session name (leave blank for default): \" >/dev/tty; ",
        "IFS= read -r sname </dev/tty || exit 0; ",
        "if [ \"$primary\" = \"tmux\" ]; then ",
        "if [ -n \"$sname\" ]; then exec tmux new-session -s \"$sname\"; else exec tmux new-session; fi; ",
        "else ",
        "if [ -n \"$sname\" ]; then exec screen -S \"$sname\"; else exec screen; fi; ",
        "fi ;; ",
        "[Ee]) exec \"${SHELL:-/bin/sh}\" -l ;; ",
        "*) if [ \"$choice\" -ge 1 ] 2>/dev/null && [ \"$choice\" -le \"$count\" ] 2>/dev/null; then ",
        "picked=$(printf \"%s\" \"$rows\" | sed -n \"${choice}p\"); ",
        "kind=${picked%%|*}; rest=${picked#*|}; target=${rest#*|}; ",
        "case \"$kind\" in ",
        "tmux) exec tmux attach-session -t \"=$target\" ;; ",
        "screen) exec screen -d -r \"$target\" ;; ",
        "esac; ",
        "else echo \"Invalid selection.\" >/dev/tty; fi ;; ",
        "esac; ",
        "done"
    ).to_string());

    // Note: printf "%s" "$rows" skips the last line unless a trailing newline is appended.
    // Replace all `printf "%s" "$rows"` above? Handled cleanly below using `echo "$rows"`!
    let mut joined = parts.join("; ");
    joined = joined.replace("printf \"%s\" \"$rows\"", "echo \"$rows\"");
    debug_assert!(!joined.contains('\''));
    debug_assert!(!joined.contains('\\'));
    debug_assert!(!joined.contains('\n'));
    joined
}

fn validate_ssh_token(label: &str, value: &str, allow_at: bool) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("SSH {label} must not be empty"));
    }
    if value.starts_with('-') {
        return Err(format!("SSH {label} must not start with '-'"));
    }
    if value
        .chars()
        .any(|c| c.is_ascii_whitespace() || c.is_control() || c == '\0')
    {
        return Err(format!(
            "SSH {label} must not contain whitespace or control characters"
        ));
    }
    if !allow_at && value.contains('@') {
        return Err(format!("SSH {label} must not contain '@'"));
    }
    Ok(())
}

fn expand_tilde_path(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if s == "~" {
        return dirs_next::home_dir().unwrap_or_else(|| path.to_path_buf());
    }
    if let Some(rest) = s.strip_prefix("~/")
        && let Some(home) = dirs_next::home_dir()
    {
        return home.join(rest);
    }
    path.to_path_buf()
}

/// Resolved command tuple `(program, args, extra_envs)` for spawning an interactive SSH PTY.
pub type SshInvocation = (String, Vec<String>, Vec<(String, String)>);

/// Build validated `(program, args, envs)` for spawning `ssh -t` over `Pty::spawn_with_env`.
pub fn build_ssh_invocation(
    profile: &SshProfile,
    attempt: Option<(&Path, &str)>,
) -> Result<SshInvocation, String> {
    let host = profile.host.trim();
    validate_ssh_token("host", host, profile.user.is_none())?;

    let destination = if let Some(ref raw_user) = profile.user {
        let user = raw_user.trim();
        validate_ssh_token("user", user, false)?;
        format!("{user}@{host}")
    } else {
        host.to_string()
    };

    if let Some(port) = profile.port
        && port == 0
    {
        return Err("SSH port must be between 1 and 65535".to_string());
    }

    let mut args = vec!["-t".to_string()];

    match profile.auth_kind {
        SshAuthKind::PasswordKeyring => {
            args.push("-o".to_string());
            args.push("PubkeyAuthentication=no".to_string());
            args.push("-o".to_string());
            args.push("PreferredAuthentications=keyboard-interactive,password".to_string());
        }
        SshAuthKind::IdentityFile => {
            args.push("-o".to_string());
            args.push("IdentitiesOnly=yes".to_string());
            if let Some(ref key_path) = profile.identity_file {
                let s = key_path.to_string_lossy();
                if !s.trim().is_empty() {
                    let expanded = expand_tilde_path(Path::new(s.trim()));
                    args.push("-i".to_string());
                    args.push(expanded.to_string_lossy().into_owned());
                }
            }
        }
        SshAuthKind::AgentOrDefault => {
            if let Some(ref key_path) = profile.identity_file {
                let s = key_path.to_string_lossy();
                if !s.trim().is_empty() {
                    let expanded = expand_tilde_path(Path::new(s.trim()));
                    args.push("-i".to_string());
                    args.push(expanded.to_string_lossy().into_owned());
                }
            }
        }
    }

    if profile.use_control_master
        && let Ok(runtime_dir) = ssh_runtime_dir()
    {
        let cm_path = runtime_dir.join("cm-%C");
        if cm_path.as_os_str().len() < 88 {
            args.push("-o".to_string());
            args.push("ControlMaster=auto".to_string());
            args.push("-o".to_string());
            args.push(format!("ControlPath={}", cm_path.to_string_lossy()));
            args.push("-o".to_string());
            args.push("ControlPersist=60s".to_string());
            args.push("-o".to_string());
            args.push("ServerAliveInterval=15".to_string());
            args.push("-o".to_string());
            args.push("ServerAliveCountMax=3".to_string());
        }
    }

    if let Some(port) = profile.port {
        args.push("-p".to_string());
        args.push(port.to_string());
    }

    args.push("--".to_string());
    args.push(destination);

    if !profile.bypass_multiplexer_probe {
        let script = build_remote_bootstrap_script(&profile.multiplexer_priority);
        args.push(format!("sh -c '{script}'"));
    }

    let mut envs = Vec::new();
    if profile.auth_kind != SshAuthKind::AgentOrDefault
        && let Some((sock_path, nonce)) = attempt
    {
        let exe = std::env::current_exe()
            .map_err(|e| format!("Failed to locate current executable for SSH_ASKPASS: {e}"))?;
        let display = std::env::var("DISPLAY")
            .ok()
            .filter(|d| !d.trim().is_empty())
            .unwrap_or_else(|| ":0".to_string());
        envs.push((
            "SSH_ASKPASS".to_string(),
            exe.to_string_lossy().into_owned(),
        ));
        envs.push(("SSH_ASKPASS_REQUIRE".to_string(), "force".to_string()));
        envs.push((
            "WHATTY_SSH_ASKPASS_SOCKET".to_string(),
            sock_path.to_string_lossy().into_owned(),
        ));
        envs.push(("WHATTY_SSH_ASKPASS_NONCE".to_string(), nonce.to_string()));
        envs.push(("DISPLAY".to_string(), display));
    }

    Ok(("ssh".to_string(), args, envs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_multiplexer_order;

    fn sample_profile() -> SshProfile {
        SshProfile {
            id: "11111111-2222-4333-8444-555555555555".into(),
            name: "Prod API".into(),
            host: "10.0.1.50".into(),
            user: Some("deploy".into()),
            port: Some(2222),
            auth_kind: SshAuthKind::PasswordKeyring,
            identity_file: None,
            multiplexer_priority: default_multiplexer_order(),
            bypass_multiplexer_probe: false,
            use_control_master: false,
        }
    }

    #[test]
    fn test_build_ssh_invocation_rejects_leading_dash_host_and_user() {
        let mut p = sample_profile();
        p.host = "-oProxyCommand=id".into();
        assert!(build_ssh_invocation(&p, None).is_err());

        p = sample_profile();
        p.user = Some("-oProxyCommand=id".into());
        assert!(build_ssh_invocation(&p, None).is_err());

        p = sample_profile();
        p.user = Some("bad user".into());
        assert!(build_ssh_invocation(&p, None).is_err());

        p = sample_profile();
        p.port = Some(0);
        assert!(build_ssh_invocation(&p, None).is_err());
    }

    #[test]
    fn test_build_ssh_invocation_places_double_dash_separator() {
        let p = sample_profile();
        let (prog, args, envs) =
            build_ssh_invocation(&p, Some((Path::new("/tmp/test.sock"), "deadbeef"))).unwrap();
        assert_eq!(prog, "ssh");
        let dd_pos = args.iter().position(|a| a == "--").expect("missing --");
        assert_eq!(args[dd_pos + 1], "deploy@10.0.1.50");
        assert!(args[dd_pos + 2].starts_with("sh -c '"));
        assert!(envs.iter().any(|(k, _)| k == "SSH_ASKPASS_REQUIRE"));
    }

    #[test]
    fn test_build_ssh_invocation_omits_remote_command_when_bypass_enabled() {
        let mut p = sample_profile();
        p.bypass_multiplexer_probe = true;
        let (_prog, args, _envs) = build_ssh_invocation(&p, None).unwrap();
        let dd_pos = args.iter().position(|a| a == "--").unwrap();
        assert_eq!(args.len(), dd_pos + 2);
        assert_eq!(args.last().unwrap(), "deploy@10.0.1.50");
    }

    #[test]
    fn test_build_ssh_invocation_constrains_auth_and_omits_agent_askpass() {
        let mut p = sample_profile();
        p.auth_kind = SshAuthKind::AgentOrDefault;
        let (_prog, args, envs) =
            build_ssh_invocation(&p, Some((Path::new("/tmp/s"), "nonce"))).unwrap();
        assert!(envs.is_empty());
        assert!(!args.iter().any(|a| a.contains("PubkeyAuthentication=no")));

        p.auth_kind = SshAuthKind::PasswordKeyring;
        let (_prog, args2, envs2) =
            build_ssh_invocation(&p, Some((Path::new("/tmp/s"), "nonce"))).unwrap();
        assert!(args2.iter().any(|a| a == "PubkeyAuthentication=no"));
        assert!(!envs2.is_empty());

        p.auth_kind = SshAuthKind::IdentityFile;
        p.identity_file = Some(PathBuf::from("~/.ssh/id_test"));
        let (_prog, args3, envs3) =
            build_ssh_invocation(&p, Some((Path::new("/tmp/s"), "nonce"))).unwrap();
        assert!(args3.iter().any(|a| a == "IdentitiesOnly=yes"));
        assert!(args3.iter().any(|a| a == "-i"));
        assert!(!envs3.is_empty());
    }

    #[test]
    fn test_remote_bootstrap_script_tmux_then_screen_order() {
        let script =
            build_remote_bootstrap_script(&[MultiplexerKind::Tmux, MultiplexerKind::Screen]);
        assert!(script.contains("for m in tmux screen;"));
        assert!(script.contains("tmux attach-session -t \"=$target\""));
        assert!(script.contains("screen -d -r \"$target\""));
    }

    #[test]
    fn test_remote_bootstrap_script_screen_then_tmux_order() {
        let script =
            build_remote_bootstrap_script(&[MultiplexerKind::Screen, MultiplexerKind::Tmux]);
        assert!(script.contains("for m in screen tmux;"));

        let empty_script = build_remote_bootstrap_script(&[MultiplexerKind::Unknown]);
        assert_eq!(empty_script, "exec \"${SHELL:-/bin/sh}\" -l");
    }

    #[test]
    fn test_remote_bootstrap_script_single_line_quote_free_invariant() {
        let script =
            build_remote_bootstrap_script(&[MultiplexerKind::Tmux, MultiplexerKind::Screen]);
        assert!(
            !script.contains('\''),
            "bootstrap must not contain single quotes"
        );
        assert!(
            !script.contains('\\'),
            "bootstrap must not contain backslashes"
        );
        assert!(
            !script.contains('\n'),
            "bootstrap must not contain newlines"
        );

        let status = std::process::Command::new("/bin/sh")
            .arg("-n")
            .arg("-c")
            .arg(&script)
            .status()
            .expect("invoke /bin/sh -n");
        assert!(
            status.success(),
            "POSIX syntax check failed for script: {script}"
        );
    }

    #[test]
    fn test_classify_askpass_prompt_first_password_vs_retry_and_confirm() {
        assert_eq!(
            classify_askpass_prompt(
                SshAuthKind::PasswordKeyring,
                "deploy@10.0.1.50's password:",
                "",
                0
            ),
            AskpassPromptClassification::KeyringLookup {
                secret_kind: "password"
            }
        );
        assert_eq!(
            classify_askpass_prompt(
                SshAuthKind::PasswordKeyring,
                "deploy@10.0.1.50's password:",
                "",
                1
            ),
            AskpassPromptClassification::SecretDialog
        );
        assert_eq!(
            classify_askpass_prompt(
                SshAuthKind::IdentityFile,
                "Enter passphrase for key '/home/u/.ssh/id_ed25519':",
                "",
                0
            ),
            AskpassPromptClassification::KeyringLookup {
                secret_kind: "passphrase"
            }
        );
        // PasswordKeyring profile must not send server password to a local key passphrase prompt
        assert_eq!(
            classify_askpass_prompt(
                SshAuthKind::PasswordKeyring,
                "Enter passphrase for key '/home/u/.ssh/id_ed25519':",
                "",
                0
            ),
            AskpassPromptClassification::SecretDialog
        );
        assert_eq!(
            classify_askpass_prompt(
                SshAuthKind::PasswordKeyring,
                "Are you sure you want to continue connecting (yes/no/[fingerprint])?",
                "confirm",
                0
            ),
            AskpassPromptClassification::ConfirmDialog {
                expects_fingerprint: true
            }
        );
        assert_eq!(
            classify_askpass_prompt(SshAuthKind::PasswordKeyring, "Banner notice", "none", 0),
            AskpassPromptClassification::InfoDialog
        );
    }

    #[test]
    fn test_askpass_broker_rejects_invalid_nonce_over_socket() {
        let state = Arc::new(Mutex::new(BrokerState {
            socket_path: PathBuf::from("/tmp/unused.sock"),
            attempts: HashMap::new(),
        }));

        // Register a valid attempt manually for pure headless socket evaluation
        let (dummy_app, dummy_tx) = (state.clone(), "valid-nonce-123".to_string());
        // Insert via AttemptRecord isn't needed for unknown nonce rejection:
        let bad_req = AskpassRequest {
            nonce: "unknown-nonce".into(),
            prompt: "deploy@host's password:".into(),
            prompt_hint: String::new(),
        };
        let res = process_request_with_lookup(&dummy_app, &bad_req, |_, _| Ok(Some("pw".into())));
        assert!(matches!(res, SocketEvalOutcome::Reject));

        // Now verify full UnixStream pair peer cred + JSON protocol round-trip on invalid nonce
        let (mut server_stream, client_stream) = UnixStream::pair().unwrap();
        verify_peer_cred(&server_stream).unwrap();

        let t = std::thread::spawn(move || {
            let mut cs = client_stream;
            writeln!(
                cs,
                "{}",
                serde_json::to_string(&AskpassRequest {
                    nonce: dummy_tx,
                    prompt: "Password:".into(),
                    prompt_hint: String::new(),
                })
                .unwrap()
            )
            .unwrap();
            cs.flush().unwrap();
            let mut line = String::new();
            BufReader::new(cs).read_line(&mut line).unwrap();
            serde_json::from_str::<AskpassResponse>(&line).unwrap()
        });

        let parsed = read_askpass_request(&server_stream).unwrap();
        let eval = process_request_with_lookup(&state, &parsed, |_, _| Ok(Some("pw".into())));
        assert!(matches!(eval, SocketEvalOutcome::Reject));
        send_askpass_response(
            &mut server_stream,
            &AskpassResponse {
                ok: false,
                secret: String::new(),
            },
        );
        let client_resp = t.join().unwrap();
        assert!(!client_resp.ok);

        // Verify Case 1 lookup hit on 1st attempt and modal fallback on 2nd attempt
        let nonce_ok = "nonce-valid-case1".to_string();
        state.lock().unwrap().attempts.insert(
            nonce_ok.clone(),
            AttemptRecord {
                profile_id: "p-1".into(),
                profile_name: "Prod".into(),
                auth_kind: SshAuthKind::PasswordKeyring,
                window: glib::SendWeakRef::from(glib::WeakRef::new()),
                password_attempts: 0,
                created_at: Instant::now(),
            },
        );
        let first_req = AskpassRequest {
            nonce: nonce_ok.clone(),
            prompt: "deploy@host's password:".into(),
            prompt_hint: String::new(),
        };
        let first_res = process_request_with_lookup(&state, &first_req, |pid, kind| {
            assert_eq!(pid, "p-1");
            assert_eq!(kind, "password");
            Ok(Some("hunter2".into()))
        });
        assert!(matches!(first_res, SocketEvalOutcome::ReplyOk(ref s) if s == "hunter2"));

        let retry_res = process_request_with_lookup(&state, &first_req, |_, _| {
            panic!("retry must not re-query keyring automatically");
        });
        assert!(matches!(
            retry_res,
            SocketEvalOutcome::NeedModal {
                classification: AskpassPromptClassification::SecretDialog,
                ..
            }
        ));
    }

    #[test]
    fn test_askpass_client_arg_extraction() {
        assert_eq!(
            extract_askpass_prompt_arg(vec!["whatty".into(), "deploy@host's password:".into(),]),
            "deploy@host's password:"
        );
        assert_eq!(
            extract_askpass_prompt_arg(vec![
                "whatty".into(),
                "--ssh-askpass".into(),
                "Verification code:".into(),
            ]),
            "Verification code:"
        );
    }
}
