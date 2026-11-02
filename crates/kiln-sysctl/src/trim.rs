//! Background process trimming (Tier 0 EcoQoS, Tier 1 Suspend).
use kiln_core::error::SysctlError;
use kiln_core::journal::{ProcessSnapshot, ProcessTrimKind};
use kiln_core::traits::ProcessApi;

pub const NEVER_TOUCH_PROCESSES: &[&str] = &[
    "system",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "services.exe",
    "lsass.exe",
    "svchost.exe",
    "dwm.exe",
    "explorer.exe",
    "audiodg.exe",
    "ctfmon.exe",
    "msmpeng.exe",
    "securityhealthservice.exe",
    "smartscreen.exe",
    "kiln-daemon.exe",
    "kiln-shell.exe",
    "kiln.exe",
    // Anti-cheat engines
    "easyanticheat.exe",
    "easyanticheat_eos.exe",
    "beservice.exe",
    "vgc.exe",
    "vgk.sys",
    "zksvc.exe",
    "ace-base.sys",
    "battleye.exe",
];

pub fn is_never_touch(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    NEVER_TOUCH_PROCESSES
        .iter()
        .any(|blocked| lower == *blocked)
        || lower.starts_with("easyanticheat")
        || lower.starts_with("beservice")
        || lower.starts_with("vgc")
        || lower.starts_with("kiln")
}

pub struct WindowsProcessTrimmer;

impl WindowsProcessTrimmer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsProcessTrimmer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
fn get_process_creation_time(handle: windows::Win32::Foundation::HANDLE) -> Option<u64> {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::GetProcessTimes;

    unsafe {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();

        if GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user).is_ok() {
            Some(((creation.dwHighDateTime as u64) << 32) | (creation.dwLowDateTime as u64))
        } else {
            None
        }
    }
}

