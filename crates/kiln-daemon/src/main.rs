//! Kiln daemon: Always-on background supervisor binary.

pub mod controller;
pub mod hotkey;
pub mod ipc_server;
pub mod supervisor;

use clap::Parser;
use controller::ModeController;
use hotkey::HotkeyManager;
use ipc_server::IpcServer;
use kiln_core::state::ModeState;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use supervisor::ShellSupervisor;

#[derive(Parser, Debug)]
#[command(
    name = "kiln-daemon",
    author,
    version,
    about = "Always-on background supervisor daemon for Kiln"
)]
pub struct Opts {
    /// Safe mode: run windowed without display switching or power plan modifications
    #[arg(long)]
    pub safe: bool,

    /// Dry run: simulate all actions without Win32 mutations
    #[arg(long)]
    pub dry_run: bool,

    /// Force emergency restore of all system settings and exit
    #[arg(long)]
    pub restore: bool,

    /// Run diagnostic self-check of system capabilities and exit
    #[arg(long)]
    pub self_check: bool,

    /// Path to kiln-shell executable
    #[arg(long)]
    pub shell_bin: Option<PathBuf>,

    /// Path to session journal file
    #[arg(long)]
    pub journal: Option<PathBuf>,
}

fn get_app_dir() -> PathBuf {
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        PathBuf::from(local_app_data).join("Kiln")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local/share/Kiln")
    } else {
        PathBuf::from(".")
    }
}

fn find_shell_binary() -> PathBuf {
    let current_exe = std::env::current_exe().unwrap_or_default();
    let parent = current_exe.parent().unwrap_or(std::path::Path::new("."));

    #[cfg(windows)]
    let shell_name = "kiln-shell.exe";
    #[cfg(not(windows))]
    let shell_name = "kiln-shell";

    let neighbor = parent.join(shell_name);
    if neighbor.exists() {
        neighbor
    } else {
        PathBuf::from(shell_name)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    let opts = Opts::parse();
    let app_dir = get_app_dir();
    let _ = std::fs::create_dir_all(&app_dir);

    let journal_path = opts.journal.unwrap_or_else(|| app_dir.join("session.json"));
    let shell_bin = opts.shell_bin.unwrap_or_else(find_shell_binary);

    println!(
        "Kiln Daemon starting (safe={}, dry_run={}, journal={})",
        opts.safe,
        opts.dry_run,
        journal_path.display()
    );

    let controller = Arc::new(Mutex::new(ModeController::new(journal_path.clone())));

    // Global Panic Hook: guarantees idempotent restore on any unexpected panic
    let panic_ctrl = controller.clone();
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        eprintln!("CRITICAL PANIC DETECTED: Triggering emergency system restore!");
        if let Ok(mut ctrl) = panic_ctrl.lock() {
            let _ = ctrl.emergency_restore();
        }
        default_panic(info);
    }));

    // Mode: --restore
    if opts.restore {
        println!("Performing emergency system restore as requested by CLI...");
        controller.lock().unwrap().emergency_restore()?;
        println!("System restored to Desktop successfully.");
        return Ok(());
    }

    // Mode: --self-check
    if opts.self_check {
        println!("=== Kiln System Capability Self-Check ===");
        let ctrl = controller.lock().unwrap();
        println!("- State machine initial state: {:?}", ctrl.state());
        println!("- Journal path: {}", ctrl.journal_path().display());
        println!("- Shell binary: {}", shell_bin.display());
        println!("Self-check passed.");
        return Ok(());
    }

    // On startup: Check for crash recovery from previous unclean session
    {
        let mut ctrl = controller.lock().unwrap();
        if let Ok(true) = ctrl.check_and_recover() {
            println!("Recovered from previous crashed/interrupted session.");
        }
    }

    // Register global hotkey (Ctrl+Alt+Shift+G)
    let hotkey = HotkeyManager::new();
    if hotkey.register() {
        println!("Global hotkey (Ctrl+Alt+Shift+G) registered.");
    } else {
        println!("Note: Global hotkey could not be registered (may be in use).");
    }

    // Start Named Pipe IPC Server
    let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);
    let ipc_server = Arc::new(IpcServer::new(controller.clone(), opts.dry_run, opts.safe));
    let server_task = tokio::spawn({
        let server = ipc_server.clone();
        async move {
            server.run_server(shutdown_rx).await;
        }
    });

    let mut supervisor =
        ShellSupervisor::new(controller.clone(), shell_bin, opts.dry_run, opts.safe);

    println!("Kiln Daemon running in background. Press Ctrl+C to terminate.");

    // Daemon background loop
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("\nShutdown signal received, restoring system and exiting...");
                let mut ctrl = controller.lock().unwrap();
                let _ = ctrl.exit_game_mode(opts.dry_run, opts.safe);
                let _ = shutdown_tx.send(());
                break;
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(200)) => {
                // Poll child process if shell is running
                if supervisor.is_running() {
                    let _ = supervisor.poll_child();
                }

                // If state is GameMode but shell is not running, spawn shell
                let current_state = controller.lock().unwrap().state();
                if current_state == ModeState::GameMode && !supervisor.is_running() {
                    if let Err(e) = supervisor.spawn_shell() {
                        tracing::error!(error = %e, "Failed to spawn Kiln Shell, reverting to Desktop");
                        let mut ctrl = controller.lock().unwrap();
                        let _ = ctrl.exit_game_mode(opts.dry_run, opts.safe);
                    }
                }
            }
        }
    }

    let _ = server_task.await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opts_parsing() {
        let opts = Opts::parse_from(["kiln-daemon", "--dry-run", "--safe"]);
        assert!(opts.dry_run);
        assert!(opts.safe);
        assert!(!opts.restore);
        assert!(!opts.self_check);
    }

    #[test]
    fn test_opts_restore() {
        let opts = Opts::parse_from(["kiln-daemon", "--restore"]);
        assert!(opts.restore);
    }
}
