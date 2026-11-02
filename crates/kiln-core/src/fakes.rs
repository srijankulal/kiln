//! Deterministic Fake* doubles for unit testing and failure-injection on Linux / CI.
use crate::error::SysctlError;
use crate::journal::{
    DisplaySnapshot, DisplayTopology, PowerSnapshot, ProcessSnapshot, TaskbarSnapshot,
};
use crate::traits::{AudioApi, Clock, DisplayApi, PowerApi, ProcessApi, TaskbarApi};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct FakeDisplayApi {
    pub current_topology: Arc<Mutex<DisplayTopology>>,
    pub external_connected: Arc<Mutex<bool>>,
    pub fail_next: Arc<Mutex<Option<String>>>,
}

impl FakeDisplayApi {
    pub fn new(initial: DisplayTopology, external_connected: bool) -> Self {
        Self {
            current_topology: Arc::new(Mutex::new(initial)),
            external_connected: Arc::new(Mutex::new(external_connected)),
            fail_next: Arc::new(Mutex::new(None)),
        }
    }
}

impl DisplayApi for FakeDisplayApi {
    fn snapshot_paths(&self) -> Result<DisplaySnapshot, SysctlError> {
        if let Some(err) = self.fail_next.lock().unwrap().take() {
            return Err(SysctlError::Display(err));
        }
        Ok(DisplaySnapshot {
            topology: *self.current_topology.lock().unwrap(),
            external_connected: *self.external_connected.lock().unwrap(),
            raw_paths: None,
        })
    }

    fn set_topology(&self, topology: DisplayTopology) -> Result<(), SysctlError> {
        if let Some(err) = self.fail_next.lock().unwrap().take() {
            return Err(SysctlError::Display(err));
        }
        *self.current_topology.lock().unwrap() = topology;
        Ok(())
    }

    fn restore_snapshot(&self, snapshot: &DisplaySnapshot) -> Result<(), SysctlError> {
        *self.current_topology.lock().unwrap() = snapshot.topology;
        *self.external_connected.lock().unwrap() = snapshot.external_connected;
        Ok(())
    }

    fn is_external_connected(&self) -> Result<bool, SysctlError> {
        Ok(*self.external_connected.lock().unwrap())
    }
}

#[derive(Debug, Clone)]
pub struct FakePowerApi {
    pub active_scheme: Arc<Mutex<Uuid>>,
    pub ac_lid: Arc<Mutex<u32>>,
    pub dc_lid: Arc<Mutex<u32>>,
    pub sleep_prevented: Arc<Mutex<bool>>,
    pub fail_next: Arc<Mutex<Option<String>>>,
}

impl FakePowerApi {
    pub fn new(scheme: Uuid, ac_lid: u32, dc_lid: u32) -> Self {
        Self {
            active_scheme: Arc::new(Mutex::new(scheme)),
            ac_lid: Arc::new(Mutex::new(ac_lid)),
            dc_lid: Arc::new(Mutex::new(dc_lid)),
            sleep_prevented: Arc::new(Mutex::new(false)),
            fail_next: Arc::new(Mutex::new(None)),
        }
    }
}

impl PowerApi for FakePowerApi {
    fn snapshot_power(&self) -> Result<PowerSnapshot, SysctlError> {
        if let Some(err) = self.fail_next.lock().unwrap().take() {
            return Err(SysctlError::Power(err));
        }
        Ok(PowerSnapshot {
            active_scheme: *self.active_scheme.lock().unwrap(),
            ac_lid_action: *self.ac_lid.lock().unwrap(),
            dc_lid_action: *self.dc_lid.lock().unwrap(),
        })
    }

    fn set_high_performance(&self) -> Result<(), SysctlError> {
        if let Some(err) = self.fail_next.lock().unwrap().take() {
            return Err(SysctlError::Power(err));
        }
        *self.active_scheme.lock().unwrap() = Uuid::new_v4();
        Ok(())
    }

    fn set_lid_do_nothing(&self) -> Result<(), SysctlError> {
        if let Some(err) = self.fail_next.lock().unwrap().take() {
            return Err(SysctlError::Power(err));
        }
        *self.ac_lid.lock().unwrap() = 0;
        *self.dc_lid.lock().unwrap() = 0;
        Ok(())
    }

    fn restore_snapshot(&self, snapshot: &PowerSnapshot) -> Result<(), SysctlError> {
        *self.active_scheme.lock().unwrap() = snapshot.active_scheme;
        *self.ac_lid.lock().unwrap() = snapshot.ac_lid_action;
        *self.dc_lid.lock().unwrap() = snapshot.dc_lid_action;
        *self.sleep_prevented.lock().unwrap() = false;
        Ok(())
    }

    fn prevent_sleep(&self, prevent: bool) -> Result<(), SysctlError> {
        *self.sleep_prevented.lock().unwrap() = prevent;
        Ok(())
    }
}

pub type FakeProcessRecord = (String, u64, bool, bool);

