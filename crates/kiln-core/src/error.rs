//! Error types for Kiln core and subsystems.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum KilnError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("System control error: {0}")]
    Sysctl(#[from] SysctlError),

    #[error("Journal error: {0}")]
    Journal(String),

    #[error("Invalid state transition from {from:?} to {to:?}")]
    InvalidStateTransition {
        from: crate::state::ModeState,
        to: crate::state::ModeState,
    },

    #[error("Timeout error: {0}")]
    Timeout(String),

    #[error("Emergency abort requested: {0}")]
    EmergencyAbort(String),
}

#[derive(Debug, Error)]
pub enum SysctlError {
    #[error("Win32 API error: {0} (code: {1:#x})")]
    Win32(String, u32),

    #[error("Display configuration error: {0}")]
    Display(String),

    #[error("Power management error: {0}")]
    Power(String),

    #[error("Process trim error: {0}")]
    Process(String),

    #[error("Audio endpoint error: {0}")]
    Audio(String),

    #[error("Shell / Taskbar error: {0}")]
    Shell(String),

    #[error("Access denied or elevation required for {0}")]
    AccessDenied(String),
}
