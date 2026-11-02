//! Abstract trait interfaces for OS boundaries and time.
use crate::error::SysctlError;
use crate::journal::{
    DisplaySnapshot, DisplayTopology, PowerSnapshot, ProcessSnapshot, TaskbarSnapshot,
};

pub trait DisplayApi: Send + Sync {
    fn snapshot_paths(&self) -> Result<DisplaySnapshot, SysctlError>;
    fn set_topology(&self, topology: DisplayTopology) -> Result<(), SysctlError>;
    fn restore_snapshot(&self, snapshot: &DisplaySnapshot) -> Result<(), SysctlError>;
    fn is_external_connected(&self) -> Result<bool, SysctlError>;
}

pub trait PowerApi: Send + Sync {
    fn snapshot_power(&self) -> Result<PowerSnapshot, SysctlError>;
    fn set_high_performance(&self) -> Result<(), SysctlError>;
    fn set_lid_do_nothing(&self) -> Result<(), SysctlError>;
    fn restore_snapshot(&self, snapshot: &PowerSnapshot) -> Result<(), SysctlError>;
    fn prevent_sleep(&self, prevent: bool) -> Result<(), SysctlError>;
}

pub trait ProcessApi: Send + Sync {
    fn set_eco_qos(&self, pid: u32, enable: bool) -> Result<(), SysctlError>;
    fn suspend_process(&self, pid: u32, creation_time: u64) -> Result<(), SysctlError>;
    fn resume_process(&self, pid: u32, creation_time: u64) -> Result<(), SysctlError>;
    fn is_process_running(&self, pid: u32, expected_creation_time: u64) -> bool;
    fn restore_processes(&self, processes: &[ProcessSnapshot]) -> Result<(), SysctlError>;
}

pub trait TaskbarApi: Send + Sync {
    fn snapshot_taskbar(&self) -> Result<TaskbarSnapshot, SysctlError>;
    fn set_visible(&self, visible: bool) -> Result<(), SysctlError>;
    fn restore_snapshot(&self, snapshot: &TaskbarSnapshot) -> Result<(), SysctlError>;
}

pub trait AudioApi: Send + Sync {
    fn get_default_endpoint(&self) -> Result<String, SysctlError>;
    fn set_default_endpoint(&self, endpoint_id: &str) -> Result<(), SysctlError>;
}

pub trait Clock: Send + Sync {
    fn now_seconds(&self) -> u64;
}
