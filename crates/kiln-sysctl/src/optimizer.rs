//! Game Mode app closing: finds the apps the user has open and asks them to close gracefully.
//!
//! Which apps are offered
//! - Every app with a normal visible window on the desktop (what you'd see in Alt+Tab), plus
//!   known background apps from `CLOSABLE_APPS` that often live only in the tray (Discord,
//!   Spotify, OneDrive...). Known apps get a friendly name and a category; anything else shows
//!   the program's own description and is listed as "App".
//!
//! Safety model
//! - `PROTECTED_SYSTEM_PROCESSES` (Windows, shell, drivers, security, anti-cheat, launchers,
//!   Kiln) are never touched, even if a caller asks for them.
//! - The app Kiln was launched from (any ancestor process, e.g. VS Code's terminal) is never closed.
//! - Closing is what clicking an app's X does: each window gets WM_CLOSE, so apps with unsaved
//!   work show their own save prompt. Apps that only hide to the tray then get the sign-out
//!   handshake Windows uses at log-off (they're asked first and may refuse), and finally
//!   Restart Manager's normal shutdown request, **without** the force flag. Anything still
//!   running after that stays open and is reported back. Nothing is ever terminated.
//! - Processes are identified by PID + start time, so a recycled PID can't be hit.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// Processes Kiln must never close, whatever the caller asks for.
pub const PROTECTED_SYSTEM_PROCESSES: &[&str] = &[
    // Windows kernel and core subsystems
    "system",
    "registry",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "services.exe",
    "lsass.exe",
    "winlogon.exe",
    "fontdrvhost.exe",
    "dwm.exe",
    "explorer.exe",
    "sihost.exe",
    "applicationframehost.exe",
    "shellexperiencehost.exe",
    "startmenuexperiencehost.exe",
    "shellhost.exe",
    "searchhost.exe",
    "searchapp.exe",
    "textinputhost.exe",
    "lockapp.exe",
    "widgets.exe",
    "widgetservice.exe",
    "gamebar.exe",
    "gamebarftserver.exe",
    "taskmgr.exe",
    "rundll32.exe",
    "dllhost.exe",
    "openconsole.exe",
    "msedgewebview2.exe",
    "useroobebroker.exe",
    "systemsettingsbroker.exe",
    "taskhostw.exe",
    "conhost.exe",
    "ctfmon.exe",
    "spoolsv.exe",
    "audiodg.exe",
    "svchost.exe",
    // Storage, backup and file system integrity
    "vds.exe",
    "vssvc.exe",
    "dfsrs.exe",
    "defrag.exe",
    "chkdsk.exe",
    "trustedinstaller.exe",
    "tiworker.exe",
    "msiexec.exe",
    "wbengine.exe",
    "searchindexer.exe",
    "cleanmgr.exe",
    // Hardware, GPU and audio drivers
    "nvvsvc.exe",
    "nvxdsync.exe",
    "nvdisplay.container.exe",
    "nvcontainer.exe",
    "nvcplui.exe",
    "amdfendrsr.exe",
    "atieclxx.exe",
    "atiesrxx.exe",
    "amdow.exe",
    "radeonsoftware.exe",
    "igfxcuiservice.exe",
    "igfxem.exe",
    "igfxtray.exe",
    "igfxhk.exe",
    "rtkauduservice64.exe",
    "nahimicservice.exe",
    "wavesaudioservice64.exe",
    // Windows security
    "msmpeng.exe",
    "securityhealthservice.exe",
    "securityhealthsystray.exe",
    "smartscreen.exe",
    "nissrv.exe",
    // Anti-cheat and DRM (must stay running for online games)
    "easyanticheat.exe",
    "easyanticheat_eos.exe",
    "beservice.exe",
    "vgc.exe",
    "vgk.sys",
    "battleye.exe",
    "ace-base.sys",
    "denuvo.exe",
    // Game launchers (needed to start games)
    "steam.exe",
    "steamwebhelper.exe",
    "epicgameslauncher.exe",
    "epicwebhelper.exe",
    // Kiln itself
    "kiln.exe",
    "kiln-shell.exe",
    "kiln-daemon.exe",
    "kiln-auth.exe",
    "kiln-updater.exe",
    "kiln-scan.exe",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AppCategory {
    Browser,
    Chat,
    Media,
    Productivity,
    Sync,
    Downloads,
    Updater,
    /// Any other app with an open window.
    Other,
}

impl AppCategory {
    pub fn label(self) -> &'static str {
        match self {
            AppCategory::Browser => "Browser",
            AppCategory::Chat => "Chat",
            AppCategory::Media => "Media",
            AppCategory::Productivity => "Work",
            AppCategory::Sync => "Cloud sync",
            AppCategory::Downloads => "Downloads",
            AppCategory::Updater => "Updater",
            AppCategory::Other => "App",
        }
    }

    /// Whether apps in this category are pre-selected for closing. Categories where unsaved
    /// work or in-progress transfers are likely (work apps, sync, downloads) default to "keep".
    pub fn closes_by_default(self) -> bool {
        matches!(
            self,
            AppCategory::Browser
                | AppCategory::Chat
                | AppCategory::Media
                | AppCategory::Updater
                | AppCategory::Other
        )
    }
}

pub struct AppProfile {
    pub exe: &'static str,
    pub name: &'static str,
    pub category: AppCategory,
}

