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
    let err = connect_with_retry_attempts(&endpoint.name, 1, std::time::Duration::from_millis(0))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
}

#[test]
fn test_connect_with_retry_attempts_reports_failure_without_attempts() {
    let temp = TempDir::new().unwrap();
    let endpoint = unique_path_endpoint(&temp, "tenex-test-daemon-connect-missing-attempts-0");
    let err = connect_with_retry_attempts(&endpoint.name, 0, std::time::Duration::from_millis(0))
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
    let mut incoming = OneShotIncoming {
        iterated: iterated.clone(),
    }
    .into_iter();
    serve_incoming(&mut incoming, Some(0));
    assert!(!iterated.load(Ordering::SeqCst));
}

#[test]
fn test_serve_incoming_iterates_when_limit_is_nonzero() {
    let iterated = Arc::new(AtomicBool::new(false));
    let mut incoming = OneShotIncoming {
        iterated: iterated.clone(),
    }
    .into_iter();
    serve_incoming(&mut incoming, Some(1));
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

    let mut incoming = std::iter::once(Ok(server_stream));
    serve_incoming(&mut incoming, Some(2));
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

    let mut client = connect_with_retry(&endpoint.name).expect("Expected mux client to connect");
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
    let err = handle_connection(&mut stream).unwrap_err();
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
        let mut stream = incoming
            .next()
            .expect("Expected client connection")
            .expect("Mux accept failed");
        handle_connection(&mut stream)
    });

    let mut client = connect_with_retry(&endpoint.name).expect("Expected mux client to connect");
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
#[expect(
    clippy::too_many_lines,
    reason = "single table-style test covers all missing target request variants"
)]
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

    let mut client = connect_with_retry(&endpoint.name).expect("Expected mux client to connect");
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

    let mut client = connect_with_retry(&endpoint.name).expect("Expected mux client to connect");
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

    let err = with_tracing_dispatch(|| run_with_connection_limit(&endpoint, Some(0))).unwrap_err();
    assert!(err.to_string().contains("Mux endpoint is already in use"));
    server.join().unwrap();
}

#[test]
fn test_serve_incoming_warns_on_accept_errors() {
    let incoming: Vec<io::Result<Stream>> = vec![Err(io::Error::other("boom"))];
    with_tracing_dispatch(|| {
        let mut incoming = incoming.into_iter();
        serve_incoming(&mut incoming, Some(1));
    });
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

    let response = dispatch(MuxRequest::KillSession { name: session }).expect("kill empty session");
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
