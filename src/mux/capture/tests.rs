use super::*;
use crate::mux::ipc;
use interprocess::local_socket::ListenerOptions;
use interprocess::local_socket::traits::ListenerExt as _;
use tempfile::TempDir;

#[test]
fn test_output_capture_new() {
    let capture = Capture::new();
    assert!(!format!("{capture:?}").is_empty());
}

fn unexpected_request_response(request: &MuxRequest) -> MuxResponse {
    MuxResponse::Err {
        message: format!("unexpected request: {request:?}"),
    }
}

fn pane_size_responder(
    expected_target: &'static str,
    response: MuxResponse,
) -> impl FnOnce(MuxRequest) -> MuxResponse + Send + 'static {
    move |request| match request {
        MuxRequest::PaneSize { target } => {
            assert_eq!(target, expected_target);
            response
        }
        other => unexpected_request_response(&other),
    }
}

fn cursor_position_responder(
    expected_target: &'static str,
    response: MuxResponse,
) -> impl FnOnce(MuxRequest) -> MuxResponse + Send + 'static {
    move |request| match request {
        MuxRequest::CursorPosition { target } => {
            assert_eq!(target, expected_target);
            response
        }
        other => unexpected_request_response(&other),
    }
}

fn pane_current_command_responder(
    expected_target: &'static str,
    response: MuxResponse,
) -> impl FnOnce(MuxRequest) -> MuxResponse + Send + 'static {
    move |request| match request {
        MuxRequest::PaneCurrentCommand { target } => {
            assert_eq!(target, expected_target);
            response
        }
        other => unexpected_request_response(&other),
    }
}

fn tail_responder(
    expected_target: &'static str,
    expected_lines: u32,
    response: MuxResponse,
) -> impl FnOnce(MuxRequest) -> MuxResponse + Send + 'static {
    move |request| match request {
        MuxRequest::Tail { target, lines } => {
            assert_eq!(target, expected_target);
            assert_eq!(lines, expected_lines);
            response
        }
        other => unexpected_request_response(&other),
    }
}

fn capture_visible_responder(
    expected_target: &'static str,
    response: MuxResponse,
) -> impl FnOnce(MuxRequest) -> MuxResponse + Send + 'static {
    move |request| match request {
        MuxRequest::Capture { target, kind } => {
            assert_eq!(target, expected_target);
            match kind {
                CaptureKind::Visible => response,
                other => MuxResponse::Err {
                    message: format!("unexpected capture kind: {other:?}"),
                },
            }
        }
        other => unexpected_request_response(&other),
    }
}

fn assert_unexpected_response<T>(call: impl FnOnce(&Capture) -> Result<T>) {
    let err = run_mux_server(
        |_| MuxResponse::Pong {
            version: "test".to_string(),
        },
        call,
    )
    .err()
    .expect("Expected unexpected response error");
    let message = format!("{err:#}");
    assert!(message.contains("Unexpected response"));
}

fn run_mux_server<T>(
    responder: impl FnOnce(MuxRequest) -> MuxResponse + Send + 'static,
    call: impl FnOnce(&Capture) -> Result<T>,
) -> Result<T> {
    let temp = TempDir::new().expect("Create temp dir for mux capture test");
    let socket_path = temp.path().join("mux.sock");
    let display = socket_path.to_string_lossy().into_owned();
    let endpoint = super::super::endpoint::socket_endpoint_from_value(&display)
        .expect("Resolve test mux endpoint");

    let listener = ListenerOptions::new()
        .name(endpoint.name.clone())
        .create_sync()
        .expect("Create mux listener");

    let server = std::thread::spawn(move || {
        let mut incoming = listener.incoming();
        let mut stream = incoming
            .next()
            .expect("Expected mux client connection")
            .expect("Mux accept failed");
        let request: MuxRequest = ipc::read_json(&mut stream).expect("Read mux request");
        let response = responder(request);
        ipc::write_json(&mut stream, &response).expect("Write mux response");
    });

    crate::mux::set_socket_override(&display).expect("Set mux socket override");

    let capture = Capture::new();
    let result = call(&capture);

    server.join().expect("Mux server thread panicked");
    result
}

