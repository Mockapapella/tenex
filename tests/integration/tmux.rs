//! Tests for tmux session operations and capture functions

use crate::common::{TestFixture, skip_if_no_tmux};
use tenex::tmux::SessionManager;

#[test]
fn test_tmux_session_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("lifecycle")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("lifecycle");

    // Ensure session doesn't exist
    let _ = manager.kill(&session_name);
    assert!(!manager.exists(&session_name));

    // Create session with a command that stays alive
    let result = manager.create(&session_name, &fixture.worktree_path(), Some("sleep 10"));
    assert!(result.is_ok());

    // Give tmux time to start
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
fn test_tmux_session_list() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
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
fn test_tmux_capture_pane() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("capture_pane")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("capture");

    // Create a session that stays alive
    let _ = manager.kill(&session_name);
    manager.create(&session_name, &fixture.worktree_path(), Some("sleep 60"))?;

    std::thread::sleep(std::time::Duration::from_millis(300));

    // Verify session exists
    assert!(
        manager.exists(&session_name),
        "Session should exist before capture"
    );

    // Capture the pane
    let capture = tenex::tmux::OutputCapture::new();
    let result = capture.capture_pane(&session_name);

    // Cleanup
    let _ = manager.kill(&session_name);

    assert!(result.is_ok(), "Capture failed: {result:?}");

    Ok(())
}

#[test]
fn test_tmux_capture_pane_with_history() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("capture_history")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("hist");

    // Create a session that stays alive
    let _ = manager.kill(&session_name);
    manager.create(&session_name, &fixture.worktree_path(), Some("sleep 60"))?;

    std::thread::sleep(std::time::Duration::from_millis(300));

    // Verify session exists
    assert!(
        manager.exists(&session_name),
        "Session should exist before capture"
    );

    // Capture with history
    let capture = tenex::tmux::OutputCapture::new();
    let result = capture.capture_pane_with_history(&session_name, 100);

    // Cleanup
    let _ = manager.kill(&session_name);

    assert!(result.is_ok(), "Capture with history failed: {result:?}");

    Ok(())
}

#[test]
fn test_tmux_capture_full_history() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("capture_full")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("full");

    // Create a session that stays alive
    let _ = manager.kill(&session_name);
    manager.create(&session_name, &fixture.worktree_path(), Some("sleep 60"))?;

    std::thread::sleep(std::time::Duration::from_millis(300));

    // Verify session exists
    assert!(
        manager.exists(&session_name),
        "Session should exist before capture"
    );

    // Capture full history
    let capture = tenex::tmux::OutputCapture::new();
    let result = capture.capture_full_history(&session_name);

    // Cleanup
    let _ = manager.kill(&session_name);

    assert!(result.is_ok(), "Capture full history failed: {result:?}");

    Ok(())
}

#[test]
fn test_tmux_capture_nonexistent_session() {
    if skip_if_no_tmux() {
        return;
    }

    let capture = tenex::tmux::OutputCapture::new();
    let result = capture.capture_pane("nonexistent-session-xyz");
    assert!(result.is_err());
}

#[test]
fn test_tmux_send_keys() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
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
fn test_tmux_send_keys_and_submit() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("send_submit")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("submit");

    // Create a session
    let _ = manager.kill(&session_name);
    manager.create(&session_name, &fixture.worktree_path(), None)?;

    std::thread::sleep(std::time::Duration::from_millis(200));

    // Send keys with submit
    let result = manager.send_keys_and_submit(&session_name, "pwd");
    assert!(result.is_ok());

    // Cleanup
    let _ = manager.kill(&session_name);

    Ok(())
}

#[test]
fn test_tmux_window_operations() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
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
    let window_result = manager.create_window(
        &session_name,
        "test-window",
        &fixture.worktree_path(),
        Some("echo hello"),
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
fn test_tmux_capture_tail() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("capture_tail")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("tail");

    // Create session with output
    let _ = manager.kill(&session_name);
    manager.create(&session_name, &fixture.worktree_path(), Some("sleep 60"))?;
    std::thread::sleep(std::time::Duration::from_millis(300));

    // Capture tail
    let capture = tenex::tmux::OutputCapture::new();
    let result = capture.tail(&session_name, 10);
    assert!(result.is_ok());

    // Cleanup
    let _ = manager.kill(&session_name);

    Ok(())
}

#[test]
fn test_tmux_pane_size() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
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
    let capture = tenex::tmux::OutputCapture::new();
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
fn test_tmux_cursor_position() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
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
    let capture = tenex::tmux::OutputCapture::new();
    let result = capture.cursor_position(&session_name);
    assert!(result.is_ok());

    // Cleanup
    let _ = manager.kill(&session_name);

    Ok(())
}

#[test]
fn test_tmux_pane_current_command() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("pane_cmd")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("cmd");

    // Create session with a specific command
    let _ = manager.kill(&session_name);
    manager.create(&session_name, &fixture.worktree_path(), Some("sleep 60"))?;
    std::thread::sleep(std::time::Duration::from_millis(300));

    // Get current command
    let capture = tenex::tmux::OutputCapture::new();
    let result = capture.pane_current_command(&session_name);
    // Should succeed (though command may vary)
    assert!(result.is_ok());

    // Cleanup
    let _ = manager.kill(&session_name);

    Ok(())
}

#[test]
fn test_tmux_session_rename() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
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
