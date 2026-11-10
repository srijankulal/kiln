//! Interface preferences persisted to `settings.json` in the Kiln data folder.
//!
//! The UI owns the live values (Theme / Prompts globals + MainWindow properties); this
//! module loads them at startup and saves them whenever something changes. Values the UI
//! doesn't own (the always-keep app list, preferred display) live only here.

use crate::app::{MainWindow, Prompts, Sfx, Theme};
use serde::{Deserialize, Serialize};
use slint::ComponentHandle;
use std::path::Path;

/// Preset counts defined in `ui/theme.slint` (`font-preset-names`, `theme-names`, `accent-names`).
const FONT_PRESETS: i32 = 3;
const THEME_PRESETS: i32 = 3;
const ACCENT_PRESETS: i32 = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSettings {
    pub ui_scale: f32,
    pub text_scale: f32,
    pub tile_size: i32,
    pub safe_area: i32,
    pub font_preset: i32,
    pub theme_preset: i32,
    pub accent_preset: i32,
    pub animations: bool,
    pub sound_enabled: bool,
    /// Interface sound volume, 0.1..=1.0.
    pub sound_volume: f32,
    pub controller_enabled: bool,
    /// 0 auto, 1 keyboard, 2 controller
    pub prompt_style: i32,
    pub close_launcher_on_exit: bool,
    /// Executable names (lowercase) Kiln must never close.
    pub always_keep: Vec<String>,
    /// "external" or "internal": where the user last chose to run Kiln.
    pub preferred_display: String,
    /// Extra folders scanned for games (each item inside is one game).
    pub game_folders: Vec<String>,
    /// Optional SteamGridDB API key: art for games that aren't on Steam or Epic.
    pub steamgriddb_api_key: String,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            ui_scale: 1.0,
            text_scale: 1.0,
            tile_size: 1,
            safe_area: 0,
            font_preset: 0,
            theme_preset: 0,
            accent_preset: 0,
            animations: true,
            sound_enabled: true,
            sound_volume: 0.7,
            controller_enabled: true,
            prompt_style: 0,
            close_launcher_on_exit: false,
            always_keep: Vec::new(),
            preferred_display: String::new(),
            game_folders: Vec::new(),
            steamgriddb_api_key: String::new(),
        }
    }
}

