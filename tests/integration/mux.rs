//! Tests for mux session operations and capture functions

use crate::common::{TestFixture, skip_if_no_mux};
use tenex::mux::SessionManager;

fn sleep_command(seconds: u32) -> Vec<String> {
    #[cfg(windows)]
    {
        vec![
            "powershell".to_string(),
            "-NoProfile".to_string(),
            "-Command".to_string(),
            format!("Start-Sleep -Seconds {seconds}"),
        ]
    }
    #[cfg(not(windows))]
    {
        vec!["sleep".to_string(), seconds.to_string()]
    }
}

fn wait_for_session(
    manager: SessionManager,
    session_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(3);
    while start.elapsed() < timeout {
        if manager.exists(session_name) {
            return Ok(());
        }

        std::thread::sleep(std::time::Duration::from_millis(25));
    }

    Err(format!("Session '{session_name}' not found").into())
}

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
    let command = sleep_command(10);
    let result = manager.create(&session_name, &fixture.worktree_path(), Some(&command));
    assert!(result.is_ok());

    wait_for_session(manager, &session_name)?;

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

    wait_for_session(manager, &session_name)?;

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
    let command = sleep_command(60);
    manager.create(&session_name, &fixture.worktree_path(), Some(&command))?;

    wait_for_session(manager, &session_name)?;

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
    let command = sleep_command(60);
    manager.create(&session_name, &fixture.worktree_path(), Some(&command))?;

    wait_for_session(manager, &session_name)?;

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