const fn app(exe: &'static str, name: &'static str, category: AppCategory) -> AppProfile {
    AppProfile {
        exe,
        name,
        category,
    }
}

/// Apps Kiln knows by name. They're offered even when they only run in the tray; any other
/// app is offered only while it has a visible window.
pub const CLOSABLE_APPS: &[AppProfile] = &[
    app("chrome.exe", "Google Chrome", AppCategory::Browser),
    app("msedge.exe", "Microsoft Edge", AppCategory::Browser),
    app("firefox.exe", "Firefox", AppCategory::Browser),
    app("brave.exe", "Brave", AppCategory::Browser),
    app("opera.exe", "Opera", AppCategory::Browser),
    app("operagx.exe", "Opera GX", AppCategory::Browser),
    app("vivaldi.exe", "Vivaldi", AppCategory::Browser),
    app("arc.exe", "Arc", AppCategory::Browser),
    app("waterfox.exe", "Waterfox", AppCategory::Browser),
    app("discord.exe", "Discord", AppCategory::Chat),
    app("slack.exe", "Slack", AppCategory::Chat),
    app("ms-teams.exe", "Microsoft Teams", AppCategory::Chat),
    app("teams.exe", "Microsoft Teams (classic)", AppCategory::Chat),
    app("skype.exe", "Skype", AppCategory::Chat),
    app("zoom.exe", "Zoom", AppCategory::Chat),
    app("telegram.exe", "Telegram", AppCategory::Chat),
    app("whatsapp.exe", "WhatsApp", AppCategory::Chat),
    app("whatsapp.root.exe", "WhatsApp", AppCategory::Chat),
    app("signal.exe", "Signal", AppCategory::Chat),
    app("element.exe", "Element", AppCategory::Chat),
    app("spotify.exe", "Spotify", AppCategory::Media),
    app("vlc.exe", "VLC", AppCategory::Media),
    app("foobar2000.exe", "foobar2000", AppCategory::Media),
    app("musicbee.exe", "MusicBee", AppCategory::Media),
    app("itunes.exe", "iTunes", AppCategory::Media),
    app("tidal.exe", "TIDAL", AppCategory::Media),
    app("deezer.exe", "Deezer", AppCategory::Media),
    app("plex.exe", "Plex", AppCategory::Media),
    app("kodi.exe", "Kodi", AppCategory::Media),
    app("code.exe", "Visual Studio Code", AppCategory::Productivity),
    app("cursor.exe", "Cursor", AppCategory::Productivity),
    app("zed.exe", "Zed", AppCategory::Productivity),
    app("claude.exe", "Claude", AppCategory::Productivity),
    app("chatgpt.exe", "ChatGPT", AppCategory::Productivity),
    app(
        "windowsterminal.exe",
        "Windows Terminal",
        AppCategory::Productivity,
    ),
    app("wezterm-gui.exe", "WezTerm", AppCategory::Productivity),
    app("alacritty.exe", "Alacritty", AppCategory::Productivity),
    app("notepad.exe", "Notepad", AppCategory::Productivity),
    app("obs64.exe", "OBS Studio", AppCategory::Productivity),
    app("photoshop.exe", "Photoshop", AppCategory::Productivity),
    app("blender.exe", "Blender", AppCategory::Productivity),
    app("devenv.exe", "Visual Studio", AppCategory::Productivity),
    app("idea64.exe", "IntelliJ IDEA", AppCategory::Productivity),
    app("pycharm64.exe", "PyCharm", AppCategory::Productivity),
    app("webstorm64.exe", "WebStorm", AppCategory::Productivity),
    app("clion64.exe", "CLion", AppCategory::Productivity),
    app(
        "sublime_text.exe",
        "Sublime Text",
        AppCategory::Productivity,
    ),
    app("notepad++.exe", "Notepad++", AppCategory::Productivity),
    app("winword.exe", "Microsoft Word", AppCategory::Productivity),
    app("excel.exe", "Microsoft Excel", AppCategory::Productivity),
    app(
        "powerpnt.exe",
        "Microsoft PowerPoint",
        AppCategory::Productivity,
    ),
    app(
        "outlook.exe",
        "Microsoft Outlook",
        AppCategory::Productivity,
    ),
    app("onenote.exe", "OneNote", AppCategory::Productivity),
    app("notion.exe", "Notion", AppCategory::Productivity),
    app("obsidian.exe", "Obsidian", AppCategory::Productivity),
    app("figma.exe", "Figma", AppCategory::Productivity),
    app("onedrive.exe", "OneDrive", AppCategory::Sync),
    app("dropbox.exe", "Dropbox", AppCategory::Sync),
    app("googledrivefs.exe", "Google Drive", AppCategory::Sync),
    app("megasync.exe", "MEGA", AppCategory::Sync),
    app("utorrent.exe", "uTorrent", AppCategory::Downloads),
    app("qbittorrent.exe", "qBittorrent", AppCategory::Downloads),
    app("deluge.exe", "Deluge", AppCategory::Downloads),
    app("adobearm.exe", "Adobe Updater", AppCategory::Updater),
    app("acrotray.exe", "Acrobat Tray", AppCategory::Updater),
    app(
        "creative cloud.exe",
        "Adobe Creative Cloud",
        AppCategory::Updater,
    ),
    app("coresync.exe", "Adobe Sync", AppCategory::Updater),
];

