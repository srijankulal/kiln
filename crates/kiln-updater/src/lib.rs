//! Kiln updates: asks GitHub for the latest release, downloads its installer and runs it.
//!
//! Every release publishes `kiln-<version>-setup.exe` (see `installer/kiln.nsi`). Updating
//! runs that installer silently with `/UPDATE`: it waits for Kiln to close, replaces the
//! files in place and starts Kiln again. Blocking calls; run them off the UI thread.

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use kiln_core::version::{is_newer, VERSION};

/// The GitHub repository releases are published to.
pub const REPO: &str = "srijankulal/kiln";

/// A release newer than the running build.
#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    pub version: String,
    pub installer_url: String,
    pub installer_name: String,
    pub size: u64,
    pub notes_url: String,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

fn client(timeout: Duration) -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(concat!("kiln-updater/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .build()?)
}

/// Picks the update from GitHub's "latest release" JSON, if it's newer than `current`.
pub fn update_from_release_json(json: &str, current: &str) -> Result<Option<Update>> {
    let rel: GhRelease = serde_json::from_str(json).context("unexpected release data")?;
    if rel.draft || rel.prerelease || !is_newer(current, &rel.tag_name) {
        return Ok(None);
    }
    let asset = rel
        .assets
        .into_iter()
        .find(|a| a.name.to_ascii_lowercase().ends_with("-setup.exe"))
        .ok_or_else(|| anyhow!("release {} has no installer", rel.tag_name))?;
    Ok(Some(Update {
        version: rel.tag_name.trim_start_matches('v').to_string(),
        installer_url: asset.browser_download_url,
        installer_name: asset.name,
        size: asset.size,
        notes_url: rel.html_url,
    }))
}

/// Asks GitHub for the latest release. `Ok(None)` when this build is already current.
pub fn check() -> Result<Option<Update>> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let resp = client(Duration::from_secs(20))?
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .context("couldn't reach GitHub")?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None); // no releases yet
    }
    if !resp.status().is_success() {
        bail!("GitHub answered {}", resp.status());
    }
    update_from_release_json(&resp.text()?, VERSION)
}

/// Downloads the update's installer into a temp folder, reporting progress (0.0..=1.0).
pub fn download(update: &Update, mut progress: impl FnMut(f32)) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join("kiln-update");
    std::fs::create_dir_all(&dir)?;
    // Only a plain file name from the release, never a path.
    let name = Path::new(&update.installer_name)
        .file_name()
        .ok_or_else(|| anyhow!("bad installer name"))?;
    let dest = dir.join(name);
    let partial = dest.with_extension("part");

    let mut resp = client(Duration::from_secs(600))?
        .get(&update.installer_url)
        .send()
        .context("download failed")?
        .error_for_status()
        .context("download failed")?;
    let total = resp.content_length().unwrap_or(update.size).max(1);
    let mut file = std::fs::File::create(&partial)?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = resp.read(&mut buf).context("download interrupted")?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        progress((done as f32 / total as f32).min(1.0));
    }
    file.flush()?;
    drop(file);
    if update.size > 0 && done != update.size {
        let _ = std::fs::remove_file(&partial);
        bail!("download incomplete ({done} of {} bytes)", update.size);
    }
    let _ = std::fs::remove_file(&dest);
    std::fs::rename(&partial, &dest)?;
    Ok(dest)
}

/// Starts the downloaded installer in update mode. The caller should exit straight after:
/// the installer waits for Kiln to close, installs, then starts Kiln again.
pub fn launch_installer(installer: &Path) -> Result<()> {
    std::process::Command::new(installer)
        .args(["/S", "/UPDATE"])
        .spawn()
        .with_context(|| format!("couldn't start {}", installer.display()))?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const RELEASE: &str = r#"{
        "tag_name": "0.003.06",
        "html_url": "https://github.com/srijankulal/kiln/releases/tag/0.003.06",
        "draft": false, "prerelease": false,
        "assets": [
            {"name": "kiln-0.003.06-windows-x64.zip", "browser_download_url": "https://x/zip", "size": 10},
            {"name": "kiln-0.003.06-setup.exe", "browser_download_url": "https://x/setup", "size": 20}
        ]
    }"#;

    #[test]
    fn finds_newer_installer() {
        let u = update_from_release_json(RELEASE, "0.003.05")
            .unwrap()
            .unwrap();
        assert_eq!(u.version, "0.003.06");
        assert_eq!(u.installer_url, "https://x/setup");
        assert_eq!(u.size, 20);
    }

    #[test]
    fn current_or_newer_build_needs_nothing() {
        assert_eq!(update_from_release_json(RELEASE, "0.003.06").unwrap(), None);
        assert_eq!(update_from_release_json(RELEASE, "0.004.00").unwrap(), None);
    }

    #[test]
    fn release_without_installer_is_an_error() {
        let json = r#"{"tag_name": "0.009.00", "html_url": "", "assets": []}"#;
        assert!(update_from_release_json(json, "0.003.05").is_err());
    }

    #[test]
    fn test_atomic_update_and_rollback() {
        let dir = tempdir().unwrap();
        let target_bin = dir.path().join("kiln.exe");
        let new_bin = dir.path().join("kiln.exe.new");

        std::fs::write(&target_bin, b"version 1.0").unwrap();
        std::fs::write(&new_bin, b"version 2.0").unwrap();

        apply_update_atomic(&target_bin, &new_bin).unwrap();
        assert_eq!(std::fs::read(&target_bin).unwrap(), b"version 2.0");
        assert!(target_bin.with_extension("old").exists());

        let rolled_back = rollback_atomic(&target_bin).unwrap();
        assert!(rolled_back);
        assert_eq!(std::fs::read(&target_bin).unwrap(), b"version 1.0");
    }
}
