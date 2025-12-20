#![cfg(not(windows))]

//! Tests for mux session operations and capture functions

use crate::common::{TestFixture, skip_if_no_mux};
use tenex::mux::SessionManager;

#[test]
fn test_mux_session_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("lifecycle")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("lifecycle");

    // Ensure session doesn't exist
    let _ = manager.kill(&session_name);
    assert!(!manager.exists(&session_name));

    // Create session with a command that stays alive
    let command = vec!["sleep".to_string(), "10".to_string()];
    let result = manager.create(&session_name, &fixture.worktree_path(), Some(&command));
    assert!(result.is_ok());

    // Give the mux a moment to start
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Verify session exists
    assert!(manager.exists(&session_name));

    // Kill session
    let result = manager.kill(&session_name);
    assert!(result.is_ok());

    // Verify session is gone
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(!manager.exists(&session_name));

    Ok(())
}

#[test]
fn test_mux_session_list() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("list_sessions")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("listtest");

    // Create a session
    let _ = manager.kill(&session_name);
    manager.create(&session_name, &fixture.worktree_path(), None)?;

    std::thread::sleep(std::time::Duration::from_millis(100));

    // List sessions and verify our session is present
    let sessions = manager.list()?;
    let found = sessions.iter().any(|s| s.name == session_name);
    assert!(found, "Created session should appear in list");

    // Cleanup
    let _ = manager.kill(&session_name);

    Ok(())
}

#[test]
fn test_mux_capture_pane() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("capture_pane")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("capture");

    // Create a session that stays alive
    let _ = manager.kill(&session_name);
    let command = vec!["sleep".to_string(), "60".to_string()];
    manager.create(&session_name, &fixture.worktree_path(), Some(&command))?;

    std::thread::sleep(std::time::Duration::from_millis(300));

    // Verify session exists
    assert!(
        manager.exists(&session_name),
        "Session should exist before capture"
    );

    // Capture the pane
    let capture = tenex::mux::OutputCapture::new();
    let result = capture.capture_pane(&session_name);

    // Cleanup
    let _ = manager.kill(&session_name);

    assert!(result.is_ok(), "Capture failed: {result:?}");

    Ok(())
}

#[test]
fn test_mux_capture_pane_with_history() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("capture_history")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("hist");

    // Create a session that stays alive
    let _ = manager.kill(&session_name);
    let command = vec!["sleep".to_string(), "60".to_string()];
    manager.create(&session_name, &fixture.worktree_path(), Some(&command))?;

    std::thread::sleep(std::time::Duration::from_millis(300));

    // Verify session exists
    assert!(
        manager.exists(&session_name),
        "Session should exist before capture"
    );

    // Capture with history
    let capture = tenex::mux::OutputCapture::new();
    let result = capture.capture_pane_with_history(&session_name, 100);

    // Cleanup
    let _ = manager.kill(&session_name);

    assert!(result.is_ok(), "Capture with history failed: {result:?}");

    Ok(())
}

#[test]
fn test_mux_capture_full_history() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("capture_full")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("full");

    // Create a session that stays alive
    let _ = manager.kill(&session_name);
    let command = vec!["sleep".to_string(), "60".to_string()];
    manager.create(&session_name, &fixture.worktree_path(), Some(&command))?;

    std::thread::sleep(std::time::Duration::from_millis(300));

    // Verify session exists
    assert!(
        manager.exists(&session_name),
        "Session should exist before capture"
    );

    // Capture full history
    let capture = tenex::mux::OutputCapture::new();
    let result = capture.capture_full_history(&session_name);

    // Cleanup
    let _ = manager.kill(&session_name);

    assert!(result.is_ok(), "Capture full history failed: {result:?}");

    Ok(())
}

#[test]
fn test_mux_capture_nonexistent_session() {
    if skip_if_no_mux() {
        return;
    }

    let capture = tenex::mux::OutputCapture::new();
    let result = capture.capture_pane("nonexistent-session-xyz");
    assert!(result.is_err());
}

#[test]
fn test_mux_send_keys() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("send_keys")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("keys");

    // Create a session
    let _ = manager.kill(&session_name);
    manager.create(&session_name, &fixture.worktree_path(), None)?;

    std::thread::sleep(std::time::Duration::from_millis(200));

    // Send keys
    let result = manager.send_keys(&session_name, "echo test");
    assert!(result.is_ok());

    // Cleanup
    let _ = manager.kill(&session_name);

    Ok(())
}

