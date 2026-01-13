//! Background mux daemon.

use super::endpoint::SocketEndpoint;
use super::ipc;
use super::protocol::{CaptureKind, MuxRequest, MuxResponse, SessionInfo, WindowInfo};
use anyhow::{Context, Result};
use interprocess::local_socket::traits::{ListenerExt, Stream as StreamTrait};
use interprocess::local_socket::{ListenerOptions, Stream};
use std::io;
use std::path::Path;
use std::time::Duration;
use tracing::{debug, info, warn};

const IPC_PING_TIMEOUT: Duration = Duration::from_millis(250);

/// Run the mux daemon in the foreground.
///
/// # Errors
///
/// Returns an error if the listener cannot be created or if a fatal I/O error occurs.
pub fn run(endpoint: &SocketEndpoint) -> Result<()> {
    if let Some(path) = endpoint
        .cleanup_path
        .as_ref()
        .and_then(|path| path.parent())
    {
        std::fs::create_dir_all(path)
            .with_context(|| format!("Failed to create mux socket directory {}", path.display()))?;
    }

    let listener = match ListenerOptions::new()
        .name(endpoint.name.clone())
        .create_sync()
    {
        Ok(listener) => listener,
        Err(err) if err.kind() == io::ErrorKind::AddrInUse => {
            if try_ping_existing(endpoint) {
                info!(endpoint = %endpoint.display, "Mux daemon already running");
                return Ok(());
            }

            if let Some(path) = endpoint.cleanup_path.as_ref() {
                let _ = std::fs::remove_file(path);
                ListenerOptions::new()
                    .name(endpoint.name.clone())
                    .create_sync()
                    .context("Failed to recreate mux listener after cleanup")?
            } else {
                return Err(err).context("Mux endpoint is already in use");
            }
        }
        Err(err) => return Err(err).context("Failed to create mux listener"),
    };

    info!(endpoint = %endpoint.display, "Mux daemon listening");

    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                std::thread::spawn(move || {
                    if let Err(err) = handle_connection(stream) {
                        debug!(error = %err, "Mux client connection closed");
                    }
                });
            }
            Err(err) => {
                warn!(error = %err, "Mux accept failed");
            }
        }
    }

    Ok(())
}

fn try_ping_existing(endpoint: &SocketEndpoint) -> bool {
    match Stream::connect(endpoint.name.clone()) {
        Ok(mut stream) => {
            if stream.set_nonblocking(true).is_err() {
                return false;
            }
            if ipc::write_json_with_timeout(&mut stream, &MuxRequest::Ping, IPC_PING_TIMEOUT)
                .is_err()
            {
                return false;
            }
            ipc::read_json_with_timeout::<_, MuxResponse>(&mut stream, IPC_PING_TIMEOUT).is_ok()
        }
        Err(_) => false,
    }
}

fn handle_connection(mut stream: Stream) -> Result<()> {
    loop {
        let request: MuxRequest = match ipc::read_json(&mut stream) {
            Ok(req) => req,
            Err(err) => {
                return Err(err);
            }
        };

        let response = match dispatch_request(request) {
            Ok(response) => response,
            Err(err) => MuxResponse::Err {
                message: err.to_string(),
            },
        };

        ipc::write_json(&mut stream, &response)?;
    }
}

