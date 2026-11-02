//! Application controller and state management for Kiln Shell.

use crate::image_loader::load_image_off_thread;
use anyhow::Result;
#[cfg(windows)]
use kiln_core::models::LaunchKind;
use kiln_core::models::{DiscoveredGame, GameSource};
use kiln_library::db::LibraryDatabase;
use kiln_library::scanners::deduplicate_games;
use kiln_library::scanners::epic::EpicScanner;
use kiln_library::scanners::standalone::StandaloneScanner;
use kiln_library::scanners::steam::SteamScanner;
use kiln_library::scanners::GameScanner;
use slint::{Image, SharedString};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

slint::include_modules!();

pub struct ShellApp {
    db: Rc<LibraryDatabase>,
    safe_mode: bool,
    dry_run: bool,
    artwork_dir: PathBuf,
}

impl ShellApp {
    pub fn new(
        db: Rc<LibraryDatabase>,
        safe_mode: bool,
        dry_run: bool,
        artwork_dir: PathBuf,
    ) -> Self {
        Self {
            db,
            safe_mode,
            dry_run,
            artwork_dir,
        }
    }

    pub fn is_safe_mode(&self) -> bool {
        self.safe_mode
    }

    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    pub fn db(&self) -> &LibraryDatabase {
        &self.db
    }

    /// Converts a DiscoveredGame into a Slint GameItem.
    pub fn to_game_item(&self, game: &DiscoveredGame) -> GameItem {
        let (has_poster, poster) = if let Some(ref path_str) = game.artwork_grid {
            let path = Path::new(path_str);
            if let Some(img) = load_image_off_thread(path, 480, 720) {
                (true, img)
            } else {
                (false, Image::default())
            }
        } else {
            // Check if there is a local cached artwork in artwork_dir
            let local_artwork = self.artwork_dir.join(format!("{}.jpg", game.id));
            if local_artwork.exists() {
                if let Some(img) = load_image_off_thread(&local_artwork, 480, 720) {
                    (true, img)
                } else {
                    (false, Image::default())
                }
            } else {
                (false, Image::default())
            }
        };

        let launcher_name = match game.source {
            GameSource::Steam => "Steam",
            GameSource::Epic => "Epic",
            GameSource::XboxMsStore => "Xbox",
            GameSource::Gog => "GOG",
            GameSource::Ea => "EA",
            GameSource::Ubisoft => "Ubisoft",
            GameSource::BattleNet => "Battle.net",
            GameSource::Standalone => "Standalone",
            GameSource::Manual => "Custom",
        };

        let last_played_str = game.last_played.map(format_timestamp).unwrap_or_default();

        let playtime_str = format_playtime(game.playtime_seconds);

        GameItem {
            id: SharedString::from(&game.id),
            title: SharedString::from(&game.title),
            launcher: SharedString::from(launcher_name),
            has_poster,
            poster,
            last_played: SharedString::from(last_played_str),
            playtime_hours: SharedString::from(playtime_str),
            is_favorite: game.is_favorite,
        }
    }

    /// Loads games for a given tab filter:
    /// 0: All, 1: Recent, 2: Favorites, 3: Steam, 4: Epic, 5: Standalone, 6: Settings
    /// Returns (tab_games, recent_games).
    pub fn load_games(&self, tab: i32) -> Result<(Vec<GameItem>, Vec<GameItem>)> {
        let all_games = self.db.list_games()?;

        // Calculate recent games (up to 6 games sorted by last_played desc)
        let mut recent_sorted = all_games.clone();
        recent_sorted.retain(|g| g.last_played.is_some() && g.last_played.unwrap() > 0);
        recent_sorted.sort_by_key(|a| std::cmp::Reverse(a.last_played));
        let recent_items: Vec<GameItem> = recent_sorted
            .into_iter()
            .take(6)
            .map(|g| self.to_game_item(&g))
            .collect();

        // Filter based on tab
        let filtered: Vec<DiscoveredGame> = match tab {
            0 => all_games, // All
            1 => {
                // Recent
                let mut rec = all_games;
                rec.sort_by_key(|a| std::cmp::Reverse(a.last_played));
                rec
            }
            2 => all_games.into_iter().filter(|g| g.is_favorite).collect(), // Favorites
            3 => all_games
                .into_iter()
                .filter(|g| g.source == GameSource::Steam)
                .collect(),
            4 => all_games
                .into_iter()
                .filter(|g| g.source == GameSource::Epic)
                .collect(),
            5 => all_games
                .into_iter()
                .filter(|g| {
                    g.source == GameSource::Standalone
                        || g.source == GameSource::Gog
                        || g.source == GameSource::XboxMsStore
                        || g.source == GameSource::Manual
                })
                .collect(),
            _ => Vec::new(),
        };

        let items = filtered
            .into_iter()
            .map(|g| self.to_game_item(&g))
            .collect();
        Ok((items, recent_items))
    }