#[derive(Debug, Clone)]
pub struct FakeProcessApi {
    /// PID -> (name, creation_time, is_suspended, is_eco)
    pub running_processes: Arc<Mutex<HashMap<u32, FakeProcessRecord>>>,
    pub fail_next: Arc<Mutex<Option<String>>>,
}

impl FakeProcessApi {
    pub fn new() -> Self {
        Self {
            running_processes: Arc::new(Mutex::new(HashMap::new())),
            fail_next: Arc::new(Mutex::new(None)),
        }
    }

    pub fn insert_process(&self, pid: u32, name: &str, creation_time: u64) {
        self.running_processes
            .lock()
            .unwrap()
            .insert(pid, (name.to_string(), creation_time, false, false));
    }
}

impl Default for FakeProcessApi {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessApi for FakeProcessApi {
    fn set_eco_qos(&self, pid: u32, enable: bool) -> Result<(), SysctlError> {
        if let Some(err) = self.fail_next.lock().unwrap().take() {
            return Err(SysctlError::Process(err));
        }
        if let Some(entry) = self.running_processes.lock().unwrap().get_mut(&pid) {
            entry.3 = enable;
            Ok(())
        } else {
            Err(SysctlError::Process(format!("PID {} not found", pid)))
        }
    }

    fn suspend_process(&self, pid: u32, creation_time: u64) -> Result<(), SysctlError> {
        if let Some(err) = self.fail_next.lock().unwrap().take() {
            return Err(SysctlError::Process(err));
        }
        if let Some(entry) = self.running_processes.lock().unwrap().get_mut(&pid) {
            if entry.1 == creation_time {
                entry.2 = true;
                Ok(())
            } else {
                Err(SysctlError::Process("Creation time mismatch".to_string()))
            }
        } else {
            Err(SysctlError::Process(format!("PID {} not found", pid)))
        }
    }

    fn resume_process(&self, pid: u32, creation_time: u64) -> Result<(), SysctlError> {
        if let Some(entry) = self.running_processes.lock().unwrap().get_mut(&pid) {
            if entry.1 == creation_time {
                entry.2 = false;
                entry.3 = false;
                Ok(())
            } else {
                // Safely skipped on PID mismatch
                Ok(())
            }
        } else {
            // Safely skipped on exited process
            Ok(())
        }
    }

    fn is_process_running(&self, pid: u32, expected_creation_time: u64) -> bool {
        if let Some(entry) = self.running_processes.lock().unwrap().get(&pid) {
            entry.1 == expected_creation_time
        } else {
            false
        }
    }

    fn restore_processes(&self, processes: &[ProcessSnapshot]) -> Result<(), SysctlError> {
        for proc in processes {
            let _ = self.resume_process(proc.pid, proc.creation_time);
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct FakeTaskbarApi {
    pub primary_visible: Arc<Mutex<bool>>,
    pub secondary_visible: Arc<Mutex<bool>>,
}

impl FakeTaskbarApi {
    pub fn new(visible: bool) -> Self {
        Self {
            primary_visible: Arc::new(Mutex::new(visible)),
            secondary_visible: Arc::new(Mutex::new(visible)),
        }
    }
}

impl TaskbarApi for FakeTaskbarApi {
    fn snapshot_taskbar(&self) -> Result<TaskbarSnapshot, SysctlError> {
        Ok(TaskbarSnapshot {
            primary_visible: *self.primary_visible.lock().unwrap(),
            secondary_visible: *self.secondary_visible.lock().unwrap(),
        })
    }

    fn set_visible(&self, visible: bool) -> Result<(), SysctlError> {
        *self.primary_visible.lock().unwrap() = visible;
        *self.secondary_visible.lock().unwrap() = visible;
        Ok(())
    }

    fn restore_snapshot(&self, snapshot: &TaskbarSnapshot) -> Result<(), SysctlError> {
        *self.primary_visible.lock().unwrap() = snapshot.primary_visible;
        *self.secondary_visible.lock().unwrap() = snapshot.secondary_visible;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct FakeAudioApi {
    pub current_endpoint: Arc<Mutex<String>>,
}

impl FakeAudioApi {
    pub fn new(endpoint: &str) -> Self {
        Self {
            current_endpoint: Arc::new(Mutex::new(endpoint.to_string())),
        }
    }
}

impl AudioApi for FakeAudioApi {
    fn get_default_endpoint(&self) -> Result<String, SysctlError> {
        Ok(self.current_endpoint.lock().unwrap().clone())
    }

    fn set_default_endpoint(&self, endpoint_id: &str) -> Result<(), SysctlError> {
        *self.current_endpoint.lock().unwrap() = endpoint_id.to_string();
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct FakeClock {
    pub time: Arc<Mutex<u64>>,
}

impl FakeClock {
    pub fn new(start_time: u64) -> Self {
        Self {
            time: Arc::new(Mutex::new(start_time)),
        }
    }

    pub fn advance(&self, seconds: u64) {
        *self.time.lock().unwrap() += seconds;
    }
}

impl Clock for FakeClock {
    fn now_seconds(&self) -> u64 {
        *self.time.lock().unwrap()
    }
}
