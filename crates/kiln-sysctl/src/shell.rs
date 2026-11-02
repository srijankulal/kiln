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
        #[cfg(windows)]
        {
            use windows::core::w;
            use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, IsWindowVisible};

            unsafe {
                let primary_hwnd = FindWindowW(w!("Shell_TrayWnd"), None);
                let primary_visible = if let Ok(hwnd) = primary_hwnd {
                    IsWindowVisible(hwnd).as_bool()
                } else {
                    true
                };

                let secondary_hwnd = FindWindowW(w!("Shell_SecondaryTrayWnd"), None);
                let secondary_visible = if let Ok(hwnd) = secondary_hwnd {
                    IsWindowVisible(hwnd).as_bool()
                } else {
                    true
                };

                Ok(TaskbarSnapshot {
                    primary_visible,
                    secondary_visible,
                })
            }
        }
        #[cfg(not(windows))]
        {
            Ok(TaskbarSnapshot {
                primary_visible: true,
                secondary_visible: true,
            })
        }
    }

    fn set_visible(&self, visible: bool) -> Result<(), SysctlError> {
        #[cfg(windows)]
        {
            use windows::core::w;
            use windows::Win32::UI::WindowsAndMessaging::{
                FindWindowW, ShowWindow, SW_HIDE, SW_SHOW,
            };

            let cmd = if visible { SW_SHOW } else { SW_HIDE };

            unsafe {
                if let Ok(primary_hwnd) = FindWindowW(w!("Shell_TrayWnd"), None) {
                    let _ = ShowWindow(primary_hwnd, cmd);
                }

                if let Ok(secondary_hwnd) = FindWindowW(w!("Shell_SecondaryTrayWnd"), None) {
                    let _ = ShowWindow(secondary_hwnd, cmd);
                }
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            tracing::info!(visible, "Mock taskbar visibility set");
            Ok(())
        }
    }

    fn restore_snapshot(&self, snapshot: &TaskbarSnapshot) -> Result<(), SysctlError> {
        self.set_visible(snapshot.primary_visible)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_taskbar_api_lifecycle() {
        let taskbar = WindowsTaskbar::new();
        if let Ok(snapshot) = taskbar.snapshot_taskbar() {
            let _ = taskbar.set_visible(false);
            let _ = taskbar.restore_snapshot(&snapshot);
        }
    }
}
