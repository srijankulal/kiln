//! Write-Ahead Journaling (WAL) engine for atomic state recording and idempotent recovery.
use crate::error::KilnError;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayTopology {
    Internal,
    External,
    Clone,
    Extend,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplaySnapshot {
    pub topology: DisplayTopology,
    pub external_connected: bool,
    pub raw_paths: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerSnapshot {
    pub active_scheme: Uuid,
    pub ac_lid_action: u32,
    pub dc_lid_action: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessTrimKind {
    EcoQoS,
    Suspended,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessSnapshot {
    pub pid: u32,
    pub name: String,
    pub creation_time: u64,
    pub trim_kind: ProcessTrimKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskbarSnapshot {
    pub primary_visible: bool,
    pub secondary_visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioSnapshot {
    pub default_endpoint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionJournal {
    pub session_id: Uuid,
    pub created_at_timestamp: u64,
    pub display: Option<DisplaySnapshot>,
    pub power: Option<PowerSnapshot>,
    pub taskbar: Option<TaskbarSnapshot>,
    pub audio: Option<AudioSnapshot>,
    pub trimmed_processes: Vec<ProcessSnapshot>,
    pub completed_cleanly: bool,
}

impl SessionJournal {
    pub fn new(timestamp: u64) -> Self {
        Self {
            session_id: Uuid::new_v4(),
            created_at_timestamp: timestamp,
            display: None,
            power: None,
            taskbar: None,
            audio: None,
            trimmed_processes: Vec::new(),
            completed_cleanly: false,
        }
    }

    /// Atomically persists the journal to disk:
    /// Writes to a temporary file, calls fsync (sync_all), then atomically renames.
    pub fn save_atomic(&self, path: &Path) -> Result<(), KilnError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let tmp_path = path.with_extension("tmp");
        {
            let mut file = File::create(&tmp_path)?;
            let json = serde_json::to_vec_pretty(self)?;
            file.write_all(&json)?;
            file.sync_all()?;
        }

        fs::rename(&tmp_path, path)?;
        Ok(())
    }

    /// Loads the journal strictly from disk. Returns Ok(None) if file does not exist.
    pub fn load_from_disk(path: &Path) -> Result<Option<Self>, KilnError> {
        if !path.exists() {
            return Ok(None);
        }

        let data = fs::read(path)?;
        let journal: Self = serde_json::from_slice(&data)?;
        Ok(Some(journal))
    }

    /// Safely deletes the journal after verified full restoration.
    pub fn remove_journal(path: &Path) -> Result<(), KilnError> {
        if path.exists() {
            fs::remove_file(path)?;
        }
        let tmp_path = path.with_extension("tmp");
        if tmp_path.exists() {
            let _ = fs::remove_file(tmp_path);
        }
        Ok(())
    }
}