impl ProcessApi for WindowsProcessTrimmer {
    fn set_eco_qos(&self, pid: u32, enable: bool) -> Result<(), SysctlError> {
        #[cfg(windows)]
        {
            use std::ffi::c_void;
            use windows::Win32::System::Threading::{
                OpenProcess, ProcessPowerThrottling, SetPriorityClass, SetProcessInformation,
                IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
                PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
                PROCESS_POWER_THROTTLING_STATE, PROCESS_SET_INFORMATION,
            };

            unsafe {
                let handle = match OpenProcess(PROCESS_SET_INFORMATION, false, pid) {
                    Ok(h) => h,
                    Err(e) => {
                        tracing::debug!(pid, error = %e, "OpenProcess for EcoQoS skipped or inaccessible");
                        return Ok(());
                    }
                };

                let priority = if enable {
                    IDLE_PRIORITY_CLASS
                } else {
                    NORMAL_PRIORITY_CLASS
                };
                let _ = SetPriorityClass(handle, priority);

                let state = PROCESS_POWER_THROTTLING_STATE {
                    Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
                    ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
                    StateMask: if enable {
                        PROCESS_POWER_THROTTLING_EXECUTION_SPEED
                    } else {
                        0
                    },
                };

                let status = SetProcessInformation(
                    handle,
                    ProcessPowerThrottling,
                    &state as *const _ as *const c_void,
                    std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
                );

                let _ = windows::Win32::Foundation::CloseHandle(handle);

                if let Err(e) = status {
                    tracing::debug!(
                        pid,
                        error = %e,
                        "SetProcessInformation EcoQoS unavailable or denied, skipping"
                    );
                }
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            tracing::info!(pid, enable, "Mock set EcoQoS");
            Ok(())
        }
    }

    fn suspend_process(&self, pid: u32, creation_time: u64) -> Result<(), SysctlError> {
        #[cfg(windows)]
        {
            use windows::core::s;
            use windows::Win32::Foundation::HANDLE;
            use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
            use windows::Win32::System::Threading::{
                OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SUSPEND_RESUME,
            };

            unsafe {
                // Verify creation time to prevent recycling collisions
                let query_handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
                {
                    Ok(h) => h,
                    Err(e) => {
                        tracing::debug!(pid, error = %e, "OpenProcess QUERY for suspend skipped");
                        return Ok(());
                    }
                };
                let actual_time = get_process_creation_time(query_handle).unwrap_or(0);
                let _ = windows::Win32::Foundation::CloseHandle(query_handle);

                if creation_time != 0 && actual_time != creation_time {
                    tracing::warn!(pid, "Process PID recycled; skipping suspend");
                    return Ok(());
                }

                let ntdll = match GetModuleHandleA(s!("ntdll.dll")) {
                    Ok(h) => h,
                    Err(e) => {
                        tracing::warn!(error = %e, "GetModuleHandleA ntdll.dll failed");
                        return Ok(());
                    }
                };
                let proc_addr = match GetProcAddress(ntdll, s!("NtSuspendProcess")) {
                    Some(p) => p,
                    None => {
                        tracing::warn!("NtSuspendProcess not found");
                        return Ok(());
                    }
                };

                type NtSuspendProcessFn = unsafe extern "system" fn(HANDLE) -> i32;
                let nt_suspend: NtSuspendProcessFn = std::mem::transmute(proc_addr);

                let handle = match OpenProcess(PROCESS_SUSPEND_RESUME, false, pid) {
                    Ok(h) => h,
                    Err(e) => {
                        tracing::debug!(pid, error = %e, "OpenProcess SUSPEND skipped");
                        return Ok(());
                    }
                };

                let status = nt_suspend(handle);
                let _ = windows::Win32::Foundation::CloseHandle(handle);

                if status != 0 {
                    tracing::warn!(status, pid, "NtSuspendProcess returned non-zero status");
                }
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            tracing::info!(pid, creation_time, "Mock suspend process");
            Ok(())
        }
    }

    fn resume_process(&self, pid: u32, creation_time: u64) -> Result<(), SysctlError> {
        #[cfg(windows)]
        {
            use windows::core::s;
            use windows::Win32::Foundation::HANDLE;
            use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
            use windows::Win32::System::Threading::{
                OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SUSPEND_RESUME,
            };

            unsafe {
                // Verify PID was not recycled before resuming
                let query_handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
                {
                    Ok(h) => h,
                    Err(_) => return Ok(()), // Process already exited
                };
                let actual_time = get_process_creation_time(query_handle).unwrap_or(0);
                let _ = windows::Win32::Foundation::CloseHandle(query_handle);

                if creation_time != 0 && actual_time != creation_time {
                    tracing::warn!(pid, "Process PID recycled; skipping resume");
                    return Ok(());
                }

                let ntdll = match GetModuleHandleA(s!("ntdll.dll")) {
                    Ok(h) => h,
                    Err(e) => {
                        tracing::warn!(error = %e, "GetModuleHandleA ntdll.dll failed");
                        return Ok(());
                    }
                };
                let proc_addr = match GetProcAddress(ntdll, s!("NtResumeProcess")) {
                    Some(p) => p,
                    None => {
                        tracing::warn!("NtResumeProcess not found");
                        return Ok(());
                    }
                };

                type NtResumeProcessFn = unsafe extern "system" fn(HANDLE) -> i32;
                let nt_resume: NtResumeProcessFn = std::mem::transmute(proc_addr);

                let handle = match OpenProcess(PROCESS_SUSPEND_RESUME, false, pid) {
                    Ok(h) => h,
                    Err(_) => return Ok(()),
                };

                let status = nt_resume(handle);
                let _ = windows::Win32::Foundation::CloseHandle(handle);

                if status != 0 {
                    tracing::warn!(status, pid, "NtResumeProcess returned non-zero status");
                }
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            tracing::info!(pid, creation_time, "Mock resume process");
            Ok(())
        }
    }

    fn is_process_running(&self, pid: u32, expected_creation_time: u64) -> bool {
        #[cfg(windows)]
        {
            use windows::Win32::System::Threading::{
                OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            };

            unsafe {
                if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                    let actual_time = get_process_creation_time(handle).unwrap_or(0);
                    let _ = windows::Win32::Foundation::CloseHandle(handle);

                    if expected_creation_time == 0 {
                        true
                    } else {
                        actual_time == expected_creation_time
                    }
                } else {
                    false
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = expected_creation_time;
            std::path::Path::new(&format!("/proc/{}", pid)).exists()
        }
    }

    fn restore_processes(&self, processes: &[ProcessSnapshot]) -> Result<(), SysctlError> {
        for proc in processes {
            match proc.trim_kind {
                ProcessTrimKind::Suspended => {
                    if let Err(e) = self.resume_process(proc.pid, proc.creation_time) {
                        tracing::warn!(pid = proc.pid, name = %proc.name, error = %e, "Failed to resume process on restore");
                    }
                }
                ProcessTrimKind::EcoQoS => {
                    if let Err(e) = self.set_eco_qos(proc.pid, false) {
                        tracing::warn!(pid = proc.pid, name = %proc.name, error = %e, "Failed to disable EcoQoS on restore");
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_never_touch_processes_filter() {
        assert!(is_never_touch("System"));
        assert!(is_never_touch("system"));
        assert!(is_never_touch("csrss.exe"));
        assert!(is_never_touch("CSRSS.EXE"));
        assert!(is_never_touch("explorer.exe"));
        assert!(is_never_touch("EasyAntiCheat.exe"));
        assert!(is_never_touch("EasyAntiCheat_EOS.exe"));
        assert!(is_never_touch("vgc.exe"));
        assert!(is_never_touch("kiln-shell.exe"));
        assert!(is_never_touch("kiln-daemon.exe"));

        // Regular apps should NOT be blocked
        assert!(!is_never_touch("chrome.exe"));
        assert!(!is_never_touch("discord.exe"));
        assert!(!is_never_touch("spotify.exe"));
        assert!(!is_never_touch("slack.exe"));
    }

    #[test]
    fn test_process_trimmer_lifecycle() {
        let trimmer = WindowsProcessTrimmer::new();
        let my_pid = std::process::id();
        assert!(trimmer.is_process_running(my_pid, 0));
        assert!(!trimmer.is_process_running(999_999_999, 0));

        assert!(trimmer.set_eco_qos(my_pid, true).is_ok());
        assert!(trimmer.set_eco_qos(my_pid, false).is_ok());
        assert!(trimmer.suspend_process(999_999_999, 1000).is_ok());
        assert!(trimmer.resume_process(999_999_999, 1000).is_ok());

        let snapshot = vec![
            ProcessSnapshot {
                pid: 1234,
                name: "chrome.exe".to_string(),
                creation_time: 1000,
                trim_kind: ProcessTrimKind::Suspended,
            },
            ProcessSnapshot {
                pid: 5678,
                name: "discord.exe".to_string(),
                creation_time: 2000,
                trim_kind: ProcessTrimKind::EcoQoS,
            },
        ];
        assert!(trimmer.restore_processes(&snapshot).is_ok());
    }
}