pub fn profile_for(exe: &str) -> Option<&'static AppProfile> {
    let lower = exe.trim().to_ascii_lowercase();
    CLOSABLE_APPS.iter().find(|p| p.exe == lower)
}

/// True for anything on the protected list (or matching a protected prefix).
pub fn is_protected(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    PROTECTED_SYSTEM_PROCESSES.iter().any(|&p| lower == p)
        || [
            "easyanticheat",
            "beservice",
            "vgc",
            "kiln",
            "nv",
            "amd",
            "intel",
            "rtk",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

/// Multi-layer check: false for core PIDs, Kiln itself and anything protected. Callers still
/// decide which apps are offered (known apps, or apps with a visible window).
pub fn is_safe_to_terminate(name: &str, pid: u32) -> bool {
    if pid <= 4 || pid == std::process::id() {
        return false;
    }
    !is_protected(name)
}

/// "vivaldi.exe" -> "Vivaldi": fallback name when the program has no description.
pub fn fallback_name(exe: &str) -> String {
    let stem = exe.trim().trim_end_matches(".exe").trim_end_matches(".EXE");
    let mut c = stem.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => exe.to_string(),
    }
}

/// One row of a process snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessEntry {
    pub pid: u32,
    pub parent_pid: u32,
    pub exe: String,
}

/// A closable app currently running (all of its processes grouped together).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningApp {
    pub exe: String,
    pub name: String,
    pub category: AppCategory,
    pub process_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseReport {
    /// Apps whose processes all exited.
    pub closed: Vec<String>,
    /// Apps that were asked to close but are still running (e.g. waiting on a save prompt).
    pub still_open: Vec<String>,
    /// Requested apps Kiln refused to touch (protected, or the app Kiln was launched from).
    pub skipped: Vec<String>,
}

/// PIDs of every ancestor of `self_pid` (its parent, grandparent, ...).
pub fn ancestor_pids(procs: &[ProcessEntry], self_pid: u32) -> HashSet<u32> {
    let parent_of: HashMap<u32, u32> = procs.iter().map(|p| (p.pid, p.parent_pid)).collect();
    let mut out = HashSet::new();
    let mut cur = self_pid;
    // Bounded walk: parent PIDs can be stale/recycled and form cycles.
    for _ in 0..64 {
        match parent_of.get(&cur) {
            Some(&parent) if parent != 0 && parent != cur && out.insert(parent) => cur = parent,
            _ => break,
        }
    }
    out
}

/// Executable names of the apps Kiln was launched from. Closing any process of these apps
/// (e.g. the main VS Code process behind its terminal) would take Kiln down with it.
pub fn launcher_app_exes(procs: &[ProcessEntry], self_pid: u32) -> HashSet<String> {
    let ancestors = ancestor_pids(procs, self_pid);
    procs
        .iter()
        .filter(|p| ancestors.contains(&p.pid))
        .map(|p| p.exe.to_ascii_lowercase())
        .collect()
}

/// Executables Kiln may offer: known apps, plus any app that owns a visible window
/// (`windowed` holds those PIDs). Protected processes, Kiln and its launcher are excluded.
fn offerable_exes(
    procs: &[ProcessEntry],
    windowed: &HashSet<u32>,
    self_pid: u32,
) -> HashSet<String> {
    let launcher = launcher_app_exes(procs, self_pid);
    procs
        .iter()
        .filter(|p| p.pid != self_pid && is_safe_to_terminate(&p.exe, p.pid))
        .map(|p| (p, p.exe.to_ascii_lowercase()))
        .filter(|(p, exe)| {
            !launcher.contains(exe) && (profile_for(exe).is_some() || windowed.contains(&p.pid))
        })
        .map(|(_, exe)| exe)
        .collect()
}

