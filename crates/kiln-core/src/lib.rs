//! Kiln Core: Shared types, journal engine, abstract traits, state machine, and IPC contracts.

pub mod config;
pub mod error;
pub mod fakes;
pub mod ipc;
pub mod journal;
pub mod models;
pub mod state;
pub mod traits;
pub mod version;

#[cfg(test)]
mod tests {
    use super::fakes::*;
    use super::journal::*;
    use super::state::*;
    use super::traits::*;
    use tempfile::NamedTempFile;
    use uuid::Uuid;

    #[test]
    fn test_journal_atomic_save_and_load() {
        let temp = NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();

        let mut journal = SessionJournal::new(1700000000);
        journal.display = Some(DisplaySnapshot {
            topology: DisplayTopology::Internal,
            external_connected: true,
            raw_paths: None,
        });
        journal.power = Some(PowerSnapshot {
            active_scheme: Uuid::new_v4(),
            ac_lid_action: 1,
            dc_lid_action: 1,
        });
        journal.taskbar = Some(TaskbarSnapshot {
            primary_visible: true,
            secondary_visible: true,
        });
        journal.trimmed_processes.push(ProcessSnapshot {
            pid: 1234,
            name: "Discord.exe".to_string(),
            creation_time: 99999,
            trim_kind: ProcessTrimKind::Suspended,
        });

        journal.save_atomic(&path).unwrap();
        let loaded = SessionJournal::load_from_disk(&path).unwrap().unwrap();

        assert_eq!(journal.session_id, loaded.session_id);
        assert_eq!(journal.display, loaded.display);
        assert_eq!(journal.power, loaded.power);
        assert_eq!(journal.taskbar, loaded.taskbar);
        assert_eq!(journal.trimmed_processes, loaded.trimmed_processes);

        SessionJournal::remove_journal(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn test_state_machine_valid_transitions() {
        let mut sm = ModeStateMachine::new();
        assert_eq!(sm.state(), ModeState::Desktop);

        sm.transition(ModeState::Entering).unwrap();
        assert_eq!(sm.state(), ModeState::Entering);

        sm.transition(ModeState::ConfirmingDisplay {
            deadline_seconds: 15,
        })
        .unwrap();
        sm.transition(ModeState::GameMode).unwrap();
        assert_eq!(sm.state(), ModeState::GameMode);

        let game_id = [1u8; 16];
        sm.transition(ModeState::Launching { game_id }).unwrap();
        sm.transition(ModeState::InGame { game_id }).unwrap();
        sm.transition(ModeState::GameMode).unwrap();

        sm.transition(ModeState::Exiting).unwrap();
        sm.transition(ModeState::Desktop).unwrap();
        assert_eq!(sm.state(), ModeState::Desktop);
    }

    #[test]
    fn test_state_machine_invalid_transition() {
        let mut sm = ModeStateMachine::new();
        let game_id = [1u8; 16];
        assert!(sm.transition(ModeState::InGame { game_id }).is_err());
    }

    #[test]
    fn test_crash_loop_breaker() {
        let mut sm = ModeStateMachine::new();
        assert!(!sm.record_shell_crash(100));
        assert!(!sm.record_shell_crash(150));
        assert!(sm.record_shell_crash(200)); // 3rd crash within 300s -> true!
    }

    #[test]
    fn test_fake_system_idempotent_restore() {
        let fake_display = FakeDisplayApi::new(DisplayTopology::Internal, true);
        let fake_power = FakePowerApi::new(Uuid::nil(), 1, 1);
        let fake_procs = FakeProcessApi::new();
        let fake_taskbar = FakeTaskbarApi::new(true);

        fake_procs.insert_process(456, "Spotify.exe", 1111);

        // Snapshot
        let disp_snap = fake_display.snapshot_paths().unwrap();
        let power_snap = fake_power.snapshot_power().unwrap();
        let taskbar_snap = fake_taskbar.snapshot_taskbar().unwrap();

        // Mutate
        fake_display
            .set_topology(DisplayTopology::External)
            .unwrap();
        fake_power.set_high_performance().unwrap();
        fake_power.set_lid_do_nothing().unwrap();
        fake_taskbar.set_visible(false).unwrap();
        fake_procs.suspend_process(456, 1111).unwrap();

        assert_eq!(
            *fake_display.current_topology.lock().unwrap(),
            DisplayTopology::External
        );
        assert!(!*fake_taskbar.primary_visible.lock().unwrap());
        assert!(
            fake_procs
                .running_processes
                .lock()
                .unwrap()
                .get(&456)
                .unwrap()
                .2
        );

        // Restore 1st time
        fake_display.restore_snapshot(&disp_snap).unwrap();
        fake_power.restore_snapshot(&power_snap).unwrap();
        fake_taskbar.restore_snapshot(&taskbar_snap).unwrap();
        fake_procs
            .restore_processes(&[ProcessSnapshot {
                pid: 456,
                name: "Spotify.exe".to_string(),
                creation_time: 1111,
                trim_kind: ProcessTrimKind::Suspended,
            }])
            .unwrap();

        assert_eq!(
            *fake_display.current_topology.lock().unwrap(),
            DisplayTopology::Internal
        );
        assert!(*fake_taskbar.primary_visible.lock().unwrap());
        assert!(
            !fake_procs
                .running_processes
                .lock()
                .unwrap()
                .get(&456)
                .unwrap()
                .2
        );

        // Restore 2nd time (Idempotence)
        fake_display.restore_snapshot(&disp_snap).unwrap();
        fake_power.restore_snapshot(&power_snap).unwrap();
        fake_taskbar.restore_snapshot(&taskbar_snap).unwrap();
        fake_procs
            .restore_processes(&[ProcessSnapshot {
                pid: 456,
                name: "Spotify.exe".to_string(),
                creation_time: 1111,
                trim_kind: ProcessTrimKind::Suspended,
            }])
            .unwrap();

        assert_eq!(
            *fake_display.current_topology.lock().unwrap(),
            DisplayTopology::Internal
        );
    }
}