fn dispatch_request(request: MuxRequest) -> Result<MuxResponse> {
    match request {
        MuxRequest::Ping => handle_ping(),
        MuxRequest::ListSessions => Ok(handle_list_sessions()),
        MuxRequest::SessionExists { name } => Ok(handle_session_exists(&name)),
        MuxRequest::CreateSession {
            name,
            working_dir,
            command,
            ..
        } => handle_create_session(&name, &working_dir, &command),
        MuxRequest::KillSession { name } => handle_kill_session(&name),
        MuxRequest::RenameSession { old_name, new_name } => {
            handle_rename_session(&old_name, &new_name)
        }
        MuxRequest::ListWindows { session } => handle_list_windows(&session),
        MuxRequest::CreateWindow {
            session,
            window_name,
            working_dir,
            command,
            ..
        } => handle_create_window(&session, &window_name, &working_dir, &command),
        MuxRequest::KillWindow {
            session,
            window_index,
        } => handle_kill_window(&session, window_index),
        MuxRequest::RenameWindow {
            session,
            window_index,
            new_name,
        } => handle_rename_window(&session, window_index, &new_name),
        MuxRequest::Resize { target, cols, rows } => handle_resize(&target, cols, rows),
        MuxRequest::SendInput { target, data } => handle_send_input(&target, &data),
        MuxRequest::Capture { target, kind } => {
            let content = handle_capture(&target, &kind)?;
            Ok(MuxResponse::Text { text: content })
        }
        MuxRequest::PaneSize { target } => handle_pane_size(&target),
        MuxRequest::CursorPosition { target } => handle_cursor_position(&target),
        MuxRequest::PaneCurrentCommand { target } => handle_pane_current_command(&target),
        MuxRequest::Tail { target, lines } => {
            let content = handle_tail(&target, lines)?;
            Ok(MuxResponse::Text { text: content })
        }
        MuxRequest::ListPanePids { session } => handle_list_pids(&session),
    }
}

fn handle_ping() -> Result<MuxResponse> {
    Ok(MuxResponse::Pong {
        version: super::version()?,
    })
}

fn handle_list_sessions() -> MuxResponse {
    let sessions = super::server::SessionManager::list()
        .into_iter()
        .map(|s| SessionInfo {
            name: s.name,
            created: s.created,
            attached: s.attached,
        })
        .collect();
    MuxResponse::Sessions { sessions }
}

fn handle_session_exists(name: &str) -> MuxResponse {
    MuxResponse::Bool {
        value: super::server::SessionManager::exists(name),
    }
}

fn handle_create_session(name: &str, working_dir: &str, command: &[String]) -> Result<MuxResponse> {
    let dir = Path::new(working_dir);
    let command = if command.is_empty() {
        None
    } else {
        Some(command)
    };
    super::server::SessionManager::create(name, dir, command)?;
    Ok(MuxResponse::Ok)
}

fn handle_kill_session(name: &str) -> Result<MuxResponse> {
    super::server::SessionManager::kill(name)?;
    Ok(MuxResponse::Ok)
}

fn handle_rename_session(old_name: &str, new_name: &str) -> Result<MuxResponse> {
    super::server::SessionManager::rename(old_name, new_name)?;
    Ok(MuxResponse::Ok)
}

fn handle_list_windows(session: &str) -> Result<MuxResponse> {
    let windows = super::server::SessionManager::list_windows(session)?
        .into_iter()
        .map(|w| WindowInfo {
            index: w.index,
            name: w.name,
        })
        .collect();
    Ok(MuxResponse::Windows { windows })
}

fn handle_create_window(
    session: &str,
    window_name: &str,
    working_dir: &str,
    command: &[String],
) -> Result<MuxResponse> {
    let dir = Path::new(working_dir);
    let command = if command.is_empty() {
        None
    } else {
        Some(command)
    };
    let index = super::server::SessionManager::create_window(session, window_name, dir, command)?;
    Ok(MuxResponse::WindowCreated { index })
}

fn handle_kill_window(session: &str, window_index: u32) -> Result<MuxResponse> {
    super::server::SessionManager::kill_window(session, window_index)?;
    Ok(MuxResponse::Ok)
}

fn handle_rename_window(session: &str, window_index: u32, new_name: &str) -> Result<MuxResponse> {
    super::server::SessionManager::rename_window(session, window_index, new_name)?;
    Ok(MuxResponse::Ok)
}

fn handle_resize(target: &str, cols: u16, rows: u16) -> Result<MuxResponse> {
    super::server::SessionManager::resize_window(target, cols, rows)?;
    Ok(MuxResponse::Ok)
}