/// Groups a process snapshot into the apps Kiln can offer to close (see module docs).
/// `name_of` names apps Kiln doesn't know (e.g. from the program's file description).
/// Sorted by display name.
pub fn closable_apps_in(
    procs: &[ProcessEntry],
    windowed: &HashSet<u32>,
    self_pid: u32,
    name_of: impl Fn(&ProcessEntry) -> Option<String>,
) -> Vec<RunningApp> {
    let offer = offerable_exes(procs, windowed, self_pid);
    let mut by_exe: HashMap<String, RunningApp> = HashMap::new();
    for p in procs {
        let exe = p.exe.to_ascii_lowercase();
        if !offer.contains(&exe) || p.pid == self_pid {
            continue;
        }
        let app = by_exe.entry(exe.clone()).or_insert_with(|| {
            let (name, category) = match profile_for(&exe) {
                Some(profile) => (profile.name.to_string(), profile.category),
                None => (String::new(), AppCategory::Other),
            };
            RunningApp {
                exe: exe.clone(),
                name,
                category,
                process_count: 0,
            }
        });
        app.process_count += 1;
        // Unknown apps: name them from the process that owns the window.
        if app.name.is_empty() && windowed.contains(&p.pid) {
            app.name = name_of(p).unwrap_or_default();
        }
    }
    let mut apps: Vec<RunningApp> = by_exe
        .into_values()
        .map(|mut a| {
            if a.name.trim().is_empty() {
                a.name = fallback_name(&a.exe);
            }
            a
        })
        .collect();
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

/// Splits a close request into processes Kiln may close and app names it refuses.
fn plan_close(
    procs: &[ProcessEntry],
    windowed: &HashSet<u32>,
    self_pid: u32,
    requested: &[String],
) -> (Vec<ProcessEntry>, Vec<String>) {
    let offer = offerable_exes(procs, windowed, self_pid);
    let mut skipped = Vec::new();
    let mut allowed: HashSet<String> = HashSet::new();
    for exe in requested {
        let exe = exe.to_ascii_lowercase();
        if offer.contains(&exe) {
            allowed.insert(exe);
        } else if procs.iter().any(|p| p.exe.eq_ignore_ascii_case(&exe)) {
            // Running, but protected / Kiln's launcher / no longer has a window.
            skipped.push(exe);
        }
        // Not running any more: nothing to do (it was closed by hand meanwhile).
    }
    let targets = procs
        .iter()
        .filter(|p| {
            let exe = p.exe.to_ascii_lowercase();
            allowed.contains(&exe) && p.pid != self_pid && is_safe_to_terminate(&exe, p.pid)
        })
        .cloned()
        .collect();
    (targets, skipped)
}

/// Lists running processes (PID, parent PID, executable name).
pub fn snapshot_processes() -> Vec<ProcessEntry> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };

        let mut out = Vec::new();
        // SAFETY: the snapshot handle is checked, used only in this scope and closed below;
        // PROCESSENTRY32W is initialised with its required dwSize.
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
                return out;
            };
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut ok = Process32FirstW(snap, &mut entry).is_ok();
            while ok {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                out.push(ProcessEntry {
                    pid: entry.th32ProcessID,
                    parent_pid: entry.th32ParentProcessID,
                    exe: String::from_utf16_lossy(&entry.szExeFile[..len]),
                });
                ok = Process32NextW(snap, &mut entry).is_ok();
            }
            let _ = CloseHandle(snap);
        }
        out
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[cfg(windows)]
fn process_start_time(pid: u32) -> Option<windows::Win32::Foundation::FILETIME> {
    use windows::Win32::Foundation::{CloseHandle, FILETIME};
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: handle is checked and closed; out-params are valid stack locals.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let (mut c, mut e, mut k, mut u) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        let ok = GetProcessTimes(handle, &mut c, &mut e, &mut k, &mut u).is_ok();
        let _ = CloseHandle(handle);
        ok.then_some(c)
    }
}

#[cfg(windows)]
fn same_process_running(pid: u32, start: windows::Win32::Foundation::FILETIME) -> bool {
    use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let Some(now_start) = process_start_time(pid) else {
        return false;
    };
    if now_start.dwLowDateTime != start.dwLowDateTime
        || now_start.dwHighDateTime != start.dwHighDateTime
    {
        return false; // PID was recycled: the original process is gone.
    }
    // SAFETY: handle is checked and closed; exit code is a valid out-param.
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut code = 0u32;
        let ok = GetExitCodeProcess(handle, &mut code).is_ok();
        let _ = CloseHandle(handle);
        ok && code == STILL_ACTIVE.0 as u32
    }
}

/// Top-level windows a user would consider "open apps": visible, unowned, not tool windows,
/// titled, and not cloaked (hidden UWP frames / other virtual desktops). Map: PID -> windows.
pub fn app_windows() -> HashMap<u32, Vec<isize>> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
        use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
        use windows::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetWindow, GetWindowLongPtrW, GetWindowTextLengthW,
            GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, GW_OWNER, WS_EX_TOOLWINDOW,
        };

        unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
            // SAFETY: lparam is the &mut map passed to EnumWindows below, alive for the call.
            let map = &mut *(lparam.0 as *mut HashMap<u32, Vec<isize>>);
            if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) == 0 {
                return true.into();
            }
            if GetWindow(hwnd, GW_OWNER).is_ok_and(|o| !o.0.is_null()) {
                return true.into();
            }
            if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_TOOLWINDOW.0 as isize != 0 {
                return true.into();
            }
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                &mut cloaked as *mut u32 as *mut _,
                std::mem::size_of::<u32>() as u32,
            );
            if cloaked != 0 {
                return true.into();
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid != 0 {
                map.entry(pid).or_default().push(hwnd.0 as isize);
            }
            true.into()
        }

        let mut map: HashMap<u32, Vec<isize>> = HashMap::new();
        // SAFETY: the callback only runs during this call, while `map` is alive.
        unsafe {
            let _ = EnumWindows(Some(visit), LPARAM(&mut map as *mut _ as isize));
        }
        map
    }
    #[cfg(not(windows))]
    {
        HashMap::new()
    }
}