#[test]
fn test_mux_send_keys_and_submit() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("send_submit")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("submit");

    // Create a session
    if manager.exists(&session_name) {
        manager.kill(&session_name)?;
    }
    manager.create(&session_name, &fixture.worktree_path(), None)?;

    std::thread::sleep(std::time::Duration::from_millis(200));

    // Send keys with submit
    let token = format!("__tenex_submit_test_{session_name}__");
    let result = manager.send_keys_and_submit(&session_name, &format!("echo {token}"));
    assert!(result.is_ok());

    // Give the shell time to execute the command and print output.
    std::thread::sleep(std::time::Duration::from_secs(2));

    // Verify the command actually ran (i.e. submit sent Enter).
    let capture = tenex::mux::OutputCapture::new();
    let output = capture.capture_pane_with_history(&session_name, 200)?;
    assert!(
        output.contains(&token),
        "Expected submitted command output to contain token {token}, got: {output:?}"
    );

    // Cleanup
    manager.kill(&session_name)?;

    Ok(())
}

#[test]
fn test_mux_window_operations() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("window_ops")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("winops");

    // Create session
    let _ = manager.kill(&session_name);
    manager.create(&session_name, &fixture.worktree_path(), None)?;
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Create a window
    let window_command = vec!["sleep".to_string(), "60".to_string()];
    let window_result = manager.create_window(
        &session_name,
        "test-window",
        &fixture.worktree_path(),
        Some(&window_command),
    );
    assert!(window_result.is_ok());

    if let Ok(window_idx) = window_result {
        // List windows
        let windows = manager.list_windows(&session_name)?;
        assert!(!windows.is_empty());

        // Kill the window (may fail if window auto-closed)
        let _ = manager.kill_window(&session_name, window_idx);
    }

    // Cleanup
    let _ = manager.kill(&session_name);

    Ok(())
}

#[test]
fn test_mux_capture_tail() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("capture_tail")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("tail");

    // Create session with output
    let _ = manager.kill(&session_name);
    let command = vec!["sleep".to_string(), "60".to_string()];
    manager.create(&session_name, &fixture.worktree_path(), Some(&command))?;
    std::thread::sleep(std::time::Duration::from_millis(300));

    // Capture tail
    let capture = tenex::mux::OutputCapture::new();
    let result = capture.tail(&session_name, 10);
    assert!(result.is_ok());

    // Cleanup
    let _ = manager.kill(&session_name);

    Ok(())
}

#[test]
fn test_mux_pane_size() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("pane_size")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("size");

    // Create session
    let _ = manager.kill(&session_name);
    manager.create(&session_name, &fixture.worktree_path(), None)?;
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Get pane size
    let capture = tenex::mux::OutputCapture::new();
    let result = capture.pane_size(&session_name);
    assert!(result.is_ok());

    if let Ok((width, height)) = result {
        assert!(width > 0);
        assert!(height > 0);
    }

    // Cleanup
    let _ = manager.kill(&session_name);

    Ok(())
}

#[test]
fn test_mux_cursor_position() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("cursor_pos")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("cursor");

    // Create session
    let _ = manager.kill(&session_name);
    manager.create(&session_name, &fixture.worktree_path(), None)?;
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Get cursor position
    let capture = tenex::mux::OutputCapture::new();
    let result = capture.cursor_position(&session_name);
    assert!(result.is_ok());

    // Cleanup
    let _ = manager.kill(&session_name);

    Ok(())
}

#[test]
fn test_mux_pane_current_command() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("pane_cmd")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("cmd");

    // Create session with a specific command
    let _ = manager.kill(&session_name);
    let command = vec!["sleep".to_string(), "60".to_string()];
    manager.create(&session_name, &fixture.worktree_path(), Some(&command))?;
    std::thread::sleep(std::time::Duration::from_millis(300));

    // Get current command
    let capture = tenex::mux::OutputCapture::new();
    let result = capture.pane_current_command(&session_name);
    // Should succeed (though command may vary)
    assert!(result.is_ok());

    // Cleanup
    let _ = manager.kill(&session_name);

    Ok(())
}

#[test]
fn test_mux_session_rename() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("session_rename")?;
    let manager = SessionManager::new();
    let old_name = fixture.session_name("old");
    let new_name = fixture.session_name("new");

    // Create session
    let _ = manager.kill(&old_name);
    let _ = manager.kill(&new_name);
    manager.create(&old_name, &fixture.worktree_path(), None)?;
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Rename
    let result = manager.rename(&old_name, &new_name);
    assert!(result.is_ok());

    // Verify
    assert!(!manager.exists(&old_name));
    assert!(manager.exists(&new_name));

    // Cleanup
    let _ = manager.kill(&new_name);

    Ok(())
}

