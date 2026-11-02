//! Background process trimming (Tier 0 EcoQoS, Tier 1 Suspend).
use kiln_core::error::SysctlError;
use kiln_core::journal::ProcessSnapshot;
use kiln_core::traits::ProcessApi;

pub const NEVER_TOUCH_PROCESSES: &[&str] = &[
    "System",
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
    "MsMpEng.exe",
    "SecurityHealthService.exe",
    "smartscreen.exe",
    // Anti-cheat
    "EasyAntiCheat.exe",
    "EasyAntiCheat_EOS.exe",
    "BEService.exe",
    "vgc.exe",
    "vgk.sys",
    "zksvc.exe",
    "ACE-Base.sys",
    "BattleEye.exe",
];

pub fn is_never_touch(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    NEVER_TOUCH_PROCESSES
        .iter()
        .any(|blocked| lower == blocked.to_ascii_lowercase())
        || lower.starts_with("easyanticheat")
        || lower.starts_with("beservice")
        || lower.starts_with("vgc")
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

impl ProcessApi for WindowsProcessTrimmer {
    fn set_eco_qos(&self, _pid: u32, _enable: bool) -> Result<(), SysctlError> {
        Ok(())
    }

    fn suspend_process(&self, _pid: u32, _creation_time: u64) -> Result<(), SysctlError> {
        Ok(())
    }

    fn resume_process(&self, _pid: u32, _creation_time: u64) -> Result<(), SysctlError> {
        Ok(())
    }

    fn is_process_running(&self, _pid: u32, _expected_creation_time: u64) -> bool {
        false
    }

    fn restore_processes(&self, _processes: &[ProcessSnapshot]) -> Result<(), SysctlError> {
        Ok(())
    }
}
