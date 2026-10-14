// src/theme_marketplace.rs
use reqwest::blocking::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{Cursor, Read};

#[derive(Deserialize, Debug)]
struct ExtensionQueryResponse {
    results: Vec<QueryResult>,
}

#[derive(Deserialize, Debug)]
struct QueryResult {
    extensions: Vec<Extension>,
}

#[allow(non_snake_case, dead_code)]
#[derive(Deserialize, Debug, Clone)]
pub struct Extension {
    pub extensionId: String,
    pub extensionName: String,
    pub displayName: Option<String>,
    pub publisher: Publisher,
    pub versions: Vec<ExtensionVersion>,
}

#[allow(non_snake_case, dead_code)]
#[derive(Deserialize, Debug, Clone)]
pub struct Publisher {
    pub publisherName: String,
}

#[allow(non_snake_case, dead_code)]
#[derive(Deserialize, Debug, Clone)]
pub struct ExtensionVersion {
    pub version: String,
    pub assetUri: Option<String>,
    pub fallbackAssetUri: Option<String>,
}

#[derive(Clone, Debug)]
pub enum DownloadProgress {
    Progress(f64, String),
    Done(HashMap<String, String>),
    Error(String),
}

/// Search for themes by a keyword on the VS Code Marketplace
pub fn search_themes(query: &str) -> Result<Vec<Extension>, Box<dyn std::error::Error>> {
    let client = Client::new();
    let body = serde_json::json!({
        "filters": [{
            "criteria": [
                { "filterType": 8, "value": "Microsoft.VisualStudio.Code" },
                { "filterType": 12, "value": "4096" },
                { "filterType": 5, "value": "Themes" },
                { "filterType": 10, "value": query }
            ],
            "pageNumber": 1,
            "pageSize": 20,
            "sortBy": 0,
            "sortOrder": 0
        }],
        "flags": 914
    });

    let resp = client.post("https://marketplace.visualstudio.com/_apis/public/gallery/extensionquery")
        .header("Accept", "application/json;api-version=3.0-preview.1")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()?
        .error_for_status()?;

    let res: ExtensionQueryResponse = resp.json()?;
    if let Some(result) = res.results.into_iter().next() {
        Ok(result.extensions)
    } else {
        Ok(vec![])
    }
}

fn remove_comments(json: &str) -> String {
    let mut out = String::new();
    let mut in_string = false;
    let mut in_line_comment = false;
    let mut chars = json.chars().peekable();

    while let Some(c) = chars.next() {
        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
                out.push('\n');
            }
            continue;
        }

        if c == '"' {
            in_string = !in_string;
        }

        if !in_string && c == '/' {
            if let Some(&'/') = chars.peek() {
                in_line_comment = true;
                chars.next();
                continue;
            }
        }
        
        out.push(c);
    }
    out
}

fn get_cache_path(ext_id: &str, version: &str) -> Option<std::path::PathBuf> {
    let mut dir = dirs_next::cache_dir()?;
    dir.push("WhattyTheShell!");
    dir.push("themes");
    let _ = std::fs::create_dir_all(&dir);
    let safe_id = ext_id.replace('/', "_").replace('\\', "_");
    dir.push(format!("{}_{}.json", safe_id, version));
    Some(dir)
}