/// The program's own description from its version info (e.g. "Vivaldi", "Notepad++").
pub fn program_description(pid: u32) -> Option<String> {
    #[cfg(windows)]
    {
        use windows::core::{w, PCWSTR, PWSTR};
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::Storage::FileSystem::{
            GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
        };
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        // SAFETY: buffers are sized by the API calls that fill them; the process handle is
        // closed; VerQueryValueW pointers point into `data`, which outlives their use.
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
            if !ok {
                return None;
            }
            let path: Vec<u16> = buf[..len as usize].iter().copied().chain([0]).collect();
            let size = GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None);
            if size == 0 {
                return None;
            }
            let mut data = vec![0u8; size as usize];
            GetFileVersionInfoW(PCWSTR(path.as_ptr()), 0, size, data.as_mut_ptr() as *mut _)
                .ok()?;
            let mut p: *mut core::ffi::c_void = std::ptr::null_mut();
            let mut n = 0u32;
            if !VerQueryValueW(
                data.as_ptr() as *const _,
                w!("\\VarFileInfo\\Translation"),
                &mut p,
                &mut n,
            )
            .as_bool()
                || n < 4
            {
                return None;
            }
            let lang = *(p as *const u16);
            let cp = *(p as *const u16).add(1);
            let key: Vec<u16> = format!("\\StringFileInfo\\{lang:04x}{cp:04x}\\FileDescription")
                .encode_utf16()
                .chain([0])
                .collect();
            if !VerQueryValueW(
                data.as_ptr() as *const _,
                PCWSTR(key.as_ptr()),
                &mut p,
                &mut n,
            )
            .as_bool()
                || n == 0
            {
                return None;
            }
            let text = std::slice::from_raw_parts(p as *const u16, n as usize);
            let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
            let s = String::from_utf16_lossy(&text[..end]).trim().to_string();
            (!s.is_empty() && s.len() <= 60).then_some(s)
        }
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        None
    }
}

/// Step 1 of closing: the same request as clicking each window's X button.
#[cfg(windows)]
fn post_close_to_windows(pids: &HashSet<u32>, windows_by_pid: &HashMap<u32, Vec<isize>>) {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
    for pid in pids {
        for &hwnd in windows_by_pid.get(pid).into_iter().flatten() {
            // SAFETY: posting a message to a window handle is safe even if it has since closed.
            unsafe {
                let _ = PostMessageW(HWND(hwnd as *mut _), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }
}

/// Every top-level window of the given processes, including hidden ones and message-only
/// windows: tray apps keep their main window hidden and listen on these.
#[cfg(windows)]
fn all_windows_of(pids: &HashSet<u32>) -> Vec<isize> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, FindWindowExW, GetWindowThreadProcessId, HWND_MESSAGE,
    };
    struct Ctx<'a> {
        pids: &'a HashSet<u32>,
        out: Vec<isize>,
    }
    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam is the &mut Ctx passed to EnumWindows below, alive for the call.
        let ctx = &mut *(lparam.0 as *mut Ctx);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if ctx.pids.contains(&pid) {
            ctx.out.push(hwnd.0 as isize);
        }
        true.into()
    }
    let mut ctx = Ctx {
        pids,
        out: Vec::new(),
    };
    // SAFETY: the callback only runs during EnumWindows while `ctx` is alive; FindWindowExW
    // walks message-only windows one by one and stops at the first error / null.
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&mut ctx as *mut Ctx as isize));
        let mut after = HWND::default();
        for _ in 0..4096 {
            let Ok(h) = FindWindowExW(HWND_MESSAGE, after, PCWSTR::null(), PCWSTR::null()) else {
                break;
            };
            if h.0.is_null() {
                break;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            if pids.contains(&pid) {
                ctx.out.push(h.0 as isize);
            }
            after = h;
        }
    }
    ctx.out
}

/// Step 2 of closing, for apps that live in the tray: the sign-out handshake Windows itself
/// uses. Each app is first asked whether it can end (WM_QUERYENDSESSION). Only if none of
/// its windows objects does it get WM_ENDSESSION(TRUE); an app that objects (e.g. unsaved
/// work) is told the session continues and is left alone. Flag ENDSESSION_CLOSEAPP marks it
/// as "close this app", not a real log-off.
#[cfg(windows)]
fn request_session_end(pids: &HashSet<u32>) {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetWindowThreadProcessId, SendMessageTimeoutW, ENDSESSION_CLOSEAPP,
        SMTO_ABORTIFHUNG, WM_ENDSESSION, WM_QUERYENDSESSION,
    };
    // Helper windows Windows creates inside every app (input methods, COM, GDI+). They never
    // decide whether the app quits and often don't answer, so skip them.
    const SYSTEM_CLASSES: &[&str] = &[
        "IME",
        "MSCTFIME UI",
        "CicMarshalWndClass",
        "OleMainThreadWndClass",
        "GDI+ Hook Window Class",
        "SystemUserAdapterWindowClass",
        "DcppUserAdapterWindowClass",
        "LiftedDMITCursorRecalculateClass",
    ];
    const PER_WINDOW_MS: u32 = 300;
    let budget = std::time::Instant::now() + Duration::from_secs(3);

    let mut by_pid: HashMap<u32, Vec<HWND>> = HashMap::new();
    for h in all_windows_of(pids) {
        let hwnd = HWND(h as *mut _);
        let mut pid = 0u32;
        let mut class = [0u16; 128];
        // SAFETY: plain queries on a window handle (stale handles yield pid 0 / no name).
        let n = unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            GetClassNameW(hwnd, &mut class)
        };
        let class = String::from_utf16_lossy(&class[..n.max(0) as usize]);
        if !SYSTEM_CLASSES.contains(&class.as_str()) {
            by_pid.entry(pid).or_default().push(hwnd);
        }
    }
    let lp = LPARAM(ENDSESSION_CLOSEAPP as isize);
    for windows in by_pid.values() {
        if std::time::Instant::now() >= budget {
            break;
        }
        // SAFETY: SendMessageTimeoutW with SMTO_ABORTIFHUNG never blocks on a hung window
        // for longer than the timeout; results are plain out-params.
        unsafe {
            let mut agreed = true;
            for &hwnd in windows {
                let mut result = 1usize;
                let sent = SendMessageTimeoutW(
                    hwnd,
                    WM_QUERYENDSESSION,
                    WPARAM(0),
                    lp,
                    SMTO_ABORTIFHUNG,
                    PER_WINDOW_MS,
                    Some(&mut result),
                );
                if sent.0 != 0 && result == 0 {
                    agreed = false; // the app asked to stay open
                    break;
                }
                if std::time::Instant::now() >= budget {
                    agreed = false; // out of time: don't end half-asked apps
                    break;
                }
            }
            for &hwnd in windows {
                let _ = SendMessageTimeoutW(
                    hwnd,
                    WM_ENDSESSION,
                    WPARAM(agreed as usize),
                    lp,
                    SMTO_ABORTIFHUNG,
                    PER_WINDOW_MS,
                    None,
                );
            }
        }
    }
}

