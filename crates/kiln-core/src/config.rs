//! Configuration data structures for Kiln.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TrimTier {
    #[serde(rename = "tier0")]
    Tier0EcoQoS,
    #[serde(rename = "tier1")]
    #[default]
    Tier1Suspend,
    #[serde(rename = "tier2")]
    Tier2Aggressive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KilnConfig {
    pub trim_tier: TrimTier,
    pub auto_prompt_tv_connect: bool,
    pub global_hotkey: String,
    pub target_tv_resolution: Option<String>,
    pub target_tv_refresh_hz: Option<u32>,
    pub close_launcher_after_game: bool,
    pub scan_folders: Vec<String>,
    pub steamgriddb_key_set: bool,
    pub accent_color: String,
    pub ui_scale: f32,
    pub controller_deadzone: f32,
}

impl Default for KilnConfig {
    fn default() -> Self {
        Self {
            trim_tier: TrimTier::Tier1Suspend,
            auto_prompt_tv_connect: true,
            global_hotkey: "Ctrl+Alt+Shift+G".to_string(),
            target_tv_resolution: None,
            target_tv_refresh_hz: None,
            close_launcher_after_game: true,
            scan_folders: vec!["C:\\Games".to_string(), "D:\\Games".to_string()],
            steamgriddb_key_set: false,
            accent_color: "#e65100".to_string(),
            ui_scale: 1.0,
            controller_deadzone: 0.15,
        }
    }
}
