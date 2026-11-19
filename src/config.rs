use serde::{Deserialize, Serialize};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{collections::HashMap, fs, io::{self, Write}};
use dirs_next::config_dir;

/// Remote terminal multiplexers supported for SSH bootstrap discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MultiplexerKind {
    Tmux,
    Screen,
    #[serde(other)]
    Unknown,
}

/// Default multiplexer discovery priority order (`tmux` first, then `screen`).
pub fn default_multiplexer_order() -> Vec<MultiplexerKind> {
    vec![MultiplexerKind::Tmux, MultiplexerKind::Screen]
}

/// Authentication mechanism for a saved SSH profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SshAuthKind {
    #[default]
    AgentOrDefault,
    PasswordKeyring,
    IdentityFile,
}

/// Non-secret metadata for a saved SSH connection profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshProfile {
    pub id: String,
    pub name: String,
    pub host: String,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub auth_kind: SshAuthKind,
    #[serde(default)]
    pub identity_file: Option<PathBuf>,
    #[serde(default = "default_multiplexer_order")]
    pub multiplexer_priority: Vec<MultiplexerKind>,
    #[serde(default)]
    pub bypass_multiplexer_probe: bool,
    #[serde(default)]
    pub use_control_master: bool,
}

/// Possible tab orientations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TabOrientation {
    Horizontal,
    Vertical,
}

/// Supported GPU back‑ends.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuBackend {
    Vulkan,
    OpenGl,
    Software,
}

/// Persisted state for an open terminal tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedTab {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
    #[serde(default)]
    pub custom_title: bool,
}

/// Top‑level configuration structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Tab layout preferred by the user.
    #[serde(default = "default_orientation")]
    pub tab_orientation: TabOrientation,

    /// Desired GPU backend (user‑override). If omitted we auto‑detect.
    #[serde(default)]
    pub gpu_backend: Option<GpuBackend>,

    /// Currently selected colour theme (VS Code theme id).
    #[serde(default)]
    pub theme_id: Option<String>,

    /// Currently selected theme display name (e.g. "Dracula").
    #[serde(default)]
    pub theme_name: Option<String>,

    /// Persisted colour palette map.
    #[serde(default)]
    pub theme_colors: Option<HashMap<String, String>>,

    /// Configured font family (e.g. "Noto Sans Mono", "Liberation Mono", "Hack", etc.).
    #[serde(default)]
    pub font_family: Option<String>,

    /// Configured font size (pt).
    #[serde(default = "default_font_size")]
    pub font_size: f32,

    /// Open tabs persisted from previous session.
    #[serde(default)]
    pub open_tabs: Vec<SavedTab>,

    /// Active tab index persisted from previous session.
    #[serde(default)]
    pub active_tab_index: Option<usize>,

    /// Window opacity level (0.20 to 1.0).
    #[serde(default = "default_opacity")]
    pub window_opacity: f32,

    /// Allow adjusting opacity with Ctrl + mouse scroll wheel.
    #[serde(default = "default_true")]
    pub ctrl_scroll_opacity: bool,

    /// Experimental: allow opacity/saturation slider to exceed 100% and supersaturate colors.
    #[serde(default = "default_false")]
    pub experimental_supersaturation: bool,

    /// Color saturation boost factor (1.0 to 2.0).
    #[serde(default = "default_saturation")]
    pub color_saturation: f32,

    /// Double-click tab header closes tab.
    #[serde(default = "default_true")]
    pub double_click_close_tab: bool,

    /// Selected application icon variant ("modern" or "classic").
    #[serde(default = "default_app_icon")]
    pub app_icon: String,

    /// Preferred default shell executable path (if `None`, uses `$SHELL` / `/bin/sh`).
    #[serde(default)]
    pub default_shell: Option<String>,

    /// Saved remote SSH session profiles (passwords/passphrases stay in OS keyring).
    #[serde(default)]
    pub ssh_profiles: Vec<SshProfile>,
}

fn default_orientation() -> TabOrientation {
    TabOrientation::Horizontal
}

