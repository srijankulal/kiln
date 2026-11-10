//! Native Windows dialogs used by the shell (read-only: they only return what the user picks).

use std::path::PathBuf;

/// Shows the standard Windows "Select folder" dialog on top of this app's window.
/// Returns `None` if the user cancels. Blocks until the dialog closes, so call it from the
/// UI thread (it runs its own message loop, keeping the app responsive underneath).
pub fn pick_folder(title: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        windows_impl::pick_folder(title)
    }
    #[cfg(not(windows))]
    {
        let _ = title;
        None
    }
}

#[cfg(windows)]
mod windows_impl {
    use std::path::PathBuf;
    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS,
        SIGDN_FILESYSPATH,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
    };

    /// This process's main visible window, so the dialog opens on top of Kiln (even when
    /// Kiln is full screen) instead of behind it.
    fn own_window() -> HWND {
        unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
            // SAFETY: lparam is the &mut HWND passed below, alive for the EnumWindows call.
            let out = &mut *(lparam.0 as *mut HWND);
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == std::process::id() && IsWindowVisible(hwnd).as_bool() {
                *out = hwnd;
                return false.into();
            }
            true.into()
        }
        let mut found = HWND::default();
        // SAFETY: the callback only runs during this call, while `found` is alive.
        unsafe {
            let _ = EnumWindows(Some(visit), LPARAM(&mut found as *mut HWND as isize));
        }
        found
    }

    pub fn pick_folder(title: &str) -> Option<PathBuf> {
        // SAFETY: standard COM usage on the calling (UI) thread. CoInitializeEx returning
        // S_FALSE / RPC_E_CHANGED_MODE (already initialised) is fine for an STA dialog.
        // The PWSTR from GetDisplayName is freed with CoTaskMemFree as documented.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
            let options = dialog.GetOptions().ok()?;
            dialog
                .SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST)
                .ok()?;
            let _ = dialog.SetTitle(&HSTRING::from(title));
            let owner = own_window();
            dialog.Show(owner).ok()?; // Err = cancelled
            let item = dialog.GetResult().ok()?;
            let name: PWSTR = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            let path = name.to_string().ok();
            CoTaskMemFree(Some(name.0 as *const _));
            path.map(PathBuf::from)
        }
    }
}
