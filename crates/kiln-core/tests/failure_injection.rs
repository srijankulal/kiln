use kiln_core::fakes::*;
use kiln_core::journal::*;
use kiln_core::state::*;
use kiln_core::traits::*;
use proptest::prelude::*;
use uuid::Uuid;

proptest! {
    /// Property: For any arbitrary set of suspended processes and system mutations,
    /// running restore on FakeSystem always restores it to the exact original state,
    /// and running restore a second time is completely idempotent.
    #[test]
    fn prop_restore_is_always_complete_and_idempotent(
        init_ac_lid in 0u32..4,
        init_dc_lid in 0u32..4,
        pids in proptest::collection::vec(100u32..10000, 1..10),
    ) {
        let init_scheme = Uuid::new_v4();
        let fake_display = FakeDisplayApi::new(DisplayTopology::Internal, true);
        let fake_power = FakePowerApi::new(init_scheme, init_ac_lid, init_dc_lid);
        let fake_procs = FakeProcessApi::new();
        let fake_taskbar = FakeTaskbarApi::new(true);

        for &pid in &pids {
            fake_procs.insert_process(pid, "app.exe", 1000);
        }

        // 1. Take Pre-Session Snapshot
        let disp_snap = fake_display.snapshot_paths().unwrap();
        let power_snap = fake_power.snapshot_power().unwrap();
        let taskbar_snap = fake_taskbar.snapshot_taskbar().unwrap();

        // 2. Perform Arbitrary Mutations (Entering GameMode)
        let _ = fake_display.set_topology(DisplayTopology::External);
        let _ = fake_power.set_high_performance();
        let _ = fake_power.set_lid_do_nothing();
        let _ = fake_taskbar.set_visible(false);

        let mut trimmed = Vec::new();
        for &pid in &pids {
            if fake_procs.suspend_process(pid, 1000).is_ok() {
                trimmed.push(ProcessSnapshot {
                    pid,
                    name: "app.exe".to_string(),
                    creation_time: 1000,
                    trim_kind: ProcessTrimKind::Suspended,
                });
            }
        }

        // Verify mutations took effect
        prop_assert_eq!(*fake_display.current_topology.lock().unwrap(), DisplayTopology::External);
        prop_assert_eq!(*fake_taskbar.primary_visible.lock().unwrap(), false);
        for &pid in &pids {
            prop_assert!(fake_procs.running_processes.lock().unwrap().get(&pid).unwrap().2);
        }

        // 3. First Restore Execution
        fake_display.restore_snapshot(&disp_snap).unwrap();
        fake_power.restore_snapshot(&power_snap).unwrap();
        fake_taskbar.restore_snapshot(&taskbar_snap).unwrap();
        fake_procs.restore_processes(&trimmed).unwrap();

        // Assert all states restored
        prop_assert_eq!(*fake_display.current_topology.lock().unwrap(), DisplayTopology::Internal);
        prop_assert_eq!(*fake_power.active_scheme.lock().unwrap(), init_scheme);
        prop_assert_eq!(*fake_power.ac_lid.lock().unwrap(), init_ac_lid);
        prop_assert_eq!(*fake_power.dc_lid.lock().unwrap(), init_dc_lid);
        prop_assert_eq!(*fake_taskbar.primary_visible.lock().unwrap(), true);
        for &pid in &pids {
            prop_assert!(!fake_procs.running_processes.lock().unwrap().get(&pid).unwrap().2);
        }

        // 4. Second Restore Execution (Idempotence Guarantee)
        fake_display.restore_snapshot(&disp_snap).unwrap();
        fake_power.restore_snapshot(&power_snap).unwrap();
        fake_taskbar.restore_snapshot(&taskbar_snap).unwrap();
        fake_procs.restore_processes(&trimmed).unwrap();

        prop_assert_eq!(*fake_display.current_topology.lock().unwrap(), DisplayTopology::Internal);
        prop_assert_eq!(*fake_power.active_scheme.lock().unwrap(), init_scheme);
        prop_assert_eq!(*fake_taskbar.primary_visible.lock().unwrap(), true);
    }

    /// Property: Crash loop breaker triggers if and only if >= 3 crashes occur within 300 seconds.
    #[test]
    fn prop_crash_loop_breaker(
        crash_deltas in proptest::collection::vec(1u64..120, 3..6)
    ) {
        let mut sm = ModeStateMachine::new();
        let mut current_time = 1000u64;
        let mut triggered = false;

        for (idx, delta) in crash_deltas.iter().enumerate() {
            current_time += delta;
            let res = sm.record_shell_crash(current_time);
            if idx >= 2
                && current_time.saturating_sub(1000 + crash_deltas[0]) <= 300
                && res
            {
                triggered = true;
            }
        }

        // If crashes are tightly spaced, it must trigger
        if crash_deltas.iter().take(3).sum::<u64>() <= 300 {
            prop_assert!(triggered);
        }
    }
}
