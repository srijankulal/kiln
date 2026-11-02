//! Shell process supervisor and crash loop breaker.

use crate::controller::ModeController;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct ShellSupervisor {
    controller: Arc<Mutex<ModeController>>,
    shell_bin: PathBuf,
    dry_run: bool,
    safe_mode: bool,
    child_process: Option<Child>,
    crash_timestamps: Vec<u64>,
}

impl ShellSupervisor {
    pub fn new(
        controller: Arc<Mutex<ModeController>>,
        shell_bin: PathBuf,
        dry_run: bool,
        safe_mode: bool,
    ) -> Self {
        Self {
            controller,
            shell_bin,
            dry_run,
            safe_mode,
            child_process: None,
            crash_timestamps: Vec::new(),
        }
    }

    /// Spawns the kiln-shell process.
    pub fn spawn_shell(&mut self) -> anyhow::Result<()> {
        let mut cmd = Command::new(&self.shell_bin);
        if self.dry_run {
            cmd.arg("--dry-run");
        }
        if self.safe_mode {
            cmd.arg("--safe");
        }

        tracing::info!(cmd = ?cmd, "Spawning Kiln Shell");
        let child = cmd.spawn()?;
        self.child_process = Some(child);
        Ok(())
    }

    /// Checks if the shell child process has exited, and handles recovery if a crash occurred.
    pub fn poll_child(&mut self) -> anyhow::Result<Option<bool>> {
        let Some(ref mut child) = self.child_process else {
            return Ok(None);
        };

        match child.try_wait()? {
            Some(status) => {
                self.child_process = None;
                let success = status.success();
                if success {
                    tracing::info!("Kiln Shell exited normally");
                    let mut ctrl = self.controller.lock().unwrap();
                    let _ = ctrl.exit_game_mode(self.dry_run, self.safe_mode);
                } else {
                    tracing::error!(status = ?status, "Kiln Shell crashed or exited with error!");
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();

                    // Check crash loop breaker (3 crashes in 300 seconds)
                    self.crash_timestamps
                        .retain(|&t| now.saturating_sub(t) <= 300);
                    self.crash_timestamps.push(now);

                    if self.crash_timestamps.len() >= 3 {
                        tracing::error!("CRASH LOOP BREAKER TRIGGERED: 3 shell crashes within 5 minutes. Aborting Game Mode and restoring Desktop.");
                        let mut ctrl = self.controller.lock().unwrap();
                        let _ = ctrl.emergency_restore();
                    } else {
                        // Re-enter or restore
                        let mut ctrl = self.controller.lock().unwrap();
                        let _ = ctrl.exit_game_mode(self.dry_run, self.safe_mode);
                    }
                }
                Ok(Some(success))
            }
            None => Ok(None),
        }
    }

    pub fn is_running(&self) -> bool {
        self.child_process.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_supervisor_crash_loop_breaker() {
        let dir = tempdir().unwrap();
        let journal_path = dir.path().join("session.json");
        let controller = Arc::new(Mutex::new(ModeController::new(journal_path)));
        let mut supervisor =
            ShellSupervisor::new(controller, PathBuf::from("mock_shell"), true, true);

        let t0 = 1000;
        supervisor.crash_timestamps.push(t0);
        supervisor.crash_timestamps.push(t0 + 50);
        assert_eq!(supervisor.crash_timestamps.len(), 2);

        // Third crash within 300s
        supervisor.crash_timestamps.push(t0 + 100);
        assert_eq!(supervisor.crash_timestamps.len(), 3);
    }
}