/// Helper to get key hex swatches for quick preview [bg, fg, cursor, red, green, yellow, blue, magenta]
pub fn extract_key_swatches(colors: &HashMap<String, String>) -> Vec<String> {
    let bg = colors
        .get("terminal.background")
        .or_else(|| colors.get("editor.background"))
        .cloned()
        .unwrap_or_else(|| "#1e1e1e".into());
    let fg = colors
        .get("terminal.foreground")
        .or_else(|| colors.get("editor.foreground"))
        .cloned()
        .unwrap_or_else(|| "#cccccc".into());
    let cur = colors
        .get("terminalCursor.foreground")
        .or_else(|| colors.get("editorCursor.foreground"))
        .cloned()
        .unwrap_or_else(|| "#ffffff".into());
    let red = colors
        .get("terminal.ansiRed")
        .or_else(|| colors.get("terminal.ansiBrightRed"))
        .cloned()
        .unwrap_or_else(|| "#e06c75".into());
    let green = colors
        .get("terminal.ansiGreen")
        .or_else(|| colors.get("terminal.ansiBrightGreen"))
        .cloned()
        .unwrap_or_else(|| "#98c379".into());
    let yellow = colors
        .get("terminal.ansiYellow")
        .or_else(|| colors.get("terminal.ansiBrightYellow"))
        .cloned()
        .unwrap_or_else(|| "#e5c07b".into());
    let blue = colors
        .get("terminal.ansiBlue")
        .or_else(|| colors.get("terminal.ansiBrightBlue"))
        .cloned()
        .unwrap_or_else(|| "#61afef".into());
    let magenta = colors
        .get("terminal.ansiMagenta")
        .or_else(|| colors.get("terminal.ansiBrightMagenta"))
        .cloned()
        .unwrap_or_else(|| "#c678dd".into());

    vec![bg, fg, cur, red, green, yellow, blue, magenta]
}

/// Built-in popular presets for instant offline preview & application
pub fn popular_presets() -> Vec<(&'static str, &'static str, HashMap<String, String>)> {
    vec![
        ("Dracula", "dracula-theme", make_theme_map(
            "#282a36", "#f8f8f2", "#f8f8f2",
            "#21222c", "#ff5555", "#50fa7b", "#f1fa8c",
            "#bd93f9", "#ff79c6", "#8be9fd", "#f8f8f2",
        )),
        ("One Dark Pro", "binaryify", make_theme_map(
            "#282c34", "#abb2bf", "#528bff",
            "#282c34", "#e06c75", "#98c379", "#e5c07b",
            "#61afef", "#c678dd", "#56b6c2", "#abb2bf",
        )),
        ("Nord", "arcticicestudio", make_theme_map(
            "#2e3440", "#d8dee9", "#d8dee9",
            "#3b4252", "#bf616a", "#a3be8c", "#ebcb8b",
            "#81a1c1", "#b48ead", "#88c0d0", "#e5e9f0",
        )),
        ("Gruvbox Dark", "jdinhlife", make_theme_map(
            "#282828", "#ebdbb2", "#ebdbb2",
            "#282828", "#cc241d", "#98971a", "#d79921",
            "#458588", "#b16286", "#689d6a", "#a89984",
        )),
        ("Monokai", "monokai", make_theme_map(
            "#272822", "#f8f8f2", "#f8f8f0",
            "#272822", "#f92672", "#a6e22e", "#e6db74",
            "#66d9ef", "#ae81ff", "#a1efe4", "#f8f8f2",
        )),
        ("Catppuccin Mocha", "Catppuccin", make_theme_map(
            "#1e1e2e", "#cdd6f4", "#f5e0dc",
            "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af",
            "#89b4fa", "#f5c2e7", "#94e2d5", "#bac2de",
        )),
        ("Tokyo Night", "enkia", make_theme_map(
            "#1a1b26", "#a9b1d6", "#c0caf5",
            "#15161e", "#f7768e", "#9ece6a", "#e0af68",
            "#7aa2f7", "#bb9af7", "#7dcfff", "#a9b1d6",
        )),
        ("Solarized Dark", "ryanolsonx", make_theme_map(
            "#002b36", "#839496", "#93a1a1",
            "#073642", "#dc322f", "#859900", "#b58900",
            "#268bd2", "#d33682", "#2aa198", "#eee8d5",
        )),
    ]
}

fn make_theme_map(
    bg: &str, fg: &str, cursor: &str,
    b0: &str, b1: &str, b2: &str, b3: &str,
    b4: &str, b5: &str, b6: &str, b7: &str,
) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("editor.background".into(), bg.into());
    m.insert("editor.foreground".into(), fg.into());
    m.insert("editorCursor.foreground".into(), cursor.into());
    m.insert("terminal.background".into(), bg.into());
    m.insert("terminal.foreground".into(), fg.into());
    m.insert("terminalCursor.foreground".into(), cursor.into());
    m.insert("terminal.ansiBlack".into(), b0.into());
    m.insert("terminal.ansiRed".into(), b1.into());
    m.insert("terminal.ansiGreen".into(), b2.into());
    m.insert("terminal.ansiYellow".into(), b3.into());
    m.insert("terminal.ansiBlue".into(), b4.into());
    m.insert("terminal.ansiMagenta".into(), b5.into());
    m.insert("terminal.ansiCyan".into(), b6.into());
    m.insert("terminal.ansiWhite".into(), b7.into());
    m
}

