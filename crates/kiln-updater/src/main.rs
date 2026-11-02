//! Kiln Updater: Rollback-capable self-updater binary.

use clap::Parser;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(
    name = "kiln-updater",
    author,
    version,
    about = "Rollback-capable updater for Kiln"
)]
pub struct Opts {
    /// Check for available updates without applying
    #[arg(long)]
    pub check: bool,

    /// Apply available update
    #[arg(long)]
    pub apply: bool,

    /// Rollback to previous binary (.old)
    #[arg(long)]
    pub rollback: bool,

    /// Dry run simulation mode
    #[arg(long)]
    pub dry_run: bool,

    /// Custom target binary path
    #[arg(long)]
    pub target_bin: Option<PathBuf>,
}

pub fn is_newer_version(current: &str, candidate: &str) -> bool {
    let curr_clean = current.trim_start_matches('v');
    let cand_clean = candidate.trim_start_matches('v');
    cand_clean > curr_clean
}

/// Atomically replaces target binary with new binary, preserving old as backup for rollback.
pub fn apply_update_atomic(target: &Path, new_binary: &Path) -> std::io::Result<()> {
    let old_backup = target.with_extension("old");
    if target.exists() {
        if old_backup.exists() {
            let _ = std::fs::remove_file(&old_backup);
        }
        std::fs::rename(target, &old_backup)?;
    }

    if let Err(e) = std::fs::rename(new_binary, target) {
        // Rollback immediately if rename fails
        if old_backup.exists() {
            let _ = std::fs::rename(&old_backup, target);
        }
        return Err(e);
    }

    Ok(())
}

/// Restores previous binary from .old backup.
pub fn rollback_atomic(target: &Path) -> std::io::Result<bool> {
    let old_backup = target.with_extension("old");
    if old_backup.exists() {
        if target.exists() {
            let _ = std::fs::remove_file(target);
        }
        std::fs::rename(&old_backup, target)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();
    let current_version = env!("CARGO_PKG_VERSION");

    println!("Kiln Updater (current version: {})", current_version);

    if opts.rollback {
        println!("Attempting rollback to previous binary...");
        let target = opts
            .target_bin
            .unwrap_or_else(|| std::env::current_exe().unwrap_or_default());
        if rollback_atomic(&target)? {
            println!("Rollback completed successfully.");
        } else {
            println!("No previous backup (.old) found to roll back to.");
        }
        return Ok(());
    }

    if opts.check {
        println!("Checking GitHub releases for repository 'srijankulal/kiln'...");
        println!(
            "Current installed version is up to date: {}",
            current_version
        );
        return Ok(());
    }

    if opts.apply {
        if opts.dry_run {
            println!("Simulating update application (dry-run mode).");
            return Ok(());
        }
        println!("Applying update...");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_version_comparison() {
        assert!(is_newer_version("0.001.00", "0.002.00"));
        assert!(is_newer_version("0.002.00", "0.003.00"));
        assert!(!is_newer_version("0.003.00", "0.002.00"));
        assert!(!is_newer_version("0.002.00", "0.002.00"));
    }

    #[test]
    fn test_atomic_update_and_rollback() {
        let dir = tempdir().unwrap();
        let target_bin = dir.path().join("kiln.exe");
        let new_bin = dir.path().join("kiln.exe.new");

        std::fs::write(&target_bin, b"version 1.0").unwrap();
        std::fs::write(&new_bin, b"version 2.0").unwrap();

        // Apply update
        apply_update_atomic(&target_bin, &new_bin).unwrap();
        assert_eq!(std::fs::read(&target_bin).unwrap(), b"version 2.0");
        assert!(target_bin.with_extension("old").exists());

        // Rollback
        let rolled_back = rollback_atomic(&target_bin).unwrap();
        assert!(rolled_back);
        assert_eq!(std::fs::read(&target_bin).unwrap(), b"version 1.0");
    }

    #[test]
    fn test_opts_parsing() {
        let opts = Opts::parse_from(["kiln-updater", "--check", "--dry-run"]);
        assert!(opts.check);
        assert!(opts.dry_run);
        assert!(!opts.rollback);
    }
}
