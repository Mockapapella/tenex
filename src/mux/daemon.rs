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
    run_with_connection_limit(endpoint, None)
}

fn run_with_connection_limit(
    endpoint: &SocketEndpoint,
    connection_limit: Option<usize>,
) -> Result<()> {
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

    #[cfg(not(target_os = "linux"))]
    let _pid_guard =
        super::pidfile::PidFileGuard::create(&endpoint.display).with_context(|| {
            format!(
                "Failed to create mux daemon pid file for socket {}",
                endpoint.display
            )
        })?;

    info!(endpoint = %endpoint.display, "Mux daemon listening");

    serve_incoming(listener.incoming(), connection_limit);

    Ok(())
}

fn serve_incoming<I>(incoming: I, connection_limit: Option<usize>)
where
    I: IntoIterator<Item = io::Result<Stream>>,
{
    if connection_limit == Some(0) {
        return;
    }

    let limit = connection_limit.unwrap_or(usize::MAX);
    let mut accepted = 0usize;
    for conn in incoming {
        match conn {
            Ok(stream) => {
                accepted = accepted.saturating_add(1);
                std::thread::spawn(move || handle_connection_spawned(stream));

                if accepted >= limit {
                    break;
                }
            }
            Err(err) => {
                warn!(error = %err, "Mux accept failed");
            }
        }
    }
}

fn handle_connection_spawned(stream: Stream) {
    handle_connection(stream).unwrap_or_else(|err| {
        debug!(error = %err, "Mux client connection closed");
    });
}

fn try_ping_stream(stream: &mut (impl io::Read + io::Write)) -> bool {
    if ipc::write_json(stream, &MuxRequest::Ping).is_err() {
        return false;
    }

    ipc::read_json::<_, MuxResponse>(stream).is_ok()
}

fn try_ping_existing(endpoint: &SocketEndpoint) -> bool {
    let Ok(mut stream) = Stream::connect(endpoint.name.clone()) else {
        return false;
    };

    try_ping_stream(&mut stream)
}

fn handle_connection<RW: std::io::Read + std::io::Write>(mut stream: RW) -> Result<()> {
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
        MuxRequest::Ping => Ok(handle_ping()),
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
            let content = handle_capture(&target, kind)?;
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
        MuxRequest::OutputCursor { target } => handle_output_cursor(&target),
        MuxRequest::ListPanePids { session } => handle_list_pids(&session),
    }
}

