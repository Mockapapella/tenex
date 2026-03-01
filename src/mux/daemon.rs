//! Background mux daemon.

use super::endpoint::SocketEndpoint;
use super::ipc;
use super::protocol::{CaptureKind, MuxRequest, MuxResponse, SessionInfo, WindowInfo};
use anyhow::{Context, Result};
use base64::Engine as _;
use interprocess::local_socket::traits::{ListenerExt, Stream as StreamTrait};
use interprocess::local_socket::{ListenerOptions, Stream};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::OnceLock;
use tracing::{debug, info, warn};

static RESIZE_MAX: OnceLock<Mutex<HashMap<String, (u16, u16)>>> = OnceLock::new();

fn resize_max() -> &'static Mutex<HashMap<String, (u16, u16)>> {
    RESIZE_MAX.get_or_init(|| Mutex::new(HashMap::new()))
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

    #[cfg(not(target_os = "linux"))]
    let _pid_guard =
        super::pidfile::PidFileGuard::create(&endpoint.display).with_context(|| {
            format!(
                "Failed to create mux daemon pid file for socket {}",
                endpoint.display
            )
        })?;

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

fn try_ping_existing(endpoint: &SocketEndpoint) -> Result<bool> {
    match Stream::connect(endpoint.name.clone()) {
        Ok(mut stream) => {
            ipc::write_json(&mut stream, &MuxRequest::Ping)?;
            Ok(ipc::read_json::<_, MuxResponse>(&mut stream).is_ok())
        }
        Err(_) => Ok(false),
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
        MuxRequest::ReadOutput {
            target,
            after,
            max_bytes,
        } => handle_read_output(&target, after, max_bytes),
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
    let prefix = format!("{name}:");
    resize_max()
        .lock()
        .retain(|target, _| target != name && !target.starts_with(&prefix));
    Ok(MuxResponse::Ok)
}

fn handle_rename_session(old_name: &str, new_name: &str) -> Result<MuxResponse> {
    super::server::SessionManager::rename(old_name, new_name)?;
    {
        let mut guard = resize_max().lock();
        if let Some(value) = guard.remove(old_name) {
            guard.insert(new_name.to_string(), value);
        }

        let old_prefix = format!("{old_name}:");
        let new_prefix = format!("{new_name}:");
        let updates: Vec<(String, String, (u16, u16))> = guard
            .iter()
            .filter_map(|(target, dims)| {
                target
                    .strip_prefix(&old_prefix)
                    .map(|suffix| (target.clone(), format!("{new_prefix}{suffix}"), *dims))
            })
            .collect();
        for (old_target, _, _) in &updates {
            guard.remove(old_target);
        }
        for (_, new_target, dims) in updates {
            guard.insert(new_target, dims);
        }
    }
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
    resize_max()
        .lock()
        .remove(&format!("{session}:{window_index}"));
    Ok(MuxResponse::Ok)
}

fn handle_rename_window(session: &str, window_index: u32, new_name: &str) -> Result<MuxResponse> {
    super::server::SessionManager::rename_window(session, window_index, new_name)?;
    Ok(MuxResponse::Ok)
}

fn handle_resize(target: &str, cols: u16, rows: u16) -> Result<MuxResponse> {
    let current = resize_max().lock().get(target).copied().unwrap_or((0, 0));
    let proposed = (current.0.max(cols), current.1.max(rows));

    super::server::SessionManager::resize_window(target, proposed.0, proposed.1)?;
    resize_max().lock().insert(target.to_string(), proposed);
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

enum ReadResult {
    Chunk { start: u64, data: Vec<u8> },
    Reset { start: u64, checkpoint: Vec<u8> },
}

fn handle_read_output(target: &str, after: u64, max_bytes: u32) -> Result<MuxResponse> {
    use base64::engine::general_purpose::STANDARD as BASE64;

    let max_bytes = usize::try_from(max_bytes).unwrap_or(usize::MAX);
    let window = super::backend::resolve_window(target)?;

    let result = {
        let guard = window.lock();

        if after < guard.output_history.seq_start {
            let checkpoint = guard
                .output_history
                .checkpoint
                .as_ref()
                .filter(|checkpoint| checkpoint.seq == guard.output_history.seq_start)
                .map(|checkpoint| checkpoint.bytes.clone())
                .unwrap_or_default();

            ReadResult::Reset {
                start: guard.output_history.seq_start,
                checkpoint,
            }
        } else if after >= guard.output_history.seq_end {
            ReadResult::Chunk {
                start: after,
                data: Vec::new(),
            }
        } else {
            let offset =
                usize::try_from(after.saturating_sub(guard.output_history.seq_start)).unwrap_or(0);
            let take = guard
                .output_history
                .buf
                .len()
                .saturating_sub(offset)
                .min(max_bytes);
            let end_offset = offset.saturating_add(take);
            ReadResult::Chunk {
                start: after,
                data: guard
                    .output_history
                    .buf
                    .get(offset..end_offset)
                    .unwrap_or_default()
                    .to_vec(),
            }
        }
    };

    match result {
        ReadResult::Chunk { start, data } => {
            let end = start.saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
            Ok(MuxResponse::OutputChunk {
                start,
                end,
                data_b64: if data.is_empty() {
                    String::new()
                } else {
                    BASE64.encode(data)
                },
            })
        }
        ReadResult::Reset { start, checkpoint } => Ok(MuxResponse::OutputReset {
            start,
            checkpoint_b64: if checkpoint.is_empty() {
                String::new()
            } else {
                BASE64.encode(checkpoint)
            },
        }),
    }
}

fn handle_list_pids(session: &str) -> Result<MuxResponse> {
    let pids = super::server::SessionManager::list_pane_pids(session)?;
    Ok(MuxResponse::Pids { pids })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dispatch(request: MuxRequest) -> Result<MuxResponse> {
        dispatch_request(request)
    }

    fn temp_working_dir() -> String {
        std::env::temp_dir().to_string_lossy().into_owned()
    }

    fn unique_session(prefix: &str) -> String {
        format!("{prefix}-{}", uuid::Uuid::new_v4())
    }

    fn unique_endpoint(prefix: &str) -> Result<SocketEndpoint> {
        super::super::endpoint::socket_endpoint_from_value(&format!(
            "{prefix}-{}",
            uuid::Uuid::new_v4()
        ))
    }

    fn long_running_command() -> Vec<String> {
        #[cfg(windows)]
        {
            vec![
                "powershell".to_string(),
                "-NoProfile".to_string(),
                "-Command".to_string(),
                "Start-Sleep -Seconds 10".to_string(),
            ]
        }
        #[cfg(not(windows))]
        {
            vec!["sh".to_string(), "-c".to_string(), "sleep 10".to_string()]
        }
    }

    fn echo_then_sleep_command(message: &str) -> Vec<String> {
        #[cfg(windows)]
        {
            vec![
                "powershell".to_string(),
                "-NoProfile".to_string(),
                "-Command".to_string(),
                format!("Write-Output '{message}'; Start-Sleep -Seconds 10"),
            ]
        }
        #[cfg(not(windows))]
        {
            vec![
                "sh".to_string(),
                "-c".to_string(),
                format!("echo '{message}'; sleep 10"),
            ]
        }
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
    fn test_try_ping_existing_true_and_false() -> Result<(), Box<dyn std::error::Error>> {
        let endpoint = unique_endpoint("tenex-test-daemon-ping")?;
        let listener = ListenerOptions::new()
            .name(endpoint.name.clone())
            .create_sync()?;

        let server = std::thread::spawn(move || -> Result<()> {
            let mut incoming = listener.incoming();
            let mut stream = match incoming.next() {
                Some(conn) => conn?,
                None => return Err(anyhow::anyhow!("Expected incoming mux ping request")),
            };
            let request: MuxRequest = ipc::read_json(&mut stream)?;
            assert!(matches!(request, MuxRequest::Ping));
            ipc::write_json(
                &mut stream,
                &MuxResponse::Pong {
                    version: "test-version".to_string(),
                },
            )?;
            Ok(())
        });

        assert!(try_ping_existing(&endpoint)?);
        match server.join() {
            Ok(result) => result?,
            Err(_) => return Err("Ping server thread panicked".into()),
        }

        if let Some(path) = endpoint.cleanup_path.as_ref() {
            let _ = std::fs::remove_file(path);
        }

        let missing_endpoint = unique_endpoint("tenex-test-daemon-ping-missing")?;
        assert!(!try_ping_existing(&missing_endpoint)?);
        if let Some(path) = missing_endpoint.cleanup_path.as_ref() {
            let _ = std::fs::remove_file(path);
        }

        Ok(())
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
        let response = dispatch(MuxRequest::Resize {
            target: target.clone(),
            cols: 100,
            rows: 40,
        })?;
        assert!(matches!(response, MuxResponse::Ok));

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
    fn test_dispatch_empty_commands_send_input_and_empty_output_paths()
    -> Result<(), Box<dyn std::error::Error>> {
        let session = unique_session("tenex-test-daemon-empty");
        let working_dir = temp_working_dir();

        let _ = super::super::server::SessionManager::kill(&session);

        let response = dispatch(MuxRequest::CreateSession {
            name: session.clone(),
            working_dir: working_dir.clone(),
            command: Vec::new(),
            cols: 80,
            rows: 24,
        })?;
        assert!(matches!(response, MuxResponse::Ok));

        let response = dispatch(MuxRequest::CreateWindow {
            session: session.clone(),
            window_name: "empty-command-window".to_string(),
            working_dir,
            command: Vec::new(),
            cols: 80,
            rows: 24,
        })?;
        let MuxResponse::WindowCreated {
            index: window_index,
        } = response
        else {
            return Err("Expected WindowCreated response".into());
        };

        let target = format!("{session}:{window_index}");
        let response = dispatch(MuxRequest::SendInput {
            target: target.clone(),
            data: b"echo tenex-send-input\n".to_vec(),
        })?;
        assert!(matches!(response, MuxResponse::Ok));

        let window = super::super::backend::resolve_window(&target)?;
        {
            let mut guard = window.lock();
            guard.output_history.seq_start = 10;
            guard.output_history.seq_end = 10;
            guard.output_history.buf.clear();
            guard.output_history.checkpoint = None;
        }

        let response = dispatch(MuxRequest::ReadOutput {
            target: target.clone(),
            after: 0,
            max_bytes: 4096,
        })?;
        let MuxResponse::OutputReset {
            start,
            checkpoint_b64,
        } = response
        else {
            return Err("Expected OutputReset response".into());
        };
        assert_eq!(start, 10);
        assert!(checkpoint_b64.is_empty());

        let response = dispatch(MuxRequest::ReadOutput {
            target,
            after: 10,
            max_bytes: 4096,
        })?;
        let MuxResponse::OutputChunk {
            start,
            end,
            data_b64,
        } = response
        else {
            return Err("Expected OutputChunk response".into());
        };
        assert_eq!(start, 10);
        assert_eq!(end, 10);
        assert!(data_b64.is_empty());

        let response = dispatch(MuxRequest::KillSession { name: session })?;
        assert!(matches!(response, MuxResponse::Ok));
        Ok(())
    }

    #[test]
    fn test_resize_is_monotonic_max_per_target() -> Result<(), Box<dyn std::error::Error>> {
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

        let response = dispatch(MuxRequest::Resize {
            target: session.clone(),
            cols: 80,
            rows: 24,
        })?;
        assert!(matches!(response, MuxResponse::Ok));

        let response = dispatch(MuxRequest::Resize {
            target: session.clone(),
            cols: 120,
            rows: 40,
        })?;
        assert!(matches!(response, MuxResponse::Ok));

        let response = dispatch(MuxRequest::PaneSize {
            target: session.clone(),
        })?;
        let MuxResponse::Size { cols, rows } = response else {
            return Err("Expected Size response".into());
        };
        assert_eq!((cols, rows), (120, 40));

        let response = dispatch(MuxRequest::Resize {
            target: session.clone(),
            cols: 80,
            rows: 24,
        })?;
        assert!(matches!(response, MuxResponse::Ok));

        let response = dispatch(MuxRequest::PaneSize {
            target: session.clone(),
        })?;
        let MuxResponse::Size { cols, rows } = response else {
            return Err("Expected Size response".into());
        };
        assert_eq!((cols, rows), (120, 40));

        let response = dispatch(MuxRequest::KillSession { name: session })?;
        assert!(matches!(response, MuxResponse::Ok));
        Ok(())
    }

    #[test]
    fn test_dispatch_read_output_returns_chunks() -> Result<(), Box<dyn std::error::Error>> {
        use base64::Engine as _;
        use base64::engine::general_purpose::STANDARD as BASE64;
        use std::time::Duration;

        let session = unique_session("tenex-test-daemon-read-output");
        let working_dir = temp_working_dir();
        let marker = "tenex-output-marker";

        let _ = super::super::server::SessionManager::kill(&session);

        let response = dispatch(MuxRequest::CreateSession {
            name: session.clone(),
            working_dir,
            command: echo_then_sleep_command(marker),
            cols: 80,
            rows: 24,
        })?;
        assert!(matches!(response, MuxResponse::Ok));

        let mut after = 0_u64;
        let mut collected: Vec<u8> = Vec::new();

        for _ in 0..50 {
            let response = dispatch(MuxRequest::ReadOutput {
                target: session.clone(),
                after,
                max_bytes: 4096,
            })?;

            let MuxResponse::OutputChunk {
                start,
                end,
                data_b64,
            } = response
            else {
                return Err("Expected OutputChunk response".into());
            };
            assert_eq!(start, after);
            after = end;

            if !data_b64.is_empty() {
                let data = BASE64.decode(data_b64.as_bytes())?;
                collected.extend_from_slice(&data);
            }

            if collected
                .windows(marker.len())
                .any(|window| window == marker.as_bytes())
            {
                break;
            }

            std::thread::sleep(Duration::from_millis(20));
        }

        assert!(
            collected
                .windows(marker.len())
                .any(|window| window == marker.as_bytes())
        );

        let response = dispatch(MuxRequest::KillSession { name: session })?;
        assert!(matches!(response, MuxResponse::Ok));
        Ok(())
    }

    #[test]
    fn test_dispatch_read_output_resets_when_after_is_stale()
    -> Result<(), Box<dyn std::error::Error>> {
        use base64::Engine as _;
        use base64::engine::general_purpose::STANDARD as BASE64;

        let session = unique_session("tenex-test-daemon-read-output-reset");
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

        let window = super::super::backend::resolve_window(&session)?;
        {
            let mut guard = window.lock();
            guard.output_history.seq_start = 5;
            guard.output_history.seq_end = 5;
            guard.output_history.buf.clear();
            guard.output_history.checkpoint = Some(super::super::backend::OutputCheckpoint {
                seq: 5,
                bytes: b"checkpoint".to_vec(),
            });
        }

        let response = dispatch(MuxRequest::ReadOutput {
            target: session.clone(),
            after: 0,
            max_bytes: 4096,
        })?;
        let MuxResponse::OutputReset {
            start,
            checkpoint_b64,
        } = response
        else {
            return Err("Expected OutputReset response".into());
        };
        assert_eq!(start, 5);
        assert_eq!(
            BASE64.decode(checkpoint_b64.as_bytes())?,
            b"checkpoint".to_vec()
        );

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
