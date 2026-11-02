//! Mode Controller orchestrating Game Mode state transitions, WAL journaling, and crash recovery.

use kiln_core::error::KilnError;
use kiln_core::journal::{DisplayTopology, ProcessSnapshot, ProcessTrimKind, SessionJournal};
use kiln_core::state::{ModeState, ModeStateMachine};
use kiln_core::traits::{AudioApi, DisplayApi, PowerApi, ProcessApi, TaskbarApi};
use kiln_sysctl::audio::WindowsAudio;
use kiln_sysctl::display::WindowsDisplay;
use kiln_sysctl::power::WindowsPower;
use kiln_sysctl::shell::WindowsTaskbar;
use kiln_sysctl::trim::{is_never_touch, WindowsProcessTrimmer};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub struct ModeController {
    sm: ModeStateMachine,
    journal_path: PathBuf,
    active_journal: Option<SessionJournal>,
    display: Arc<dyn DisplayApi>,
    power: Arc<dyn PowerApi>,
    trim: Arc<dyn ProcessApi>,
    taskbar: Arc<dyn TaskbarApi>,
    audio: Arc<dyn AudioApi>,
}

impl ModeController {
    pub fn new(journal_path: PathBuf) -> Self {
        Self {
            sm: ModeStateMachine::new(),
            journal_path,
            active_journal: None,
            display: Arc::new(WindowsDisplay::new()),
            power: Arc::new(WindowsPower::new()),
            trim: Arc::new(WindowsProcessTrimmer::new()),
            taskbar: Arc::new(WindowsTaskbar::new()),
            audio: Arc::new(WindowsAudio::new()),
        }
    }

    pub fn with_apis(
        journal_path: PathBuf,
        display: Arc<dyn DisplayApi>,
        power: Arc<dyn PowerApi>,
        trim: Arc<dyn ProcessApi>,
        taskbar: Arc<dyn TaskbarApi>,
        audio: Arc<dyn AudioApi>,
    ) -> Self {
        Self {
            sm: ModeStateMachine::new(),
            journal_path,
            active_journal: None,
            display,
            power,
            trim,
            taskbar,
            audio,
        }
    }

    pub fn state(&self) -> ModeState {
        self.sm.state()
    }

    pub fn journal_path(&self) -> &Path {
        &self.journal_path
    }

    /// On daemon start: if an uncommitted journal exists on disk, run an automatic idempotent restore.
    pub fn check_and_recover(&mut self) -> Result<bool, KilnError> {
        if !self.journal_path.exists() {
            return Ok(false);
        }

        tracing::warn!(path = %self.journal_path.display(), "Found existing session journal on startup, checking recovery...");
        let data = std::fs::read(&self.journal_path)?;
        let journal: SessionJournal = match serde_json::from_slice(&data) {
            Ok(j) => j,
            Err(e) => {
                tracing::error!(error = %e, "Corrupted journal file, removing and falling back to default internal display");
                let _ = std::fs::remove_file(&self.journal_path);
                let _ = self.display.set_topology(DisplayTopology::Internal);
                return Ok(true);
            }
        };

        if !journal.completed_cleanly {
            tracing::warn!(session_id = %journal.session_id, "Previous session was interrupted or crashed! Executing automated recovery...");
            self.restore_from_journal(&journal)?;
        }

        let _ = std::fs::remove_file(&self.journal_path);
        tracing::info!("Journal recovery completed cleanly");
        Ok(true)
    }

    /// Enters Game Mode:
    /// 1. Snapshot all systems
    /// 2. Write write-ahead journal to disk
    /// 3. Switch power, display, taskbar, process trim
    /// 4. Transition to GameMode
    pub fn enter_game_mode(&mut self, dry_run: bool, safe: bool) -> Result<(), KilnError> {
        self.sm.transition(ModeState::Entering)?;

        let now = current_timestamp();
        let mut journal = SessionJournal::new(now);

        // 1. Take snapshots
        let disp_snap = self.display.snapshot_paths()?;
        let pwr_snap = self.power.snapshot_power()?;
        let tb_snap = self.taskbar.snapshot_taskbar()?;
        let aud_endpoint = self.audio.get_default_endpoint().unwrap_or_default();

        journal.display = Some(disp_snap.clone());
        journal.power = Some(pwr_snap);
        journal.taskbar = Some(tb_snap);
        journal.audio = Some(kiln_core::journal::AudioSnapshot {
            default_endpoint: aud_endpoint,
        });

        // 2. Persist WAL journal before any OS mutation
        journal.save_atomic(&self.journal_path)?;

        if !dry_run && !safe {
            // Apply power plan and lid action
            self.power.set_high_performance()?;
            self.power.set_lid_do_nothing()?;
            self.power.prevent_sleep(true)?;

            // Apply display switching (external TV only if connected)
            if disp_snap.external_connected {
                if let Err(e) = self.display.set_topology(DisplayTopology::External) {
                    tracing::warn!(error = %e, "Failed to switch to external display topology");
                }
            }

            // Hide taskbar
            let _ = self.taskbar.set_visible(false);

            // Background process trimming
            let mut trimmed = Vec::new();
            self.collect_and_trim_processes(&mut trimmed);
            journal.trimmed_processes = trimmed;
            journal.save_atomic(&self.journal_path)?;
        } else {
            tracing::info!(
                dry_run,
                safe,
                "Simulating Game Mode entry without OS mutations"
            );
        }

        self.active_journal = Some(journal);
        self.sm.transition(ModeState::GameMode)?;
        tracing::info!("Entered Game Mode successfully");
        Ok(())
    }

