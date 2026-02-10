//! Background mux daemon.

use super::endpoint::SocketEndpoint;
use super::ipc;
use super::protocol::{CaptureKind, MuxRequest, MuxResponse, SessionInfo, WindowInfo};
use anyhow::{Context, Result};
use interprocess::local_socket::traits::{ListenerExt, Stream as StreamTrait};
use interprocess::local_socket::{ListenerOptions, Stream};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{debug, info, warn};

static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);

static RESIZE_STATE: OnceLock<Mutex<ResizeState>> = OnceLock::new();

fn resize_state() -> &'static Mutex<ResizeState> {
    RESIZE_STATE.get_or_init(|| Mutex::new(ResizeState::default()))
}

#[derive(Debug, Default)]
struct ResizeState {
    per_target: HashMap<String, HashMap<u64, (u16, u16)>>,
    per_connection: HashMap<u64, HashSet<String>>,
}

impl ResizeState {
    fn record_resize(
        &mut self,
        connection_id: u64,
        target: &str,
        cols: u16,
        rows: u16,
    ) -> (u16, u16) {
        let entry = self.per_target.entry(target.to_string()).or_default();
        entry.insert(connection_id, (cols, rows));

        self.per_connection
            .entry(connection_id)
            .or_default()
            .insert(target.to_string());

        effective_size(entry)
    }

    fn remove_resize(&mut self, connection_id: u64, target: &str) {
        if let Some(targets) = self.per_connection.get_mut(&connection_id) {
            targets.remove(target);
            if targets.is_empty() {
                self.per_connection.remove(&connection_id);
            }
        }

        if let Some(entry) = self.per_target.get_mut(target) {
            entry.remove(&connection_id);
            if entry.is_empty() {
                self.per_target.remove(target);
            }
        }
    }

    fn remove_connection(&mut self, connection_id: u64) -> Vec<(String, (u16, u16))> {
        let Some(targets) = self.per_connection.remove(&connection_id) else {
            return Vec::new();
        };

        let mut updates = Vec::with_capacity(targets.len());
        for target in targets {
            if let Some(entry) = self.per_target.get_mut(&target) {
                entry.remove(&connection_id);
                if entry.is_empty() {
                    self.per_target.remove(&target);
                    continue;
                }

                updates.push((target, effective_size(entry)));
            }
        }

        updates
    }
}

fn effective_size(entry: &HashMap<u64, (u16, u16)>) -> (u16, u16) {
    entry
        .values()
        .fold((0, 0), |(max_cols, max_rows), (cols, rows)| {
            (max_cols.max(*cols), max_rows.max(*rows))
        })
}

struct ConnectionResizeGuard {
    connection_id: u64,
}

impl ConnectionResizeGuard {
    const fn new(connection_id: u64) -> Self {
        Self { connection_id }
    }
}

