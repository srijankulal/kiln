//! Domain models for game discovery, launchers, and library management.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameSource {
    Steam,
    Epic,
    Gog,
    Ea,
    Ubisoft,
    BattleNet,
    XboxMsStore,
    Standalone,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LaunchKind {
    DirectExe,
    Uri,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredGame {
    pub id: String,
    pub title: String,
    pub source: GameSource,
    pub install_dir: String,
    pub exe_path: Option<String>,
    pub launch_kind: LaunchKind,
    pub launch_target: String,
    pub launcher: Option<String>,
    pub artwork_grid: Option<String>,
    pub artwork_hero: Option<String>,
    pub last_played: Option<u64>,
    pub playtime_seconds: u64,
    pub is_hidden: bool,
    pub is_favorite: bool,
    pub is_manual: bool,
}