fn run_mux_server_drop_response<T>(call: impl FnOnce(&Capture) -> Result<T>) -> Result<T> {
    let temp = TempDir::new().expect("Create temp dir for mux capture test");
    let socket_path = temp.path().join("mux.sock");
    let display = socket_path.to_string_lossy().into_owned();
    let endpoint = super::super::endpoint::socket_endpoint_from_value(&display)
        .expect("Resolve test mux endpoint");

    let listener = ListenerOptions::new()
        .name(endpoint.name.clone())
        .create_sync()
        .expect("Create mux listener");

    let server = std::thread::spawn(move || {
        let mut incoming = listener.incoming();
        for _ in 0..2 {
            let mut stream = incoming
                .next()
                .expect("Expected mux client connection")
                .expect("Mux accept failed");
            let _: MuxRequest = ipc::read_json(&mut stream).expect("Read mux request");
            drop(stream);
        }
    });

    crate::mux::set_socket_override(&display).expect("Set mux socket override");

    let capture = Capture::new();
    let result = call(&capture);

    server.join().expect("Mux server thread panicked");
    result
}

#[test]
fn test_pane_size_errors_on_unexpected_response() {
    assert_unexpected_response(|capture| capture.pane_size("session"));
}

#[test]
fn test_cursor_position_errors_on_unexpected_response() {
    assert_unexpected_response(|capture| capture.cursor_position("session"));
}

#[test]
fn test_pane_current_command_errors_on_unexpected_response() {
    assert_unexpected_response(|capture| capture.pane_current_command("session"));
}

#[test]
fn test_tail_errors_on_unexpected_response() {
    assert_unexpected_response(|capture| capture.tail("session", 10));
}

#[test]
fn test_pane_size_errors_when_request_fails() {
    assert!(run_mux_server_drop_response(|capture| capture.pane_size("session")).is_err());
}

#[test]
fn test_cursor_position_errors_when_request_fails() {
    assert!(run_mux_server_drop_response(|capture| capture.cursor_position("session")).is_err());
}

#[test]
fn test_pane_current_command_errors_when_request_fails() {
    assert!(
        run_mux_server_drop_response(|capture| capture.pane_current_command("session")).is_err()
    );
}

#[test]
fn test_tail_errors_when_request_fails() {
    assert!(run_mux_server_drop_response(|capture| capture.tail("session", 10)).is_err());
}

#[test]
fn test_capture_pane_errors_on_unexpected_response() {
    assert_unexpected_response(|capture| capture.capture_pane("session"));
}

#[test]
fn test_pane_size_responder_reports_unexpected_request() {
    let err = run_mux_server(
        pane_size_responder("session", MuxResponse::Size { cols: 80, rows: 24 }),
        |capture| capture.cursor_position("session"),
    )
    .expect_err("expected unexpected request error");
    assert!(format!("{err:#}").contains("unexpected request"));
}

#[test]
fn test_cursor_position_responder_reports_unexpected_request() {
    let err = run_mux_server(
        cursor_position_responder(
            "session",
            MuxResponse::Position {
                x: 3,
                y: 4,
                hidden: true,
            },
        ),
        |capture| capture.pane_size("session"),
    )
    .expect_err("expected unexpected request error");
    assert!(format!("{err:#}").contains("unexpected request"));
}

#[test]
fn test_pane_current_command_responder_reports_unexpected_request() {
    let err = run_mux_server(
        pane_current_command_responder(
            "session",
            MuxResponse::Text {
                text: "bash".to_string(),
            },
        ),
        |capture| capture.pane_size("session"),
    )
    .expect_err("expected unexpected request error");
    assert!(format!("{err:#}").contains("unexpected request"));
}

#[test]
fn test_tail_responder_reports_unexpected_request() {
    let err = run_mux_server(
        tail_responder(
            "session",
            10,
            MuxResponse::Text {
                text: "one\ntwo\n".to_string(),
            },
        ),
        |capture| capture.pane_size("session"),
    )
    .expect_err("expected unexpected request error");
    assert!(format!("{err:#}").contains("unexpected request"));
}