#[cfg(unix)]
#[test]
fn test_mux_capture_pane_with_history_includes_full_output_tail()
-> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("capture_history_tail")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("histtail");

    let _ = manager.kill(&session_name);

    let start_marker = format!("__tenex_hist_start_{session_name}__");
    let end_marker = format!("__tenex_hist_end_{session_name}__");
    let script = format!(
        "echo {start_marker}; \
         i=0; \
         while [ $i -lt 120 ]; do echo LINE_$i; i=$((i+1)); done; \
         echo {end_marker}; \
         sleep 60"
    );
    let command = vec!["sh".to_string(), "-c".to_string(), script];

    manager.create(&session_name, &fixture.worktree_path(), Some(&command))?;
    std::thread::sleep(std::time::Duration::from_millis(300));

    let capture = tenex::mux::OutputCapture::new();
    let output = capture.capture_pane_with_history(&session_name, 1000)?;

    manager.kill(&session_name)?;

    assert!(
        output.contains(&start_marker),
        "Expected capture to include full history start marker {start_marker}, got: {output:?}"
    );

    let tail_has_end_marker = output
        .lines()
        .rev()
        .take(32)
        .any(|line| line.contains(&end_marker));
    assert!(
        tail_has_end_marker,
        "Expected capture tail to include end marker {end_marker}, got: {output:?}"
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn test_mux_capture_pane_with_history_includes_alternate_screen_scrollback()
-> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("capture_history_alt_screen")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("alt_hist");

    let _ = manager.kill(&session_name);

    // Keep markers short so they don't wrap in the 80-column default PTY.
    let start_marker = "__tenex_alt_hist_start__";
    let end_marker = "__tenex_alt_hist_end__";
    let script = format!(
        "printf '\\033[?1049h'; \
         printf '\\033[1;23r'; \
         printf '\\033[H'; \
         echo {start_marker}; \
         i=0; \
         while [ $i -lt 120 ]; do echo ALT_LINE_$i; i=$((i+1)); done; \
         echo {end_marker}; \
         sleep 60"
    );
    let command = vec!["sh".to_string(), "-c".to_string(), script];

    manager.create(&session_name, &fixture.worktree_path(), Some(&command))?;
    std::thread::sleep(std::time::Duration::from_millis(400));

    let capture = tenex::mux::OutputCapture::new();
    let output = capture.capture_pane_with_history(&session_name, 1000)?;

    manager.kill(&session_name)?;

    assert!(
        output.contains(start_marker),
        "Expected alt-screen capture to include start marker {start_marker}, got: {output:?}"
    );

    let tail_has_end_marker = output
        .lines()
        .rev()
        .take(32)
        .any(|line| line.contains(end_marker));
    assert!(
        tail_has_end_marker,
        "Expected alt-screen capture tail to include end marker {end_marker}, got: {output:?}"
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn test_mux_capture_pane_with_history_ends_with_visible_pane()
-> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("capture_history_suffix")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("histsuffix");

    let _ = manager.kill(&session_name);

    let tail_marker = format!("__tenex_hist_suffix_{session_name}__");
    let script = format!(
        "i=0; \
         while [ $i -lt 200 ]; do echo LINE_$i; i=$((i+1)); done; \
         echo {tail_marker}; \
         sleep 60"
    );
    let command = vec!["sh".to_string(), "-c".to_string(), script];

    manager.create(&session_name, &fixture.worktree_path(), Some(&command))?;
    std::thread::sleep(std::time::Duration::from_millis(300));

    let capture = tenex::mux::OutputCapture::new();
    let visible = capture.capture_pane(&session_name)?;
    let with_history = capture.capture_pane_with_history(&session_name, 1000)?;

    manager.kill(&session_name)?;

    assert!(
        with_history.contains(&tail_marker),
        "Expected history capture to include tail marker {tail_marker}, got: {with_history:?}"
    );
    assert!(
        visible.contains(&tail_marker),
        "Expected visible capture to include tail marker {tail_marker}, got: {visible:?}"
    );

    let visible_lines: Vec<&str> = visible.lines().collect();
    let history_lines: Vec<&str> = with_history.lines().collect();
    assert!(
        history_lines.len() >= visible_lines.len(),
        "Expected history capture to be at least as long as visible capture; history has {}, visible has {}",
        history_lines.len(),
        visible_lines.len()
    );

    let history_tail = &history_lines[history_lines.len().saturating_sub(visible_lines.len())..];
    assert_eq!(
        history_tail,
        &visible_lines[..],
        "Expected history capture to end with the visible pane content",
    );

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
    let command = sleep_command(60);
    manager.create(&session_name, &fixture.worktree_path(), Some(&command))?;

    wait_for_session(manager, &session_name)?;

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

    wait_for_session(manager, &session_name)?;

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

    wait_for_session(manager, &session_name)?;

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
    wait_for_session(manager, &session_name)?;

    // Create a window
    let window_command = sleep_command(60);
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
    let command = sleep_command(60);
    manager.create(&session_name, &fixture.worktree_path(), Some(&command))?;
    wait_for_session(manager, &session_name)?;

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
    wait_for_session(manager, &session_name)?;

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
    wait_for_session(manager, &session_name)?;

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
    let command = sleep_command(60);
    manager.create(&session_name, &fixture.worktree_path(), Some(&command))?;
    wait_for_session(manager, &session_name)?;

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
    wait_for_session(manager, &old_name)?;

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

    wait_for_session(manager, &session_name)?;

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

    wait_for_session(manager, &session_name)?;

    // Test default path (uses send_keys + carriage return).
    let token_default = format!("__tenex_submit_default_test_{session_name}__");
    let result = manager.send_keys_and_submit_for_program(
        &session_name,
        "bash",
        &format!("echo {token_default}"),
    );
    assert!(result.is_ok());

    std::thread::sleep(std::time::Duration::from_secs(2));

    // Verify the command actually ran
    let capture = tenex::mux::OutputCapture::new();
    let output = capture.capture_pane_with_history(&session_name, 200)?;
    assert!(
        output.contains(&token_default),
        "Expected submitted command output to contain token {token_default}, got: {output:?}"
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

#[cfg(unix)]
#[test]
fn test_mux_send_keys_and_submit_for_program_claude_uses_csi_u_enter_when_pane_is_claude()
-> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("program_submit_claude_csi_u")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("claude-csi-u");

    // Create a mock "claude" executable that only submits when it receives CSI-u Enter as its
    // own read chunk (mirrors Claude Code behavior).
    let claude_path = fixture.worktree_path().join("claude");
    std::fs::write(
        &claude_path,
        r#"#!/usr/bin/env python3
import os
import sys
import select
import time
import tty

CSI_U_ENTER = b"\x1b[13;1u"

def main() -> int:
    tty.setraw(sys.stdin.fileno())
    deadline = time.time() + 5.0
    while time.time() < deadline:
        r, _, _ = select.select([sys.stdin], [], [], 0.1)
        if not r:
            continue
        chunk = os.read(sys.stdin.fileno(), 1024)
        if not chunk:
            break
        if chunk == CSI_U_ENTER:
            sys.stdout.write("SUBMIT_OK\n")
            sys.stdout.flush()
            return 0
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
"#,
    )?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut perms = std::fs::metadata(&claude_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&claude_path, perms)?;
    }

    if manager.exists(&session_name) {
        manager.kill(&session_name)?;
    }

    let cmd = vec![claude_path.to_string_lossy().into_owned()];
    manager.create(&session_name, &fixture.worktree_path(), Some(&cmd))?;

    wait_for_session(manager, &session_name)?;
    std::thread::sleep(std::time::Duration::from_millis(200));

    let result = manager.send_keys_and_submit_for_program(
        &session_name,
        &claude_path.to_string_lossy(),
        "hello",
    );
    assert!(result.is_ok());

    std::thread::sleep(std::time::Duration::from_secs(1));

    let capture = tenex::mux::OutputCapture::new();
    let output = capture.capture_pane_with_history(&session_name, 200)?;
    assert!(
        output.contains("SUBMIT_OK"),
        "Expected CSI-u submit marker, got: {output:?}"
    );

    manager.kill(&session_name)?;
    Ok(())
}

#[cfg(unix)]
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

#[test]
fn test_mux_additional_session_ops() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("extra_ops")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("extra");

    if manager.exists(&session_name)
        && let Err(err) = manager.kill(&session_name)
    {
        eprintln!("Warning: failed to kill existing session {session_name}: {err}");
    }

    let command = sleep_command(60);
    manager.create(&session_name, &fixture.worktree_path(), Some(&command))?;
    wait_for_session(manager, &session_name)?;

    let pids = manager.list_pane_pids(&session_name)?;
    assert!(
        !pids.is_empty(),
        "Expected at least one pane PID for session {session_name}"
    );

    let window_idx = manager.create_window(
        &session_name,
        "extra-window",
        &fixture.worktree_path(),
        Some(&command),
    )?;
    manager.rename_window(&session_name, window_idx, "renamed-window")?;

    let windows = manager.list_windows(&session_name)?;
    assert!(
        windows.iter().any(|w| w.name == "renamed-window"),
        "Expected renamed window to be listed"
    );

    let target = SessionManager::window_target(&session_name, 0);
    manager.resize_window(&target, 120, 40)?;

    let keys = vec!["echo ".to_string(), "tenex".to_string()];
    manager.send_keys_batch(&session_name, &keys)?;

    if let Err(err) = manager.kill_window(&session_name, window_idx) {
        eprintln!("Warning: failed to kill window {session_name}:{window_idx}: {err}");
    }
    if let Err(err) = manager.kill(&session_name) {
        eprintln!("Warning: failed to kill session {session_name}: {err}");
    }

    Ok(())
}

#[test]
fn test_mux_error_paths_for_missing_session() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("missing_session")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("missing");
    let capture = tenex::mux::OutputCapture::new();

    assert!(manager.list_windows(&session_name).is_err());
    assert!(manager.list_pane_pids(&session_name).is_err());
    assert!(manager.rename_window(&session_name, 0, "new-name").is_err());
    assert!(manager.kill_window(&session_name, 0).is_err());

    let target = SessionManager::window_target(&session_name, 0);
    assert!(manager.resize_window(&target, 80, 24).is_err());
    assert!(manager.send_keys(&session_name, "echo nope").is_err());

    assert!(capture.pane_size(&session_name).is_err());
    assert!(capture.cursor_position(&session_name).is_err());
    assert!(capture.pane_current_command(&session_name).is_err());
    assert!(capture.tail(&session_name, 10).is_err());

    Ok(())
}