/// Asks the given processes to close via Restart Manager (no force) and waits up to
/// `timeout` for them to exit. Returns the PIDs that are still running afterwards.
/// Callers must have already filtered `pids` through the safety checks.
#[cfg(windows)]
fn request_graceful_close(pids: &[u32], timeout: Duration) -> HashSet<u32> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::RestartManager::{
        RmEndSession, RmRegisterResources, RmShutdown, RmStartSession, CCH_RM_SESSION_KEY,
        RM_UNIQUE_PROCESS,
    };

    let targets: Vec<RM_UNIQUE_PROCESS> = pids
        .iter()
        .filter_map(|&pid| {
            process_start_time(pid).map(|t| RM_UNIQUE_PROCESS {
                dwProcessId: pid,
                ProcessStartTime: t,
            })
        })
        .collect();
    if targets.is_empty() {
        return HashSet::new();
    }

    // RmShutdown can block for ~30 s when an app ignores it, so it runs on its own thread
    // and Kiln stops waiting at `timeout` (the thread finishes on its own later).
    let rm_targets = targets.clone();
    let _ = std::thread::Builder::new()
        .name("kiln-restart-manager".into())
        .spawn(move || {
            // SAFETY: session key buffer is sized per CCH_RM_SESSION_KEY (+1 for NUL); the
            // session is always ended; the application array outlives the calls that read it.
            unsafe {
                let mut session = 0u32;
                let mut key = [0u16; CCH_RM_SESSION_KEY as usize + 1];
                if RmStartSession(&mut session, 0, PWSTR(key.as_mut_ptr())) == ERROR_SUCCESS {
                    if RmRegisterResources(session, None, Some(&rm_targets), None) == ERROR_SUCCESS
                    {
                        // lActionFlags = 0: graceful only. RmForceShutdown is NOT used.
                        let status = RmShutdown(session, 0, None);
                        if status != ERROR_SUCCESS {
                            tracing::info!(
                                status = status.0,
                                "Some apps declined to close; leaving them open"
                            );
                        }
                    }
                    let _ = RmEndSession(session);
                } else {
                    tracing::warn!("Restart Manager unavailable; no apps were closed");
                }
            }
        });

    // Give apps the rest of the timeout to finish exiting.
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let running: HashSet<u32> = targets
            .iter()
            .filter(|t| same_process_running(t.dwProcessId, t.ProcessStartTime))
            .map(|t| t.dwProcessId)
            .collect();
        if running.is_empty() || std::time::Instant::now() >= deadline {
            return running;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

pub struct GameModeOptimizer;

impl GameModeOptimizer {
    pub fn new() -> Self {
        Self
    }

    /// Queries system RAM metrics (total_mb, avail_mb).
    pub fn get_ram_metrics(&self) -> (u64, u64) {
        #[cfg(windows)]
        {
            use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
            // SAFETY: MEMORYSTATUSEX is initialised with its required dwLength.
            unsafe {
                let mut status = MEMORYSTATUSEX {
                    dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
                    ..Default::default()
                };
                if GlobalMemoryStatusEx(&mut status).is_ok() {
                    return (
                        status.ullTotalPhys / (1024 * 1024),
                        status.ullAvailPhys / (1024 * 1024),
                    );
                }
            }
            (0, 0)
        }
        #[cfg(not(windows))]
        {
            (0, 0)
        }
    }

    /// Read-only: apps the user has open that Kiln can offer to close.
    pub fn scan_running_apps(&self) -> Vec<RunningApp> {
        let windowed: HashSet<u32> = app_windows().into_keys().collect();
        closable_apps_in(&snapshot_processes(), &windowed, std::process::id(), |p| {
            program_description(p.pid)
        })
    }

    /// Asks the requested apps (by executable name) to close gracefully. Never forces:
    /// anything still running after `timeout` is reported in `still_open`.
    pub fn close_apps(&self, exes: &[String], timeout: Duration) -> CloseReport {
        let procs = snapshot_processes();
        let windows_by_pid = app_windows();
        let windowed: HashSet<u32> = windows_by_pid.keys().copied().collect();
        let (targets, skipped) = plan_close(&procs, &windowed, std::process::id(), exes);
        let mut report = CloseReport {
            skipped,
            ..Default::default()
        };
        // Names as shown on the startup screen.
        let names: HashMap<String, String> =
            closable_apps_in(&procs, &windowed, std::process::id(), |p| {
                program_description(p.pid)
            })
            .into_iter()
            .map(|a| (a.exe, a.name))
            .collect();

        #[cfg(windows)]
        let still_running = {
            let started = std::time::Instant::now();
            let tracked: Vec<(u32, windows::Win32::Foundation::FILETIME)> = targets
                .iter()
                .filter_map(|p| process_start_time(p.pid).map(|t| (p.pid, t)))
                .collect();
            let alive = |set: &[(u32, windows::Win32::Foundation::FILETIME)]| -> Vec<u32> {
                set.iter()
                    .filter(|(pid, t)| same_process_running(*pid, *t))
                    .map(|(pid, _)| *pid)
                    .collect()
            };
            // 1. Click X on every window of the requested apps, then wait for them to exit.
            let pids: HashSet<u32> = tracked.iter().map(|(p, _)| *p).collect();
            post_close_to_windows(&pids, &windows_by_pid);
            let window_phase = timeout.min(Duration::from_secs(4));
            let mut left = alive(&tracked);
            while !left.is_empty() && started.elapsed() < window_phase {
                std::thread::sleep(Duration::from_millis(200));
                left = alive(&tracked);
                // Everything left has hidden its windows (minimised to the tray): no point
                // waiting for it to exit on its own.
                let visible = app_windows();
                if left.iter().all(|pid| !visible.contains_key(pid)) {
                    break;
                }
            }
            // 2. Apps that hid to the tray: the sign-out handshake (they can still say no).
            tracing::debug!(elapsed = ?started.elapsed(), left = left.len(), "after WM_CLOSE");
            if !left.is_empty() {
                let remaining: HashSet<u32> = left.iter().copied().collect();
                request_session_end(&remaining);
                tracing::debug!(elapsed = ?started.elapsed(), "after end-session handshake");
                let phase_end = started.elapsed() + Duration::from_secs(3);
                while !left.is_empty() && started.elapsed() < phase_end.min(timeout) {
                    std::thread::sleep(Duration::from_millis(200));
                    left = alive(&tracked);
                }
            }
            // 3. Anything else: Restart Manager's normal shutdown request, no force.
            tracing::debug!(elapsed = ?started.elapsed(), left = left.len(), "before Restart Manager");
            if left.is_empty() {
                HashSet::new()
            } else {
                request_graceful_close(&left, timeout.saturating_sub(started.elapsed()))
            }
        };
        #[cfg(not(windows))]
        let still_running: HashSet<u32> = {
            let _ = timeout;
            HashSet::new()
        };

        let mut seen: Vec<String> = Vec::new();
        for t in &targets {
            let exe = t.exe.to_ascii_lowercase();
            if !seen.contains(&exe) {
                seen.push(exe);
            }
        }
        for exe in seen {
            let open = targets
                .iter()
                .any(|t| t.exe.eq_ignore_ascii_case(&exe) && still_running.contains(&t.pid));
            let name = names
                .get(&exe)
                .cloned()
                .or_else(|| profile_for(&exe).map(|p| p.name.to_string()))
                .unwrap_or_else(|| fallback_name(&exe));
            if open {
                report.still_open.push(name);
            } else {
                report.closed.push(name);
            }
        }
        report
    }
}

impl Default for GameModeOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, parent_pid: u32, exe: &str) -> ProcessEntry {
        ProcessEntry {
            pid,
            parent_pid,
            exe: exe.to_string(),
        }
    }

    #[test]
    fn system_and_security_processes_are_never_closable() {
        for name in [
            "System",
            "csrss.exe",
            "lsass.exe",
            "svchost.exe",
            "dwm.exe",
            "explorer.exe",
            "MsMpEng.exe",
            "vssvc.exe",
            "chkdsk.exe",
            "trustedinstaller.exe",
            "EasyAntiCheat_EOS.exe",
            "vgc.exe",
            "nvdisplay.container.exe",
            "steam.exe",
            "epicwebhelper.exe",
            "kiln-shell.exe",
        ] {
            assert!(
                !is_safe_to_terminate(name, 5000),
                "{name} must be protected"
            );
        }
    }

    #[test]
    fn protected_and_core_processes_are_never_closable() {
        assert!(is_safe_to_terminate("chrome.exe", 5000));
        assert!(is_safe_to_terminate("random-tool.exe", 5000));
        assert!(!is_safe_to_terminate("svchost.exe", 5000));
        assert!(!is_safe_to_terminate("ApplicationFrameHost.exe", 5000));
        assert!(!is_safe_to_terminate("Kiln.exe", 5000));
        assert!(
            !is_safe_to_terminate("chrome.exe", 4),
            "core PIDs are protected"
        );
        assert!(!is_safe_to_terminate("chrome.exe", std::process::id()));
    }

    #[test]
    fn closable_list_has_no_protected_entries() {
        for p in CLOSABLE_APPS {
            assert!(
                !is_protected(p.exe),
                "{} is both closable and protected",
                p.exe
            );
            assert_eq!(p.exe, p.exe.to_ascii_lowercase());
        }
    }

    #[test]
    fn editors_and_sync_default_to_keep() {
        assert!(!AppCategory::Productivity.closes_by_default());
        assert!(!AppCategory::Sync.closes_by_default());
        assert!(!AppCategory::Downloads.closes_by_default());
        assert!(AppCategory::Browser.closes_by_default());
    }

    #[test]
    fn groups_processes_per_app_and_skips_the_launcher_app() {
        // explorer -> code (main) -> code (pty host) -> pwsh -> kiln-shell (self = 900)
        let procs = vec![
            proc(10, 1, "explorer.exe"),
            proc(100, 10, "Code.exe"),
            proc(101, 100, "Code.exe"),
            proc(102, 100, "Code.exe"),
            proc(500, 101, "pwsh.exe"),
            proc(900, 500, "kiln-shell.exe"),
            proc(200, 10, "chrome.exe"),
            proc(201, 200, "chrome.exe"),
            proc(300, 10, "Discord.exe"),
            proc(400, 10, "svchost.exe"),
            proc(600, 10, "MyTool.exe"),  // unknown app with a window
            proc(601, 600, "MyTool.exe"), // its windowless helper
            proc(700, 10, "helper.exe"),  // unknown, no window: not offered
            proc(800, 10, "nowin.exe"),   // unknown, no description: fallback name
        ];
        // Windows: Kiln's own, VS Code's, svchost's (bogus) and MyTool's / nowin's.
        let windowed: HashSet<u32> = [900, 100, 400, 600, 800].into_iter().collect();
        let name_of = |p: &ProcessEntry| (p.pid == 600).then(|| "My Tool".to_string());
        let apps = closable_apps_in(&procs, &windowed, 900, name_of);
        let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Discord", "Google Chrome", "My Tool", "Nowin"]);
        assert_eq!(apps[1].process_count, 2);
        assert_eq!(apps[2].process_count, 2);
        assert_eq!(apps[2].category, AppCategory::Other);
        assert!(AppCategory::Other.closes_by_default());

        // Even if asked explicitly, VS Code (the launcher), svchost and a windowless unknown
        // app are refused; an app that's no longer running is simply ignored.
        let (targets, skipped) = plan_close(
            &procs,
            &windowed,
            900,
            &[
                "code.exe".into(),
                "svchost.exe".into(),
                "chrome.exe".into(),
                "helper.exe".into(),
                "gone.exe".into(),
                "mytool.exe".into(),
            ],
        );
        let mut exes: Vec<&str> = targets.iter().map(|t| t.exe.as_str()).collect();
        exes.sort();
        assert_eq!(
            exes,
            ["MyTool.exe", "MyTool.exe", "chrome.exe", "chrome.exe"]
        );
        assert_eq!(skipped, ["code.exe", "svchost.exe", "helper.exe"]);
    }

    #[test]
    fn ancestor_walk_survives_cycles() {
        let procs = vec![
            proc(1, 2, "a.exe"),
            proc(2, 1, "b.exe"),
            proc(3, 1, "c.exe"),
        ];
        let a = ancestor_pids(&procs, 3);
        assert!(a.contains(&1) && a.contains(&2));
    }

    #[test]
    fn scanning_is_read_only_and_safe() {
        // Only enumerates; never closes anything.
        let apps = GameModeOptimizer::new().scan_running_apps();
        for a in &apps {
            assert!(!is_protected(&a.exe));
            assert!(a.process_count > 0 && !a.name.trim().is_empty());
        }
    }

    /// Manual end-to-end check of the full close path (WM_CLOSE first) on a Character Map
    /// window this test opens. Closes any other Character Map windows too.
    ///   cargo test -p kiln-sysctl close_apps_closes_a_windowed_app -- --ignored
    #[cfg(windows)]
    #[test]
    #[ignore = "opens and closes a Character Map window"]
    fn close_apps_closes_a_windowed_app() {
        let mut child = std::process::Command::new("charmap.exe")
            .spawn()
            .expect("start charmap");
        std::thread::sleep(Duration::from_millis(1500));
        let listed = GameModeOptimizer::new().scan_running_apps();
        assert!(
            listed.iter().any(|a| a.exe == "charmap.exe"),
            "charmap should be offered: {listed:?}"
        );
        let report =
            GameModeOptimizer::new().close_apps(&["charmap.exe".into()], Duration::from_secs(6));
        let exited = child.try_wait().ok().flatten().is_some();
        if !exited {
            let _ = child.kill();
        }
        assert!(exited, "charmap didn't close: {report:?}");
        assert_eq!(report.closed.len(), 1, "{report:?}");
    }

    /// Manual end-to-end check of the graceful-close path on a process this test starts
    /// itself (Character Map). Ignored by default because it opens a window.
    ///   cargo test -p kiln-sysctl graceful_close_closes_own_test_window -- --ignored
    #[cfg(windows)]
    #[test]
    #[ignore = "opens and closes a Character Map window"]
    fn graceful_close_closes_own_test_window() {
        let mut child = std::process::Command::new("charmap.exe")
            .spawn()
            .expect("start charmap");
        std::thread::sleep(Duration::from_millis(1500));
        let still = request_graceful_close(&[child.id()], Duration::from_secs(5));
        let exited = child.try_wait().ok().flatten().is_some();
        if !exited {
            let _ = child.kill();
        }
        assert!(
            still.is_empty() && exited,
            "test window should close gracefully"
        );
    }
}
