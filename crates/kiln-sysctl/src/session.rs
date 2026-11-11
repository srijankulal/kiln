//! Session-level helpers for the shell: "is Kiln the window in front?" and the power menu
//! actions (sleep, restart, shut down).
//!
//! Restart and shut down use Windows' normal (non-forced) path, the same as the Start menu:
//! apps with unsaved work can still hold it up and Windows shows its usual prompt.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    Sleep,
    Restart,
    ShutDown,
}

impl PowerAction {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "sleep" => Some(Self::Sleep),
            "restart" => Some(Self::Restart),
            "shutdown" => Some(Self::ShutDown),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct PowerError(pub String);

impl fmt::Display for PowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PowerError {}

/// True when the foreground window belongs to this process. Used to ignore the controller
/// while a game (or anything else) is in front of Kiln.
pub fn this_process_is_foreground() -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };
        // SAFETY: plain queries; a null HWND simply yields pid 0.
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return false;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            pid == std::process::id()
        }
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// The current local time as Windows shows it (the user's time zone and regional format,
/// e.g. "3:42 PM" or "15:42"), without seconds.
pub fn local_clock() -> String {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Globalization::{GetTimeFormatEx, TIME_NOSECONDS};
        let mut buf = [0u16; 64];
        // SAFETY: null locale = user default, no explicit time = now, null format = the
        // locale's; the output buffer is a valid slice.
        let n = unsafe {
            GetTimeFormatEx(
                PCWSTR::null(),
                TIME_NOSECONDS,
                None,
                PCWSTR::null(),
                Some(&mut buf),
            )
        };
        if n > 1 {
            return String::from_utf16_lossy(&buf[..(n - 1) as usize]);
        }
    }
    // Fallback (no Windows, or the call failed): UTC, 24-hour.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{:02}:{:02}", (now / 3600) % 24, (now / 60) % 60)
}

/// Performs a power action. With `dry_run` it only reports what it would do.
pub fn perform(action: PowerAction, dry_run: bool) -> Result<(), PowerError> {
    if dry_run {
        println!("[DRY-RUN] Would {:?}", action);
        return Ok(());
    }
    #[cfg(windows)]
    {
        match action {
            PowerAction::Sleep => sleep(),
            PowerAction::Restart => run_shutdown_exe(&["/r", "/t", "0"]),
            PowerAction::ShutDown => run_shutdown_exe(&["/s", "/t", "0"]),
        }
    }
    #[cfg(not(windows))]
    {
        Err(PowerError(format!(
            "{action:?} is only supported on Windows"
        )))
    }
}

#[cfg(windows)]
fn run_shutdown_exe(args: &[&str]) -> Result<(), PowerError> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let exe = std::path::Path::new(&system).join(r"System32\shutdown.exe");
    let status = std::process::Command::new(exe)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| PowerError(format!("couldn't run shutdown.exe: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(PowerError(format!("shutdown.exe exited with {status}")))
    }
}

#[cfg(windows)]
fn sleep() -> Result<(), PowerError> {
    use windows::Win32::System::Power::SetSuspendState;
    enable_shutdown_privilege();
    // SAFETY: plain call; hibernate=false requests sleep, force=false lets apps object.
    let ok = unsafe { SetSuspendState(false, false, false) };
    if ok.as_bool() {
        Ok(())
    } else {
        Err(PowerError(
            "Windows didn't allow sleep (it may be disabled on this PC)".into(),
        ))
    }
}

/// Suspending requires SeShutdownPrivilege to be enabled on the process token. Every
/// interactive user holds it; it's just disabled by default.
#[cfg(windows)]
fn enable_shutdown_privilege() {
    use windows::core::w;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID};
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    // SAFETY: token handle is checked and closed; TOKEN_PRIVILEGES holds exactly one entry.
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
        .is_err()
        {
            return;
        }
        let mut luid = LUID::default();
        if LookupPrivilegeValueW(None, w!("SeShutdownPrivilege"), &mut luid).is_ok() {
            let tp = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: luid,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };
            let _ = AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None);
        }
        let _ = CloseHandle(token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_map_to_actions() {
        assert_eq!(PowerAction::from_name("sleep"), Some(PowerAction::Sleep));
        assert_eq!(
            PowerAction::from_name("restart"),
            Some(PowerAction::Restart)
        );
        assert_eq!(
            PowerAction::from_name("shutdown"),
            Some(PowerAction::ShutDown)
        );
        assert_eq!(PowerAction::from_name("exit"), None);
    }

    #[test]
    fn dry_run_never_acts() {
        assert!(perform(PowerAction::ShutDown, true).is_ok());
    }
}
