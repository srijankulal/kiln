//! Store launchers (Steam, Epic): which processes belong to them, where they're installed,
//! and starting them quietly in the background (no main window) so they're signed in and
//! ready before the user presses Play.

use crate::optimizer::snapshot_processes;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launcher {
    Steam,
    Epic,
}

impl Launcher {
    pub fn name(self) -> &'static str {
        match self {
            Launcher::Steam => "Steam",
            Launcher::Epic => "Epic Games",
        }
    }

    /// Lower-case exe names of the launcher's own processes (main + UI helpers).
    pub fn process_names(self) -> &'static [&'static str] {
        match self {
            Launcher::Steam => &["steam.exe", "steamwebhelper.exe"],
            Launcher::Epic => &["epicgameslauncher.exe", "epicwebhelper.exe"],
        }
    }

    fn main_exe(self) -> &'static str {
        self.process_names()[0]
    }

    /// Which launcher an exe name belongs to, if any.
    pub fn from_exe(exe: &str) -> Option<Launcher> {
        let exe = exe.to_ascii_lowercase();
        [Launcher::Steam, Launcher::Epic]
            .into_iter()
            .find(|l| l.process_names().contains(&exe.as_str()))
    }

    /// PIDs of the launcher's processes.
    pub fn pids(self) -> Vec<u32> {
        snapshot_processes()
            .into_iter()
            .filter(|p| {
                self.process_names()
                    .contains(&p.exe.to_ascii_lowercase().as_str())
            })
            .map(|p| p.pid)
            .collect()
    }

    pub fn is_running(self) -> bool {
        let main = self.main_exe();
        snapshot_processes()
            .iter()
            .any(|p| p.exe.eq_ignore_ascii_case(main))
    }

    /// Path of the launcher's exe, from the registry, else the default install location.
    pub fn exe_path(self) -> Option<PathBuf> {
        let from_registry = match self {
            Launcher::Steam => read_reg_string(true, r"Software\Valve\Steam", "SteamExe"),
            Launcher::Epic => {
                read_reg_string(false, r"com.epicgames.launcher\shell\open\command", "")
                    .and_then(|cmd| command_exe(&cmd))
            }
        };
        let defaults: &[&str] = match self {
            Launcher::Steam => &[r"C:\Program Files (x86)\Steam\steam.exe"],
            Launcher::Epic => &[
                r"C:\Program Files (x86)\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe",
                r"C:\Program Files (x86)\Epic Games\Launcher\Portal\Binaries\Win32\EpicGamesLauncher.exe",
            ],
        };
        from_registry
            .map(|p| PathBuf::from(p.replace('/', "\\")))
            .into_iter()
            .chain(defaults.iter().map(PathBuf::from))
            .find(|p| p.is_file())
    }

    /// Starts the launcher in the background without its main window. No-op if running.
    /// `extra` is appended (e.g. a steam:// URL to launch a game at the same time).
    pub fn start_silent(self, extra: Option<&str>) -> std::io::Result<bool> {
        if self.is_running() {
            return Ok(false);
        }
        let exe = self.exe_path().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "launcher not installed")
        })?;
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("-silent");
        if let Some(e) = extra {
            cmd.arg(e);
        }
        if let Some(dir) = exe.parent() {
            cmd.current_dir(dir);
        }
        cmd.spawn()?;
        Ok(true)
    }
}

/// Opens a URI (steam://, com.epicgames.launcher://) with its registered handler.
/// ShellExecute, not `cmd /c start`: cmd treats `&` in the URI as a command separator
/// (which silently dropped Epic's `&silent=true`) and flashes a console window.
pub fn open_uri(uri: &str) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use windows::core::{w, PCWSTR};
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let wide: Vec<u16> = uri.encode_utf16().chain([0]).collect();
        // SAFETY: all strings are NUL-terminated and outlive the call.
        let result = unsafe {
            ShellExecuteW(
                HWND::default(),
                w!("open"),
                PCWSTR(wide.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        // Values above 32 mean success.
        if result.0 as isize > 32 {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "ShellExecute failed ({}) for {uri}",
                result.0 as isize
            )))
        }
    }
    #[cfg(not(windows))]
    {
        println!("[HOST] Would open {uri}");
        Ok(())
    }
}

/// The exe from a shell command line like `"C:\x\y.exe" %1` or `C:\x\y.exe %1`.
pub fn command_exe(cmd: &str) -> Option<String> {
    let cmd = cmd.trim();
    if let Some(rest) = cmd.strip_prefix('"') {
        return rest.split('"').next().map(str::to_string);
    }
    let lower = cmd.to_ascii_lowercase();
    lower.find(".exe").map(|i| cmd[..i + 4].to_string())
}

/// Reads a REG_SZ value from HKCU (`current_user`) or HKCR. Empty `value` = default value.
fn read_reg_string(current_user: bool, subkey: &str, value: &str) -> Option<String> {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::System::Registry::{
            RegGetValueW, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, RRF_RT_REG_SZ,
        };
        let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let (sub, val) = (wide(subkey), wide(value));
        let root = if current_user {
            HKEY_CURRENT_USER
        } else {
            HKEY_CLASSES_ROOT
        };
        let val_ptr = if value.is_empty() {
            PCWSTR::null()
        } else {
            PCWSTR(val.as_ptr())
        };
        let mut buf = [0u16; 1024];
        let mut bytes = (buf.len() * 2) as u32;
        // SAFETY: strings are NUL-terminated and outlive the call; the buffer size in bytes
        // is passed and the API writes at most that much.
        let status = unsafe {
            RegGetValueW(
                root,
                PCWSTR(sub.as_ptr()),
                val_ptr,
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut _),
                Some(&mut bytes),
            )
        };
        if status.is_err() {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len])).filter(|s| !s.is_empty())
    }
    #[cfg(not(windows))]
    {
        let _ = (current_user, subkey, value);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_exe_lookup() {
        assert_eq!(
            Launcher::from_exe("SteamWebHelper.exe"),
            Some(Launcher::Steam)
        );
        assert_eq!(
            Launcher::from_exe("EpicGamesLauncher.exe"),
            Some(Launcher::Epic)
        );
        assert_eq!(Launcher::from_exe("Stardew Valley.exe"), None);
    }

    #[test]
    fn command_line_exe_parsing() {
        assert_eq!(
            command_exe(r#""C:\Epic\EpicGamesLauncher.exe" %1"#).as_deref(),
            Some(r"C:\Epic\EpicGamesLauncher.exe")
        );
        assert_eq!(
            command_exe(r"C:\Epic\EpicGamesLauncher.EXE %1").as_deref(),
            Some(r"C:\Epic\EpicGamesLauncher.EXE")
        );
        assert_eq!(command_exe("nothing here"), None);
    }
}