/// Find a built-in theme by matching query string
pub fn find_builtin_theme(query: &str) -> Option<HashMap<String, String>> {
    let q = query.to_lowercase();
    for (name, _, map) in popular_presets() {
        let n = name.to_lowercase();
        if n == q || q.contains(&n) || n.contains(&q) {
            return Some(map);
        }
    }
    None
}

/// Check if a theme is in disk cache
pub fn get_cached_theme(ext: &Extension) -> Option<HashMap<String, String>> {
    let version = ext.versions.first()?;
    let path = get_cache_path(&ext.extensionId, &version.version)?;
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(colors) = serde_json::from_str::<HashMap<String, String>>(&content) {
                return Some(colors);
            }
        }
    }
    None
}

/// Download and extract theme with continuous progress notification.
pub fn download_and_extract_theme_with_progress(
    ext: &Extension,
    progress_tx: async_channel::Sender<DownloadProgress>,
) {
    // 1. Check built-in preset
    let name_lower = ext.extensionName.to_lowercase();
    let display_lower = ext.displayName.as_deref().unwrap_or("").to_lowercase();
    for query in [&name_lower, &display_lower] {
        if let Some(colors) = find_builtin_theme(query) {
            let _ = progress_tx.send_blocking(DownloadProgress::Progress(1.0, "Carregado do preset embutido".into()));
            let _ = progress_tx.send_blocking(DownloadProgress::Done(colors));
            return;
        }
    }

    // 2. Check disk cache
    let version = match ext.versions.first() {
        Some(v) => v,
        None => {
            let _ = progress_tx.send_blocking(DownloadProgress::Error("Versão da extensão não encontrada".into()));
            return;
        }
    };

    if let Some(colors) = get_cached_theme(ext) {
        let _ = progress_tx.send_blocking(DownloadProgress::Progress(1.0, "Carregado do cache local".into()));
        let _ = progress_tx.send_blocking(DownloadProgress::Done(colors));
        return;
    }

    // 3. Download from marketplace
    let uri = match version.fallbackAssetUri.as_ref().or(version.assetUri.as_ref()) {
        Some(u) => u,
        None => {
            let _ = progress_tx.send_blocking(DownloadProgress::Error("URI de download não disponível".into()));
            return;
        }
    };

    let vsix_url = format!("{}/Microsoft.VisualStudio.Services.VSIXPackage", uri);
    let _ = progress_tx.send_blocking(DownloadProgress::Progress(0.05, "Iniciando download...".into()));

    let client = Client::new();
    let mut resp = match client.get(&vsix_url).send().and_then(|r| r.error_for_status()) {
        Ok(r) => r,
        Err(e) => {
            let _ = progress_tx.send_blocking(DownloadProgress::Error(format!("Erro no download: {}", e)));
            return;
        }
    };

    let total_size = resp.content_length().unwrap_or(0);
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 32768];
    let mut downloaded: u64 = 0;

    loop {
        match resp.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                bytes.extend_from_slice(&buffer[..n]);
                downloaded += n as u64;
                if total_size > 0 {
                    let fraction = (downloaded as f64 / total_size as f64).min(1.0);
                    let pct = (fraction * 100.0) as u32;
                    let _ = progress_tx.send_blocking(DownloadProgress::Progress(
                        0.05 + fraction * 0.80,
                        format!("Baixando tema... ({}%)", pct),
                    ));
                }
            }
            Err(e) => {
                let _ = progress_tx.send_blocking(DownloadProgress::Error(format!("Erro na conexão: {}", e)));
                return;
            }
        }
    }

    let _ = progress_tx.send_blocking(DownloadProgress::Progress(0.90, "Extraindo pacote VSIX...".into()));

    let reader = Cursor::new(bytes);
    let mut zip = match zip::ZipArchive::new(reader) {
        Ok(z) => z,
        Err(e) => {
            let _ = progress_tx.send_blocking(DownloadProgress::Error(format!("Erro ao descompactar: {}", e)));
            return;
        }
    };

    let mut package_json = String::new();
    {
        let mut file = match zip.by_name("extension/package.json") {
            Ok(f) => f,
            Err(e) => {
                let _ = progress_tx.send_blocking(DownloadProgress::Error(format!("package.json não encontrado: {}", e)));
                return;
            }
        };
        if let Err(e) = file.read_to_string(&mut package_json) {
            let _ = progress_tx.send_blocking(DownloadProgress::Error(format!("Falha ao ler package.json: {}", e)));
            return;
        }
    }

    let package_val: serde_json::Value = match serde_json::from_str(&package_json) {
        Ok(v) => v,
        Err(e) => {
            let _ = progress_tx.send_blocking(DownloadProgress::Error(format!("Falha ao analisar package.json: {}", e)));
            return;
        }
    };

    let theme_def = package_val
        .get("contributes")
        .and_then(|c| c.get("themes"))
        .and_then(|t| t.as_array())
        .and_then(|arr| arr.first());

    let path = match theme_def.and_then(|t| t.get("path")).and_then(|p| p.as_str()) {
        Some(p) => p,
        None => {
            let _ = progress_tx.send_blocking(DownloadProgress::Error("Definição de tema ausente no package.json".into()));
            return;
        }
    };

    let clean_path = path.trim_start_matches("./");
    let zip_path = format!("extension/{}", clean_path);

    let mut theme_json = String::new();
    {
        let mut file = match zip.by_name(&zip_path) {
            Ok(f) => f,
            Err(e) => {
                let _ = progress_tx.send_blocking(DownloadProgress::Error(format!("Arquivo de tema {} não encontrado: {}", zip_path, e)));
                return;
            }
        };
        if let Err(e) = file.read_to_string(&mut theme_json) {
            let _ = progress_tx.send_blocking(DownloadProgress::Error(format!("Falha ao ler arquivo de cores: {}", e)));
            return;
        }
    }

    let clean_json = remove_comments(&theme_json);

    #[derive(Deserialize)]
    struct ThemeColors {
        colors: HashMap<String, String>,
    }

    let theme: ThemeColors = match serde_json::from_str(&clean_json) {
        Ok(t) => t,
        Err(e) => {
            let _ = progress_tx.send_blocking(DownloadProgress::Error(format!("Falha ao analisar JSON do tema: {}", e)));
            return;
        }
    };

    // Cache on disk
    if let Some(path) = get_cache_path(&ext.extensionId, &version.version) {
        if let Ok(json_str) = serde_json::to_string(&theme.colors) {
            let _ = std::fs::write(path, json_str);
        }
    }

    let _ = progress_tx.send_blocking(DownloadProgress::Progress(1.0, "Pronto!".into()));
    let _ = progress_tx.send_blocking(DownloadProgress::Done(theme.colors));
}

/// Backward compatible helper
#[allow(dead_code)]
pub fn download_and_extract_theme(ext: &Extension) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let (tx, rx) = async_channel::bounded(10);
    let ext_clone = ext.clone();
    std::thread::spawn(move || {
        download_and_extract_theme_with_progress(&ext_clone, tx);
    });

    while let Ok(msg) = rx.recv_blocking() {
        match msg {
            DownloadProgress::Done(colors) => return Ok(colors),
            DownloadProgress::Error(err) => return Err(err.into()),
            DownloadProgress::Progress(_, _) => {}
        }
    }
    Err("Download terminado sem dados".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_popular_presets() {
        let presets = popular_presets();
        assert_eq!(presets.len(), 8);
        let dracula = find_builtin_theme("dracula");
        assert!(dracula.is_some());
        let colors = dracula.unwrap();
        assert_eq!(colors.get("editor.background").unwrap(), "#282a36");
        let swatches = extract_key_swatches(&colors);
        assert_eq!(swatches.len(), 8);
    }
}
