//! On-Screen Keyboard (OSK / TabTip) launcher abstraction for gamepad navigation.

use std::io::Result;
use std::sync::atomic::{AtomicBool, Ordering};

pub trait VirtualKeyboard: Send + Sync {
    fn show(&self) -> Result<()>;
    fn hide(&self) -> Result<()>;
    fn is_visible(&self) -> bool;
}

#[derive(Default)]
pub struct FakeVirtualKeyboard {
    visible: AtomicBool,
}

impl FakeVirtualKeyboard {
    pub fn new() -> Self {
        Self::default()
    }
}

impl VirtualKeyboard for FakeVirtualKeyboard {
    fn show(&self) -> Result<()> {
        self.visible.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn hide(&self) -> Result<()> {
        self.visible.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn is_visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }
}

pub struct SystemVirtualKeyboard;

impl SystemVirtualKeyboard {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SystemVirtualKeyboard {
    fn default() -> Self {
        Self::new()
    }
}

impl VirtualKeyboard for SystemVirtualKeyboard {
    fn show(&self) -> Result<()> {
        #[cfg(windows)]
        {
            // Try modern Windows 11 TabTip.exe first
            let tab_tip = r"C:\Program Files\Common Files\microsoft shared\ink\TabTip.exe";
            if std::path::Path::new(tab_tip).exists() {
                if std::process::Command::new(tab_tip).spawn().is_ok() {
                    return Ok(());
                }
            }
            // Fallback to classic osk.exe
            std::process::Command::new("osk.exe").spawn()?;
        }

        #[cfg(not(windows))]
        {
            println!("[HOST] Virtual keyboard show requested");
        }

        Ok(())
    }

    fn hide(&self) -> Result<()> {
        #[cfg(windows)]
        {
            // Close tabtip or osk via taskkill
            let _ = std::process::Command::new("taskkill")
                .args(["/IM", "osk.exe", "/F"])
                .output();
        }

        #[cfg(not(windows))]
        {
            println!("[HOST] Virtual keyboard hide requested");
        }

        Ok(())
    }

    fn is_visible(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fake_keyboard() {
        let kb = FakeVirtualKeyboard::new();
        assert!(!kb.is_visible());
        assert!(kb.show().is_ok());
        assert!(kb.is_visible());
        assert!(kb.hide().is_ok());
        assert!(!kb.is_visible());
    }
}
