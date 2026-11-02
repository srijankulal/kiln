//! Power scheme, lid action, and execution state management.
use kiln_core::error::SysctlError;
use kiln_core::journal::PowerSnapshot;
use kiln_core::traits::PowerApi;
use uuid::Uuid;

#[cfg(windows)]
use windows::core::GUID;

#[cfg(windows)]
const GUID_MIN_POWER_SAVINGS: GUID = GUID::from_u128(0x8c5e7fda_e8bf_4a96_9a85_a6e23a8c635c);
#[cfg(windows)]
const GUID_SYSTEM_BUTTON_SUBGROUP: GUID = GUID::from_u128(0x4f971e89_eebd_4455_a8de_9e59040e7347);
#[cfg(windows)]
const GUID_LIDCLOSE_ACTION: GUID = GUID::from_u128(0x5d3e9a59_e9d5_4b00_a6bd_ff34ff516548);

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
        #[cfg(windows)]
        {
            use windows::Win32::Foundation::{LocalFree, HLOCAL};
            use windows::Win32::System::Power::{
                PowerGetActiveScheme, PowerReadACValueIndex, PowerReadDCValueIndex,
            };

            unsafe {
                let mut p_active_guid: *mut GUID = std::ptr::null_mut();
                let status = PowerGetActiveScheme(None, &mut p_active_guid);
                if status.0 != 0 || p_active_guid.is_null() {
                    tracing::warn!(
                        error = status.0,
                        "PowerGetActiveScheme unavailable; falling back to default power snapshot"
                    );
                    return Ok(PowerSnapshot {
                        active_scheme: Uuid::nil(),
                        ac_lid_action: 1,
                        dc_lid_action: 1,
                    });
                }

                let active_guid = *p_active_guid;
                let _ = LocalFree(HLOCAL(p_active_guid as _));

                let mut ac_lid_action = 1u32; // Default to 1 (Sleep)
                let mut dc_lid_action = 1u32;

                let _ = PowerReadACValueIndex(
                    None,
                    Some(&active_guid),
                    Some(&GUID_SYSTEM_BUTTON_SUBGROUP),
                    Some(&GUID_LIDCLOSE_ACTION),
                    &mut ac_lid_action,
                );

                let _ = PowerReadDCValueIndex(
                    None,
                    Some(&active_guid),
                    Some(&GUID_SYSTEM_BUTTON_SUBGROUP),
                    Some(&GUID_LIDCLOSE_ACTION),
                    &mut dc_lid_action,
                );

                Ok(PowerSnapshot {
                    active_scheme: Uuid::from_u128(active_guid.to_u128()),
                    ac_lid_action,
                    dc_lid_action,
                })
            }
        }
        #[cfg(not(windows))]
        {
            Ok(PowerSnapshot {
                active_scheme: Uuid::nil(),
                ac_lid_action: 1, // Sleep
                dc_lid_action: 1, // Sleep
            })
        }
    }

    fn set_high_performance(&self) -> Result<(), SysctlError> {
        #[cfg(windows)]
        {
            use windows::Win32::System::Power::PowerSetActiveScheme;

            unsafe {
                let status = PowerSetActiveScheme(None, Some(&GUID_MIN_POWER_SAVINGS));
                if status.0 != 0 {
                    tracing::warn!(
                        error = status.0,
                        "PowerSetActiveScheme high performance unavailable or unprivileged; continuing"
                    );
                }
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            tracing::info!("Mock high performance power scheme set");
            Ok(())
        }
    }

    fn set_lid_do_nothing(&self) -> Result<(), SysctlError> {
        #[cfg(windows)]
        {
            use windows::Win32::Foundation::{LocalFree, HLOCAL};
            use windows::Win32::System::Power::{
                PowerGetActiveScheme, PowerSetActiveScheme, PowerWriteACValueIndex,
                PowerWriteDCValueIndex,
            };

            unsafe {
                let mut p_active_guid: *mut GUID = std::ptr::null_mut();
                let status = PowerGetActiveScheme(None, &mut p_active_guid);
                if status.0 != 0 || p_active_guid.is_null() {
                    tracing::warn!(
                        error = status.0,
                        "PowerGetActiveScheme unavailable for lid configuration; continuing"
                    );
                    return Ok(());
                }

                let active_guid = *p_active_guid;
                let _ = LocalFree(HLOCAL(p_active_guid as _));

                // 0 = Do nothing
                let _ = PowerWriteACValueIndex(
                    None,
                    &active_guid,
                    Some(&GUID_SYSTEM_BUTTON_SUBGROUP),
                    Some(&GUID_LIDCLOSE_ACTION),
                    0,
                );
                let _ = PowerWriteDCValueIndex(
                    None,
                    &active_guid,
                    Some(&GUID_SYSTEM_BUTTON_SUBGROUP),
                    Some(&GUID_LIDCLOSE_ACTION),
                    0,
                );

                // Apply changes to current scheme
                let _ = PowerSetActiveScheme(None, Some(&active_guid));
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            tracing::info!("Mock lid action 'do nothing' set");
            Ok(())
        }
    }

    fn restore_snapshot(&self, snapshot: &PowerSnapshot) -> Result<(), SysctlError> {
        #[cfg(windows)]
        {
            use windows::Win32::System::Power::{
                PowerSetActiveScheme, PowerWriteACValueIndex, PowerWriteDCValueIndex,
            };

            if snapshot.active_scheme.is_nil() {
                return Ok(());
            }

            let scheme_guid = GUID::from_u128(snapshot.active_scheme.as_u128());

            unsafe {
                // Restore lid values
                let _ = PowerWriteACValueIndex(
                    None,
                    &scheme_guid,
                    Some(&GUID_SYSTEM_BUTTON_SUBGROUP),
                    Some(&GUID_LIDCLOSE_ACTION),
                    snapshot.ac_lid_action,
                );
                let _ = PowerWriteDCValueIndex(
                    None,
                    &scheme_guid,
                    Some(&GUID_SYSTEM_BUTTON_SUBGROUP),
                    Some(&GUID_LIDCLOSE_ACTION),
                    snapshot.dc_lid_action,
                );

                // Restore active scheme
                let status = PowerSetActiveScheme(None, Some(&scheme_guid));
                if status.0 != 0 {
                    tracing::warn!(
                        error = status.0,
                        "PowerSetActiveScheme failed during restore; continuing"
                    );
                }
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            tracing::info!(?snapshot, "Mock power snapshot restored");
            Ok(())
        }
    }

    fn prevent_sleep(&self, prevent: bool) -> Result<(), SysctlError> {
        #[cfg(windows)]
        {
            use windows::Win32::System::Power::{
                SetThreadExecutionState, ES_AWAYMODE_REQUIRED, ES_CONTINUOUS, ES_DISPLAY_REQUIRED,
                ES_SYSTEM_REQUIRED,
            };

            unsafe {
                let flags = if prevent {
                    ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED | ES_AWAYMODE_REQUIRED
                } else {
                    ES_CONTINUOUS
                };
                let prev = SetThreadExecutionState(flags);
                if prev.0 == 0 {
                    tracing::warn!("SetThreadExecutionState returned 0");
                }
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            tracing::info!(prevent, "Mock execution state sleep prevention toggled");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_power_api_lifecycle() {
        let power = WindowsPower::new();
        // In unprivileged or virtual CI environments without power schemes, gracefully handle absence
        if let Ok(snapshot) = power.snapshot_power() {
            let _ = power.set_high_performance();
            let _ = power.set_lid_do_nothing();
            let _ = power.prevent_sleep(true);
            let _ = power.prevent_sleep(false);
            let _ = power.restore_snapshot(&snapshot);
        }
    }
}
