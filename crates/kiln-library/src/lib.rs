//! Kiln library: Scanners, database store, artwork cache, and session tracking.

pub mod artwork;
pub mod db;
pub mod scanners;
pub mod tracker;

#[cfg(test)]
mod tests {
    use super::db::LibraryDatabase;
    use super::scanners::epic::EpicScanner;
    use super::scanners::standalone::StandaloneScanner;
    use super::scanners::steam::SteamScanner;
    use super::scanners::{deduplicate_games, GameScanner};
    use kiln_core::models::{DiscoveredGame, GameSource, LaunchKind};
    use std::path::PathBuf;

    #[test]
    fn test_steam_scanner_with_fixtures() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join("steam")
            .join("steamapps");

        let scanner = SteamScanner::new(vec![manifest_dir]);
        let games = scanner.scan().expect("Steam scan must succeed");

        // Portal 2 (620) must be discovered
        let portal2 = games.iter().find(|g| g.id == "steam_620");
        assert!(portal2.is_some(), "Portal 2 must be found");
        let p = portal2.unwrap();
        assert_eq!(p.title, "Portal 2");
        assert_eq!(p.launch_target, "steam://rungameid/620");

        // Common Redistributables (228980) and Proton (1493710) MUST be filtered out
        assert!(
            games.iter().all(|g| g.id != "steam_228980"),
            "Redistributables must be excluded"
        );
        assert!(
            games.iter().all(|g| g.id != "steam_1493710"),
            "Proton runtime must be excluded"
        );
        assert_eq!(games.len(), 1, "Only Portal 2 should be in final list");
    }

    #[test]
    fn test_epic_scanner_with_fixtures() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join("epic")
            .join("Manifests");

        let scanner = EpicScanner::new(vec![manifest_dir]);
        let games = scanner.scan().expect("Epic scan must succeed");

        // Alan Wake 2 must be found
        let aw2 = games.iter().find(|g| g.id == "epic_AlanWake2");
        assert!(aw2.is_some(), "Alan Wake 2 must be found");
        let aw = aw2.unwrap();
        assert_eq!(aw.title, "Alan Wake 2");
        assert!(aw
            .launch_target
            .contains("com.epicgames.launcher://apps/AlanWake2"));

        // Unreal Engine (UE_5.4) MUST be filtered out
        assert!(
            games.iter().all(|g| !g.id.contains("UE_")),
            "Engine installs must be excluded"
        );
        assert_eq!(games.len(), 1, "Only Alan Wake 2 should be discovered");
    }

    #[test]
    fn test_standalone_scanner_with_fixtures() {
        let standalone_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join("standalone");

        let scanner = StandaloneScanner::new(vec![standalone_root]);
        let games = scanner.scan().expect("Standalone scan must succeed");

        // Hades must be found
        let hades = games.iter().find(|g| g.title.contains("Hades"));
        assert!(hades.is_some(), "Hades standalone game must be discovered");

        // Crash handlers and uninstallers must NEVER be registered as separate games
        assert!(games
            .iter()
            .all(|g| !g.title.to_lowercase().contains("crashhandler")));
        assert!(games
            .iter()
            .all(|g| !g.title.to_lowercase().contains("unins")));
        assert!(games
            .iter()
            .all(|g| !g.title.to_lowercase().contains("crashpad")));
    }

    #[test]
    fn test_deduplicate_games() {
        let game1 = DiscoveredGame {
            id: "steam_123".to_string(),
            title: "Game One".to_string(),
            source: GameSource::Steam,
            install_dir: "C:\\Games\\GameOne".to_string(),
            exe_path: None,
            launch_kind: LaunchKind::Uri,
            launch_target: "steam://123".to_string(),
            launcher: Some("Steam".to_string()),
            artwork_grid: None,
            artwork_hero: None,
            last_played: None,
            playtime_seconds: 0,
            is_hidden: false,
            is_favorite: false,
            is_manual: false,
        };

        let game2 = DiscoveredGame {
            id: "standalone_abc".to_string(),
            title: "Game One Standalone".to_string(),
            source: GameSource::Standalone,
            install_dir: "c:/games/gameone/".to_string(), // duplicate path with forward slashes and trailing slash
            exe_path: Some("C:\\Games\\GameOne\\game.exe".to_string()),
            launch_kind: LaunchKind::DirectExe,
            launch_target: "C:\\Games\\GameOne\\game.exe".to_string(),
            launcher: None,
            artwork_grid: None,
            artwork_hero: None,
            last_played: None,
            playtime_seconds: 0,
            is_hidden: false,
            is_favorite: false,
            is_manual: false,
        };

        let deduped = deduplicate_games(vec![game1, game2]);
        assert_eq!(deduped.len(), 1, "Duplicate install dir must be merged");
        assert_eq!(
            deduped[0].source,
            GameSource::Standalone,
            "Standalone direct exe has higher priority than launcher URI"
        );
    }

    #[test]
    fn test_sqlite_db_lifecycle() {
        let db = LibraryDatabase::open_in_memory().expect("In-memory SQLite DB must open");

        let game = DiscoveredGame {
            id: "test_game_1".to_string(),
            title: "Test Game".to_string(),
            source: GameSource::Steam,
            install_dir: "D:\\Games\\TestGame".to_string(),
            exe_path: Some("D:\\Games\\TestGame\\test.exe".to_string()),
            launch_kind: LaunchKind::DirectExe,
            launch_target: "D:\\Games\\TestGame\\test.exe".to_string(),
            launcher: None,
            artwork_grid: None,
            artwork_hero: None,
            last_played: None,
            playtime_seconds: 120,
            is_hidden: false,
            is_favorite: false,
            is_manual: false,
        };

        db.upsert_game(&game).expect("Upsert must succeed");

        let games = db.list_games().expect("Listing games must succeed");
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].title, "Test Game");
        assert_eq!(games[0].playtime_seconds, 120);

        // Update playtime
        db.update_playtime("test_game_1", 300, 1700000000)
            .expect("Update playtime must succeed");
        let updated = db.list_games().expect("List after update");
        assert_eq!(updated[0].playtime_seconds, 420);
        assert_eq!(updated[0].last_played, Some(1700000000));
    }
}
