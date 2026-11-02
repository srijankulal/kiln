//! Xbox and Microsoft Store package scanner.

use super::GameScanner;
use kiln_core::models::DiscoveredGame;

pub struct XboxScanner;

impl XboxScanner {
    pub fn new() -> Self {
        Self
    }
}

impl Default for XboxScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl GameScanner for XboxScanner {
    fn name(&self) -> &'static str {
        "Xbox"
    }

    fn scan(&self) -> Result<Vec<DiscoveredGame>, String> {
        let games = Vec::new();

        #[cfg(windows)]
        {
            // On Windows: query PackageManager for modern appx packages
            // with XboxLive or GameExplorer capabilities.
            // Placeholder for WinRT integration in M1/M2.
        }

        Ok(games)
    }
}
