//! Standalone game scanner: directory heuristics, binary scoring, and helper filtering.

use super::GameScanner;
use kiln_core::models::{DiscoveredGame, GameSource, LaunchKind};
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
    "crashreport",
    "crashhandler",
    "redist",
    "dxwebsetup",
    "prereq",
    "cefprocess",
    "launcherhelper",
    "notification_helper",
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

impl StandaloneScanner {
    /// Best game executable inside `dir` (searched a few levels deep: many games keep the
    /// real binary in bin/ or Binaries/Win64/).
    fn best_executable(dir: &Path) -> Option<PathBuf> {
        let mut best: Option<(PathBuf, i32)> = None;
        for entry in WalkDir::new(dir)
            .max_depth(4)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            let is_exe = path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
            if !is_exe || !path.is_file() {
                continue;
            }
            // Prefer shallower binaries a little: launchers usually sit at the top.
            let depth_penalty = (entry.depth() as i32 - 1).max(0) * 5;
            let score = Self::score_executable(path) - depth_penalty;
            if score > 0 && best.as_ref().is_none_or(|(_, s)| score > *s) {
                best = Some((path.to_path_buf(), score));
            }
        }
        best.map(|(p, _)| p)
    }

    fn id_for(path: &Path) -> String {
        let safe: String = path
            .to_string_lossy()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect::<String>()
            .to_lowercase();
        format!("standalone_{safe}")
    }

    fn game(
        title: String,
        id_path: &Path,
        install_dir: &Path,
        exe: Option<&Path>,
        kind: LaunchKind,
        target: &Path,
    ) -> DiscoveredGame {
        DiscoveredGame {
            id: Self::id_for(id_path),
            title,
            source: GameSource::Standalone,
            install_dir: install_dir.to_string_lossy().to_string(),
            exe_path: exe.map(|e| e.to_string_lossy().to_string()),
            launch_kind: kind,
            launch_target: target.to_string_lossy().to_string(),
            launcher: None,
            artwork_grid: None,
            artwork_hero: None,
            last_played: None,
            playtime_seconds: 0,
            is_hidden: false,
            is_favorite: false,
            is_manual: false,
        }
    }
}

/// "Hollow_Knight" / "hollow.knight" -> "Hollow Knight".
pub fn title_from_name(name: &str) -> String {
    let spaced: String = name
        .chars()
        .map(|c| if c == '_' || c == '.' { ' ' } else { c })
        .collect();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl GameScanner for StandaloneScanner {
    fn name(&self) -> &'static str {
        "Standalone"
    }

    /// Each item directly inside a games folder is one game:
    /// - a sub-folder: its best executable (named after the folder);
    /// - a shortcut (.lnk / .url): launched as-is, named after the shortcut;
    /// - a loose .exe: launched directly, named after the file.
    fn scan(&self) -> Result<Vec<DiscoveredGame>, String> {
        let mut games = Vec::new();
        for root in &self.scan_roots {
            let Ok(entries) = std::fs::read_dir(root) else {
                continue;
            };
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || name.starts_with('$') {
                    continue;
                }
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase());
                if path.is_dir() {
                    if let Some(exe) = Self::best_executable(&path) {
                        games.push(Self::game(
                            title_from_name(&name),
                            &path,
                            &path,
                            Some(&exe),
                            LaunchKind::DirectExe,
                            &exe,
                        ));
                    }
                } else if matches!(ext.as_deref(), Some("lnk") | Some("url")) {
                    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Game");
                    games.push(Self::game(
                        title_from_name(stem),
                        &path,
                        root,
                        None,
                        LaunchKind::Uri,
                        &path,
                    ));
                } else if ext.as_deref() == Some("exe") && Self::score_executable(&path) > 0 {
                    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Game");
                    let title = title_from_name(stem.trim_end_matches("-Win64-Shipping"));
                    games.push(Self::game(
                        title,
                        &path,
                        root,
                        Some(&path),
                        LaunchKind::DirectExe,
                        &path,
                    ));
                }
            }
        }
        Ok(games)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_entry_in_a_games_folder_is_one_game() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        // Folder game with the real binary nested, plus helpers that must be ignored.
        std::fs::create_dir_all(r.join("Hollow_Knight/bin")).unwrap();
        std::fs::write(r.join("Hollow_Knight/bin/hollow_knight.exe"), b"x").unwrap();
        std::fs::write(r.join("Hollow_Knight/UnityCrashHandler64.exe"), b"x").unwrap();
        std::fs::write(r.join("Hollow_Knight/unins000.exe"), b"x").unwrap();
        // Folder with only helpers: not a game.
        std::fs::create_dir_all(r.join("Redist")).unwrap();
        std::fs::write(r.join("Redist/vc_redist.x64.exe"), b"x").unwrap();
        // Shortcut and loose exe.
        std::fs::write(r.join("Celeste.lnk"), b"x").unwrap();
        std::fs::write(r.join("Doom-Win64-Shipping.exe"), b"x").unwrap();
        std::fs::write(r.join("notes.txt"), b"x").unwrap();

        let mut games = StandaloneScanner::new(vec![r.to_path_buf()])
            .scan()
            .unwrap();
        games.sort_by(|a, b| a.title.cmp(&b.title));
        let titles: Vec<&str> = games.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(titles, ["Celeste", "Doom", "Hollow Knight"]);

        let hk = &games[2];
        assert!(hk.launch_target.ends_with("hollow_knight.exe"));
        assert_eq!(hk.launch_kind, LaunchKind::DirectExe);
        assert!(hk.install_dir.ends_with("Hollow_Knight"));
        assert_eq!(games[0].launch_kind, LaunchKind::Uri);
        // Stable ids: rescanning gives the same ids.
        let again = StandaloneScanner::new(vec![r.to_path_buf()])
            .scan()
            .unwrap();
        assert!(again.iter().all(|g| games.iter().any(|h| h.id == g.id)));
    }
}
