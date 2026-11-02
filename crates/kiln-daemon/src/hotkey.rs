//! Global hotkey listener for Game Mode toggle (Ctrl+Alt+Shift+G).

pub const HOTKEY_ID: i32 = 0x4B49; // 'KI'

pub struct HotkeyManager;

impl HotkeyManager {
    pub fn new() -> Self {
        Self
    }

    pub fn register(&self) -> bool {
        #[cfg(windows)]
        {
            use windows::Win32::UI::Input::KeyboardAndMouse::{
                RegisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT,
            };

            unsafe {
                let modifiers =
                    HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0 | MOD_SHIFT.0 | MOD_NOREPEAT.0);
                let res = RegisterHotKey(None, HOTKEY_ID, modifiers, 'G' as u32);
                res.is_ok()
            }
        }
        #[cfg(not(windows))]
        {
            tracing::info!("Mock global hotkey registered (Ctrl+Alt+Shift+G)");
            true
        }
    }

    pub fn unregister(&self) {
        #[cfg(windows)]
        {
            use windows::Win32::UI::Input::KeyboardAndMouse::UnregisterHotKey;
            unsafe {
                let _ = UnregisterHotKey(None, HOTKEY_ID);
            }
        }
        #[cfg(not(windows))]
        {
            tracing::info!("Mock global hotkey unregistered");
        }
    }
}

impl Default for HotkeyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        self.unregister();
    }
}
