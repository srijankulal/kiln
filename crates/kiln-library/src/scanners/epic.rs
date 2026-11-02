//! Epic Games scanner: parses .item manifest files from Epic Games Launcher.

use super::GameScanner;
use kiln_core::models::{DiscoveredGame, GameSource, LaunchKind};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
struct EpicManifestItem {
    AppName: String,
    DisplayName: String,
    InstallLocation: String,
    LaunchExecutable: Option<String>,
    #[serde(default)]
    bIsApplication: bool,
}

pub struct EpicScanner {
    manifest_dirs: Vec<PathBuf>,
}

impl EpicScanner {
    pub fn new(manifest_dirs: Vec<PathBuf>) -> Self {
        Self { manifest_dirs }
    }
}

impl GameScanner for EpicScanner {
    fn name(&self) -> &'static str {
        "EpicGames"
    }

    fn scan(&self) -> Result<Vec<DiscoveredGame>, String> {
        let mut games = Vec::new();

        for dir in &self.manifest_dirs {
            if !dir.exists() {
                continue;
            }

            let entries = match fs::read_dir(dir) {
                Ok(e) => e,
                Err(err) => {
                    tracing::warn!("Failed reading Epic manifest dir {:?}: {}", dir, err);
                    continue;
                }
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) == Some("item") {
                    if let Ok(content) = fs::read_to_string(&path) {
                        if let Ok(item) = serde_json::from_str::<EpicManifestItem>(&content) {
                            // Filter out Unreal Engine versions (UE_5.x) and non-applications
                            if item.AppName.starts_with("UE_") || !item.bIsApplication {
                                continue;
                            }

                            let exe_path = item.LaunchExecutable.as_ref().map(|exe| {
                                Path::new(&item.InstallLocation)
                                    .join(exe)
                                    .to_string_lossy()
                                    .to_string()
                            });

                            games.push(DiscoveredGame {
                                id: format!("epic_{}", item.AppName),
                                title: item.DisplayName,
                                source: GameSource::Epic,
                                install_dir: item.InstallLocation,
                                exe_path,
                                launch_kind: LaunchKind::Uri,
                                launch_target: format!(
                                    "com.epicgames.launcher://apps/{}?action=launch&silent=true",
                                    item.AppName
                                ),
                                launcher: Some("EpicGamesLauncher".to_string()),
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

        Ok(games)
    }
}