fn handle_send_input(target: &str, data: &[u8]) -> Result<MuxResponse> {
    super::server::SessionManager::send_input(target, data)?;
    Ok(MuxResponse::Ok)
}

fn handle_capture(target: &str, kind: &CaptureKind) -> Result<String> {
    match kind {
        CaptureKind::Visible => super::server::OutputCapture::capture_pane(target),
        CaptureKind::History { lines } => {
            super::server::OutputCapture::capture_pane_with_history(target, *lines)
        }
        CaptureKind::FullHistory => super::server::OutputCapture::capture_full_history(target),
    }
}

fn handle_pane_size(target: &str) -> Result<MuxResponse> {
    let (cols, rows) = super::server::OutputCapture::pane_size(target)?;
    Ok(MuxResponse::Size { cols, rows })
}

fn handle_cursor_position(target: &str) -> Result<MuxResponse> {
    let (x, y, hidden) = super::server::OutputCapture::cursor_position(target)?;
    Ok(MuxResponse::Position { x, y, hidden })
}

fn handle_pane_current_command(target: &str) -> Result<MuxResponse> {
    let cmd = super::server::OutputCapture::pane_current_command(target)?;
    Ok(MuxResponse::Text { text: cmd })
}

fn handle_tail(target: &str, lines: u32) -> Result<String> {
    let lines = usize::try_from(lines).map_or(usize::MAX, |value| value);
    Ok(super::server::OutputCapture::tail(target, lines)?.join("\n"))
}