    /// Toggles favorite status in database.
    pub fn toggle_favorite(&self, id: &str) -> Result<bool> {
        Ok(self.db.toggle_favorite(id)?)
    }

    /// Launches a game by ID or simulates in dry-run mode.
    pub fn play_game(&self, id: &str) -> Result<()> {
        let game = self
            .db
            .get_game(id)?
            .ok_or_else(|| anyhow::anyhow!("Game not found: {}", id))?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Update last played timestamp in database
        self.db.update_playtime(id, 60, now)?;

        if self.dry_run {
            println!(
                "[DRY-RUN] Simulating game launch:\n  Title: {}\n  Source: {:?}\n  Kind: {:?}\n  Target: {}\n  Install Dir: {}",
                game.title, game.source, game.launch_kind, game.launch_target, game.install_dir
            );
            return Ok(());
        }

        println!("Launching game: {} ({})", game.title, game.launch_target);

        // Real execution
        #[cfg(windows)]
        {
            match game.launch_kind {
                LaunchKind::DirectExe => {
                    if let Some(ref exe) = game.exe_path {
                        std::process::Command::new(exe)
                            .current_dir(&game.install_dir)
                            .spawn()?;
                    } else {
                        std::process::Command::new(&game.launch_target)
                            .current_dir(&game.install_dir)
                            .spawn()?;
                    }
                }
                LaunchKind::Uri => {
                    // Launch URI on Windows via rundll32 or explorer
                    std::process::Command::new("cmd")
                        .args(["/c", "start", "", &game.launch_target])
                        .spawn()?;
                }
                LaunchKind::Custom => {
                    std::process::Command::new(&game.launch_target).spawn()?;
                }
            }
        }

        #[cfg(not(windows))]
        {
            println!(
                "[HOST] Launch command simulated on host: {:?} target={}",
                game.launch_kind, game.launch_target
            );
        }

        Ok(())
    }

    /// Performs library scan across known directories and test fixtures.
    pub fn rescan(&self) -> Result<usize> {
        let mut discovered = Vec::new();

        // 1. Steam
        let steam_roots = vec![
            PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps"),
            PathBuf::from(r"D:\SteamLibrary\steamapps"),
            PathBuf::from(r"E:\SteamLibrary\steamapps"),
            PathBuf::from("crates/kiln-library/fixtures/steam/steamapps"),
        ];
        if let Ok(mut games) = SteamScanner::new(steam_roots).scan() {
            discovered.append(&mut games);
        }

        // 2. Epic
        let epic_roots = vec![
            PathBuf::from(r"C:\ProgramData\Epic\EpicGamesLauncher\Data\Manifests"),
            PathBuf::from(r"C:\ProgramData\Epic\UnrealEngineLauncher\Data\Manifests"),
            PathBuf::from("crates/kiln-library/fixtures/epic/Manifests"),
        ];
        if let Ok(mut games) = EpicScanner::new(epic_roots).scan() {
            discovered.append(&mut games);
        }

        // 3. Standalone
        let standalone_roots = vec![
            PathBuf::from(r"C:\Games"),
            PathBuf::from(r"D:\Games"),
            PathBuf::from(r"E:\Games"),
            PathBuf::from(r"C:\Program Files\Games"),
            PathBuf::from("crates/kiln-library/fixtures/standalone"),
        ];
        if let Ok(mut games) = StandaloneScanner::new(standalone_roots).scan() {
            discovered.append(&mut games);
        }

        let deduped = deduplicate_games(discovered);
        let count = deduped.len();
        for g in &deduped {
            let _ = self.db.upsert_game(g);
        }

        Ok(count)
    }
}

