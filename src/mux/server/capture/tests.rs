use super::super::SessionManager;
use super::*;

fn test_command() -> Vec<String> {
    #[cfg(windows)]
    {
        vec![
            "powershell".to_string(),
            "-NoProfile".to_string(),
            "-Command".to_string(),
            "Start-Sleep -Seconds 2".to_string(),
        ]
    }
    #[cfg(not(windows))]
    {
        vec!["sh".to_string(), "-c".to_string(), "sleep 2".to_string()]
    }
}

#[test]
fn test_output_capture_new() {
    let capture = Capture;
    assert!(!format!("{capture:?}").is_empty());
}

#[test]
fn test_capture_with_real_session() {
    let session_name = "tenex-test-capture";
    let tmp = std::env::temp_dir();

    let _ = SessionManager::kill(session_name);

    let command = test_command();
    let result = SessionManager::create(session_name, &tmp, Some(&command));
    assert!(result.is_ok());

    let _ = Capture::capture_pane(session_name);
    let _ = Capture::capture_pane_with_history(session_name, 10);
    let _ = Capture::capture_full_history(session_name);
    let _ = Capture::pane_size(session_name);
    let _ = Capture::cursor_position(session_name);
    let _ = Capture::pane_current_command(session_name);
    let _ = Capture::tail(session_name, 10);

    let _ = SessionManager::kill(session_name);
}

#[test]
fn test_capture_propagates_missing_session_errors() {
    let session_name = "tenex-test-capture-missing-session";
    let _ = SessionManager::kill(session_name);

    let err = Capture::capture_pane(session_name).expect_err("capture pane should error");
    assert!(format!("{err}").contains("Session"));

    let err = Capture::capture_pane_with_history(session_name, 10)
        .expect_err("capture pane with history should error");
    assert!(format!("{err}").contains("Session"));

    let err =
        Capture::capture_full_history(session_name).expect_err("capture full history should error");
    assert!(format!("{err}").contains("Session"));

    let err = Capture::pane_size(session_name).expect_err("pane size should error");
    assert!(format!("{err}").contains("Session"));

    let err = Capture::cursor_position(session_name).expect_err("cursor position should error");
    assert!(format!("{err}").contains("Session"));

    let err =
        Capture::pane_current_command(session_name).expect_err("pane current command should error");
    assert!(format!("{err}").contains("Session"));

    let err = Capture::tail(session_name, 10).expect_err("tail should error");
    assert!(format!("{err}").contains("Session"));
}

#[test]
fn test_visible_text_detection() {
    assert!(!has_visible_text("   \t"));
    assert!(!has_visible_text("\u{1b}[0m   \u{1b}[0m"));
    assert!(has_visible_text(" x "));
}

#[test]
fn test_skip_escape_sequence_handles_trailing_escape_byte() {
    let bytes = b"\x1b";
    assert_eq!(skip_escape_sequence(bytes, 0), 1);
}

#[test]
fn test_skip_escape_sequence_advances_for_unknown_sequence() {
    let bytes = b"\x1bX";
    assert_eq!(skip_escape_sequence(bytes, 0), 2);
}

#[test]
fn test_skip_escape_sequence_handles_osc_sequences() {
    let bytes = b"\x1b]0;123";
    assert_eq!(
        skip_escape_sequence(bytes, 0),
        bytes.len().saturating_add(1)
    );
}

#[test]
fn test_skip_escape_sequence_advances_when_csi_missing_terminator() {
    let bytes = b"\x1b[123";
    assert_eq!(
        skip_escape_sequence(bytes, 0),
        bytes.len().saturating_add(1)
    );
}
