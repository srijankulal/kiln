//! Kiln Updater CLI: check for, download and install Kiln updates from GitHub releases.

use clap::Parser;
use kiln_updater::{rollback_atomic, VERSION};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "kiln-updater", author, version = VERSION, about = "Updater for Kiln")]
pub struct Opts {
    /// Check for available updates without applying
    #[arg(long)]
    pub check: bool,

    /// Download and install the latest release (closes and restarts Kiln)
    #[arg(long)]
    pub apply: bool,

    /// Rollback to previous binary (.old)
    #[arg(long)]
    pub rollback: bool,

    /// With --apply: download the installer but don't run it
    #[arg(long)]
    pub dry_run: bool,

    /// Custom target binary path (for --rollback)
    #[arg(long)]
    pub target_bin: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();
    println!("Kiln Updater (current version: {VERSION})");

    if opts.rollback {
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

    if !opts.check && !opts.apply {
        println!("Nothing to do. Use --check or --apply (see --help).");
        return Ok(());
    }

    println!("Checking GitHub releases for {}...", kiln_updater::REPO);
    let Some(update) = kiln_updater::check()? else {
        println!("Kiln is up to date.");
        return Ok(());
    };
    println!("Kiln {} is available: {}", update.version, update.notes_url);
    if !opts.apply {
        return Ok(());
    }

    let mut last = -1i32;
    let installer = kiln_updater::download(&update, |p| {
        let pct = (p * 100.0) as i32;
        if pct / 10 != last / 10 {
            println!("  downloading... {pct}%");
            last = pct;
        }
    })?;
    println!("Downloaded {}", installer.display());
    if opts.dry_run {
        println!("Dry run: not starting the installer.");
        return Ok(());
    }
    kiln_updater::launch_installer(&installer)?;
    println!("Installer started; Kiln will restart when it finishes.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opts_parsing() {
        let opts = Opts::parse_from(["kiln-updater", "--check", "--dry-run"]);
        assert!(opts.check);
        assert!(opts.dry_run);
        assert!(!opts.rollback);
    }
}
