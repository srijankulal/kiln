//! State machine for Kiln Game Mode transitions.
use crate::error::KilnError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModeState {
    Desktop,
    Entering,
    ConfirmingDisplay { deadline_seconds: u64 },
    GameMode,
    Launching { game_id: [u8; 16] },
    InGame { game_id: [u8; 16] },
    Exiting,
}

pub fn can_transition(from: ModeState, to: ModeState) -> bool {
    use ModeState::*;
    match (from, to) {
        // Normal entry flow
        (Desktop, Entering) => true,
        (Entering, ConfirmingDisplay { .. }) => true,
        (Entering, GameMode) => true,
        (ConfirmingDisplay { .. }, GameMode) => true,

        // Launch and InGame cycle
        (GameMode, Launching { .. }) => true,
        (Launching { .. }, InGame { .. }) => true,
        (InGame { .. }, GameMode) => true,
        (Launching { .. }, GameMode) => true, // Game launch cancelled or failed

        // Exit routes (any non-Desktop state can transition to Exiting)
        (Entering, Exiting) => true,
        (ConfirmingDisplay { .. }, Exiting) => true,
        (GameMode, Exiting) => true,
        (Launching { .. }, Exiting) => true,
        (InGame { .. }, Exiting) => true,
        (Exiting, Desktop) => true,

        // Disallowed transitions
        _ => false,
    }
}

pub struct ModeStateMachine {
    current_state: ModeState,
    crash_count_window: Vec<u64>,
}

impl ModeStateMachine {
    pub fn new() -> Self {
        Self {
            current_state: ModeState::Desktop,
            crash_count_window: Vec::new(),
        }
    }

    pub fn state(&self) -> ModeState {
        self.current_state
    }

    pub fn transition(&mut self, next: ModeState) -> Result<(), KilnError> {
        if can_transition(self.current_state, next) {
            self.current_state = next;
            Ok(())
        } else {
            Err(KilnError::InvalidStateTransition {
                from: self.current_state,
                to: next,
            })
        }
    }

    /// Records a shell crash timestamp and returns true if crash loop breaker triggered (>=3 crashes in 300s).
    pub fn record_shell_crash(&mut self, timestamp: u64) -> bool {
        self.crash_count_window
            .retain(|&t| timestamp.saturating_sub(t) <= 300);
        self.crash_count_window.push(timestamp);
        self.crash_count_window.len() >= 3
    }
}

impl Default for ModeStateMachine {
    fn default() -> Self {
        Self::new()
    }
}
