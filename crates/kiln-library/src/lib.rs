//! Kiln library: Scanners, database store, and artwork cache.
use kiln_core::models::DiscoveredGame;

pub trait GameScanner: Send + Sync {
    fn scan(&self) -> Result<Vec<DiscoveredGame>, String>;
}