impl Drop for ConnectionResizeGuard {
    fn drop(&mut self) {
        let updates = {
            let mut guard = resize_state().lock();
            guard.remove_connection(self.connection_id)
        };

        for (target, (cols, rows)) in updates {
            let _ = super::server::SessionManager::resize_window(&target, cols, rows);
        }
    }
}

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
            if try_ping_existing(endpoint)? {
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
                let connection_id = NEXT_CONNECTION_ID.fetch_add(1, Ordering::Relaxed);
                std::thread::spawn(move || {
                    if let Err(err) = handle_connection(stream, connection_id) {
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

fn try_ping_existing(endpoint: &SocketEndpoint) -> Result<bool> {
    match Stream::connect(endpoint.name.clone()) {
        Ok(mut stream) => {
            ipc::write_json(&mut stream, &MuxRequest::Ping)?;
            Ok(ipc::read_json::<_, MuxResponse>(&mut stream).is_ok())
        }
        Err(_) => Ok(false),
    }
}

fn handle_connection(mut stream: Stream, connection_id: u64) -> Result<()> {
    let _resize_guard = ConnectionResizeGuard::new(connection_id);
    loop {
        let request: MuxRequest = match ipc::read_json(&mut stream) {
            Ok(req) => req,
            Err(err) => {
                return Err(err);
            }
        };

        let response = match dispatch_request(request, connection_id) {
            Ok(response) => response,
            Err(err) => MuxResponse::Err {
                message: err.to_string(),
            },
        };

        ipc::write_json(&mut stream, &response)?;
    }
}

fn dispatch_request(request: MuxRequest, connection_id: u64) -> Result<MuxResponse> {
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
        MuxRequest::Resize { target, cols, rows } => {
            handle_resize(connection_id, &target, cols, rows)
        }
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

fn handle_resize(connection_id: u64, target: &str, cols: u16, rows: u16) -> Result<MuxResponse> {
    let effective = {
        let mut guard = resize_state().lock();
        guard.record_resize(connection_id, target, cols, rows)
    };

    match super::server::SessionManager::resize_window(target, effective.0, effective.1) {
        Ok(()) => Ok(MuxResponse::Ok),
        Err(err) => {
            resize_state().lock().remove_resize(connection_id, target);
            Err(err)
        }
    }
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

    fn dispatch(request: MuxRequest) -> Result<MuxResponse> {
        dispatch_request(request, 0)
    }

    fn dispatch_for(connection_id: u64, request: MuxRequest) -> Result<MuxResponse> {
        dispatch_request(request, connection_id)
    }

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
        let response = dispatch(MuxRequest::Ping)?;
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

        let response = dispatch(MuxRequest::CreateSession {
            name: session.clone(),
            working_dir,
            command: long_running_command(),
            cols: 80,
            rows: 24,
        })?;
        assert!(matches!(response, MuxResponse::Ok));

        let response = dispatch(MuxRequest::SessionExists {
            name: session.clone(),
        })?;
        assert!(matches!(response, MuxResponse::Bool { value: true }));

        let response = dispatch(MuxRequest::ListSessions)?;
        let MuxResponse::Sessions { sessions } = response else {
            return Err("Expected Sessions response".into());
        };
        assert!(sessions.iter().any(|s| s.name == session));

        let renamed = format!("{session}-renamed");
        let response = dispatch(MuxRequest::RenameSession {
            old_name: session,
            new_name: renamed.clone(),
        })?;
        assert!(matches!(response, MuxResponse::Ok));

        let response = dispatch(MuxRequest::SessionExists {
            name: renamed.clone(),
        })?;
        assert!(matches!(response, MuxResponse::Bool { value: true }));

        let response = dispatch(MuxRequest::KillSession { name: renamed })?;
        assert!(matches!(response, MuxResponse::Ok));

        Ok(())
    }

    #[test]
    fn test_dispatch_window_capture_and_introspection() -> Result<(), Box<dyn std::error::Error>> {
        let session = unique_session("tenex-test-daemon-win");
        let working_dir = temp_working_dir();
        let command = long_running_command();

        let _ = super::super::server::SessionManager::kill(&session);

        let response = dispatch(MuxRequest::CreateSession {
            name: session.clone(),
            working_dir: working_dir.clone(),
            command: command.clone(),
            cols: 80,
            rows: 24,
        })?;
        assert!(matches!(response, MuxResponse::Ok));

        let response = dispatch(MuxRequest::ListWindows {
            session: session.clone(),
        })?;
        let MuxResponse::Windows { windows } = response else {
            return Err("Expected Windows response".into());
        };
        assert!(windows.iter().any(|w| w.index == 0));

        let response = dispatch(MuxRequest::CreateWindow {
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

        let response = dispatch(MuxRequest::RenameWindow {
            session: session.clone(),
            window_index,
            new_name: "renamed".to_string(),
        })?;
        assert!(matches!(response, MuxResponse::Ok));

        let target = format!("{session}:{window_index}");
        {
            let connection_id = 1;
            let _resize_guard = ConnectionResizeGuard::new(connection_id);
            let response = dispatch_for(
                connection_id,
                MuxRequest::Resize {
                    target: target.clone(),
                    cols: 100,
                    rows: 40,
                },
            )?;
            assert!(matches!(response, MuxResponse::Ok));
        }

        let response = dispatch(MuxRequest::PaneSize {
            target: target.clone(),
        })?;
        let MuxResponse::Size { cols, rows } = response else {
            return Err("Expected Size response".into());
        };
        assert_eq!(cols, 100);
        assert_eq!(rows, 40);

        let response = dispatch(MuxRequest::CursorPosition {
            target: target.clone(),
        })?;
        assert!(matches!(response, MuxResponse::Position { .. }));

        let response = dispatch(MuxRequest::PaneCurrentCommand {
            target: target.clone(),
        })?;
        let MuxResponse::Text { text } = response else {
            return Err("Expected Text response".into());
        };
        assert!(!text.is_empty());

        let response = dispatch(MuxRequest::Tail {
            target: target.clone(),
            lines: 5,
        })?;
        assert!(matches!(response, MuxResponse::Text { .. }));

        for kind in [
            CaptureKind::Visible,
            CaptureKind::History { lines: 50 },
            CaptureKind::FullHistory,
        ] {
            let response = dispatch(MuxRequest::Capture {
                target: target.clone(),
                kind,
            })?;
            assert!(matches!(response, MuxResponse::Text { .. }));
        }

        let response = dispatch(MuxRequest::ListPanePids {
            session: session.clone(),
        })?;
        assert!(matches!(response, MuxResponse::Pids { .. }));

        let response = dispatch(MuxRequest::KillWindow {
            session: session.clone(),
            window_index,
        })?;
        assert!(matches!(response, MuxResponse::Ok));

        let response = dispatch(MuxRequest::KillSession { name: session })?;
        assert!(matches!(response, MuxResponse::Ok));

        Ok(())
    }

    #[test]
    fn test_resize_uses_largest_client_dimensions_and_recovers_on_disconnect()
    -> Result<(), Box<dyn std::error::Error>> {
        let session = unique_session("tenex-test-daemon-resize-multi");
        let working_dir = temp_working_dir();
        let command = long_running_command();

        let _ = super::super::server::SessionManager::kill(&session);

        let response = dispatch(MuxRequest::CreateSession {
            name: session.clone(),
            working_dir,
            command,
            cols: 80,
            rows: 24,
        })?;
        assert!(matches!(response, MuxResponse::Ok));

        let small_id = 10;
        let large_id = 20;
        let small_guard = ConnectionResizeGuard::new(small_id);
        let large_guard = ConnectionResizeGuard::new(large_id);

        let response = dispatch_for(
            small_id,
            MuxRequest::Resize {
                target: session.clone(),
                cols: 80,
                rows: 24,
            },
        )?;
        assert!(matches!(response, MuxResponse::Ok));

        let response = dispatch_for(
            large_id,
            MuxRequest::Resize {
                target: session.clone(),
                cols: 120,
                rows: 40,
            },
        )?;
        assert!(matches!(response, MuxResponse::Ok));

        let response = dispatch(MuxRequest::PaneSize {
            target: session.clone(),
        })?;
        let MuxResponse::Size { cols, rows } = response else {
            return Err("Expected Size response".into());
        };
        assert_eq!((cols, rows), (120, 40));

        drop(large_guard);

        let response = dispatch(MuxRequest::PaneSize {
            target: session.clone(),
        })?;
        let MuxResponse::Size { cols, rows } = response else {
            return Err("Expected Size response".into());
        };
        assert_eq!((cols, rows), (80, 24));

        drop(small_guard);

        let response = dispatch(MuxRequest::KillSession { name: session })?;
        assert!(matches!(response, MuxResponse::Ok));
        Ok(())
    }

    #[test]
    fn test_dispatch_duplicate_session_errors() {
        let session = unique_session("tenex-test-duplicate");
        let working_dir = std::env::temp_dir().to_string_lossy().into_owned();
        let command = long_running_command();

        let _ = super::super::server::SessionManager::kill(&session);

        let first = dispatch(MuxRequest::CreateSession {
            name: session.clone(),
            working_dir: working_dir.clone(),
            command: command.clone(),
            cols: 80,
            rows: 24,
        });
        assert!(first.is_ok());

        let second = dispatch(MuxRequest::CreateSession {
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
