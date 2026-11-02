//! Power scheme and lid action management.
use kiln_core::error::SysctlError;
use kiln_core::journal::PowerSnapshot;
use kiln_core::traits::PowerApi;
use uuid::Uuid;

pub struct WindowsPower;

impl WindowsPower {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsPower {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerApi for WindowsPower {
    fn snapshot_power(&self) -> Result<PowerSnapshot, SysctlError> {
        Ok(PowerSnapshot {
            active_scheme: Uuid::nil(),
            ac_lid_action: 1, // Sleep
            dc_lid_action: 1, // Sleep
        })
    }

    fn set_high_performance(&self) -> Result<(), SysctlError> {
        Ok(())
    }

    fn set_lid_do_nothing(&self) -> Result<(), SysctlError> {
        Ok(())
    }

    fn restore_snapshot(&self, _snapshot: &PowerSnapshot) -> Result<(), SysctlError> {
        Ok(())
    }

    fn prevent_sleep(&self, _prevent: bool) -> Result<(), SysctlError> {
        Ok(())
    }
}
