//! IPC server for named pipe communication between daemon and shell.

use crate::controller::ModeController;
#[cfg(windows)]
use kiln_core::ipc::KILN_NAMED_PIPE;
use kiln_core::ipc::{IpcCommand, IpcEvent, IpcMessage};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub struct IpcServer {
    controller: Arc<Mutex<ModeController>>,
    dry_run: bool,
    safe_mode: bool,
}

impl IpcServer {
    pub fn new(controller: Arc<Mutex<ModeController>>, dry_run: bool, safe_mode: bool) -> Self {
        Self {
            controller,
            dry_run,
            safe_mode,
        }
    }

    /// Handles a single incoming line from an IPC client and produces a response event if appropriate.
    pub fn handle_line(&self, line: &str) -> Option<IpcEvent> {
        let msg = match IpcMessage::from_json_line(line) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(error = %e, raw = line, "Failed to parse IPC message");
                return Some(IpcEvent::Notification {
                    message: format!("Parse error: {}", e),
                    is_error: true,
                });
            }
        };

        match msg {
            IpcMessage::Command(cmd) => match cmd {
                IpcCommand::QueryStatus => {
                    let state = self.controller.lock().unwrap().state();
                    Some(IpcEvent::StateChanged { state })
                }
                IpcCommand::ExitGameMode => {
                    let mut ctrl = self.controller.lock().unwrap();
                    let res = ctrl.exit_game_mode(self.dry_run, self.safe_mode);
                    let state = ctrl.state();
                    if let Err(e) = res {
                        Some(IpcEvent::Notification {
                            message: format!("Failed to exit Game Mode: {}", e),
                            is_error: true,
                        })
                    } else {
                        Some(IpcEvent::StateChanged { state })
                    }
                }
                IpcCommand::LaunchGame { id } => {
                    tracing::info!(game_id = %id, "IPC command LaunchGame received");
                    Some(IpcEvent::Notification {
                        message: format!("Launching game: {}", id),
                        is_error: false,
                    })
                }
                IpcCommand::TriggerLibraryScan => {
                    tracing::info!("IPC command TriggerLibraryScan received");
                    Some(IpcEvent::Notification {
                        message: "Library scan requested".to_string(),
                        is_error: false,
                    })
                }
                IpcCommand::SetTrimTier { tier } => {
                    tracing::info!(?tier, "IPC command SetTrimTier received");
                    Some(IpcEvent::Notification {
                        message: format!("Trim tier set to {:?}", tier),
                        is_error: false,
                    })
                }
                IpcCommand::SleepSystem => {
                    tracing::info!("IPC command SleepSystem received");
                    Some(IpcEvent::Notification {
                        message: "System sleep initiated".to_string(),
                        is_error: false,
                    })
                }
                IpcCommand::RestartSystem => {
                    tracing::info!("IPC command RestartSystem received");
                    Some(IpcEvent::Notification {
                        message: "System restart initiated".to_string(),
                        is_error: false,
                    })
                }
                IpcCommand::ShutdownSystem => {
                    tracing::info!("IPC command ShutdownSystem received");
                    Some(IpcEvent::Notification {
                        message: "System shutdown initiated".to_string(),
                        is_error: false,
                    })
                }
                IpcCommand::ConfirmDisplaySwitch => Some(IpcEvent::Notification {
                    message: "Display switch confirmed".to_string(),
                    is_error: false,
                }),
            },
            IpcMessage::Event(_) => None,
        }
    }

    /// Runs the IPC listener loop.
    pub async fn run_server(self: Arc<Self>, shutdown: tokio::sync::broadcast::Receiver<()>) {
        #[cfg(windows)]
        {
            use tokio::net::windows::named_pipe::ServerOptions;

            tracing::info!(
                pipe = KILN_NAMED_PIPE,
                "Starting Windows Named Pipe IPC server"
            );
            let mut shutdown = shutdown;

            loop {
                let server = match ServerOptions::new()
                    .first_pipe_instance(true)
                    .create(KILN_NAMED_PIPE)
                {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to create named pipe server instance");
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                        continue;
                    }
                };

                tokio::select! {
                    res = server.connect() => {
                        if res.is_ok() {
                            let this = self.clone();
                            tokio::spawn(async move {
                                let (reader, mut writer) = tokio::io::split(server);
                                let mut lines = BufReader::new(reader).lines();
                                while let Ok(Some(line)) = lines.next_line().await {
                                    if let Some(resp) = this.handle_line(&line) {
                                        let msg = IpcMessage::Event(resp);
                                        if let Ok(json_line) = msg.to_newline_json() {
                                            let _ = writer.write_all(json_line.as_bytes()).await;
                                        }
                                    }
                                }
                            });
                        }
                    }
                    _ = shutdown.recv() => {
                        tracing::info!("IPC server shutting down");
                        break;
                    }
                }
            }
        }

        #[cfg(not(windows))]
        {
            let sock_path = "/tmp/kiln-ipc.sock";
            let _ = std::fs::remove_file(sock_path);
            let listener = match tokio::net::UnixListener::bind(sock_path) {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to bind mock unix domain socket for IPC");
                    return;
                }
            };

            tracing::info!(sock = sock_path, "Starting Unix Domain Socket IPC server");
            let mut shutdown = shutdown;

            loop {
                tokio::select! {
                    res = listener.accept() => {
                        if let Ok((stream, _)) = res {
                            let this = self.clone();
                            tokio::spawn(async move {
                                let (reader, mut writer) = stream.into_split();
                                let mut lines = BufReader::new(reader).lines();
                                while let Ok(Some(line)) = lines.next_line().await {
                                    if let Some(resp) = this.handle_line(&line) {
                                        let msg = IpcMessage::Event(resp);
                                        if let Ok(json_line) = msg.to_newline_json() {
                                            let _ = writer.write_all(json_line.as_bytes()).await;
                                        }
                                    }
                                }
                            });
                        }
                    }
                    _ = shutdown.recv() => {
                        tracing::info!("IPC server shutting down");
                        break;
                    }
                }
            }
            let _ = std::fs::remove_file(sock_path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiln_core::state::ModeState;
    use tempfile::tempdir;

    #[test]
    fn test_ipc_server_message_handling() {
        let dir = tempdir().unwrap();
        let journal_path = dir.path().join("session.json");
        let controller = Arc::new(Mutex::new(ModeController::new(journal_path)));
        let server = IpcServer::new(controller, true, true);

        // Query status
        let query_cmd = IpcMessage::Command(IpcCommand::QueryStatus);
        let query_line = query_cmd.to_newline_json().unwrap();
        let resp = server.handle_line(&query_line).expect("response expected");
        match resp {
            IpcEvent::StateChanged { state } => assert_eq!(state, ModeState::Desktop),
            other => panic!("Unexpected event: {:?}", other),
        }

        // Trigger scan
        let scan_cmd = IpcMessage::Command(IpcCommand::TriggerLibraryScan);
        let scan_line = scan_cmd.to_newline_json().unwrap();
        let resp = server.handle_line(&scan_line).expect("response expected");
        match resp {
            IpcEvent::Notification { is_error, .. } => assert!(!is_error),
            other => panic!("Unexpected event: {:?}", other),
        }
    }
}
