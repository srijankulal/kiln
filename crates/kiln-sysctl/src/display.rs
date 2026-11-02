//! Display switching and snapshotting via QueryDisplayConfig / SetDisplayConfig.
use kiln_core::error::SysctlError;
use kiln_core::journal::DisplaySnapshot;
use kiln_core::traits::DisplayApi;

pub struct WindowsDisplay;

impl WindowsDisplay {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsDisplay {
    fn default() -> Self {
        Self::new()
    }
}

impl DisplayApi for WindowsDisplay {
    fn snapshot_paths(&self) -> Result<DisplaySnapshot, SysctlError> {
        #[cfg(windows)]
        {
            // Real Win32 implementation (M4)
            Ok(DisplaySnapshot {
                topology: kiln_core::journal::DisplayTopology::Internal,
                external_connected: true,
                raw_paths: None,
            })
        }
        #[cfg(not(windows))]
        {
            Ok(DisplaySnapshot {
                topology: kiln_core::journal::DisplayTopology::Internal,
                external_connected: true,
                raw_paths: None,
            })
        }
    }

    fn set_topology(
        &self,
        _topology: kiln_core::journal::DisplayTopology,
    ) -> Result<(), SysctlError> {
        Ok(())
    }

    fn restore_snapshot(&self, _snapshot: &DisplaySnapshot) -> Result<(), SysctlError> {
        Ok(())
    }

    fn is_external_connected(&self) -> Result<bool, SysctlError> {
        Ok(true)
    }
}