#[test]
fn test_mux_paste_keys_and_submit() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("paste_submit")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("paste");

    // Create a session
    if manager.exists(&session_name) {
        manager.kill(&session_name)?;
    }
    manager.create(&session_name, &fixture.worktree_path(), None)?;

    std::thread::sleep(std::time::Duration::from_millis(200));

    // Send keys with bracketed paste and submit
    let token = format!("__tenex_paste_test_{session_name}__");
    let result = manager.paste_keys_and_submit(&session_name, &format!("echo {token}"));
    assert!(result.is_ok());

    // Give the shell time to execute the command and print output.
    std::thread::sleep(std::time::Duration::from_secs(2));

    // Verify the command actually ran (i.e. submit sent C-m).
    let capture = tenex::mux::OutputCapture::new();
    let output = capture.capture_pane_with_history(&session_name, 200)?;
    assert!(
        output.contains(&token),
        "Expected submitted command output to contain token {token}, got: {output:?}"
    );

    // Cleanup
    manager.kill(&session_name)?;

    Ok(())
}

#[test]
fn test_mux_send_keys_and_submit_for_program() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("program_submit")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("progsub");

    // Create a session
    if manager.exists(&session_name) {
        manager.kill(&session_name)?;
    }
    manager.create(&session_name, &fixture.worktree_path(), None)?;

    std::thread::sleep(std::time::Duration::from_millis(200));

    // Test with "claude" program (uses send_keys path)
    let token_claude = format!("__tenex_claude_test_{session_name}__");
    let result = manager.send_keys_and_submit_for_program(
        &session_name,
        "claude",
        &format!("echo {token_claude}"),
    );
    assert!(result.is_ok());

    std::thread::sleep(std::time::Duration::from_secs(2));

    // Verify the command actually ran
    let capture = tenex::mux::OutputCapture::new();
    let output = capture.capture_pane_with_history(&session_name, 200)?;
    assert!(
        output.contains(&token_claude),
        "Expected submitted command output to contain token {token_claude}, got: {output:?}"
    );

    // Test with "codex" program (uses paste_keys path)
    let token_codex = format!("__tenex_codex_test_{session_name}__");
    let result = manager.send_keys_and_submit_for_program(
        &session_name,
        "codex",
        &format!("echo {token_codex}"),
    );
    assert!(result.is_ok());

    std::thread::sleep(std::time::Duration::from_secs(2));

    // Verify the command actually ran
    let output = capture.capture_pane_with_history(&session_name, 200)?;
    assert!(
        output.contains(&token_codex),
        "Expected submitted command output to contain token {token_codex}, got: {output:?}"
    );

    // Cleanup
    manager.kill(&session_name)?;

    Ok(())
}

#[test]
fn test_mux_responds_to_terminal_queries() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("terminal_queries")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("termq");

    let _ = manager.kill(&session_name);

    let script = concat!(
        "stty raw -echo min 0 time 10; ",
        "printf 'DA:'; ",
        "printf '\\033[c\\n'; ",
        "dd bs=1 count=32 2>/dev/null | od -An -tx1; ",
        "printf '\\n'; ",
        "printf 'CPR:'; ",
        "printf '\\033[6n\\n'; ",
        "dd bs=1 count=32 2>/dev/null | od -An -tx1; ",
        "printf '\\n'; ",
        "stty sane",
    );
    let command = vec!["sh".to_string(), "-c".to_string(), script.to_string()];

    manager.create(&session_name, &fixture.worktree_path(), Some(&command))?;

    std::thread::sleep(std::time::Duration::from_secs(3));

    let capture = tenex::mux::OutputCapture::new();
    let output = capture.capture_full_history(&session_name)?;

    manager.kill(&session_name)?;

    assert!(
        output.contains("DA:"),
        "Expected DA marker, got: {output:?}"
    );
    assert!(
        output.contains("CPR:"),
        "Expected CPR marker, got: {output:?}"
    );

    let normalized_output = output.split_whitespace().collect::<Vec<_>>().join(" ");

    // Primary device attributes response: ESC [ ? 1 ; 0 c
    assert!(
        normalized_output.contains("1b 5b 3f 31 3b 30 63"),
        "Expected primary device attributes response bytes, got: {output:?}"
    );

    // Cursor position report response: ESC [ <row> ; <col> R
    let cpr_section = normalized_output.split("CPR:").nth(1).unwrap_or_default();
    assert!(
        cpr_section.contains("1b 5b") && cpr_section.contains("3b") && cpr_section.contains("52"),
        "Expected cursor position report bytes after CPR marker, got: {output:?}"
    );

    Ok(())
}
