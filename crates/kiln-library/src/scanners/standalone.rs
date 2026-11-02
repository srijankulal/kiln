//! Standalone game scanner: directory heuristics, binary scoring, and helper filtering.

use super::GameScanner;
use kiln_core::models::{DiscoveredGame, GameSource, LaunchKind};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub const IGNORED_EXE_PATTERNS: &[&str] = &[
    "unitycrashhandler",
    "crashpad_handler",
    "unins000",
    "unins001",
    "uninstall",
    "dxsetup",
    "vc_redist",
    "vcredist",
    "dotnetfx",
    "setup",
    "installer",
    "updater",
    "easyanticheat",
    "beservice",
];

pub struct StandaloneScanner {
    scan_roots: Vec<PathBuf>,
}

impl StandaloneScanner {
    pub fn new(scan_roots: Vec<PathBuf>) -> Self {
        Self { scan_roots }
    }

    /// Evaluates if an executable filename is an uninstaller, crash handler, or setup tool.
    pub fn is_ignored_executable(file_name: &str) -> bool {
        let lower = file_name.to_ascii_lowercase();
        IGNORED_EXE_PATTERNS.iter().any(|&p| lower.contains(p))
    }

    /// Scores an executable candidate in a folder based on game engine artifacts and naming.
    pub fn score_executable(exe_path: &Path) -> i32 {
        let file_name = exe_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if Self::is_ignored_executable(file_name) {
            return -1000;
        }

        let mut score = 10;
        let parent = match exe_path.parent() {
            Some(p) => p,
            None => return score,
        };

        // Unreal Engine shipping binary
        if file_name.ends_with("-Win64-Shipping.exe") {
            score += 60;
        }

        // Unity game indicators
        if parent.join("UnityPlayer.dll").exists() {
            score += 50;
        }

        // Steam API integration
        if parent.join("steam_api64.dll").exists() || parent.join("steam_api.dll").exists() {
            score += 40;
        }

        // Graphics runtime DLLs
        if parent.join("d3d11.dll").exists()
            || parent.join("vulkan-1.dll").exists()
            || parent.join("dxgi.dll").exists()
        {
            score += 30;
        }

        // Large file size (> 20 MB)
        if let Ok(meta) = std::fs::metadata(exe_path) {
            if meta.len() > 20 * 1024 * 1024 {
                score += 25;
            }
        }

        score
    }
}

impl GameScanner for StandaloneScanner {
    fn name(&self) -> &'static str {
        "Standalone"
    }

    fn scan(&self) -> Result<Vec<DiscoveredGame>, String> {
        let mut candidates_by_dir: HashMap<PathBuf, (PathBuf, i32)> = HashMap::new();

        for root in &self.scan_roots {
            if !root.exists() {
                continue;
            }

            for entry in WalkDir::new(root)
                .max_depth(3)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                        if ext.eq_ignore_ascii_case("exe") {
                            let score = Self::score_executable(path);
                            if score > 0 {
                                if let Some(parent) = path.parent() {
                                    let dir = parent.to_path_buf();
                                    let current_best =
                                        candidates_by_dir.get(&dir).map(|(_, s)| *s).unwrap_or(0);
                                    if score > current_best {
                                        candidates_by_dir.insert(dir, (path.to_path_buf(), score));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut games = Vec::new();
        for (install_dir, (exe_path, _)) in candidates_by_dir {
            let title = exe_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Game")
                .trim_end_matches("-Win64-Shipping")
                .to_string();

            let dir_str = install_dir.to_string_lossy();
            let safe_id = dir_str
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect::<String>()
                .to_lowercase();
            let id = format!("standalone_{}", safe_id);

            games.push(DiscoveredGame {
                id,
                title,
                source: GameSource::Standalone,
                install_dir: install_dir.to_string_lossy().to_string(),
                exe_path: Some(exe_path.to_string_lossy().to_string()),
                launch_kind: LaunchKind::DirectExe,
                launch_target: exe_path.to_string_lossy().to_string(),
                launcher: None,
                artwork_grid: None,
                artwork_hero: None,
                last_played: None,
                playtime_seconds: 0,
                is_hidden: false,
                is_favorite: false,
                is_manual: false,
            });
        }

        Ok(games)
    }
}
