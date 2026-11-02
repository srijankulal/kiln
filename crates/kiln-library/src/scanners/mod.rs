//! Game scanners for Steam, Epic, Xbox/MS Store, and Standalone directories.

pub mod epic;
pub mod standalone;
pub mod steam;
pub mod xbox;

use kiln_core::models::DiscoveredGame;
use std::collections::HashSet;

pub trait GameScanner: Send + Sync {
    fn name(&self) -> &'static str;
    fn scan(&self) -> Result<Vec<DiscoveredGame>, String>;
}

/// Deduplicates discovered games by canonicalized install directory.
/// When two sources report the same install directory, manual > direct > launcher.
pub fn deduplicate_games(games: Vec<DiscoveredGame>) -> Vec<DiscoveredGame> {
    let mut seen_dirs = HashSet::new();
    let mut deduplicated = Vec::new();

    // Sort so manual entries come first, then standalone direct exe, then launcher URIs
    let mut sorted = games;
    sorted.sort_by(|a, b| {
        let score_a = match a.source {
            kiln_core::models::GameSource::Manual => 10,
            kiln_core::models::GameSource::Standalone => 8,
            _ => 5,
        };
        let score_b = match b.source {
            kiln_core::models::GameSource::Manual => 10,
            kiln_core::models::GameSource::Standalone => 8,
            _ => 5,
        };
        score_b.cmp(&score_a)
    });

    for game in sorted {
        let norm_dir = normalize_path(&game.install_dir);
        if seen_dirs.insert(norm_dir) {
            deduplicated.push(game);
        }
    }

    deduplicated
}

fn normalize_path(p: &str) -> String {
    let clean = p.replace('\\', "/");
    let trimmed = clean.trim_end_matches('/');
    trimmed.to_lowercase()
}
