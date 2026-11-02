//! Taskbar and shell visibility management.
use kiln_core::error::SysctlError;
use kiln_core::journal::TaskbarSnapshot;
use kiln_core::traits::TaskbarApi;

pub struct WindowsTaskbar;

impl WindowsTaskbar {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsTaskbar {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskbarApi for WindowsTaskbar {
    fn snapshot_taskbar(&self) -> Result<TaskbarSnapshot, SysctlError> {
        Ok(TaskbarSnapshot {
            primary_visible: true,
            secondary_visible: true,
        })
    }

    fn set_visible(&self, _visible: bool) -> Result<(), SysctlError> {
        Ok(())
    }

    fn restore_snapshot(&self, _snapshot: &TaskbarSnapshot) -> Result<(), SysctlError> {
        Ok(())
    }
}