fn default_font_size() -> f32 {
    14.0
}

fn default_opacity() -> f32 {
    1.0
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_saturation() -> f32 {
    1.0
}

fn default_app_icon() -> String {
    "modern".to_string()
}

impl Config {
    pub fn app_icon_name(&self) -> &'static str {
        if self.app_icon.eq_ignore_ascii_case("classic") {
            "io.github.whattytheshell-classic"
        } else {
            "io.github.whattytheshell"
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            tab_orientation: default_orientation(),
            gpu_backend: None,
            theme_id: None,
            theme_name: None,
            theme_colors: None,
            font_family: None,
            font_size: default_font_size(),
            open_tabs: Vec::new(),
            active_tab_index: None,
            window_opacity: default_opacity(),
            ctrl_scroll_opacity: default_true(),
            experimental_supersaturation: default_false(),
            color_saturation: default_saturation(),
            double_click_close_tab: default_true(),
            app_icon: default_app_icon(),
            default_shell: None,
            ssh_profiles: Vec::new(),
        }
    }
}

fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

fn backup_corrupt_bytes(config_dir: &Path, raw: &[u8]) -> io::Result<PathBuf> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let pid = std::process::id();
    for suffix in 0..100u32 {
        let filename = if suffix == 0 {
            format!("config.toml.corrupt.{ts}-{pid}")
        } else {
            format!("config.toml.corrupt.{ts}-{pid}-{suffix}")
        };
        let backup_path = config_dir.join(filename);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&backup_path)
        {
            Ok(mut f) => {
                f.write_all(raw)?;
                f.sync_all()?;
                return Ok(backup_path);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Exhausted corrupt config backup filenames",
    ))
}

#[derive(Clone)]
pub struct ConfigManager {
    pub config: Config,
    /// Full path to the TOML file.
    path: PathBuf,
    /// True after loading an unparseable non-empty config until an explicit user save.
    pub corrupt_backup_pending: bool,
}

impl ConfigManager {
    /// Load the config from the standard location or create a fresh one.
    pub fn load_or_default() -> Self {
        let config_dir = config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("WhattyTheShell!");
        Self::load_or_default_in(&config_dir)
    }

    /// Load or initialize configuration inside an explicit directory (enforces `0700` dir mode
    /// and backs up corrupt non-empty `config.toml` files without overwriting them automatically).
    pub fn load_or_default_in(config_dir: &Path) -> Self {
        let _ = ensure_private_dir(config_dir);
        let config_path = config_dir.join("config.toml");

        let (config, should_save, corrupt_backup_pending) = if config_path.exists() {
            match fs::read(&config_path) {
                Ok(raw) => {
                    if raw.is_empty() {
                        (Config::default(), false, false)
                    } else {
                        match std::str::from_utf8(&raw)
                            .ok()
                            .and_then(|txt| toml::from_str::<Config>(txt).ok())
                        {
                            Some(parsed) => (parsed, false, false),
                            None => {
                                match backup_corrupt_bytes(config_dir, &raw) {
                                    Ok(backup) => eprintln!(
                                        "Failed to parse {}; backed up corrupt file to {}",
                                        config_path.display(),
                                        backup.display()
                                    ),
                                    Err(err) => eprintln!(
                                        "Failed to parse {} and could not create backup: {}",
                                        config_path.display(),
                                        err
                                    ),
                                }
                                (Config::default(), false, true)
                            }
                        }
                    }
                }
                Err(err) => {
                    eprintln!("Failed to read {}: {}", config_path.display(), err);
                    (Config::default(), false, true)
                }
            }
        } else {
            (Config::default(), true, false)
        };

        let mut manager = ConfigManager {
            config,
            path: config_path,
            corrupt_backup_pending,
        };
        if should_save {
            let _ = manager.save();
        }
        manager
    }

