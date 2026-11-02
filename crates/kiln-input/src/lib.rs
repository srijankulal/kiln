//! Kiln input: Gamepad polling and navigation mapping.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    EmergencyAbort,
}