fn handle_list_pids(session: &str) -> Result<MuxResponse> {
    let pids = super::server::SessionManager::list_pane_pids(session)?;
    Ok(MuxResponse::Pids { pids })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_working_dir() -> String {
        std::env::temp_dir().to_string_lossy().into_owned()
    }

    fn unique_session(prefix: &str) -> String {
        format!("{prefix}-{}", uuid::Uuid::new_v4())
    }

    fn long_running_command() -> Vec<String> {
        vec!["sh".to_string(), "-c".to_string(), "sleep 10".to_string()]
    }

    #[test]
    fn test_dispatch_ping() -> Result<(), Box<dyn std::error::Error>> {
        let response = dispatch_request(MuxRequest::Ping)?;
        match response {
            MuxResponse::Pong { version } => {
                assert!(version.starts_with("tenex-mux/"));
                Ok(())
            }
            _ => Err("Expected Pong response".into()),
        }
    }

    #[test]
    fn test_dispatch_session_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
        let session = unique_session("tenex-test-daemon");
        let working_dir = temp_working_dir();

        let _ = super::super::server::SessionManager::kill(&session);

        let response = dispatch_request(MuxRequest::CreateSession {
            name: session.clone(),
            working_dir,
            command: long_running_command(),
            cols: 80,
            rows: 24,
        })?;
        assert!(matches!(response, MuxResponse::Ok));

        let response = dispatch_request(MuxRequest::SessionExists {
            name: session.clone(),
        })?;
        assert!(matches!(response, MuxResponse::Bool { value: true }));

        let response = dispatch_request(MuxRequest::ListSessions)?;
        let MuxResponse::Sessions { sessions } = response else {
            return Err("Expected Sessions response".into());
        };
        assert!(sessions.iter().any(|s| s.name == session));

        let renamed = format!("{session}-renamed");
        let response = dispatch_request(MuxRequest::RenameSession {
            old_name: session,
            new_name: renamed.clone(),
        })?;
        assert!(matches!(response, MuxResponse::Ok));

        let response = dispatch_request(MuxRequest::SessionExists {
            name: renamed.clone(),
        })?;
        assert!(matches!(response, MuxResponse::Bool { value: true }));

        let response = dispatch_request(MuxRequest::KillSession { name: renamed })?;
        assert!(matches!(response, MuxResponse::Ok));

        Ok(())
    }

    #[test]
    fn test_dispatch_window_capture_and_introspection() -> Result<(), Box<dyn std::error::Error>> {
        let session = unique_session("tenex-test-daemon-win");
        let working_dir = temp_working_dir();
        let command = long_running_command();

        let _ = super::super::server::SessionManager::kill(&session);

        let response = dispatch_request(MuxRequest::CreateSession {
            name: session.clone(),
            working_dir: working_dir.clone(),
            command: command.clone(),
            cols: 80,
            rows: 24,
        })?;
        assert!(matches!(response, MuxResponse::Ok));

        let response = dispatch_request(MuxRequest::ListWindows {
            session: session.clone(),
        })?;
        let MuxResponse::Windows { windows } = response else {
            return Err("Expected Windows response".into());
        };
        assert!(windows.iter().any(|w| w.index == 0));

        let response = dispatch_request(MuxRequest::CreateWindow {
            session: session.clone(),
            window_name: "child".to_string(),
            working_dir,
            command,
            cols: 80,
            rows: 24,
        })?;
        let MuxResponse::WindowCreated {
            index: window_index,
        } = response
        else {
            return Err("Expected WindowCreated response".into());
        };

        let response = dispatch_request(MuxRequest::RenameWindow {
            session: session.clone(),
            window_index,
            new_name: "renamed".to_string(),
        })?;
        assert!(matches!(response, MuxResponse::Ok));

        let target = format!("{session}:{window_index}");
        let response = dispatch_request(MuxRequest::Resize {
            target: target.clone(),
            cols: 100,
            rows: 40,
        })?;
        assert!(matches!(response, MuxResponse::Ok));

        let response = dispatch_request(MuxRequest::PaneSize {
            target: target.clone(),
        })?;
        let MuxResponse::Size { cols, rows } = response else {
            return Err("Expected Size response".into());
        };
        assert_eq!(cols, 100);
        assert_eq!(rows, 40);

        let response = dispatch_request(MuxRequest::CursorPosition {
            target: target.clone(),
        })?;
        assert!(matches!(response, MuxResponse::Position { .. }));

        let response = dispatch_request(MuxRequest::PaneCurrentCommand {
            target: target.clone(),
        })?;
        let MuxResponse::Text { text } = response else {
            return Err("Expected Text response".into());
        };
        assert!(!text.is_empty());

        let response = dispatch_request(MuxRequest::Tail {
            target: target.clone(),
            lines: 5,
        })?;
        assert!(matches!(response, MuxResponse::Text { .. }));

        for kind in [
            CaptureKind::Visible,
            CaptureKind::History { lines: 50 },
            CaptureKind::FullHistory,
        ] {
            let response = dispatch_request(MuxRequest::Capture {
                target: target.clone(),
                kind,
            })?;
            assert!(matches!(response, MuxResponse::Text { .. }));
        }

        let response = dispatch_request(MuxRequest::ListPanePids {
            session: session.clone(),
        })?;
        assert!(matches!(response, MuxResponse::Pids { .. }));

        let response = dispatch_request(MuxRequest::KillWindow {
            session: session.clone(),
            window_index,
        })?;
        assert!(matches!(response, MuxResponse::Ok));

        let response = dispatch_request(MuxRequest::KillSession { name: session })?;
        assert!(matches!(response, MuxResponse::Ok));

        Ok(())
    }

    #[test]
    fn test_dispatch_duplicate_session_errors() {
        let session = unique_session("tenex-test-duplicate");
        let working_dir = std::env::temp_dir().to_string_lossy().into_owned();
        let command = long_running_command();

        let _ = super::super::server::SessionManager::kill(&session);

        let first = dispatch_request(MuxRequest::CreateSession {
            name: session.clone(),
            working_dir: working_dir.clone(),
            command: command.clone(),
            cols: 80,
            rows: 24,
        });
        assert!(first.is_ok());

        let second = dispatch_request(MuxRequest::CreateSession {
            name: session.clone(),
            working_dir,
            command,
            cols: 80,
            rows: 24,
        });
        assert!(second.is_err());

        let _ = super::super::server::SessionManager::kill(&session);
    }
}