    /// Atomically persist the current configuration with `0600` file permissions.
    pub fn save(&mut self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            ensure_private_dir(parent)?;
        }
        let toml_text = toml::to_string_pretty(&self.config)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let tmp_path = self.path.with_file_name("config.toml.tmp");
        let _ = fs::remove_file(&tmp_path);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp_path)?;
        file.write_all(toml_text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp_path, &self.path)?;
        self.corrupt_backup_pending = false;
        Ok(())
    }

    /// Explicit user save that clears any corrupt-config auto-save guard.
    pub fn save_user_config(&mut self) -> io::Result<()> {
        self.corrupt_backup_pending = false;
        self.save()
    }

    pub fn tab_orientation(&self) -> TabOrientation {
        self.config.tab_orientation
    }

    pub fn set_orientation(&mut self, orientation: TabOrientation) {
        self.config.tab_orientation = orientation;
        let _ = self.save_user_config();
    }

    pub fn gpu_backend_preference(&self) -> Option<GpuBackend> {
        self.config.gpu_backend.clone()
    }

    #[allow(dead_code)]
    pub fn theme_id(&self) -> Option<String> {
        self.config.theme_id.clone()
    }

    #[allow(dead_code)]
    pub fn theme_name(&self) -> Option<String> {
        self.config.theme_name.clone()
    }

    pub fn theme_colors(&self) -> Option<HashMap<String, String>> {
        self.config.theme_colors.clone()
    }

    pub fn set_theme(&mut self, name: Option<String>, colors: HashMap<String, String>) {
        self.config.theme_name = name;
        self.config.theme_colors = Some(colors);
        let _ = self.save_user_config();
    }

    pub fn font_family(&self) -> Option<String> {
        self.config.font_family.clone()
    }

    pub fn font_size(&self) -> f32 {
        self.config.font_size
    }

    pub fn set_font(&mut self, family: Option<String>, size: f32) {
        self.config.font_family = family;
        self.config.font_size = size;
        let _ = self.save_user_config();
    }

    pub fn open_tabs(&self) -> Vec<SavedTab> {
        self.config.open_tabs.clone()
    }

    pub fn set_open_tabs(&mut self, tabs: Vec<SavedTab>) {
        self.config.open_tabs = tabs;
        if !self.corrupt_backup_pending {
            let _ = self.save();
        }
    }

    pub fn active_tab_index(&self) -> Option<usize> {
        self.config.active_tab_index
    }

    pub fn set_active_tab_index(&mut self, idx: Option<usize>) {
        self.config.active_tab_index = idx;
        if !self.corrupt_backup_pending {
            let _ = self.save();
        }
    }

    pub fn window_opacity(&self) -> f32 {
        self.config.window_opacity
    }

    pub fn set_window_opacity(&mut self, opacity: f32) {
        self.config.window_opacity = opacity.clamp(0.20, 1.0);
        let _ = self.save_user_config();
    }

    pub fn ctrl_scroll_opacity(&self) -> bool {
        self.config.ctrl_scroll_opacity
    }

    pub fn set_ctrl_scroll_opacity(&mut self, val: bool) {
        self.config.ctrl_scroll_opacity = val;
        let _ = self.save_user_config();
    }

    pub fn experimental_supersaturation(&self) -> bool {
        self.config.experimental_supersaturation
    }

    pub fn set_experimental_supersaturation(&mut self, val: bool) {
        self.config.experimental_supersaturation = val;
        let _ = self.save_user_config();
    }

    pub fn color_saturation(&self) -> f32 {
        self.config.color_saturation
    }

    pub fn set_color_saturation(&mut self, val: f32) {
        self.config.color_saturation = val.clamp(1.0, 2.0);
        let _ = self.save_user_config();
    }

    pub fn double_click_close_tab(&self) -> bool {
        self.config.double_click_close_tab
    }

    pub fn set_double_click_close_tab(&mut self, val: bool) {
        self.config.double_click_close_tab = val;
        let _ = self.save_user_config();
    }

    pub fn app_icon(&self) -> String {
        self.config.app_icon.clone()
    }

    pub fn set_app_icon(&mut self, icon: String) {
        self.config.app_icon = icon;
        let _ = self.save_user_config();
    }

    pub fn default_shell(&self) -> Option<String> {
        self.config.default_shell.clone()
    }

    pub fn set_default_shell(&mut self, shell: Option<String>) {
        self.config.default_shell = shell.and_then(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });
        let _ = self.save_user_config();
    }

    /// Return all saved SSH profiles in configuration order.
    pub fn ssh_profiles(&self) -> Vec<SshProfile> {
        self.config.ssh_profiles.clone()
    }

    /// Look up a saved SSH profile by its immutable UUID.
    pub fn get_ssh_profile(&self, id: &str) -> Option<SshProfile> {
        self.config.ssh_profiles.iter().find(|p| p.id == id).cloned()
    }

    /// Insert or update an SSH profile by `id` and atomically persist config.
    pub fn upsert_ssh_profile(&mut self, profile: SshProfile) -> io::Result<()> {
        if let Some(existing) = self.config.ssh_profiles.iter_mut().find(|p| p.id == profile.id) {
            *existing = profile;
        } else {
            self.config.ssh_profiles.push(profile);
        }
        self.save_user_config()
    }

    /// Remove a saved SSH profile by `id` and persist if found.
    pub fn remove_ssh_profile(&mut self, id: &str) -> io::Result<Option<SshProfile>> {
        if let Some(pos) = self.config.ssh_profiles.iter().position(|p| p.id == id) {
            let removed = self.config.ssh_profiles.remove(pos);
            self.save_user_config()?;
            Ok(Some(removed))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_serialization_with_tabs() {
        let mut cfg = Config::default();
        cfg.open_tabs.push(SavedTab {
            title: Some("Terminal 1".into()),
            working_dir: Some(PathBuf::from("/home/user/project")),
            custom_title: true,
        });
        cfg.font_family = Some("Fira Code".into());
        cfg.font_size = 16.0;
        cfg.app_icon = "classic".into();

        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.open_tabs.len(), 1);
        assert_eq!(parsed.open_tabs[0].working_dir, Some(PathBuf::from("/home/user/project")));
        assert!(parsed.open_tabs[0].custom_title);
        assert_eq!(parsed.font_family, Some("Fira Code".into()));
        assert_eq!(parsed.font_size, 16.0);
        assert_eq!(parsed.app_icon, "classic");
        assert_eq!(parsed.app_icon_name(), "io.github.whattytheshell-classic");
    }

    #[test]
    fn test_config_window_and_ergonomics_options() {
        let mut cfg = Config::default();
        assert_eq!(cfg.window_opacity, 1.0);
        assert!(cfg.ctrl_scroll_opacity);
        assert!(!cfg.experimental_supersaturation);
        assert_eq!(cfg.color_saturation, 1.0);
        assert!(cfg.double_click_close_tab);
        assert_eq!(cfg.app_icon, "modern");
        assert_eq!(cfg.app_icon_name(), "io.github.whattytheshell");
        assert_eq!(cfg.default_shell, None);

        cfg.active_tab_index = Some(2);
        cfg.window_opacity = 0.85;
        cfg.ctrl_scroll_opacity = false;
        cfg.experimental_supersaturation = true;
        cfg.color_saturation = 1.35;
        cfg.double_click_close_tab = false;
        cfg.app_icon = "classic".into();
        cfg.default_shell = Some("/usr/bin/fish".into());

        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.active_tab_index, Some(2));
        assert_eq!(parsed.window_opacity, 0.85);
        assert!(!parsed.ctrl_scroll_opacity);
        assert!(parsed.experimental_supersaturation);
        assert_eq!(parsed.color_saturation, 1.35);
        assert!(!parsed.double_click_close_tab);
        assert_eq!(parsed.app_icon, "classic");
        assert_eq!(parsed.default_shell.as_deref(), Some("/usr/bin/fish"));
    }

    #[test]
    fn test_ssh_profile_serialization_roundtrip() {
        let mut cfg = Config::default();
        cfg.ssh_profiles.push(SshProfile {
            id: "11111111-2222-4333-8444-555555555555".into(),
            name: "Prod API".into(),
            host: "10.0.1.50".into(),
            user: Some("deploy".into()),
            port: Some(2222),
            auth_kind: SshAuthKind::PasswordKeyring,
            identity_file: Some(PathBuf::from("~/.ssh/id_ed25519")),
            multiplexer_priority: vec![MultiplexerKind::Screen, MultiplexerKind::Tmux],
            bypass_multiplexer_probe: true,
            use_control_master: true,
        });

        let encoded = toml::to_string_pretty(&cfg).unwrap();
        let decoded: Config = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded.ssh_profiles, cfg.ssh_profiles);
        assert!(!decoded.ssh_profiles[0].multiplexer_priority.is_empty());
    }

    #[test]
    fn test_unknown_multiplexer_kind_deserializes_gracefully() {
        let raw = r#"
[[ssh_profiles]]
id = "abc"
name = "Lab"
host = "lab.example.org"
multiplexer_priority = ["zellij", "tmux", "screen"]
"#;
        let parsed: Config = toml::from_str(raw).unwrap();
        assert_eq!(parsed.ssh_profiles.len(), 1);
        assert_eq!(
            parsed.ssh_profiles[0].multiplexer_priority,
            vec![
                MultiplexerKind::Unknown,
                MultiplexerKind::Tmux,
                MultiplexerKind::Screen,
            ]
        );
        assert!(!parsed.ssh_profiles[0].use_control_master);
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "whatty_test_{label}_{}_{nanos}",
            std::process::id()
        ))
    }

    #[test]
    fn test_atomic_save_permissions_0600() {
        let dir = unique_temp_dir("atomic0600");
        let _ = fs::remove_dir_all(&dir);

        let mut mgr = ConfigManager::load_or_default_in(&dir);
        let dir_mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);

        mgr.upsert_ssh_profile(SshProfile {
            id: "id-1".into(),
            name: "Dev".into(),
            host: "127.0.0.1".into(),
            user: None,
            port: None,
            auth_kind: SshAuthKind::AgentOrDefault,
            identity_file: None,
            multiplexer_priority: default_multiplexer_order(),
            bypass_multiplexer_probe: false,
            use_control_master: false,
        })
        .unwrap();

        let cfg_file = dir.join("config.toml");
        assert!(cfg_file.exists());
        assert!(!dir.join("config.toml.tmp").exists());
        let file_mode = fs::metadata(&cfg_file).unwrap().permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600);

        let reloaded = ConfigManager::load_or_default_in(&dir);
        assert_eq!(reloaded.ssh_profiles().len(), 1);
        assert_eq!(reloaded.get_ssh_profile("id-1").unwrap().host, "127.0.0.1");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_corrupt_config_creates_backup_and_blocks_autosave() {
        let dir = unique_temp_dir("corrupt");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let cfg_file = dir.join("config.toml");
        let corrupt_payload = b"[[ssh_profiles\nbroken = [1, 2\n";
        fs::write(&cfg_file, corrupt_payload).unwrap();

        let mut mgr = ConfigManager::load_or_default_in(&dir);
        assert!(mgr.corrupt_backup_pending);
        assert!(mgr.ssh_profiles().is_empty());

        // Original corrupt config.toml must not be overwritten by shutdown tab autosave.
        mgr.set_open_tabs(vec![SavedTab {
            title: Some("Local".into()),
            working_dir: Some(PathBuf::from("/tmp")),
            custom_title: false,
        }]);
        assert_eq!(fs::read(&cfg_file).unwrap(), corrupt_payload);

        let backups: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("config.toml.corrupt.")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        let backup_mode = backups[0].metadata().unwrap().permissions().mode() & 0o777;
        assert_eq!(backup_mode, 0o600);
        assert_eq!(fs::read(backups[0].path()).unwrap(), corrupt_payload);

        // Explicit user save clears corrupt_backup_pending and updates config.toml cleanly.
        mgr.save_user_config().unwrap();
        assert!(!mgr.corrupt_backup_pending);
        let reloaded = ConfigManager::load_or_default_in(&dir);
        assert_eq!(reloaded.open_tabs().len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }
}
