//! Steam scanner: parses Steam library folders and appmanifest_*.acf files.

use super::GameScanner;
use kiln_core::models::{DiscoveredGame, GameSource, LaunchKind};
use std::fs;
use std::path::PathBuf;

pub struct SteamScanner {
    library_paths: Vec<PathBuf>,
}

impl SteamScanner {
    pub fn new(library_paths: Vec<PathBuf>) -> Self {
        Self { library_paths }
    }

    /// Helper to parse a single .acf file.
    pub fn parse_acf(acf_content: &str) -> Option<(String, String, String)> {
        let mut appid = None;
        let mut name = None;
        let mut installdir = None;

        for line in acf_content.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("\"appid\"") {
                let val = extract_quoted_value(rest);
                if !val.is_empty() {
                    appid = Some(val);
                }
            } else if let Some(rest) = line.strip_prefix("\"name\"") {
                let val = extract_quoted_value(rest);
                if !val.is_empty() {
                    name = Some(val);
                }
            } else if let Some(rest) = line.strip_prefix("\"installdir\"") {
                let val = extract_quoted_value(rest);
                if !val.is_empty() {
                    installdir = Some(val);
                }
            }
        }

        match (appid, name, installdir) {
            (Some(a), Some(n), Some(i)) => Some((a, n, i)),
            _ => None,
        }
    }
}

fn extract_quoted_value(s: &str) -> String {
    let s = s.trim();
    if let Some(start) = s.find('"') {
        let after_start = &s[start + 1..];
        if let Some(end) = after_start.find('"') {
            return after_start[..end].to_string();
        }
    }
    String::new()
}

pub fn is_steam_redist_or_tool(appid: &str, name: &str) -> bool {
    // 228980: Steamworks Common Redistributables
    // 1493710: Proton Experimental
    // 250820: SteamVR
    let lower_name = name.to_ascii_lowercase();
    appid == "228980"
        || appid == "1493710"
        || appid == "250820"
        || lower_name.contains("steamworks common redistributables")
        || lower_name.contains("proton")
        || lower_name.contains("steam linux runtime")
        || lower_name.contains("steamworks shared")
}

impl GameScanner for SteamScanner {
    fn name(&self) -> &'static str {
        "Steam"
    }

    fn scan(&self) -> Result<Vec<DiscoveredGame>, String> {
        let mut games = Vec::new();

        for steamapps_dir in &self.library_paths {
            if !steamapps_dir.exists() {
                continue;
            }

            let entries = match fs::read_dir(steamapps_dir) {
                Ok(e) => e,
                Err(err) => {
                    tracing::warn!("Failed reading Steam dir {:?}: {}", steamapps_dir, err);
                    continue;
                }
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                    if file_name.starts_with("appmanifest_") && file_name.ends_with(".acf") {
                        if let Ok(content) = fs::read_to_string(&path) {
                            if let Some((appid, name, installdir)) = Self::parse_acf(&content) {
                                if is_steam_redist_or_tool(&appid, &name) {
                                    continue;
                                }

                                let install_dir_path =
                                    steamapps_dir.join("common").join(&installdir);
                                games.push(DiscoveredGame {
                                    id: format!("steam_{}", appid),
                                    title: name,
                                    source: GameSource::Steam,
                                    install_dir: install_dir_path.to_string_lossy().to_string(),
                                    exe_path: None,
                                    launch_kind: LaunchKind::Uri,
                                    launch_target: format!("steam://rungameid/{}", appid),
                                    launcher: Some("Steam".to_string()),
                                    artwork_grid: None,
                                    artwork_hero: None,
                                    last_played: None,
                                    playtime_seconds: 0,
                                    is_hidden: false,
                                    is_favorite: false,
                                    is_manual: false,
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(games)
    }
}
