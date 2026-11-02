//! Process lifecycle tracking for direct exe and launcher-managed game sessions.

use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStatus {
    Starting,
    Running { pid: u32 },
    Exited { duration_seconds: u64 },
}

pub struct ActiveGameSession {
    pub game_id: String,
    pub install_dir: PathBuf,
    pub started_at: Instant,
    pub started_launcher_for_session: bool,
    pub status: SessionStatus,
}

impl ActiveGameSession {
    pub fn new(game_id: String, install_dir: PathBuf, started_launcher_for_session: bool) -> Self {
        Self {
            game_id,
            install_dir,
            started_at: Instant::now(),
            started_launcher_for_session,
            status: SessionStatus::Starting,
        }
    }

    pub fn elapsed_seconds(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    /// Checks if a given executable path belongs to this game's installation directory.
    pub fn is_image_inside_install_dir(&self, image_path: &Path) -> bool {
        image_path.starts_with(&self.install_dir)
    }

    pub fn mark_running(&mut self, pid: u32) {
        self.status = SessionStatus::Running { pid };
    }

    pub fn mark_exited(&mut self) -> u64 {
        let dur = self.elapsed_seconds();
        self.status = SessionStatus::Exited {
            duration_seconds: dur,
        };
        dur
    }
}
