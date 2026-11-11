//! Application controller and state management for Kiln Shell.

use crate::game_watch::WatchRequest;
use crate::image_loader::load_image_off_thread;
use anyhow::Result;
#[cfg(windows)]
use kiln_core::models::LaunchKind;
use kiln_core::models::{DiscoveredGame, GameSource};
use kiln_library::artwork::{
    find_local_folder_artwork, sanitize_filename, ArtworkManager, LookupCache,
};
use kiln_library::db::LibraryDatabase;
use kiln_library::scanners::deduplicate_games;
use kiln_library::scanners::epic::EpicScanner;
use kiln_library::scanners::standalone::StandaloneScanner;
use kiln_library::scanners::steam::SteamScanner;
use kiln_library::scanners::GameScanner;
#[cfg(windows)]
use kiln_sysctl::launchers;
use kiln_sysctl::launchers::Launcher;
use slint::{Image, SharedString};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

slint::include_modules!();

pub struct ShellApp {
    db: Rc<LibraryDatabase>,
    safe_mode: bool,
    dry_run: bool,
    artwork_dir: PathBuf,
    /// Decoded artwork keyed by file path, so switching tabs doesn't re-decode images.
    image_cache: RefCell<HashMap<PathBuf, Image>>,
    /// Folders the user added in Settings > Game folders.
    game_folders: RefCell<Vec<PathBuf>>,
}