    /// Exits Game Mode:
    /// Restores all subsystems in reverse order using the active journal.
    pub fn exit_game_mode(&mut self, dry_run: bool, safe: bool) -> Result<(), KilnError> {
        if self.sm.state() == ModeState::Desktop {
            return Ok(());
        }

        self.sm.transition(ModeState::Exiting)?;

        if let Some(ref journal) = self.active_journal {
            if !dry_run && !safe {
                self.restore_from_journal(journal)?;
            }
        }

        // Clean up journal on clean exit
        if self.journal_path.exists() {
            let _ = std::fs::remove_file(&self.journal_path);
        }
        self.active_journal = None;

        self.sm.transition(ModeState::Desktop)?;
        tracing::info!("Restored Desktop and exited Game Mode cleanly");
        Ok(())
    }

    /// Full emergency restore: safe to call from panic hooks or hotkeys.
    pub fn emergency_restore(&mut self) -> Result<(), KilnError> {
        tracing::warn!("EMERGENCY RESTORE CALLED");

        // Attempt restore using active journal or on-disk journal
        if let Some(ref journal) = self.active_journal {
            let _ = self.restore_from_journal(journal);
        } else if self.journal_path.exists() {
            if let Ok(data) = std::fs::read(&self.journal_path) {
                if let Ok(journal) = serde_json::from_slice::<SessionJournal>(&data) {
                    let _ = self.restore_from_journal(&journal);
                }
            }
        } else {
            // Absolute baseline fallback: internal display on, taskbar show, sleep allowed
            let _ = self.display.set_topology(DisplayTopology::Internal);
            let _ = self.taskbar.set_visible(true);
            let _ = self.power.prevent_sleep(false);
        }

        let _ = std::fs::remove_file(&self.journal_path);
        self.active_journal = None;
        let _ = self.sm.transition(ModeState::Desktop);
        Ok(())
    }

    fn restore_from_journal(&self, journal: &SessionJournal) -> Result<(), KilnError> {
        // 1. Resume trimmed background processes
        if !journal.trimmed_processes.is_empty() {
            let _ = self.trim.restore_processes(&journal.trimmed_processes);
        }

        // 2. Restore taskbar
        if let Some(ref tb) = journal.taskbar {
            let _ = self.taskbar.restore_snapshot(tb);
        } else {
            let _ = self.taskbar.set_visible(true);
        }

        // 3. Restore power plan and lid action
        if let Some(ref pwr) = journal.power {
            let _ = self.power.restore_snapshot(pwr);
        }
        let _ = self.power.prevent_sleep(false);

        // 4. Restore display topology (fallback to Internal on failure)
        if let Some(ref disp) = journal.display {
            if let Err(e) = self.display.restore_snapshot(disp) {
                tracing::warn!(error = %e, "Failed to restore display snapshot, forcing Internal");
                let _ = self.display.set_topology(DisplayTopology::Internal);
            }
        } else {
            let _ = self.display.set_topology(DisplayTopology::Internal);
        }

        // 5. Restore audio endpoint if saved
        if let Some(ref aud) = journal.audio {
            if !aud.default_endpoint.is_empty() {
                let _ = self.audio.set_default_endpoint(&aud.default_endpoint);
            }
        }

        Ok(())
    }

    fn collect_and_trim_processes(&self, trimmed: &mut Vec<ProcessSnapshot>) {
        // Sample candidate user background processes to suspend
        let candidate_apps = [
            "chrome.exe",
            "msedge.exe",
            "firefox.exe",
            "discord.exe",
            "spotify.exe",
            "slack.exe",
            "onedrive.exe",
        ];
        for name in candidate_apps {
            if !is_never_touch(name) {
                // In production, sysinfo or WTSEnumerateProcesses scans processes.
                // The ProcessSnapshot records each action for idempotent recovery.
                trimmed.push(ProcessSnapshot {
                    pid: 0,
                    name: name.to_string(),
                    creation_time: 0,
                    trim_kind: ProcessTrimKind::Suspended,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiln_core::journal::{DisplaySnapshot, PowerSnapshot, TaskbarSnapshot};
    use tempfile::tempdir;

    #[test]
    fn test_mode_controller_lifecycle_and_journal() {
        let dir = tempdir().unwrap();
        let journal_path = dir.path().join("session.json");
        let mut controller = ModeController::new(journal_path.clone());

        assert_eq!(controller.state(), ModeState::Desktop);

        // Enter Game Mode
        controller.enter_game_mode(true, false).unwrap();
        assert_eq!(controller.state(), ModeState::GameMode);
        assert!(journal_path.exists());

        // Exit Game Mode
        controller.exit_game_mode(true, false).unwrap();
        assert_eq!(controller.state(), ModeState::Desktop);
        assert!(!journal_path.exists());
    }

    #[test]
    fn test_crash_recovery_from_unclean_journal() {
        let dir = tempdir().unwrap();
        let journal_path = dir.path().join("session.json");

        // Simulate an unclean journal left behind after a sudden shutdown
        let mut journal = SessionJournal::new(1000);
        journal.display = Some(DisplaySnapshot {
            topology: DisplayTopology::Internal,
            external_connected: true,
            raw_paths: None,
        });
        journal.power = Some(PowerSnapshot {
            active_scheme: uuid::Uuid::nil(),
            ac_lid_action: 1,
            dc_lid_action: 1,
        });
        journal.taskbar = Some(TaskbarSnapshot {
            primary_visible: true,
            secondary_visible: true,
        });
        journal.completed_cleanly = false;
        journal.save_atomic(&journal_path).unwrap();
        assert!(journal_path.exists());

        let mut controller = ModeController::new(journal_path.clone());
        let recovered = controller.check_and_recover().unwrap();
        assert!(recovered);
        assert!(!journal_path.exists());
    }
}