impl UiSettings {
    /// Reads settings from disk. A missing or unreadable file yields defaults, and
    /// out-of-range values (hand-edited file, older version) are clamped.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Self>(&s).ok())
            .unwrap_or_default()
            .sanitized()
    }

    fn sanitized(mut self) -> Self {
        let finite_or = |v: f32, d: f32| if v.is_finite() { v } else { d };
        self.ui_scale = finite_or(self.ui_scale, 1.0).clamp(0.8, 1.4);
        self.text_scale = finite_or(self.text_scale, 1.0).clamp(0.9, 1.3);
        self.tile_size = self.tile_size.clamp(0, 2);
        self.safe_area = self.safe_area.clamp(0, 5);
        self.font_preset = self.font_preset.clamp(0, FONT_PRESETS - 1);
        self.theme_preset = self.theme_preset.clamp(0, THEME_PRESETS - 1);
        self.accent_preset = self.accent_preset.clamp(0, ACCENT_PRESETS - 1);
        self.prompt_style = self.prompt_style.clamp(0, 2);
        self.sound_volume =
            (finite_or(self.sound_volume, 0.7).clamp(0.1, 1.0) * 10.0).round() / 10.0;
        for k in &mut self.always_keep {
            *k = k.trim().to_ascii_lowercase();
        }
        self.always_keep.retain(|k| !k.is_empty());
        self.always_keep.sort();
        self.always_keep.dedup();
        if self.preferred_display != "external" && self.preferred_display != "internal" {
            self.preferred_display.clear();
        }
        let mut seen = std::collections::HashSet::new();
        self.game_folders.retain(|f| {
            !f.trim().is_empty()
                && seen.insert(f.trim().trim_end_matches(['\\', '/']).to_lowercase())
        });
        self.steamgriddb_api_key = self.steamgriddb_api_key.trim().to_string();
        self
    }

    /// Writes settings atomically (temp file + rename) so a crash can't truncate them.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)
    }

    /// The SteamGridDB key, if one is set.
    pub fn sgdb_key(&self) -> Option<String> {
        (!self.steamgriddb_api_key.is_empty()).then(|| self.steamgriddb_api_key.clone())
    }

    pub fn apply(&self, ui: &MainWindow) {
        let theme = ui.global::<Theme>();
        theme.set_ui_scale(self.ui_scale);
        theme.set_text_scale(self.text_scale);
        theme.set_tile_size(self.tile_size);
        theme.set_safe_area(self.safe_area);
        theme.set_font_preset(self.font_preset);
        theme.set_theme_preset(self.theme_preset);
        theme.set_accent_preset(self.accent_preset);
        theme.set_animations(self.animations);
        let sfx = ui.global::<Sfx>();
        sfx.set_enabled(self.sound_enabled);
        sfx.set_volume(self.sound_volume);
        let prompts = ui.global::<Prompts>();
        prompts.set_controller_enabled(self.controller_enabled);
        prompts.set_style(self.prompt_style);
        ui.set_close_launcher_on_exit(self.close_launcher_on_exit);
        ui.set_kept_apps_count(self.always_keep.len() as i32);
        self.apply_game_folders(ui);
    }

    /// Shows the game folder list in Settings > Game folders.
    pub fn apply_game_folders(&self, ui: &MainWindow) {
        ui.set_game_folders(slint::ModelRc::new(slint::VecModel::from(
            self.game_folders
                .iter()
                .map(|f| slint::SharedString::from(f.as_str()))
                .collect::<Vec<_>>(),
        )));
    }

    /// Copies the values the UI owns into `self`, leaving app-owned fields untouched.
    pub fn capture_ui(&mut self, ui: &MainWindow) {
        let theme = ui.global::<Theme>();
        let prompts = ui.global::<Prompts>();
        self.ui_scale = theme.get_ui_scale();
        self.text_scale = theme.get_text_scale();
        self.tile_size = theme.get_tile_size();
        self.safe_area = theme.get_safe_area();
        self.font_preset = theme.get_font_preset();
        self.theme_preset = theme.get_theme_preset();
        self.accent_preset = theme.get_accent_preset();
        self.animations = theme.get_animations();
        let sfx = ui.global::<Sfx>();
        self.sound_enabled = sfx.get_enabled();
        self.sound_volume = sfx.get_volume();
        self.controller_enabled = prompts.get_controller_enabled();
        self.prompt_style = prompts.get_style();
        self.close_launcher_on_exit = ui.get_close_launcher_on_exit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            UiSettings::load(&dir.path().join("nope.json")),
            UiSettings::default()
        );
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let s = UiSettings {
            ui_scale: 1.2,
            text_scale: 1.1,
            tile_size: 2,
            safe_area: 3,
            font_preset: 1,
            theme_preset: 2,
            accent_preset: 5,
            animations: false,
            sound_enabled: false,
            sound_volume: 0.4,
            controller_enabled: false,
            prompt_style: 2,
            close_launcher_on_exit: true,
            always_keep: vec!["code.exe".into(), "slack.exe".into()],
            preferred_display: "internal".into(),
            game_folders: vec![r"D:\Games".into()],
            steamgriddb_api_key: "abc".into(),
        };
        s.save(&path).unwrap();
        assert_eq!(UiSettings::load(&path), s);
    }

    #[test]
    fn out_of_range_and_partial_files_are_clamped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"ui_scale": 9.0, "font_preset": 42, "safe_area": -3, "accent_preset": 99,
                "always_keep": [" Code.EXE ", "code.exe", ""], "preferred_display": "moon",
                "trim_tier": 2}"#,
        )
        .unwrap();
        let s = UiSettings::load(&path);
        assert_eq!(s.ui_scale, 1.4);
        assert_eq!(s.font_preset, FONT_PRESETS - 1);
        assert_eq!(s.accent_preset, ACCENT_PRESETS - 1);
        assert_eq!(s.safe_area, 0);
        assert_eq!(s.text_scale, 1.0);
        assert_eq!(s.always_keep, ["code.exe"]);
        assert_eq!(s.preferred_display, "");
        assert_eq!(s.sound_volume, 0.7);
        assert!(s.sound_enabled);
    }
}
