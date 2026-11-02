//! Kiln input: Gamepad polling, emergency chord detection, navigation routing, and mouse mode.

pub mod chord;
pub mod deadzone;
pub mod mouse;
pub mod osk;
pub mod poller;

pub use chord::{ChordDetector, ChordEvent};
pub use deadzone::{AnalogConfig, AnalogProcessor};
#[cfg(windows)]
pub use mouse::Win32MouseDriver;
pub use mouse::{FakeMouseDriver, MouseAction, MouseController, MouseDriver};
pub use osk::{FakeVirtualKeyboard, SystemVirtualKeyboard, VirtualKeyboard};
pub use poller::{FakeInputPoller, GilrsPoller, InputPoller};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NavAction {
    Up,
    Down,
    Left,
    Right,
    Select,
    Back,
    ContextMenu,
    Options,
    NextTab,
    PrevTab,
    ToggleMouseMode,
    EmergencyAbort,
}
