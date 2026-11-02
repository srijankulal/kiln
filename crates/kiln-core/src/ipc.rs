//! IPC schema for named pipe communication between kiln-daemon and kiln-shell.
use crate::config::TrimTier;
use crate::state::ModeState;
use serde::{Deserialize, Serialize};

pub const KILN_NAMED_PIPE: &str = r"\\.\pipe\kiln-ipc";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcCommand {
    LaunchGame { id: String },
    ExitGameMode,
    ConfirmDisplaySwitch,
    SetTrimTier { tier: TrimTier },
    TriggerLibraryScan,
    QueryStatus,
    SleepSystem,
    RestartSystem,
    ShutdownSystem,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcEvent {
    StateChanged {
        state: ModeState,
    },
    DisplayConfirmPrompt {
        remaining_seconds: u32,
    },
    GameExited {
        id: String,
        playtime_seconds: u64,
    },
    TrimReport {
        suspended_count: usize,
        memory_saved_mb: u64,
    },
    Notification {
        message: String,
        is_error: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcMessage {
    Command(IpcCommand),
    Event(IpcEvent),
}

impl IpcMessage {
    pub fn to_newline_json(&self) -> Result<String, serde_json::Error> {
        let mut s = serde_json::to_string(self)?;
        s.push('\n');
        Ok(s)
    }

    pub fn from_json_line(line: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(line.trim())
    }
}
