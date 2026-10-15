use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, io::{self, Write}, path::PathBuf};
use dirs_next::config_dir;

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
        }
    }
}

#[derive(Clone)]
pub struct ConfigManager {
    pub config: Config,
    /// Full path to the TOML file.
    path: PathBuf,
}

impl ConfigManager {
    /// Load the config from the standard location or create a fresh one.
    pub fn load_or_default() -> Self {
        let config_dir = config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("WhattyTheShell!");
        let config_path = config_dir.join("config.toml");
        let _ = fs::create_dir_all(&config_dir);
        let (config, should_save) = if config_path.exists() {
            let data = fs::read_to_string(&config_path).unwrap_or_default();
            (toml::from_str(&data).unwrap_or_default(), false)
        } else {
            (Config::default(), true)
        };
        let manager = ConfigManager { config, path: config_path };
        if should_save {
            let _ = manager.save();
        }
        manager
    }

    /// Persist the current configuration back to disk.
    pub fn save(&self) -> io::Result<()> {
        let toml = toml::to_string_pretty(&self.config).expect("serialisation failed");
        let mut file = fs::File::create(&self.path)?;
        file.write_all(toml.as_bytes())
    }

    pub fn tab_orientation(&self) -> TabOrientation {
        self.config.tab_orientation
    }

    pub fn set_orientation(&mut self, orientation: TabOrientation) {
        self.config.tab_orientation = orientation;
        let _ = self.save();
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
        let _ = self.save();
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
        let _ = self.save();
    }

    pub fn open_tabs(&self) -> Vec<SavedTab> {
        self.config.open_tabs.clone()
    }

    pub fn set_open_tabs(&mut self, tabs: Vec<SavedTab>) {
        self.config.open_tabs = tabs;
        let _ = self.save();
    }

    pub fn active_tab_index(&self) -> Option<usize> {
        self.config.active_tab_index
    }

    pub fn set_active_tab_index(&mut self, idx: Option<usize>) {
        self.config.active_tab_index = idx;
        let _ = self.save();
    }

    pub fn window_opacity(&self) -> f32 {
        self.config.window_opacity
    }

    pub fn set_window_opacity(&mut self, opacity: f32) {
        self.config.window_opacity = opacity.clamp(0.20, 1.0);
        let _ = self.save();
    }

    pub fn ctrl_scroll_opacity(&self) -> bool {
        self.config.ctrl_scroll_opacity
    }

    pub fn set_ctrl_scroll_opacity(&mut self, val: bool) {
        self.config.ctrl_scroll_opacity = val;
        let _ = self.save();
    }

    pub fn experimental_supersaturation(&self) -> bool {
        self.config.experimental_supersaturation
    }

    pub fn set_experimental_supersaturation(&mut self, val: bool) {
        self.config.experimental_supersaturation = val;
        let _ = self.save();
    }

    pub fn color_saturation(&self) -> f32 {
        self.config.color_saturation
    }

    pub fn set_color_saturation(&mut self, val: f32) {
        self.config.color_saturation = val.clamp(1.0, 2.0);
        let _ = self.save();
    }

    pub fn double_click_close_tab(&self) -> bool {
        self.config.double_click_close_tab
    }

    pub fn set_double_click_close_tab(&mut self, val: bool) {
        self.config.double_click_close_tab = val;
        let _ = self.save();
    }

    pub fn app_icon(&self) -> String {
        self.config.app_icon.clone()
    }

    pub fn set_app_icon(&mut self, icon: String) {
        self.config.app_icon = icon;
        let _ = self.save();
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
        });
        cfg.font_family = Some("Fira Code".into());
        cfg.font_size = 16.0;
        cfg.app_icon = "classic".into();

        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.open_tabs.len(), 1);
        assert_eq!(parsed.open_tabs[0].working_dir, Some(PathBuf::from("/home/user/project")));
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

        cfg.active_tab_index = Some(2);
        cfg.window_opacity = 0.85;
        cfg.ctrl_scroll_opacity = false;
        cfg.experimental_supersaturation = true;
        cfg.color_saturation = 1.35;
        cfg.double_click_close_tab = false;
        cfg.app_icon = "classic".into();

        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.active_tab_index, Some(2));
        assert_eq!(parsed.window_opacity, 0.85);
        assert!(!parsed.ctrl_scroll_opacity);
        assert!(parsed.experimental_supersaturation);
        assert_eq!(parsed.color_saturation, 1.35);
        assert!(!parsed.double_click_close_tab);
        assert_eq!(parsed.app_icon, "classic");
    }
}