#[test]
fn test_capture_visible_responder_reports_unexpected_kind() {
    let err = run_mux_server(
        capture_visible_responder(
            "session",
            MuxResponse::Text {
                text: "hello".to_string(),
            },
        ),
        |capture| capture.capture_pane_with_history("session", 10),
    )
    .expect_err("expected unexpected kind error");
    assert!(format!("{err:#}").contains("unexpected capture kind"));
}

#[test]
fn test_capture_visible_responder_reports_unexpected_request() {
    let err = run_mux_server(
        capture_visible_responder(
            "session",
            MuxResponse::Text {
                text: "hello".to_string(),
            },
        ),
        |capture| capture.pane_size("session"),
    )
    .expect_err("expected unexpected request error");
    assert!(format!("{err:#}").contains("unexpected request"));
}

#[test]
fn test_pane_size_returns_size() {
    let result = run_mux_server(
        pane_size_responder("session", MuxResponse::Size { cols: 80, rows: 24 }),
        |capture| capture.pane_size("session"),
    )
    .expect("pane size should succeed");
    assert_eq!(result, (80, 24));
}

#[test]
fn test_cursor_position_returns_position() {
    let result = run_mux_server(
        cursor_position_responder(
            "session",
            MuxResponse::Position {
                x: 3,
                y: 4,
                hidden: true,
            },
        ),
        |capture| capture.cursor_position("session"),
    )
    .expect("cursor position should succeed");
    assert_eq!(result, (3, 4, true));
}

#[test]
fn test_pane_current_command_returns_text() {
    let text = "bash".to_string();
    let result = run_mux_server(
        pane_current_command_responder("session", MuxResponse::Text { text }),
        |capture| capture.pane_current_command("session"),
    )
    .expect("pane current command should succeed");
    assert_eq!(result, "bash");
}

#[test]
fn test_tail_returns_lines_and_saturates_large_line_counts() {
    let result = run_mux_server(
        tail_responder(
            "session",
            u32::MAX,
            MuxResponse::Text {
                text: "one\ntwo\n".to_string(),
            },
        ),
        |capture| capture.tail("session", usize::MAX),
    )
    .expect("tail should succeed");
    assert_eq!(result, vec!["one".to_string(), "two".to_string()]);
}

#[test]
fn test_capture_pane_returns_text() {
    let text = "hello".to_string();
    let result = run_mux_server(
        capture_visible_responder("session", MuxResponse::Text { text }),
        |capture| capture.capture_pane("session"),
    )
    .expect("capture pane should succeed");
    assert_eq!(result, "hello");
}

#[test]
fn test_pane_size_errors_on_err_response() {
    let err = run_mux_server(
        pane_size_responder(
            "session",
            MuxResponse::Err {
                message: "nope".to_string(),
            },
        ),
        |capture| capture.pane_size("session"),
    )
    .expect_err("expected error response");
    assert!(format!("{err:#}").contains("nope"));
}

#[test]
fn test_cursor_position_errors_on_err_response() {
    let err = run_mux_server(
        cursor_position_responder(
            "session",
            MuxResponse::Err {
                message: "nope".to_string(),
            },
        ),
        |capture| capture.cursor_position("session"),
    )
    .expect_err("expected error response");
    assert!(format!("{err:#}").contains("nope"));
}

#[test]
fn test_pane_current_command_errors_on_err_response() {
    let err = run_mux_server(
        pane_current_command_responder(
            "session",
            MuxResponse::Err {
                message: "nope".to_string(),
            },
        ),
        |capture| capture.pane_current_command("session"),
    )
    .expect_err("expected error response");
    assert!(format!("{err:#}").contains("nope"));
}

#[test]
fn test_tail_errors_on_err_response() {
    let err = run_mux_server(
        tail_responder(
            "session",
            10,
            MuxResponse::Err {
                message: "nope".to_string(),
            },
        ),
        |capture| capture.tail("session", 10),
    )
    .expect_err("expected error response");
    assert!(format!("{err:#}").contains("nope"));
}

#[test]
fn test_capture_pane_errors_on_err_response() {
    let err = run_mux_server(
        capture_visible_responder(
            "session",
            MuxResponse::Err {
                message: "nope".to_string(),
            },
        ),
        |capture| capture.capture_pane("session"),
    )
    .expect_err("expected error response");
    assert!(format!("{err:#}").contains("nope"));
}
