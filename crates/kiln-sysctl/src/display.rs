//! Display switching and snapshotting via QueryDisplayConfig / SetDisplayConfig.
use kiln_core::error::SysctlError;
use kiln_core::journal::{DisplaySnapshot, DisplayTopology};
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
            use windows::Win32::Devices::Display::{
                GetDisplayConfigBufferSizes, QueryDisplayConfig, DISPLAYCONFIG_MODE_INFO,
                DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL, DISPLAYCONFIG_PATH_INFO, QDC_ALL_PATHS,
            };

            let mut num_paths = 0u32;
            let mut num_modes = 0u32;

            unsafe {
                let status =
                    GetDisplayConfigBufferSizes(QDC_ALL_PATHS, &mut num_paths, &mut num_modes);
                if status.is_err() {
                    return Err(SysctlError::Win32(
                        "GetDisplayConfigBufferSizes failed".to_string(),
                        status.0,
                    ));
                }

                let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); num_paths as usize];
                let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); num_modes as usize];

                let status = QueryDisplayConfig(
                    QDC_ALL_PATHS,
                    &mut num_paths,
                    paths.as_mut_ptr(),
                    &mut num_modes,
                    modes.as_mut_ptr(),
                    None,
                );

                if status.is_err() {
                    return Err(SysctlError::Win32(
                        "QueryDisplayConfig failed".to_string(),
                        status.0,
                    ));
                }

                paths.truncate(num_paths as usize);
                modes.truncate(num_modes as usize);

                let mut has_external = false;
                for path in &paths {
                    if (path.flags & 1) != 0
                        && path.targetInfo.outputTechnology
                            != DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL
                    {
                        has_external = true;
                        break;
                    }
                }

                Ok(DisplaySnapshot {
                    topology: if has_external {
                        DisplayTopology::Extend
                    } else {
                        DisplayTopology::Internal
                    },
                    external_connected: has_external,
                    raw_paths: None,
                })
            }
        }
        #[cfg(not(windows))]
        {
            Ok(DisplaySnapshot {
                topology: DisplayTopology::Internal,
                external_connected: true,
                raw_paths: None,
            })
        }
    }

    fn set_topology(&self, topology: DisplayTopology) -> Result<(), SysctlError> {
        #[cfg(windows)]
        {
            use windows::Win32::Devices::Display::{
                SetDisplayConfig, SDC_APPLY, SDC_TOPOLOGY_CLONE, SDC_TOPOLOGY_EXTEND,
                SDC_TOPOLOGY_EXTERNAL, SDC_TOPOLOGY_INTERNAL,
            };

            let flag = match topology {
                DisplayTopology::Internal => SDC_TOPOLOGY_INTERNAL,
                DisplayTopology::External => SDC_TOPOLOGY_EXTERNAL,
                DisplayTopology::Clone => SDC_TOPOLOGY_CLONE,
                DisplayTopology::Extend => SDC_TOPOLOGY_EXTEND,
            };

            unsafe {
                let status = SetDisplayConfig(None, None, SDC_APPLY | flag);
                if status != 0 {
                    return Err(SysctlError::Win32(
                        format!("SetDisplayConfig failed for topology {:?}", topology),
                        status as u32,
                    ));
                }
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            tracing::info!(?topology, "Mock display topology set");
            Ok(())
        }
    }

    fn restore_snapshot(&self, snapshot: &DisplaySnapshot) -> Result<(), SysctlError> {
        #[cfg(windows)]
        {
            if let Err(e) = self.set_topology(snapshot.topology) {
                tracing::warn!(error = %e, "Failed to restore snapshot topology, falling back to Internal");
                self.set_topology(DisplayTopology::Internal)?;
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            tracing::info!(?snapshot.topology, "Mock display snapshot restored");
            Ok(())
        }
    }

    fn is_external_connected(&self) -> Result<bool, SysctlError> {
        let snapshot = self.snapshot_paths()?;
        Ok(snapshot.external_connected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_api_lifecycle() {
        let display = WindowsDisplay::new();
        let snapshot = display.snapshot_paths().expect("snapshot should succeed");
        assert!(display.set_topology(DisplayTopology::Internal).is_ok());
        assert!(display.restore_snapshot(&snapshot).is_ok());
    }
}