/// Folders always checked for games (if they exist), on top of the user's own.
pub fn default_game_folders() -> Vec<PathBuf> {
    [r"C:\Games", r"D:\Games", r"E:\Games"]
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

/// Finds and downloads art for every game that has none (see `ArtworkManager::resolve_missing`
/// for the sources). Blocking: run it on a background thread. Returns how many games got art.
pub fn sync_missing_artwork(db_path: &Path, artwork_dir: &Path, sgdb_key: Option<String>) -> usize {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return 0;
    };
    let Ok(db) = LibraryDatabase::open(db_path) else {
        return 0;
    };
    let Ok(games) = db.list_games() else {
        return 0;
    };
    let mgr = ArtworkManager::new(artwork_dir.to_path_buf(), sgdb_key);
    let mut lookups = LookupCache::load(artwork_dir);
    let mut fetched = 0;
    for game in games {
        let has_art = game
            .artwork_grid
            .as_ref()
            .is_some_and(|p| Path::new(p).exists())
            || mgr.get_cached_grid_path(&game.id).is_some()
            || find_local_folder_artwork(Path::new(&game.install_dir)).is_some();
        if has_art {
            continue;
        }
        let (grid, hero) = rt.block_on(mgr.resolve_missing(&game, &mut lookups));
        if let Some(g) = grid {
            let h = hero.as_ref().and_then(|p| p.to_str());
            let _ = db.update_artwork(&game.id, g.to_str(), h);
            fetched += 1;
        }
    }
    lookups.save();
    fetched
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
            image_cache: RefCell::new(HashMap::new()),
            game_folders: RefCell::new(Vec::new()),
        }
    }

    /// Sets the user's game folders (scanned on the next `rescan`).
    pub fn set_game_folders(&self, folders: &[String]) {
        *self.game_folders.borrow_mut() = folders.iter().map(PathBuf::from).collect();
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
        let safe_id = sanitize_filename(&game.id);
        let candidate_paths = [
            game.artwork_grid.as_ref().map(PathBuf::from),
            Some(self.artwork_dir.join(format!("grid_{}.jpg", safe_id))),
            Some(self.artwork_dir.join(format!("grid_{}.png", safe_id))),
            Some(self.artwork_dir.join(format!("{}.jpg", safe_id))),
            Some(self.artwork_dir.join(format!("{}.png", safe_id))),
            find_local_folder_artwork(Path::new(&game.install_dir)),
        ];

        let poster_img = candidate_paths
            .into_iter()
            .flatten()
            .find_map(|p| self.load_cached(&p, 480, 720));
        let (has_poster, poster) = match poster_img {
            Some(img) => (true, img),
            None => (false, Image::default()),
        };

        let hero_paths = [
            game.artwork_hero.as_ref().map(PathBuf::from),
            Some(self.artwork_dir.join(format!("hero_{}.jpg", safe_id))),
            Some(self.artwork_dir.join(format!("hero_{}.png", safe_id))),
        ];
        let hero_img = hero_paths
            .into_iter()
            .flatten()
            .find_map(|p| self.load_cached(&p, 1920, 1080));
        let (has_hero, hero) = match hero_img {
            Some(img) => (true, img),
            None => (false, Image::default()),
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
            has_hero,
            hero,
            last_played: SharedString::from(last_played_str),
            playtime_hours: SharedString::from(playtime_str),
            is_favorite: game.is_favorite,
        }
    }

    /// Loads an image from disk once and reuses the decoded pixels afterwards.
    fn load_cached(&self, path: &Path, max_w: u32, max_h: u32) -> Option<Image> {
        if let Some(img) = self.image_cache.borrow().get(path) {
            return Some(img.clone());
        }
        let img = load_image_off_thread(path, max_w, max_h)?;
        self.image_cache
            .borrow_mut()
            .insert(path.to_path_buf(), img.clone());
        Some(img)
    }

    /// Pre-seeds procedural / fallback artwork for demo entries so cards are never blank offline.
    pub fn ensure_demo_artworks(&self) {
        let _ = std::fs::create_dir_all(&self.artwork_dir);
        let demos = [
            ("demo_portal_2", "Portal 2", [0, 210, 255]),
            ("demo_hades_ii", "Hades II", [255, 69, 58]),
            ("demo_alan_wake_2", "Alan Wake 2", [142, 142, 147]),
            ("demo_cyberpunk_2077", "Cyberpunk 2077", [255, 214, 10]),
            ("demo_forza_horizon_5", "Forza Horizon 5", [255, 159, 10]),
            ("demo_dead_cells", "Dead Cells", [175, 82, 222]),
        ];

        for (id, title, color) in demos {
            let safe_id = sanitize_filename(id);
            let target = self.artwork_dir.join(format!("grid_{}.jpg", safe_id));
            if !target.exists() {
                let _ = generate_fallback_poster(&target, title, color);
            }
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

    /// Store launchers that have games in the library (started quietly ahead of time).
    pub fn launchers_in_library(&self) -> Vec<Launcher> {
        let games = self.db.list_games().unwrap_or_default();
        [
            (GameSource::Steam, Launcher::Steam),
            (GameSource::Epic, Launcher::Epic),
        ]
        .into_iter()
        .filter(|(src, _)| games.iter().any(|g| &g.source == src))
        .map(|(_, l)| l)
        .collect()
    }

    /// Launches a game by ID (or simulates it in dry-run mode). Returns what the game
    /// watcher needs to track the session, when it can be tracked.
    pub fn play_game(&self, id: &str) -> Result<Option<WatchRequest>> {
        let game = self
            .db
            .get_game(id)?
            .ok_or_else(|| anyhow::anyhow!("Game not found: {}", id))?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Last played = now. Play time itself is added by the game watcher when the game
        // closes (see game_watch.rs).
        self.db.update_playtime(id, 0, now)?;

        if self.dry_run {
            println!(
                "[DRY-RUN] Simulating game launch:
  Title: {}
  Source: {:?}
  Kind: {:?}
  Target: {}
  Install Dir: {}",
                game.title, game.source, game.launch_kind, game.launch_target, game.install_dir
            );
            return Ok(None);
        }

        println!("Launching game: {} ({})", game.title, game.launch_target);

        let launcher = match game.source {
            GameSource::Steam => Some(Launcher::Steam),
            GameSource::Epic => Some(Launcher::Epic),
            _ => None,
        };

        // Real execution
        #[cfg(windows)]
        {
            match game.launch_kind {
                LaunchKind::DirectExe => {
                    if let Some(ref exe) = game.exe_path {
                        // Run from the executable's own folder: many games load files
                        // relative to it (it can be nested below the game folder).
                        let dir = Path::new(exe)
                            .parent()
                            .map(Path::to_path_buf)
                            .unwrap_or_else(|| PathBuf::from(&game.install_dir));
                        std::process::Command::new(exe).current_dir(dir).spawn()?;
                    } else {
                        std::process::Command::new(&game.launch_target)
                            .current_dir(&game.install_dir)
                            .spawn()?;
                    }
                }
                LaunchKind::Uri => match launcher {
                    // Steam not running: start it with -silent and the game URL in one go,
                    // so the game opens without the whole Steam window.
                    Some(Launcher::Steam)
                        if Launcher::Steam
                            .start_silent(Some(&game.launch_target))
                            .unwrap_or(false) => {}
                    Some(Launcher::Epic) => {
                        // Start Epic in the background first (it ignores "silent" on a
                        // cold start), then hand it the game.
                        let _ = Launcher::Epic.start_silent(None);
                        launchers::open_uri(&game.launch_target)?;
                    }
                    _ => launchers::open_uri(&game.launch_target)?,
                },
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

        let req = WatchRequest {
            game_id: game.id.clone(),
            title: game.title.clone(),
            install_dir: game.install_dir.clone(),
            launcher,
        };
        Ok(req.is_trackable().then_some(req))
    }

    /// Scans Steam, Epic and the game folders, then updates the library. Games from game
    /// folders that are gone (folder removed in Settings, or deleted) are dropped.
    pub fn rescan(&self) -> Result<usize> {
        let mut discovered = Vec::new();

        // 1. Steam
        let steam_roots = vec![
            PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps"),
            PathBuf::from(r"D:\SteamLibrary\steamapps"),
            PathBuf::from(r"E:\SteamLibrary\steamapps"),
        ];
        if let Ok(mut games) = SteamScanner::new(steam_roots).scan() {
            discovered.append(&mut games);
        }

        // 2. Epic
        let epic_roots = vec![
            PathBuf::from(r"C:\ProgramData\Epic\EpicGamesLauncher\Data\Manifests"),
            PathBuf::from(r"C:\ProgramData\Epic\UnrealEngineLauncher\Data\Manifests"),
        ];
        if let Ok(mut games) = EpicScanner::new(epic_roots).scan() {
            discovered.append(&mut games);
        }

        // 3. Game folders: the defaults plus the user's own (Settings > Game folders).
        let mut standalone_roots = default_game_folders();
        for f in self.game_folders.borrow().iter() {
            if !standalone_roots.contains(f) {
                standalone_roots.push(f.clone());
            }
        }
        if let Ok(mut games) = StandaloneScanner::new(standalone_roots).scan() {
            discovered.append(&mut games);
        }

        let deduped = deduplicate_games(discovered);
        let count = deduped.len();
        for g in &deduped {
            let _ = self.db.upsert_game(g);
        }

        // Drop folder games that weren't found this time (manually added games stay).
        if let Ok(existing) = self.db.list_games() {
            for g in existing {
                let stale = g.source == GameSource::Standalone
                    && !g.is_manual
                    && !deduped.iter().any(|d| d.id == g.id);
                if stale {
                    let _ = self.db.delete_game(&g.id);
                }
            }
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

pub fn generate_fallback_poster(dest: &Path, _title: &str, accent: [u8; 3]) -> anyhow::Result<()> {
    let width = 480u32;
    let height = 720u32;
    let mut img = image::RgbImage::new(width, height);

    for (x, y, pixel) in img.enumerate_pixels_mut() {
        let fy = y as f32 / height as f32;
        let r = ((12.0 + (accent[0] as f32 * 0.15)) * (1.0 - fy * 0.6)) as u8;
        let g = ((16.0 + (accent[1] as f32 * 0.15)) * (1.0 - fy * 0.6)) as u8;
        let b = ((24.0 + (accent[2] as f32 * 0.15)) * (1.0 - fy * 0.6)) as u8;

        if y < 12 || y > height - 12 || x < 8 || x > width - 8 {
            *pixel = image::Rgb(accent);
        } else {
            *pixel = image::Rgb([r, g, b]);
        }
    }

    img.save(dest)?;
    Ok(())
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

    #[test]
    fn test_shell_app_artwork_generation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let art_path = temp_dir.path().join("grid_test.jpg");
        let res = generate_fallback_poster(&art_path, "Test Game", [0, 210, 255]);
        assert!(res.is_ok());
        assert!(art_path.exists());
    }
}
