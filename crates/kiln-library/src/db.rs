//! SQLite local database store for discovered games and library metadata.

use kiln_core::models::{DiscoveredGame, GameSource, LaunchKind};
use rusqlite::{params, Connection, Result};
use std::path::Path;

pub struct LibraryDatabase {
    conn: Connection,
}

impl LibraryDatabase {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS games (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                source TEXT NOT NULL,
                install_dir TEXT NOT NULL,
                exe TEXT,
                launch_kind TEXT NOT NULL,
                launch_target TEXT NOT NULL,
                launcher TEXT,
                artwork_grid TEXT,
                artwork_hero TEXT,
                last_played INTEGER,
                playtime_s INTEGER NOT NULL DEFAULT 0,
                hidden INTEGER NOT NULL DEFAULT 0,
                favorite INTEGER NOT NULL DEFAULT 0,
                manual INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_games_source ON games(source);
            CREATE INDEX IF NOT EXISTS idx_games_favorite ON games(favorite);
            CREATE INDEX IF NOT EXISTS idx_games_hidden ON games(hidden);",
        )?;
        Ok(())
    }

    pub fn upsert_game(&self, game: &DiscoveredGame) -> Result<()> {
        let source_str = match game.source {
            GameSource::Steam => "steam",
            GameSource::Epic => "epic",
            GameSource::Gog => "gog",
            GameSource::Ea => "ea",
            GameSource::Ubisoft => "ubisoft",
            GameSource::BattleNet => "battlenet",
            GameSource::XboxMsStore => "xbox",
            GameSource::Standalone => "standalone",
            GameSource::Manual => "manual",
        };

        let kind_str = match game.launch_kind {
            LaunchKind::DirectExe => "direct",
            LaunchKind::Uri => "uri",
            LaunchKind::Custom => "custom",
        };

        self.conn.execute(
            "INSERT INTO games (
                id, title, source, install_dir, exe, launch_kind, launch_target,
                launcher, artwork_grid, artwork_hero, last_played, playtime_s,
                hidden, favorite, manual
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
            ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                install_dir = excluded.install_dir,
                exe = COALESCE(excluded.exe, games.exe),
                launch_target = excluded.launch_target,
                launcher = excluded.launcher;",
            params![
                game.id,
                game.title,
                source_str,
                game.install_dir,
                game.exe_path,
                kind_str,
                game.launch_target,
                game.launcher,
                game.artwork_grid,
                game.artwork_hero,
                game.last_played,
                game.playtime_seconds,
                if game.is_hidden { 1 } else { 0 },
                if game.is_favorite { 1 } else { 0 },
                if game.is_manual { 1 } else { 0 },
            ],
        )?;
        Ok(())
    }

    pub fn list_games(&self) -> Result<Vec<DiscoveredGame>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, source, install_dir, exe, launch_kind, launch_target,
                    launcher, artwork_grid, artwork_hero, last_played, playtime_s,
                    hidden, favorite, manual
             FROM games
             ORDER BY title COLLATE NOCASE ASC",
        )?;

        let game_iter = stmt.query_map([], |row| {
            let source_str: String = row.get(2)?;
            let kind_str: String = row.get(5)?;

            let source = match source_str.as_str() {
                "steam" => GameSource::Steam,
                "epic" => GameSource::Epic,
                "gog" => GameSource::Gog,
                "ea" => GameSource::Ea,
                "ubisoft" => GameSource::Ubisoft,
                "battlenet" => GameSource::BattleNet,
                "xbox" => GameSource::XboxMsStore,
                "manual" => GameSource::Manual,
                _ => GameSource::Standalone,
            };

            let launch_kind = match kind_str.as_str() {
                "uri" => LaunchKind::Uri,
                "custom" => LaunchKind::Custom,
                _ => LaunchKind::DirectExe,
            };

            let hidden_int: i32 = row.get(12)?;
            let favorite_int: i32 = row.get(13)?;
            let manual_int: i32 = row.get(14)?;

            Ok(DiscoveredGame {
                id: row.get(0)?,
                title: row.get(1)?,
                source,
                install_dir: row.get(3)?,
                exe_path: row.get(4)?,
                launch_kind,
                launch_target: row.get(6)?,
                launcher: row.get(7)?,
                artwork_grid: row.get(8)?,
                artwork_hero: row.get(9)?,
                last_played: row.get(10)?,
                playtime_seconds: row.get(11)?,
                is_hidden: hidden_int != 0,
                is_favorite: favorite_int != 0,
                is_manual: manual_int != 0,
            })
        })?;

        let mut results = Vec::new();
        for g in game_iter {
            results.push(g?);
        }
        Ok(results)
    }

    pub fn update_playtime(
        &self,
        id: &str,
        additional_seconds: u64,
        last_played_timestamp: u64,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE games
             SET playtime_s = playtime_s + ?1, last_played = ?2
             WHERE id = ?3",
            params![additional_seconds, last_played_timestamp, id],
        )?;
        Ok(())
    }

    pub fn update_artwork(&self, id: &str, grid: Option<&str>, hero: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE games
             SET artwork_grid = COALESCE(?1, artwork_grid),
                 artwork_hero = COALESCE(?2, artwork_hero)
             WHERE id = ?3",
            params![grid, hero, id],
        )?;
        Ok(())
    }
}