fn handle_ping() -> MuxResponse {
    MuxResponse::Pong {
        version: super::version(),
    }
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

fn handle_capture(target: &str, kind: CaptureKind) -> Result<String> {
    match kind {
        CaptureKind::Visible => super::server::OutputCapture::capture_pane(target),
        CaptureKind::History { lines } => {
            super::server::OutputCapture::capture_pane_with_history(target, lines)
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

fn handle_output_cursor(target: &str) -> Result<MuxResponse> {
    let window = super::backend::resolve_window(target)?;
    let (start, end) = {
        let guard = window.lock();
        (guard.output_history.seq_start, guard.output_history.seq_end)
    };
    Ok(MuxResponse::OutputCursor { start, end })
}

fn handle_list_pids(session: &str) -> Result<MuxResponse> {
    let pids = super::server::SessionManager::list_pane_pids(session)?;
    Ok(MuxResponse::Pids { pids })
}

#[cfg(test)]
mod tests {
    use super::*;
    use interprocess::local_socket::{GenericFilePath, prelude::*};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tempfile::TempDir;

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

    fn unique_path_endpoint(temp: &TempDir, prefix: &str) -> SocketEndpoint {
        let socket_path = temp
            .path()
            .join(format!("{prefix}-{}.sock", uuid::Uuid::new_v4()));
        let value = socket_path.to_string_lossy().into_owned();
        super::super::endpoint::socket_endpoint_from_value(&value).unwrap()
    }

    fn connect_with_retry_attempts(
        name: &interprocess::local_socket::Name<'static>,
        attempts: usize,
        delay: std::time::Duration,
    ) -> io::Result<Stream> {
        let mut last_error: Option<io::Error> = None;
        for _ in 0..attempts {
            match Stream::connect(name.clone()) {
                Ok(stream) => return Ok(stream),
                Err(err) => {
                    last_error = Some(err);
                    std::thread::sleep(delay);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| io::Error::other("connect failed")))
    }

    fn connect_with_retry(name: &interprocess::local_socket::Name<'static>) -> io::Result<Stream> {
        connect_with_retry_attempts(name, 50, std::time::Duration::from_millis(10))
    }

    fn with_tracing_dispatch<T>(f: impl FnOnce() -> T) -> T {
        let subscriber = tracing_subscriber::fmt()
            .with_writer(std::io::sink)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        let dispatch = tracing::dispatcher::Dispatch::new(subscriber);
        tracing::dispatcher::with_default(&dispatch, f)
    }

    fn cleanup_endpoint(endpoint: &SocketEndpoint) {
        if let Some(path) = endpoint.cleanup_path.as_ref() {
            let _ = std::fs::remove_file(path);
        }
    }

    fn request_is_ping(request: &MuxRequest) -> bool {
        matches!(request, MuxRequest::Ping)
    }

    fn response_is_pong(response: &MuxResponse) -> bool {
        matches!(response, MuxResponse::Pong { .. })
    }

    fn response_is_err(response: &MuxResponse) -> bool {
        matches!(response, MuxResponse::Err { .. })
    }

    fn response_is_ok(response: &MuxResponse) -> bool {
        matches!(response, MuxResponse::Ok)
    }

    fn pong_version(response: &MuxResponse) -> Option<&str> {
        match response {
            MuxResponse::Pong { version } => Some(version),
            _ => None,
        }
    }

    fn response_bool_value(response: &MuxResponse) -> Option<bool> {
        match response {
            MuxResponse::Bool { value } => Some(*value),
            _ => None,
        }
    }

    fn response_is_position(response: &MuxResponse) -> bool {
        matches!(response, MuxResponse::Position { .. })
    }

    fn response_is_pids(response: &MuxResponse) -> bool {
        matches!(response, MuxResponse::Pids { .. })
    }

    struct OneShotIncoming {
        iterated: Arc<AtomicBool>,
    }

    struct OneShotIter {
        iterated: Arc<AtomicBool>,
    }

    impl Iterator for OneShotIter {
        type Item = io::Result<Stream>;

        fn next(&mut self) -> Option<Self::Item> {
            self.iterated.store(true, Ordering::SeqCst);
            None
        }
    }

    impl IntoIterator for OneShotIncoming {
        type Item = io::Result<Stream>;
        type IntoIter = OneShotIter;

        fn into_iter(self) -> Self::IntoIter {
            OneShotIter {
                iterated: self.iterated,
            }
        }
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
    fn test_dispatch_ping() {
        let response = dispatch(MuxRequest::Ping).unwrap();
        assert!(response_is_pong(&response));
        assert!(
            pong_version(&response)
                .expect("expected pong version")
                .starts_with("tenex-mux/")
        );
    }

    fn expect_sessions(response: MuxResponse) -> Result<Vec<SessionInfo>> {
        let MuxResponse::Sessions { sessions } = response else {
            return Err(anyhow::anyhow!("Expected Sessions response"));
        };
        Ok(sessions)
    }

    fn expect_windows(response: MuxResponse) -> Result<Vec<WindowInfo>> {
        let MuxResponse::Windows { windows } = response else {
            return Err(anyhow::anyhow!("Expected Windows response"));
        };
        Ok(windows)
    }

    fn expect_window_created(response: &MuxResponse) -> Result<u32> {
        let MuxResponse::WindowCreated { index } = response else {
            return Err(anyhow::anyhow!("Expected WindowCreated response"));
        };
        Ok(*index)
    }

    fn expect_output_reset(response: MuxResponse) -> Result<(u64, String)> {
        let MuxResponse::OutputReset {
            start,
            checkpoint_b64,
        } = response
        else {
            return Err(anyhow::anyhow!("Expected OutputReset response"));
        };
        Ok((start, checkpoint_b64))
    }

    fn expect_output_chunk(response: MuxResponse) -> Result<(u64, u64, String)> {
        let MuxResponse::OutputChunk {
            start,
            end,
            data_b64,
        } = response
        else {
            return Err(anyhow::anyhow!("Expected OutputChunk response"));
        };
        Ok((start, end, data_b64))
    }

    fn expect_output_cursor(response: &MuxResponse) -> Result<(u64, u64)> {
        let MuxResponse::OutputCursor { start, end } = response else {
            return Err(anyhow::anyhow!("Expected OutputCursor response"));
        };
        Ok((*start, *end))
    }

    fn expect_size(response: &MuxResponse) -> Result<(u16, u16)> {
        let MuxResponse::Size { cols, rows } = response else {
            return Err(anyhow::anyhow!("Expected Size response"));
        };
        Ok((*cols, *rows))
    }

    fn expect_text(response: MuxResponse) -> Result<String> {
        let MuxResponse::Text { text } = response else {
            return Err(anyhow::anyhow!("Expected Text response"));
        };
        Ok(text)
    }

    #[test]
    fn test_response_helpers_error_on_wrong_variant() {
        assert!(
            expect_sessions(MuxResponse::Ok)
                .unwrap_err()
                .to_string()
                .contains("Expected Sessions response")
        );
        assert!(
            expect_windows(MuxResponse::Ok)
                .unwrap_err()
                .to_string()
                .contains("Expected Windows response")
        );
        assert!(
            expect_window_created(&MuxResponse::Ok)
                .unwrap_err()
                .to_string()
                .contains("Expected WindowCreated response")
        );
        assert!(
            expect_output_reset(MuxResponse::Ok)
                .unwrap_err()
                .to_string()
                .contains("Expected OutputReset response")
        );
        assert!(
            expect_output_chunk(MuxResponse::Ok)
                .unwrap_err()
                .to_string()
                .contains("Expected OutputChunk response")
        );
        assert!(
            expect_output_cursor(&MuxResponse::Ok)
                .unwrap_err()
                .to_string()
                .contains("Expected OutputCursor response")
        );
        assert!(
            expect_size(&MuxResponse::Ok)
                .unwrap_err()
                .to_string()
                .contains("Expected Size response")
        );
        assert!(
            expect_text(MuxResponse::Ok)
                .unwrap_err()
                .to_string()
                .contains("Expected Text response")
        );
    }

    #[test]
    fn test_connect_with_retry_reports_failure() {
        let temp = TempDir::new().unwrap();
        let endpoint = unique_path_endpoint(&temp, "tenex-test-daemon-connect-missing");
        let err =
            connect_with_retry_attempts(&endpoint.name, 1, std::time::Duration::from_millis(0))
                .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn test_connect_with_retry_attempts_reports_failure_without_attempts() {
        let temp = TempDir::new().unwrap();
        let endpoint = unique_path_endpoint(&temp, "tenex-test-daemon-connect-missing-attempts-0");
        let err =
            connect_with_retry_attempts(&endpoint.name, 0, std::time::Duration::from_millis(0))
                .unwrap_err();
        assert!(err.to_string().contains("connect failed"));
    }

    #[test]
    fn test_test_helpers_cover_both_match_arms() {
        let ping = MuxRequest::Ping;
        assert!(request_is_ping(&ping));
        assert!(!request_is_ping(&MuxRequest::ListSessions));

        let pong_response = MuxResponse::Pong {
            version: "test".to_string(),
        };
        let ok = MuxResponse::Ok;
        let err = MuxResponse::Err {
            message: "nope".to_string(),
        };

        assert!(response_is_pong(&pong_response));
        assert!(!response_is_pong(&ok));
        assert!(response_is_err(&err));
        assert!(!response_is_err(&pong_response));
        assert!(response_is_ok(&ok));
        assert!(!response_is_ok(&err));
        assert!(response_is_position(&MuxResponse::Position {
            x: 0,
            y: 0,
            hidden: false,
        }));
        assert!(!response_is_position(&ok));
        assert!(response_is_pids(&MuxResponse::Pids { pids: Vec::new() }));
        assert!(!response_is_pids(&pong_response));

        assert_eq!(pong_version(&pong_response), Some("test"));
        assert_eq!(pong_version(&ok), None);
        assert_eq!(
            response_bool_value(&MuxResponse::Bool { value: true }),
            Some(true)
        );
        assert_eq!(response_bool_value(&pong_response), None);

        let temp = TempDir::new().unwrap();
        cleanup_endpoint(&unique_path_endpoint(&temp, "tenex-test-daemon-cleanup"));
        cleanup_endpoint(&unique_endpoint("tenex-test-daemon-cleanup-name").unwrap());
    }

    #[test]
    fn test_run_reports_error_when_socket_dir_creation_fails() {
        let temp = TempDir::new().unwrap();
        let blocked = temp.path().join("blocked");
        std::fs::write(&blocked, "not-a-dir").unwrap();
        let socket_path = blocked.join("socket.sock");
        let value = socket_path.to_string_lossy().into_owned();
        let endpoint = super::super::endpoint::socket_endpoint_from_value(&value).unwrap();
        let err = run_with_connection_limit(&endpoint, Some(0)).unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to create mux socket directory")
        );
    }

    #[test]
    fn test_run_reports_error_when_listener_create_fails() {
        let temp = TempDir::new().unwrap();
        let socket_path = temp.path().join("missing-parent").join("socket.sock");
        let name = socket_path
            .as_path()
            .to_fs_name::<GenericFilePath>()
            .unwrap()
            .into_owned();
        let endpoint = SocketEndpoint {
            name,
            cleanup_path: None,
            display: socket_path.to_string_lossy().into_owned(),
        };
        let err = run_with_connection_limit(&endpoint, Some(0)).unwrap_err();
        assert!(err.to_string().contains("Failed to create mux listener"));
    }

    #[test]
    fn test_serve_incoming_returns_early_when_limit_zero() {
        let iterated = Arc::new(AtomicBool::new(false));
        serve_incoming(
            OneShotIncoming {
                iterated: iterated.clone(),
            },
            Some(0),
        );
        assert!(!iterated.load(Ordering::SeqCst));
    }

    #[test]
    fn test_serve_incoming_iterates_when_limit_is_nonzero() {
        let iterated = Arc::new(AtomicBool::new(false));
        serve_incoming(
            OneShotIncoming {
                iterated: iterated.clone(),
            },
            Some(1),
        );
        assert!(iterated.load(Ordering::SeqCst));
    }

    #[test]
    fn test_serve_incoming_does_not_break_when_below_limit() {
        let temp = TempDir::new().unwrap();
        let endpoint = unique_path_endpoint(&temp, "tenex-test-serve-incoming-below-limit");
        let listener = ListenerOptions::new()
            .name(endpoint.name.clone())
            .create_sync()
            .unwrap();

        let client = connect_with_retry(&endpoint.name).expect("Expected mux client to connect");
        let server_stream = listener
            .incoming()
            .next()
            .expect("Expected incoming stream")
            .expect("Mux accept failed");
        drop(client);

        serve_incoming(std::iter::once(Ok(server_stream)), Some(2));
        cleanup_endpoint(&endpoint);
    }

    #[test]
    fn test_handle_connection_spawned_logs_on_error() {
        use std::io::Write as _;

        let subscriber = tracing_subscriber::fmt()
            .with_writer(std::io::sink)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        let trace_dispatch = tracing::dispatcher::Dispatch::new(subscriber);
        let temp = TempDir::new().unwrap();
        let endpoint = unique_path_endpoint(&temp, "tenex-test-daemon-connection-error");
        let listener = ListenerOptions::new()
            .name(endpoint.name.clone())
            .create_sync()
            .unwrap();

        let server = std::thread::spawn(move || {
            let mut incoming = listener.incoming();
            let stream = incoming
                .next()
                .expect("Expected client connection")
                .expect("Mux accept failed");
            tracing::dispatcher::with_default(&trace_dispatch, || {
                handle_connection_spawned(stream);
            });
        });

        let mut client =
            connect_with_retry(&endpoint.name).expect("Expected mux client to connect");
        let payload = b"not-json";
        client
            .write_all(
                &u32::try_from(payload.len())
                    .expect("payload length")
                    .to_le_bytes(),
            )
            .expect("write length");
        client.write_all(payload).expect("write payload");
        client.flush().expect("flush payload");
        drop(client);
        server.join().unwrap();
        cleanup_endpoint(&endpoint);
    }

    #[test]
    fn test_handle_connection_spawned_returns_when_client_closes_without_sending() {
        let subscriber = tracing_subscriber::fmt()
            .with_writer(std::io::sink)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        let trace_dispatch = tracing::dispatcher::Dispatch::new(subscriber);
        let temp = TempDir::new().unwrap();
        let endpoint = unique_path_endpoint(&temp, "tenex-test-daemon-connection-close");
        let listener = ListenerOptions::new()
            .name(endpoint.name.clone())
            .create_sync()
            .unwrap();

        let server = std::thread::spawn(move || {
            let mut incoming = listener.incoming();
            let stream = incoming
                .next()
                .expect("Expected client connection")
                .expect("Mux accept failed");
            tracing::dispatcher::with_default(&trace_dispatch, || {
                handle_connection_spawned(stream);
            });
        });

        let client = connect_with_retry(&endpoint.name).expect("Expected mux client to connect");
        drop(client);
        server.join().unwrap();
        cleanup_endpoint(&endpoint);
    }

    #[test]
    fn test_handle_connection_returns_error_when_response_write_fails() {
        struct WriteFailStream {
            read: std::io::Cursor<Vec<u8>>,
        }

        impl std::io::Read for WriteFailStream {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.read.read(buf)
            }
        }

        impl std::io::Write for WriteFailStream {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("write fail"))
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let mut payload = Vec::new();
        ipc::write_json(&mut payload, &MuxRequest::Ping).unwrap();

        let mut stream = WriteFailStream {
            read: std::io::Cursor::new(payload),
        };
        std::io::Write::flush(&mut stream).expect("flush");
        let err = handle_connection(stream).unwrap_err();
        assert!(err.to_string().contains("Failed to write message length"));
    }

    #[test]
    fn test_handle_connection_roundtrips_and_wraps_dispatch_errors() {
        let temp = TempDir::new().unwrap();
        let endpoint = unique_path_endpoint(&temp, "tenex-test-daemon-handle-conn");

        let listener = ListenerOptions::new()
            .name(endpoint.name.clone())
            .create_sync()
            .unwrap();

        let server = std::thread::spawn(move || {
            let mut incoming = listener.incoming();
            let stream = incoming
                .next()
                .expect("Expected client connection")
                .expect("Mux accept failed");
            handle_connection(stream)
        });

        let mut client =
            connect_with_retry(&endpoint.name).expect("Expected mux client to connect");
        ipc::write_json(&mut client, &MuxRequest::Ping).unwrap();
        let response: MuxResponse = ipc::read_json(&mut client).unwrap();
        assert!(response_is_pong(&response));

        ipc::write_json(
            &mut client,
            &MuxRequest::ListWindows {
                session: "definitely-missing".to_string(),
            },
        )
        .unwrap();
        let response: MuxResponse = ipc::read_json(&mut client).unwrap();
        assert!(response_is_err(&response));

        drop(client);
        let result = server.join().unwrap();
        assert!(result.is_err());

        cleanup_endpoint(&endpoint);
    }

    #[test]
    fn test_dispatch_errors_for_missing_sessions_and_targets() {
        let missing_session = unique_session("tenex-test-daemon-missing");

        assert!(
            dispatch(MuxRequest::KillSession {
                name: missing_session.clone(),
            })
            .is_err()
        );
        assert!(
            dispatch(MuxRequest::RenameSession {
                old_name: missing_session.clone(),
                new_name: format!("{missing_session}-renamed"),
            })
            .is_err()
        );
        assert!(
            dispatch(MuxRequest::ListPanePids {
                session: missing_session.clone(),
            })
            .is_err()
        );
        assert!(
            dispatch(MuxRequest::CreateWindow {
                session: missing_session.clone(),
                window_name: "child".to_string(),
                working_dir: temp_working_dir(),
                command: Vec::new(),
                cols: 80,
                rows: 24,
            })
            .is_err()
        );
        assert!(
            dispatch(MuxRequest::KillWindow {
                session: missing_session.clone(),
                window_index: 0,
            })
            .is_err()
        );
        assert!(
            dispatch(MuxRequest::RenameWindow {
                session: missing_session.clone(),
                window_index: 0,
                new_name: "renamed".to_string(),
            })
            .is_err()
        );

        let missing_target = format!("{missing_session}:0");
        assert!(
            dispatch(MuxRequest::Resize {
                target: missing_target.clone(),
                cols: 80,
                rows: 24,
            })
            .is_err()
        );
        assert!(
            dispatch(MuxRequest::SendInput {
                target: missing_target.clone(),
                data: vec![b'x'],
            })
            .is_err()
        );
        assert!(
            dispatch(MuxRequest::Capture {
                target: missing_target.clone(),
                kind: CaptureKind::Visible,
            })
            .is_err()
        );
        assert!(
            dispatch(MuxRequest::Tail {
                target: missing_target.clone(),
                lines: 1,
            })
            .is_err()
        );
        assert!(
            dispatch(MuxRequest::PaneSize {
                target: missing_target.clone(),
            })
            .is_err()
        );
        assert!(
            dispatch(MuxRequest::CursorPosition {
                target: missing_target.clone(),
            })
            .is_err()
        );
        assert!(
            dispatch(MuxRequest::OutputCursor {
                target: missing_target.clone(),
            })
            .is_err()
        );
        assert!(
            dispatch(MuxRequest::PaneCurrentCommand {
                target: missing_target.clone(),
            })
            .is_err()
        );
        assert!(
            dispatch(MuxRequest::ReadOutput {
                target: missing_target,
                after: 0,
                max_bytes: 1024,
            })
            .is_err()
        );
    }

    #[test]
    fn test_run_accepts_single_connection_then_returns() {
        let temp = TempDir::new().unwrap();
        let socket_path = temp.path().join("mux").join("socket.sock");
        let endpoint_value = socket_path.to_string_lossy().into_owned();
        let endpoint = super::super::endpoint::socket_endpoint_from_value(&endpoint_value).unwrap();

        let endpoint_for_thread = endpoint.clone();
        let server = std::thread::spawn(move || {
            with_tracing_dispatch(|| run_with_connection_limit(&endpoint_for_thread, Some(1)))
        });

        let mut client =
            connect_with_retry(&endpoint.name).expect("Expected mux client to connect");
        ipc::write_json(&mut client, &MuxRequest::Ping).unwrap();
        let response: MuxResponse = ipc::read_json(&mut client).unwrap();
        assert!(response_is_pong(&response));
        drop(client);

        server.join().unwrap().unwrap();
        cleanup_endpoint(&endpoint);
    }

    #[test]
    fn test_run_returns_ok_when_existing_daemon_responds_to_ping() {
        let temp = TempDir::new().unwrap();
        let endpoint = unique_path_endpoint(&temp, "tenex-test-daemon-already-running");
        let listener = ListenerOptions::new()
            .name(endpoint.name.clone())
            .create_sync()
            .unwrap();

        let server = std::thread::spawn(move || {
            let mut incoming = listener.incoming();
            let mut stream = incoming
                .next()
                .expect("Expected ping connection")
                .expect("Mux accept failed");
            let request: MuxRequest = ipc::read_json(&mut stream).unwrap();
            assert!(request_is_ping(&request));
            ipc::write_json(
                &mut stream,
                &MuxResponse::Pong {
                    version: "test-version".to_string(),
                },
            )
            .unwrap();
        });

        with_tracing_dispatch(|| run_with_connection_limit(&endpoint, Some(0))).unwrap();
        server.join().unwrap();

        cleanup_endpoint(&endpoint);
    }

    #[test]
    fn test_run_removes_stale_socket_path_and_rebinds() {
        let temp = TempDir::new().unwrap();
        let socket_path = temp.path().join("mux").join("stale.sock");
        std::fs::create_dir_all(socket_path.parent().unwrap()).unwrap();
        std::fs::write(&socket_path, "stale").unwrap();

        let endpoint_value = socket_path.to_string_lossy().into_owned();
        let endpoint = super::super::endpoint::socket_endpoint_from_value(&endpoint_value).unwrap();

        let endpoint_for_thread = endpoint.clone();
        let server = std::thread::spawn(move || {
            with_tracing_dispatch(|| run_with_connection_limit(&endpoint_for_thread, Some(1)))
        });

        let mut client =
            connect_with_retry(&endpoint.name).expect("Expected mux client to connect");
        ipc::write_json(&mut client, &MuxRequest::Ping).unwrap();
        let response: MuxResponse = ipc::read_json(&mut client).unwrap();
        assert!(response_is_pong(&response));
        drop(client);

        server.join().unwrap().unwrap();
        cleanup_endpoint(&endpoint);
    }

    #[test]
    fn test_run_errors_when_addr_in_use_and_no_cleanup_path() {
        let endpoint = unique_endpoint("tenex-test-daemon-in-use-no-cleanup").unwrap();
        let listener = ListenerOptions::new()
            .name(endpoint.name.clone())
            .create_sync()
            .unwrap();

        let server = std::thread::spawn(move || {
            let mut incoming = listener.incoming();
            let mut stream = incoming
                .next()
                .expect("Expected ping connection")
                .expect("Mux accept failed");
            let _request: MuxRequest = ipc::read_json(&mut stream).unwrap();
        });

        let err =
            with_tracing_dispatch(|| run_with_connection_limit(&endpoint, Some(0))).unwrap_err();
        assert!(err.to_string().contains("Mux endpoint is already in use"));
        server.join().unwrap();
    }

    #[test]
    fn test_serve_incoming_warns_on_accept_errors() {
        let incoming: Vec<io::Result<Stream>> = vec![Err(io::Error::other("boom"))];
        with_tracing_dispatch(|| serve_incoming(incoming, Some(1)));
    }

    #[test]
    fn test_rename_session_updates_resize_state() {
        resize_max().lock().clear();

        let session = unique_session("tenex-test-daemon-resize-rename");
        let working_dir = temp_working_dir();

        let _ = super::super::server::SessionManager::kill(&session);

        let _ = dispatch(MuxRequest::CreateSession {
            name: session.clone(),
            working_dir: working_dir.clone(),
            command: long_running_command(),
            cols: 80,
            rows: 24,
        })
        .unwrap();

        let response = dispatch(MuxRequest::CreateWindow {
            session: session.clone(),
            window_name: "child".to_string(),
            working_dir,
            command: long_running_command(),
            cols: 80,
            rows: 24,
        })
        .unwrap();
        let window_index = expect_window_created(&response).unwrap();

        let _ = dispatch(MuxRequest::Resize {
            target: session.clone(),
            cols: 90,
            rows: 30,
        })
        .unwrap();
        let _ = dispatch(MuxRequest::Resize {
            target: format!("{session}:{window_index}"),
            cols: 120,
            rows: 40,
        })
        .unwrap();

        let renamed = format!("{session}-renamed");
        let _ = dispatch(MuxRequest::RenameSession {
            old_name: session.clone(),
            new_name: renamed.clone(),
        })
        .unwrap();

        {
            let guard = resize_max().lock();
            assert!(guard.contains_key(&renamed));
            assert!(!guard.contains_key(&session));
            assert!(guard.contains_key(&format!("{renamed}:{window_index}")));
            assert!(!guard.contains_key(&format!("{session}:{window_index}")));
            drop(guard);
        }

        let _ = dispatch(MuxRequest::KillSession {
            name: renamed.clone(),
        })
        .unwrap();

        let guard = resize_max().lock();
        assert!(!guard.contains_key(&renamed));
        assert!(guard.is_empty());
        drop(guard);
    }

    #[test]
    fn test_try_ping_existing_true_and_false() {
        let temp = TempDir::new().unwrap();
        let endpoint = unique_path_endpoint(&temp, "tenex-test-daemon-ping");
        let listener = ListenerOptions::new()
            .name(endpoint.name.clone())
            .create_sync()
            .unwrap();

        let server = std::thread::spawn(move || {
            let mut incoming = listener.incoming();
            let mut stream = incoming
                .next()
                .expect("Expected incoming mux ping request")
                .expect("Mux accept failed");
            let request: MuxRequest = ipc::read_json(&mut stream).unwrap();
            assert!(request_is_ping(&request));
            ipc::write_json(
                &mut stream,
                &MuxResponse::Pong {
                    version: "test-version".to_string(),
                },
            )
            .unwrap();
        });

        assert!(try_ping_existing(&endpoint));
        server.join().unwrap();

        cleanup_endpoint(&endpoint);

        let missing_endpoint = unique_path_endpoint(&temp, "tenex-test-daemon-ping-missing");
        assert!(!try_ping_existing(&missing_endpoint));
        cleanup_endpoint(&missing_endpoint);
    }

    #[test]
    fn test_try_ping_stream_returns_false_when_write_json_fails() {
        struct FlushFailStream(std::io::Cursor<Vec<u8>>);

        impl std::io::Read for FlushFailStream {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.0.read(buf)
            }
        }

        impl std::io::Write for FlushFailStream {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.write(buf)
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::other("flush boom"))
            }
        }

        let mut stream = FlushFailStream(std::io::Cursor::new(Vec::new()));
        let mut probe = [0u8; 1];
        let _ = std::io::Read::read(&mut stream, &mut probe).expect("read");
        assert!(!try_ping_stream(&mut stream));
    }

    #[test]
    fn test_dispatch_session_lifecycle() {
        let session = unique_session("tenex-test-daemon");
        let working_dir = temp_working_dir();

        let _ = super::super::server::SessionManager::kill(&session);

        let response = dispatch(MuxRequest::CreateSession {
            name: session.clone(),
            working_dir,
            command: long_running_command(),
            cols: 80,
            rows: 24,
        })
        .unwrap();
        assert!(response_is_ok(&response));

        let response = dispatch(MuxRequest::SessionExists {
            name: session.clone(),
        })
        .unwrap();
        assert_eq!(response_bool_value(&response), Some(true));

        let sessions = expect_sessions(dispatch(MuxRequest::ListSessions).unwrap()).unwrap();
        assert!(sessions.iter().any(|s| s.name == session));

        let renamed = format!("{session}-renamed");
        let response = dispatch(MuxRequest::RenameSession {
            old_name: session,
            new_name: renamed.clone(),
        })
        .unwrap();
        assert!(response_is_ok(&response));

        let response = dispatch(MuxRequest::SessionExists {
            name: renamed.clone(),
        })
        .unwrap();
        assert_eq!(response_bool_value(&response), Some(true));

        let response = dispatch(MuxRequest::KillSession { name: renamed }).unwrap();
        assert!(response_is_ok(&response));
    }

    #[test]
    fn test_dispatch_window_capture_and_introspection() {
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
        })
        .unwrap();
        assert!(response_is_ok(&response));

        let windows = expect_windows(
            dispatch(MuxRequest::ListWindows {
                session: session.clone(),
            })
            .unwrap(),
        )
        .unwrap();
        assert!(windows.iter().any(|w| w.index == 0));

        let response = dispatch(MuxRequest::CreateWindow {
            session: session.clone(),
            window_name: "child".to_string(),
            working_dir,
            command,
            cols: 80,
            rows: 24,
        })
        .unwrap();
        let window_index = expect_window_created(&response).unwrap();

        let response = dispatch(MuxRequest::RenameWindow {
            session: session.clone(),
            window_index,
            new_name: "renamed".to_string(),
        })
        .unwrap();
        assert!(response_is_ok(&response));

        let target = format!("{session}:{window_index}");
        let response = dispatch(MuxRequest::Resize {
            target: target.clone(),
            cols: 100,
            rows: 40,
        })
        .unwrap();
        assert!(response_is_ok(&response));

        let response = dispatch(MuxRequest::PaneSize {
            target: target.clone(),
        })
        .unwrap();
        let (cols, rows) = expect_size(&response).unwrap();
        assert_eq!(cols, 100);
        assert_eq!(rows, 40);

        let response = dispatch(MuxRequest::CursorPosition {
            target: target.clone(),
        })
        .unwrap();
        assert!(response_is_position(&response));

        let text = expect_text(
            dispatch(MuxRequest::PaneCurrentCommand {
                target: target.clone(),
            })
            .unwrap(),
        )
        .unwrap();
        assert!(!text.is_empty());

        let response = dispatch(MuxRequest::Tail {
            target: target.clone(),
            lines: 5,
        })
        .unwrap();
        let _ = expect_text(response).expect("tail text");

        for kind in [
            CaptureKind::Visible,
            CaptureKind::History { lines: 50 },
            CaptureKind::FullHistory,
        ] {
            let response = dispatch(MuxRequest::Capture {
                target: target.clone(),
                kind,
            })
            .unwrap();
            let _ = expect_text(response).expect("capture text");
        }

        let response = dispatch(MuxRequest::ListPanePids {
            session: session.clone(),
        })
        .unwrap();
        assert!(response_is_pids(&response));

        let response = dispatch(MuxRequest::KillWindow {
            session: session.clone(),
            window_index,
        })
        .unwrap();
        assert!(response_is_ok(&response));

        let response = dispatch(MuxRequest::KillSession { name: session }).unwrap();
        assert!(response_is_ok(&response));
    }

    #[test]
    fn test_dispatch_empty_commands_send_input_and_empty_output_paths() {
        let session = unique_session("tenex-test-daemon-empty");
        let working_dir = temp_working_dir();

        let _ = super::super::server::SessionManager::kill(&session);

        let response = dispatch(MuxRequest::CreateSession {
            name: session.clone(),
            working_dir: working_dir.clone(),
            command: Vec::new(),
            cols: 80,
            rows: 24,
        })
        .expect("create empty session");
        assert!(response_is_ok(&response));

        let response = dispatch(MuxRequest::CreateWindow {
            session: session.clone(),
            window_name: "empty-command-window".to_string(),
            working_dir: working_dir.clone(),
            command: Vec::new(),
            cols: 80,
            rows: 24,
        })
        .expect("create empty command window");
        let window_index = expect_window_created(&response).expect("window created");

        let target = format!("{session}:{window_index}");
        let response = dispatch(MuxRequest::SendInput {
            target,
            data: Vec::new(),
        })
        .expect("send empty input");
        assert!(response_is_ok(&response));

        let response =
            dispatch(MuxRequest::KillSession { name: session }).expect("kill empty session");
        assert!(response_is_ok(&response));

        let quiet_session = unique_session("tenex-test-daemon-read-empty");
        let _ = super::super::server::SessionManager::kill(&quiet_session);

        let response = dispatch(MuxRequest::CreateSession {
            name: quiet_session.clone(),
            working_dir,
            command: long_running_command(),
            cols: 80,
            rows: 24,
        })
        .expect("create quiet session");
        assert!(response_is_ok(&response));

        let window = super::super::backend::resolve_window(&quiet_session).expect("resolve window");
        {
            let mut guard = window.lock();
            guard.output_history.seq_start = 10;
            guard.output_history.seq_end = 10;
            guard.output_history.buf.clear();
            guard.output_history.checkpoint = None;
        }

        let response = dispatch(MuxRequest::ReadOutput {
            target: quiet_session.clone(),
            after: 0,
            max_bytes: 4096,
        })
        .expect("read output reset");
        let (start, checkpoint_b64) = expect_output_reset(response).expect("output reset");
        assert_eq!(start, 10);
        assert!(checkpoint_b64.is_empty());

        let response = dispatch(MuxRequest::ReadOutput {
            target: quiet_session.clone(),
            after: start,
            max_bytes: 4096,
        })
        .expect("read output chunk");
        let (start, end, data_b64) = expect_output_chunk(response).expect("output chunk");
        assert_eq!(start, end);
        assert!(data_b64.is_empty());

        let response = dispatch(MuxRequest::KillSession {
            name: quiet_session,
        })
        .expect("kill quiet session");
        assert!(response_is_ok(&response));
    }

    #[test]
    fn test_dispatch_output_cursor_reflects_history_bounds() {
        let session = unique_session("tenex-test-daemon-output-cursor");
        let working_dir = temp_working_dir();

        let _ = super::super::server::SessionManager::kill(&session);

        let response = dispatch(MuxRequest::CreateSession {
            name: session.clone(),
            working_dir,
            command: long_running_command(),
            cols: 80,
            rows: 24,
        })
        .expect("create session");
        assert!(response_is_ok(&response));

        let window = super::super::backend::resolve_window(&session).expect("resolve window");
        {
            let mut guard = window.lock();
            guard.output_history.seq_start = 5;
            guard.output_history.seq_end = 42;
        }

        let response = dispatch(MuxRequest::OutputCursor {
            target: session.clone(),
        })
        .expect("output cursor");
        let (start, end) = expect_output_cursor(&response).expect("output cursor response");
        assert_eq!(start, 5);
        assert_eq!(end, 42);

        let response = dispatch(MuxRequest::KillSession { name: session }).expect("kill session");
        assert!(response_is_ok(&response));
    }

    #[test]
    fn test_resize_is_monotonic_max_per_target() {
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
        })
        .expect("create session");
        assert!(response_is_ok(&response));

        let response = dispatch(MuxRequest::Resize {
            target: session.clone(),
            cols: 80,
            rows: 24,
        })
        .expect("resize");
        assert!(response_is_ok(&response));

        let response = dispatch(MuxRequest::Resize {
            target: session.clone(),
            cols: 120,
            rows: 40,
        })
        .expect("resize");
        assert!(response_is_ok(&response));

        let response = dispatch(MuxRequest::PaneSize {
            target: session.clone(),
        })
        .expect("pane size");
        let (cols, rows) = expect_size(&response).expect("size response");
        assert_eq!((cols, rows), (120, 40));

        let response = dispatch(MuxRequest::Resize {
            target: session.clone(),
            cols: 80,
            rows: 24,
        })
        .expect("resize");
        assert!(response_is_ok(&response));

        let response = dispatch(MuxRequest::PaneSize {
            target: session.clone(),
        })
        .expect("pane size");
        let (cols, rows) = expect_size(&response).expect("size response");
        assert_eq!((cols, rows), (120, 40));

        let response = dispatch(MuxRequest::KillSession { name: session }).expect("kill session");
        assert!(response_is_ok(&response));
    }

    #[test]
    fn test_dispatch_read_output_returns_chunks() {
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
        })
        .expect("create session");
        assert!(response_is_ok(&response));

        let mut after = 0_u64;
        let mut collected: Vec<u8> = Vec::new();

        for _ in 0..50 {
            let response = dispatch(MuxRequest::ReadOutput {
                target: session.clone(),
                after,
                max_bytes: 4096,
            })
            .expect("read output");

            let (start, end, data_b64) = expect_output_chunk(response).expect("output chunk");
            assert_eq!(start, after);
            after = end;

            if !data_b64.is_empty() {
                let data = BASE64
                    .decode(data_b64.as_bytes())
                    .expect("decode output chunk");
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

        let response = dispatch(MuxRequest::KillSession { name: session }).expect("kill session");
        assert!(response_is_ok(&response));
    }

    #[test]
    fn test_dispatch_read_output_resets_when_after_is_stale() {
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
        })
        .expect("create session");
        assert!(response_is_ok(&response));

        let window = super::super::backend::resolve_window(&session).expect("resolve window");
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
        })
        .expect("read output");
        let (start, checkpoint_b64) = expect_output_reset(response).expect("output reset");
        assert_eq!(start, 5);
        assert_eq!(
            BASE64
                .decode(checkpoint_b64.as_bytes())
                .expect("decode checkpoint"),
            b"checkpoint".to_vec()
        );

        let response = dispatch(MuxRequest::KillSession { name: session }).expect("kill session");
        assert!(response_is_ok(&response));
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
