//! Window focus helpers used while a game launches: who is in front, bringing a process's
//! window to the front, and minimizing / restoring another process's windows.
//!
//! Windows only lets a background process take the foreground in limited cases. Kiln uses
//! the usual workaround: while a (synthetic) Alt key is held, `SetForegroundWindow` is
//! allowed. The Alt press is released straight away and lands in the window just raised.

use crate::optimizer::{app_windows, snapshot_processes};

/// Full exe path of a process, if it can be read (protected processes can't).
pub fn image_path(pid: u32) -> Option<String> {
    #[cfg(windows)]
    {
        use windows::core::PWSTR;
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        // SAFETY: the handle is checked and closed; the buffer length is passed in and the
        // API writes at most that many UTF-16 units back, updating `len`.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .is_ok();
            let _ = CloseHandle(handle);
            ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        None
    }
}

/// Every running process with a readable exe path: (pid, full path).
pub fn process_image_paths() -> Vec<(u32, String)> {
    snapshot_processes()
        .into_iter()
        .filter(|p| p.pid > 4)
        .filter_map(|p| image_path(p.pid).map(|path| (p.pid, path)))
        .collect()
}

/// PID owning the window in front, if any.
pub fn foreground_pid() -> Option<u32> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };
        // SAFETY: plain queries; a null HWND is checked.
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            (pid != 0).then_some(pid)
        }
    }
    #[cfg(not(windows))]
    {
        // No window system to ask: treat this process as the one in front.
        Some(std::process::id())
    }
}

/// Exe file name (lower case) of the process in front, e.g. "steamwebhelper.exe".
pub fn foreground_exe() -> Option<(u32, String)> {
    let pid = foreground_pid()?;
    let path = image_path(pid)?;
    let name = path.rsplit(['\\', '/']).next()?.to_ascii_lowercase();
    Some((pid, name))
}

/// Brings the main window of `pid` to the front (restoring it if minimized). Returns false
/// when the process has no suitable window or Windows refused.
pub fn bring_process_to_front(pid: u32) -> bool {
    let Some(&hwnd) = app_windows().get(&pid).and_then(|w| w.first()) else {
        return false;
    };
    bring_window_to_front(hwnd)
}

fn bring_window_to_front(hwnd: isize) -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
            KEYEVENTF_KEYUP, VK_MENU,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            BringWindowToTop, GetForegroundWindow, IsIconic, SetForegroundWindow, ShowWindowAsync,
            SW_RESTORE,
        };
        let hwnd = HWND(hwnd as *mut _);
        let alt = |flags: KEYBD_EVENT_FLAGS| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_MENU,
                    dwFlags: flags,
                    ..Default::default()
                },
            },
        };
        // SAFETY: `hwnd` came from EnumWindows moments ago; every call here tolerates a
        // window that has since closed. INPUT structs are fully initialised.
        unsafe {
            if GetForegroundWindow() == hwnd {
                return true;
            }
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindowAsync(hwnd, SW_RESTORE);
            }
            let size = std::mem::size_of::<INPUT>() as i32;
            SendInput(&[alt(KEYBD_EVENT_FLAGS(0))], size);
            let ok = SetForegroundWindow(hwnd).as_bool();
            SendInput(&[alt(KEYEVENTF_KEYUP)], size);
            let _ = BringWindowToTop(hwnd);
            ok
        }
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        false
    }
}

/// Minimizes (`minimize`) or restores every app window of the given processes. Async, so a
/// hung launcher can't block Kiln.
pub fn set_windows_minimized(pids: &[u32], minimize: bool) -> usize {
    let windows = app_windows();
    let mut n = 0;
    for pid in pids {
        for &hwnd in windows.get(pid).into_iter().flatten() {
            show_async(hwnd, minimize);
            n += 1;
        }
    }
    n
}

fn show_async(hwnd: isize, minimize: bool) {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            IsIconic, ShowWindowAsync, SW_FORCEMINIMIZE, SW_RESTORE,
        };
        let hwnd = HWND(hwnd as *mut _);
        // SAFETY: tolerates a window that has closed meanwhile; async, never blocks.
        unsafe {
            if minimize {
                if !IsIconic(hwnd).as_bool() {
                    let _ = ShowWindowAsync(hwnd, SW_FORCEMINIMIZE);
                }
            } else if IsIconic(hwnd).as_bool() {
                let _ = ShowWindowAsync(hwnd, SW_RESTORE);
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (hwnd, minimize);
    }
}

/// True when the process's main window is minimized or no longer fills its monitor (Windows
/// can drop a full-screen window back to its normal size when it is minimized and restored).
pub fn process_window_needs_fullscreen(pid: u32) -> bool {
    let Some(&hwnd) = app_windows().get(&pid).and_then(|w| w.first()) else {
        return false;
    };
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{HWND, RECT};
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        };
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsIconic};
        let hwnd = HWND(hwnd as *mut _);
        // SAFETY: plain queries on a window that may have closed (calls then fail and we
        // report "no fix needed"); out-params are valid stack locals with cbSize set.
        unsafe {
            if IsIconic(hwnd).as_bool() {
                return true;
            }
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_err() {
                return false;
            }
            let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(mon, &mut info).as_bool() {
                return false;
            }
            let m = info.rcMonitor;
            !(rect.left <= m.left
                && rect.top <= m.top
                && rect.right >= m.right
                && rect.bottom >= m.bottom)
        }
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        false
    }
}