fn format_timestamp(ts: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if ts == 0 {
        return String::new();
    }

    let diff = now.saturating_sub(ts);
    if diff < 3600 {
        "Just now".to_string()
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else if diff < 604800 {
        format!("{}d ago", diff / 86400)
    } else {
        format!("{}w ago", diff / 604800)
    }
}

fn format_playtime(seconds: u64) -> String {
    if seconds == 0 {
        return "Not played yet".to_string();
    }
    let hours = seconds / 3600;
    let mins = (seconds % 3600) / 60;
    if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else {
        format!("{}m", mins)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiln_core::models::LaunchKind;

    fn make_test_game(id: &str, title: &str, source: GameSource, fav: bool) -> DiscoveredGame {
        DiscoveredGame {
            id: id.to_string(),
            title: title.to_string(),
            source,
            install_dir: format!("C:\\Games\\{}", title),
            exe_path: Some(format!("C:\\Games\\{}\\{}.exe", title, id)),
            launch_kind: LaunchKind::DirectExe,
            launch_target: format!("C:\\Games\\{}\\{}.exe", title, id),
            launcher: None,
            artwork_grid: None,
            artwork_hero: None,
            last_played: Some(1700000000),
            playtime_seconds: 7200,
            is_hidden: false,
            is_favorite: fav,
            is_manual: false,
        }
    }

    #[test]
    fn test_shell_app_tabs_and_filtering() {
        let db = Rc::new(LibraryDatabase::open_in_memory().unwrap());
        db.upsert_game(&make_test_game("g1", "Portal 2", GameSource::Steam, true))
            .unwrap();
        db.upsert_game(&make_test_game(
            "g2",
            "Alan Wake 2",
            GameSource::Epic,
            false,
        ))
        .unwrap();
        db.upsert_game(&make_test_game("g3", "Hades", GameSource::Standalone, true))
            .unwrap();

        let app = ShellApp::new(db, true, true, PathBuf::from("/tmp/art"));

        // Tab 0: All
        let (all, recent) = app.load_games(0).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(recent.len(), 3);

        // Tab 2: Favorites
        let (favs, _) = app.load_games(2).unwrap();
        assert_eq!(favs.len(), 2);

        // Tab 3: Steam
        let (steam, _) = app.load_games(3).unwrap();
        assert_eq!(steam.len(), 1);
        assert_eq!(steam[0].title.as_str(), "Portal 2");

        // Tab 4: Epic
        let (epic, _) = app.load_games(4).unwrap();
        assert_eq!(epic.len(), 1);
        assert_eq!(epic[0].title.as_str(), "Alan Wake 2");
    }

    #[test]
    fn test_shell_app_toggle_favorite() {
        let db = Rc::new(LibraryDatabase::open_in_memory().unwrap());
        db.upsert_game(&make_test_game("g1", "Portal 2", GameSource::Steam, false))
            .unwrap();

        let app = ShellApp::new(db, true, true, PathBuf::from("/tmp/art"));

        let is_fav = app.toggle_favorite("g1").unwrap();
        assert!(is_fav);

        let (favs, _) = app.load_games(2).unwrap();
        assert_eq!(favs.len(), 1);
        assert!(favs[0].is_favorite);
    }

    #[test]
    fn test_shell_app_play_game_dry_run() {
        let db = Rc::new(LibraryDatabase::open_in_memory().unwrap());
        db.upsert_game(&make_test_game("g1", "Portal 2", GameSource::Steam, false))
            .unwrap();

        let app = ShellApp::new(db, true, true, PathBuf::from("/tmp/art"));
        assert!(app.is_dry_run());
        assert!(app.is_safe_mode());

        let res = app.play_game("g1");
        assert!(res.is_ok());
    }

    #[test]
    fn test_formatting_helpers() {
        assert_eq!(format_playtime(0), "Not played yet");
        assert_eq!(format_playtime(1800), "30m");
        assert_eq!(format_playtime(3600), "1h 0m");
        assert_eq!(format_playtime(5400), "1h 30m");
        assert_eq!(format_timestamp(0), "");
    }
}
