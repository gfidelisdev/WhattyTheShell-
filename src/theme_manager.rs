// src/theme_manager.rs
//! Minimal theme manager that can download a VS Code colour theme JSON
//! from the Marketplace (or load a local file) and apply its colour palette
//! to a GTK window via a CSS provider.

use gtk4::{prelude::*, CssProvider, Widget};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::collections::HashMap;

use crate::config::ConfigManager;

/// Only the colour palette portion of a VS Code theme JSON is needed.
#[derive(Debug, Deserialize)]
struct VsCodeTheme {
    #[serde(rename = "colors")]
    colors: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ThemeManager {
    current_theme_id: Option<String>,
    css_provider: Option<CssProvider>,
}
impl ThemeManager {
    /// Initialise from configuration. No automatic loading – the caller may
    /// invoke `set_theme` later.
    pub fn new(_cfg: &ConfigManager) -> Self {
        ThemeManager {
            current_theme_id: None,
            css_provider: None,
        }
    }

    /// Apply the loaded CSS to a top‑level GTK window.
    pub fn apply_to_window(&mut self, window: &impl IsA<Widget>) {
        // Ensure CSS provider is created after GTK init.
        let provider = self.css_provider.get_or_insert_with(CssProvider::new);
        gtk4::style_context_add_provider_for_display(
            &window.as_ref().display(),
            provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    /// Load a theme (by marketplace ID) and install its colours.
    fn load_theme(&mut self, theme_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Simplified: assume raw JSON hosted on GitHub.
        let url = format!("https://raw.githubusercontent.com/{}/master/theme.json", theme_id);
        let client = Client::new();
        let resp = client.get(&url).send()?.error_for_status()?;
        let theme: VsCodeTheme = resp.json()?;
        self.apply_palette(&theme.colors);
        self.current_theme_id = Some(theme_id.to_string());
        Ok(())
    }

    /// Convert the colour map into CSS custom properties.
    pub fn apply_palette(&mut self, colors: &HashMap<String, String>) {
        let mut css = String::new();
        for (key, value) in colors {
            let var = match key.as_str() {
                "editor.background" => "--gtktmux-bg",
                "editor.foreground" => "--gtktmux-fg",
                "terminal.ansiBlack" => "--gtktmux-ansi0",
                "terminal.ansiRed" => "--gtktmux-ansi1",
                "terminal.ansiGreen" => "--gtktmux-ansi2",
                "terminal.ansiYellow" => "--gtktmux-ansi3",
                "terminal.ansiBlue" => "--gtktmux-ansi4",
                "terminal.ansiMagenta" => "--gtktmux-ansi5",
                "terminal.ansiCyan" => "--gtktmux-ansi6",
                "terminal.ansiWhite" => "--gtktmux-ansi7",
                _ => continue,
            };
            css.push_str(&format!(":root {{ {}: {}; }}\n", var, value));
        }
        let provider = self.css_provider.get_or_insert_with(|| CssProvider::new());
        provider.load_from_string(&css);
    }

    /// Public method to change the theme at runtime.
    #[allow(dead_code)]
    pub fn set_theme(&mut self, theme_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.load_theme(theme_id)
    }
}
